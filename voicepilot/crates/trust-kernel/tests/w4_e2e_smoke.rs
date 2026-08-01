use std::fs;
use std::io::Cursor;
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::repo::McpServerRepo;
use trust_kernel::mcp::server::McpServer;
use trust_kernel::repo::step_repo::StepRecord;

fn tmp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w4-e2e-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn end_to_end_mcp_server_smoke() {
    // ===== Setup: kernel + seed builtin server + load allowed_paths =====
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let repo = McpServerRepo::new();
    repo.seed_builtin_filesystem(&kernel.conn()).unwrap();

    // Create a task + step for the tools/call audit test.
    // FK constraint: audit_logs.step_id REFERENCES steps(step_id).
    kernel.create_task("e2e-task", "mcp e2e test").unwrap();
    kernel.create_step(&StepRecord::new("e2e-step", "e2e-task", 1)).unwrap();

    // ===== Setup: temp filesystem with a PDF =====
    let dir = tmp_dir();
    // Use a path under one of the default allowed_paths (C:/Users or D:/).
    // On Windows, std::env::temp_dir() is usually under C:/Users.
    fs::write(dir.join("a.pdf"), b"pdf-content").unwrap();

    // ===== Construct McpServer (no allowed_paths injection here —
    // the e2e test uses the kernel's default FilesystemTool which has
    // no whitelist, so search_files works on any path) =====
    let server = McpServer::with_arc(McpHandler::new(), kernel.clone());

    // ===== Build NDJSON input: 4 messages =====
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    let search = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"filesystem.search_files","arguments":{{"root":"{}","pattern":"*.pdf","task_id":"e2e-task","step_id":"e2e-step"}}}}}}"#,
        dir.to_string_lossy().replace('\\', "/").replace('"', "\\\"")
    );
    let reject = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"filesystem.move_files","arguments":{"task_id":"e2e-task","step_id":"e2e-step","sources":["C:/nonexistent"],"destination":"C:/out"}}}"#;
    let input = format!("{}\n{}\n{}\n{}\n", init, list, search, reject);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    let output = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    // ===== Assert: 4 responses =====
    assert_eq!(lines.len(), 4, "expected 4 responses, got: {}", output);

    // ===== Assert: initialize response =====
    assert!(lines[0].contains(r#""protocolVersion":"2025-11-25""#));
    assert!(lines[0].contains(r#""serverInfo""#));

    // ===== Assert: tools/list response contains 3 filesystem tools =====
    assert!(lines[1].contains(r#""filesystem.search_files""#));
    assert!(lines[1].contains(r#""filesystem.move_files""#));
    assert!(lines[1].contains(r#""filesystem.verify_move""#));

    // ===== Assert: tools/call search_files response contains a.pdf =====
    assert!(lines[2].contains("a.pdf"));
    assert!(lines[2].contains(r#""isError":false"#));

    // ===== Assert: tools/call move_files response is an error =====
    // Per Task 8 deviation: move_files rejection produces a JSON-RPC Error
    // (code field), not a Response with isError:true. Assert either.
    assert!(
        lines[3].contains(r#""code""#) || lines[3].contains(r#""isError":true"#),
        "move_files must be rejected, got: {}",
        lines[3]
    );

    // ===== Assert: audit trail has the mcp_tools_call event =====
    let audit_count = kernel.audit_count_for_task("e2e-task").unwrap();
    // W9 Plan 3: search_files carried task_id=e2e-task → 2 audit events
    //   (mcp_tools_call + taint_propagated for mcp_tool:<server_id> taint).
    // move_files rejection: Task 8 deviation catches the error BEFORE
    // audit_append_external, so no audit event for the rejection.
    // Also: create_task emits task_created, create_step emits step_created.
    // So total audit events = 4 (task_created + step_created + mcp_tools_call + taint_propagated).
    assert_eq!(audit_count, 4, "expected 4 audit events (task_created + step_created + mcp_tools_call + taint_propagated), got {}", audit_count);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn end_to_end_allowed_paths_enforced_after_replace() {
    use std::path::Path;
    use trust_kernel::allowed_paths::AllowedPaths;

    let kernel = TrustKernel::open_in_memory().unwrap();
    // Replace with a tool that enforces C:/Users whitelist.
    let allowed = AllowedPaths::new(vec!["C:/Users".to_string()]);
    kernel.replace_filesystem_with_allowed_paths(allowed);

    // Path outside whitelist must be rejected. We use `E:/definitely_nonexistent`,
    // which is clearly outside C:/Users. The assertion is that search_files
    // returns Err — either from allowed_paths rejection or from missing path,
    // both demonstrate that the whitelist is in effect (the default tool would
    // also return Err for a missing path, but for a DIFFERENT reason).
    // To make the test meaningful, verify the error is specifically
    // PathNotAllowed, not "search root missing".
    let result = kernel.filesystem().search_files(Path::new("E:/definitely_nonexistent"), "*.pdf");
    assert!(result.is_err(), "search_files outside allowed_paths must fail after replace");
    // Verify the error is PathNotAllowed (whitelist enforcement), not
    // "search root missing" (which would happen even without a whitelist).
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("not under any allowed root") || err_msg.contains("PathNotAllowed"),
        "error must be from whitelist enforcement, got: {}",
        err_msg
    );
}
