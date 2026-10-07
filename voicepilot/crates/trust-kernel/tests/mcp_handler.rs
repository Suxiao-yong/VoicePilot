use std::fs;
use std::path::PathBuf;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::{McpCallResult, McpHandler};
use trust_kernel::mcp::schema::McpAnnotations;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-mcp-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn mcp_handler_lists_filesystem_tools_with_full_schema() {
    let handler = McpHandler::new();
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"filesystem.search_files"));
    assert!(names.contains(&"filesystem.move_files"));
    assert!(names.contains(&"filesystem.verify_move"));

    // Each tool must have inputSchema, outputSchema, and annotations.
    for t in &tools {
        assert!(t.input_schema.is_object(), "{} missing inputSchema", t.name);
        assert!(
            t.output_schema.is_object(),
            "{} missing outputSchema",
            t.name
        );
        // annotations must be present (even if all hints default to false).
        let _ann: &McpAnnotations = &t.annotations;
    }
}

#[test]
fn mcp_handler_move_files_annotations_match_v1_1_appendix_b() {
    let handler = McpHandler::new();
    let tools = handler.list_tools();
    let move_tool = tools
        .iter()
        .find(|t| t.name == "filesystem.move_files")
        .expect("filesystem.move_files must be registered");

    // V1.1 Appendix B: move_files is non-readOnly, non-destructive (it's reversible),
    // idempotent (in the prepare→commit sense), closed-world.
    assert!(!move_tool.annotations.read_only_hint);
    assert!(!move_tool.annotations.destructive_hint);
    assert!(move_tool.annotations.idempotent_hint);
    assert!(!move_tool.annotations.open_world_hint);
}

#[test]
fn mcp_handler_call_search_files_dispatches_to_filesystem() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let handler = McpHandler::new();
    let dir = tmp_dir();
    fs::write(dir.join("a.pdf"), b"pdf").unwrap();
    fs::write(dir.join("b.txt"), b"txt").unwrap();

    let args = serde_json::json!({
        "root": dir.to_string_lossy(),
        "pattern": "*.pdf"
    });
    let result = handler
        .call_tool(&kernel, "filesystem.search_files", &args)
        .unwrap();
    if let McpCallResult::Ok(value) = result {
        let matches = value.get("matches").and_then(|v| v.as_array()).unwrap();
        assert_eq!(matches.len(), 1);
    } else {
        panic!("expected Ok, got {:?}", result);
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn mcp_handler_call_unknown_tool_returns_error() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let handler = McpHandler::new();
    let result = handler.call_tool(&kernel, "nonexistent.tool", &serde_json::json!({}));
    assert!(result.is_err());
}

#[test]
fn mcp_handler_call_move_files_is_rejected() {
    // W3b: move_files must go through the Skill executor (Task 7) — direct
    // MCP calls are refused because they would bypass the approve phase.
    // W4 will expose prepare_move + commit_move as separate MCP tools.
    let kernel = TrustKernel::open_in_memory().unwrap();
    let handler = McpHandler::new();
    let args = serde_json::json!({
        "task_id": "t1",
        "step_id": "s1",
        "sources": [],
        "destination": "/tmp/out"
    });
    let result = handler.call_tool(&kernel, "filesystem.move_files", &args);
    // W3b: returns Err because the handler defers move_files to the Skill executor.
    // Direct calls would bypass approval — handler refuses.
    assert!(result.is_err());
}
