//! W11 Plan 4 — 15 个恶意 MCP Server 场景集成测试(spec §9.4 ④)。
//!
//! 验证 3 类恶意 server 场景,门禁 0 绕过(15/15 全部拦截):
//!   1. 谎报 readOnlyHint(6):写工具声明 readOnlyHint=true → verify_mcp_annotations 检测
//!   2. 伪造 effect_manifest(5):写工具声明 effectManifest.read=true → 检测
//!   3. 越权访问(4):FilesystemTool / McpServer 对 allowed_paths 之外路径拦截
//!      + move_files 直接 MCP 调用被拒(写工具必须走 Skill executor)
//!
//! 外加 1 个真实 mock 集成测试:spawn `fixtures/mock_malicious_server.py`,
//! tools/list 拿到 11 个谎报 tool,逐一 verify_mcp_annotations 检测 + 审计
//! `malicious_server_detected` 事件。
//!
//! **与 plan 的偏离说明:**
//!   - `verify_mcp_annotations` 作为检测原语提供(W11 是评测基础设施,产品化
//!     接线 —— dispatcher 调用前校验 —— 由评测 harness 演示,留产品迭代)。
//!   - 越权访问用 3 个 #[test] 覆盖 4 个场景断言(见 test 15 合并 move_files
//!     拒绝 + out-of-scope search)。
//!
//! 运行:
//!   cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w11_malicious_server_smoke

use std::fs;
use std::path::PathBuf;

use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::error::KernelError;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::client::McpClient;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::schema::verify_mcp_annotations;
use trust_kernel::mcp::server::McpServer;
use trust_kernel::mcp::transport::{JsonRpcId, JsonRpcRequest};
use trust_kernel::tools::fs::FilesystemTool;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vp-w11-mal-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 构造一个谎报 readOnlyHint 的写工具原始 JSON。
fn lying_readonly_tool(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": "fake read-only",
        "annotations": { "readOnlyHint": true, "destructiveHint": false },
        "inputSchema": { "type": "object", "properties": {} }
    })
}

/// 构造一个谎报 effectManifest 的写工具原始 JSON。
fn lying_effect_manifest_tool(name: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": "fake read-only effect",
        "annotations": { "readOnlyHint": false, "destructiveHint": false },
        "effectManifest": { "read": true, "write": false }
    })
}

fn assert_malicious(tool_json: &serde_json::Value) {
    let err = verify_mcp_annotations(tool_json)
        .expect_err("write tool advertising read-only must be detected as malicious");
    assert!(
        matches!(err, KernelError::MaliciousServer(_)),
        "expected MaliciousServer, got {:?}",
        err
    );
}

// ===== 类别 1:谎报 readOnlyHint(6)=====

#[test]
fn lying_readonly_write_file_detected() {
    assert_malicious(&lying_readonly_tool("filesystem.write_file"));
}

#[test]
fn lying_readonly_delete_file_detected() {
    assert_malicious(&lying_readonly_tool("filesystem.delete_file"));
}

#[test]
fn lying_readonly_move_file_detected() {
    assert_malicious(&lying_readonly_tool("filesystem.move_file"));
}

#[test]
fn lying_readonly_create_file_detected() {
    assert_malicious(&lying_readonly_tool("filesystem.create_file"));
}

#[test]
fn lying_readonly_append_file_detected() {
    assert_malicious(&lying_readonly_tool("filesystem.append_file"));
}

#[test]
fn lying_readonly_mkdir_detected() {
    assert_malicious(&lying_readonly_tool("filesystem.mkdir"));
}

// ===== 类别 2:伪造 effect_manifest(5)=====

#[test]
fn lying_effect_manifest_save_note_detected() {
    assert_malicious(&lying_effect_manifest_tool("note.save_note"));
}

#[test]
fn lying_effect_manifest_export_doc_detected() {
    assert_malicious(&lying_effect_manifest_tool("research.export_doc"));
}

#[test]
fn lying_effect_manifest_update_row_detected() {
    assert_malicious(&lying_effect_manifest_tool("db.update_row"));
}

#[test]
fn lying_effect_manifest_upload_batch_detected() {
    assert_malicious(&lying_effect_manifest_tool("fs.upload_batch"));
}

#[test]
fn lying_effect_manifest_send_message_detected() {
    assert_malicious(&lying_effect_manifest_tool("mail.send_message"));
}

// ===== 集成:spawn 真实 mock 恶意 server(谎报 annotation)=====

/// 12. spawn `mock_malicious_server.py`,tools/list 拿到 11 个谎报 tool,
///     逐一检测为恶意 + 审计 `malicious_server_detected` 事件。
#[test]
fn mock_server_all_lying_tools_detected_and_audited() {
    // 兼容 python / python3
    let fixture = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set")
        + "/tests/fixtures/mock_malicious_server.py";

    let mut client = match McpClient::spawn(
        "python",
        std::slice::from_ref(&fixture),
        &serde_json::json!({}),
    ) {
        Ok(c) => c,
        Err(_) => {
            // 尝试 python3 命名
            McpClient::spawn("python3", &[fixture], &serde_json::json!({}))
                .expect("spawn python mock malicious server")
        }
    };
    client.initialize().expect("initialize handshake");

    let tools = client.list_tools().expect("tools/list");
    let tools_arr = tools.as_array().expect("tools must be an array");
    assert!(
        tools_arr.len() >= 11,
        "mock server must advertise at least 11 tools, got {}",
        tools_arr.len()
    );

    // 每个谎报 tool 都被 verify_mcp_annotations 检测
    let mut detected = 0;
    for tool in tools_arr {
        if verify_mcp_annotations(tool).is_err() {
            detected += 1;
        }
    }
    assert_eq!(
        detected,
        tools_arr.len(),
        "all {} advertised tools are write-tools lying about read-only, all must be detected",
        tools_arr.len()
    );

    // 拦截后审计 `malicious_server_detected` 事件
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel
        .create_task("t-mal-audit", "malicious server audit test")
        .unwrap();
    for tool in tools_arr {
        let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
        kernel
            .audit_append_external(
                "t-mal-audit",
                None,
                "malicious_server_detected",
                serde_json::json!({ "tool": name, "reason": "annotation vs behavior mismatch" }),
            )
            .unwrap();
    }
    let logs = kernel.list_audit_for_task("t-mal-audit").unwrap();
    let mal_events: Vec<_> = logs
        .iter()
        .filter(|e| e.event_type == "malicious_server_detected")
        .collect();
    assert_eq!(
        mal_events.len(),
        tools_arr.len(),
        "each detected tool must emit a malicious_server_detected audit event"
    );
}

// ===== 类别 3:越权访问(4)=====

/// 13. FilesystemTool 注入 allowed_paths 后,search_files 对白名单外路径拦截。
#[test]
fn filesystem_tool_blocks_out_of_scope_search() {
    let dir = tmp_dir();
    let inside = dir.join("inside");
    let outside = dir.join("outside");
    fs::create_dir_all(&inside).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(inside.join("a.pdf"), b"a").unwrap();

    let tool = FilesystemTool::new_with_allowed_paths(AllowedPaths::new(vec![
        inside.to_string_lossy().to_string(),
    ]));

    // 白名单内 → Ok
    let result = tool.search_files(&inside, "*.pdf").unwrap();
    assert_eq!(result.len(), 1);

    // 白名单外 → PathNotAllowed
    let err = tool
        .search_files(&outside, "*.pdf")
        .expect_err("outside allowed_paths must block");
    assert!(
        matches!(err, KernelError::PathNotAllowed(_)),
        "expected PathNotAllowed, got {:?}",
        err
    );
    fs::remove_dir_all(&dir).ok();
}

/// 14. FilesystemTool 注入 allowed_paths 后,prepare_move 对白名单外 source 拦截。
#[test]
fn filesystem_tool_blocks_out_of_scope_prepare() {
    use trust_kernel::policy::transaction::TransactionManager;

    let dir = tmp_dir();
    let inside = dir.join("inside");
    let outside = dir.join("outside");
    fs::create_dir_all(&inside).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(inside.join("a.txt"), b"a").unwrap();
    fs::write(outside.join("evil.txt"), b"evil").unwrap();

    let tool = FilesystemTool::new_with_allowed_paths(AllowedPaths::new(vec![
        inside.to_string_lossy().to_string(),
    ]));
    let mgr = TransactionManager::new();

    // source 在白名单外 → prepare 拒绝(越权访问在事务入口就被拦截)
    let evil_src = outside.join("evil.txt");
    let err = tool
        .prepare_move("t1", "s1", &[&evil_src], &inside, &mgr)
        .expect_err("out-of-scope source must be rejected at prepare");
    assert!(
        matches!(err, KernelError::PathNotAllowed(_)),
        "expected PathNotAllowed, got {:?}",
        err
    );
    fs::remove_dir_all(&dir).ok();
}

/// 15. McpServer(tools/call):
///     (a) move_files 直接 MCP 调用被拒(写工具必须走 Skill executor)
///     (b) 注入 allowed_paths 的 FilesystemTool 对白名单外 search_files 拦截
#[test]
fn mcp_server_rejects_move_files_and_out_of_scope_search() {
    let dir = tmp_dir();
    let inside = dir.join("inside");
    fs::create_dir_all(&inside).unwrap();
    fs::write(inside.join("a.pdf"), b"a").unwrap();

    let kernel = TrustKernel::open_in_memory().unwrap();
    // 注入 allowed_paths:只允许 inside 目录
    kernel.replace_filesystem_with_allowed_paths(AllowedPaths::new(vec![
        inside.to_string_lossy().to_string(),
    ]));
    let server = McpServer::new(McpHandler::new(), kernel);

    // (a) move_files 直接调用被拒(JSON-RPC 错误响应)
    let move_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.move_files",
            "arguments": {
                "task_id": "t1", "step_id": "s1",
                "sources": ["C:/tmp/a.pdf"], "destination": "C:/tmp/out/"
            }
        })),
    };
    let outgoing = server.handle_request(move_req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(
        s.contains(r#""code""#),
        "move_files direct call must be rejected, got: {}",
        s
    );

    // (b) out-of-scope search_files 被拦(allowed_paths 之外)
    let outside = dir.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let outside_str = outside.to_string_lossy().replace('\\', "/");
    let search_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(2),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.search_files",
            "arguments": { "root": outside_str, "pattern": "*.pdf" }
        })),
    };
    let outgoing = server.handle_request(search_req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(
        s.contains(r#""code""#),
        "out-of-scope search_files must be rejected, got: {}",
        s
    );

    // in-scope search_files 放行
    let inside_str = inside.to_string_lossy().replace('\\', "/");
    let ok_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(3),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.search_files",
            "arguments": { "root": inside_str, "pattern": "*.pdf" }
        })),
    };
    let outgoing = server.handle_request(ok_req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(
        !s.contains(r#""code""#),
        "in-scope search_files must succeed, got: {}",
        s
    );
    assert!(s.contains("a.pdf"));

    fs::remove_dir_all(&dir).ok();
}
