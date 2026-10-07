use std::io::Cursor;
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::server::McpServer;
use trust_kernel::mcp::transport::{JsonRpcId, JsonRpcRequest};
use trust_kernel::repo::step_repo::StepRecord;

#[test]
fn server_dispatch_unknown_method_returns_method_not_found() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        method: "foo/bar".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be an error response with code -32601.
    assert!(s.contains(r#""code":-32601"#));
    assert!(s.contains(r#""id":1"#));
}

#[test]
fn server_initialize_returns_protocol_2025_11_25_and_capabilities() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("init-1".to_string()),
        method: "initialize".to_string(),
        params: Some(serde_json::json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": {"name": "test-client", "version": "0.1.0"}
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be a Response (not Error).
    assert!(
        !s.contains(r#""code""#),
        "initialize must succeed, got: {}",
        s
    );
    assert!(s.contains(r#""protocolVersion":"2025-11-25""#));
    // Must advertise tools capability.
    assert!(s.contains(r#""tools""#));
    // Must include serverInfo with name + version.
    assert!(s.contains(r#""serverInfo""#));
    assert!(s.contains(r#""voicepilot""#));
}

#[test]
fn server_tools_list_returns_3_filesystem_tools() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(2),
        method: "tools/list".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(!s.contains(r#""code""#), "tools/list must succeed: {}", s);
    assert!(s.contains(r#""filesystem.search_files""#));
    assert!(s.contains(r#""filesystem.move_files""#));
    assert!(s.contains(r#""filesystem.verify_move""#));
}

#[test]
fn server_tools_call_dispatches_search_files() {
    use std::fs;
    let kernel = TrustKernel::open_in_memory().unwrap();
    // Set up a temp dir with a file.
    let dir = std::env::temp_dir().join(format!("vp-w4-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.pdf"), b"pdf").unwrap();

    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(3),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.search_files",
            "arguments": {
                "root": dir.to_string_lossy(),
                "pattern": "*.pdf"
            }
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(
        !s.contains(r#""code""#),
        "tools/call search_files must succeed: {}",
        s
    );
    assert!(s.contains("a.pdf"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn server_tools_call_rejects_move_files() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(4),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.move_files",
            "arguments": {
                "task_id": "t1",
                "step_id": "s1",
                "sources": ["C:/tmp/a.pdf"],
                "destination": "C:/tmp/out/"
            }
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be an error — move_files must go through the Skill executor.
    assert!(s.contains(r#""code""#));
}

#[test]
fn server_tools_call_logs_audit_when_task_id_present() {
    use std::fs;
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    kernel.create_task("t-audit", "audit test").unwrap();
    // Pre-create step row so audit_logs.step_id FK is satisfied
    // (migration 001_init.sql: step_id REFERENCES steps(step_id)).
    kernel
        .create_step(&StepRecord::new("s-audit", "t-audit", 1))
        .unwrap();
    let before = kernel.audit_count_for_task("t-audit").unwrap();

    let dir = std::env::temp_dir().join(format!("vp-w4-audit-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("x.pdf"), b"x").unwrap();

    let server = McpServer::with_arc(McpHandler::new(), kernel.clone());
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(5),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.search_files",
            "arguments": {
                "root": dir.to_string_lossy(),
                "pattern": "*.pdf",
                "task_id": "t-audit",
                "step_id": "s-audit"
            }
        })),
    };
    server.handle_request(req).unwrap();
    let after = kernel.audit_count_for_task("t-audit").unwrap();
    // W9 Plan 3: 成功的 tools/call 现在记录 2 条审计事件:
    //   1. mcp_tools_call(既有,call_tool 调用记录)
    //   2. taint_propagated(W9 Plan 3 新增,mcp_tool:<server_id> taint 标记)
    assert_eq!(
        after,
        before + 2,
        "tools/call with task_id must log mcp_tools_call + taint_propagated audit events"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_stdio_handles_initialize_then_tools_list() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // Two NDJSON lines on stdin: initialize + tools/list.
    let init_line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let list_line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":null}"#;
    let input = format!("{}\n{}\n", init_line, list_line);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    // Two response lines.
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 responses, got: {:?}", lines);
    assert!(lines[0].contains(r#""protocolVersion":"2025-11-25""#));
    assert!(lines[1].contains(r#""filesystem.search_files""#));
}

#[test]
fn run_stdio_skips_notifications() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // A notification (no id) should be silently dropped — no response written.
    let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let input = format!("{}\n", notif);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    assert!(
        writer.is_empty(),
        "notification must not produce a response"
    );
}

#[test]
fn run_stdio_continues_after_parse_error() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // Invalid JSON line, then a valid request.
    let input = "{not valid json\n".to_string()
        + r#"{"jsonrpc":"2.0","id":99,"method":"tools/list"}"#
        + "\n";
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    let output = String::from_utf8(writer).unwrap();
    // Parse error response for the bad line, then tools/list response.
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2);
    assert!(
        lines[0].contains(r#""code":-32700"#),
        "first must be parse error"
    );
    assert!(lines[1].contains(r#""filesystem.search_files""#));
}

#[test]
fn run_stdio_continues_after_invalid_params_missing_name() {
    // Spec issue #37: malformed JSON-RPC (valid JSON but missing required
    // fields) must produce an error response, not crash the loop.
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // tools/call without "name" field (InvalidParams), then a valid tools/list.
    let bad_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"arguments":{}}}"#;
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    let input = format!("{}\n{}\n", bad_call, list);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    let output = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "loop must continue after invalid params, got: {:?}",
        lines
    );
    // First response: InvalidParams -32602 for id=1.
    assert!(
        lines[0].contains(r#""code":-32602"#),
        "missing name must be InvalidParams -32602, got: {}",
        lines[0]
    );
    assert!(lines[0].contains(r#""id":1"#));
    // Second response: tools/list succeeded — loop did not crash.
    assert!(lines[1].contains(r#""filesystem.search_files""#));
}

#[test]
fn run_stdio_continues_after_audit_failure() {
    // Spec issue #37: when audit_append_external fails (e.g. task_id FK
    // violation), the loop must emit InternalError -32603 and continue,
    // not propagate the kernel error and exit.
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // tools/call with task_id that doesn't exist — FK violation on audit log.
    // Use search_files with a real temp dir so the call_tool itself succeeds,
    // then audit_append_external fails on the bogus task_id.
    let dir = std::env::temp_dir().join(format!("vp-w4-audit-fail-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.pdf"), b"a").unwrap();

    let dir_str = dir.to_string_lossy().replace('\\', "/");
    let bad_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"filesystem.search_files","arguments":{"root":"__DIR__","pattern":"*.pdf","task_id":"nonexistent-task"}}}"#
        .replace("__DIR__", &dir_str);
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    let input = format!("{}\n{}\n", bad_call, list);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    let output = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "loop must continue after audit failure, got: {:?}",
        lines
    );
    // First response: InternalError -32603 for id=1.
    assert!(
        lines[0].contains(r#""code":-32603"#),
        "audit failure must be InternalError -32603, got: {}",
        lines[0]
    );
    assert!(lines[0].contains(r#""id":1"#));
    // Second response: tools/list succeeded — loop did not crash.
    assert!(lines[1].contains(r#""filesystem.search_files""#));

    std::fs::remove_dir_all(&dir).ok();
}
