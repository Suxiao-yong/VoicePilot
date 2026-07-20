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
    assert!(!s.contains(r#""code""#), "initialize must succeed, got: {}", s);
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
    assert!(!s.contains(r#""code""#), "tools/call search_files must succeed: {}", s);
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
    assert_eq!(after, before + 1, "tools/call with task_id must log audit event");

    fs::remove_dir_all(&dir).ok();
}
