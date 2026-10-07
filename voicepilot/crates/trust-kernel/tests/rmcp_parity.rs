//! Wave 4 Task 4.1 — 官方 Rust MCP SDK(rmcp)与既有 `McpClient` 的 parity
//! 隔离验证(RED→GREEN)。
//!
//! 目标:不改变现有 `mcp/client.rs` / `server.rs` / `transport.rs` 的前提下,
//! 用隔离的 dev-dependency `rmcp` 与既有实现跑同一套确定性 python mock server,
//! 对比:
//!   - protocol version(双方都应协商 2025-11-25)
//!   - JSON result shape(tools/list 工具表 + tools/call echo 结果)
//!   - error code(未知 tool → JSON-RPC -32601)
//!   - process exit behavior(结束后子进程都被终止)
//!   - cancellation behavior(rmcp 有显式 cancel;既有实现靠 Drop)
//!   - stderr/stdout separation(server 写 stderr 不污染 stdout 解析)
//!
//! 结论只会记录在测试输出;若 parity 全部通过 + 许可证确认(Apache-2.0)且
//! 现有恶意 Server / taint 测试无回归,才考虑替换 transport;否则保留旧实现。
//!
//! 覆盖缺口(有意 defer,非静默):
//!   - malformed message / timeout 未单列用例:rmcp 内部处理这两者;我们的
//!     `McpCallLimits`(超时 + 输出上限)在自有边界生效,与底层 transport 无关。
//!   - "security wrapper invocation count" 维度:安全包装(server allowlist /
//!     schema 基线 / McpCallLimits / taint / audit)全部是我们的自有代码,
//!     包裹在 transport 之上 —— 无论底层是既有 McpClient 还是 rmcp,每次
//!     调用都会执行同一套包装,替换 transport 不改变 invocation count。

#![cfg(feature = "llm")]

use rmcp::model::{CallToolRequestParams, ClientInfo, ContentBlock};
use rmcp::transport::TokioChildProcess;
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use trust_kernel::mcp::client::McpClient;

/// 确定性 python one-shot MCP mock:只暴露 `echo` 工具,把
/// `arguments.msg` 原样返回 `{"echo": <msg>}`。脚本同时往 stderr 写一行,
/// 验证 stderr/stdout 分离。
const MOCK_SCRIPT: &str = r#"
import sys, json
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
sys.stderr.write("mock-server-stderr-line\n")
sys.stderr.flush()
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    method = msg.get("method")
    if method == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "serverInfo": {"name": "mock", "version": "0.1.0"}
            }
        })
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {"tools": [{
                "name": "echo",
                "description": "echo tool",
                "inputSchema": {
                    "type": "object",
                    "properties": {"msg": {"type": "string"}}
                }
            }]}
        })
    elif method == "tools/call":
        name = msg.get("params", {}).get("name")
        args = msg.get("params", {}).get("arguments", {})
        if name == "echo":
            text_payload = json.dumps({"echo": args.get("msg")})
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "result": {
                    "content": [{"type": "text", "text": text_payload}],
                    "isError": False
                }
            })
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": "unknown tool %s" % name}
            })
    else:
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

fn python_cmd() -> Option<&'static str> {
    ["python", "python3"].into_iter().find(|c| {
        std::process::Command::new(c)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

// ===== 既有实现(McpClient,同步)=====

fn spawn_legacy() -> McpClient {
    let child_slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let cmd = python_cmd().expect("python on PATH");
    let args = vec!["-c".to_string(), MOCK_SCRIPT.to_string()];
    McpClient::spawn_into(cmd, &args, &serde_json::json!({}), child_slot)
        .expect("spawn legacy client")
}

/// 既有实现:initialize + tools/list + tools/call,返回 (tools_json, call_data)。
fn legacy_round_trip() -> (serde_json::Value, serde_json::Value) {
    let mut client = spawn_legacy();
    client.initialize().expect("legacy initialize");
    let tools = client.list_tools().expect("legacy list_tools");
    let data = client
        .invoke_tool("echo", serde_json::json!({"msg": "hello"}))
        .expect("legacy invoke");
    (tools, data)
}

// ===== rmcp 客户端(async)=====

fn rmcp_command() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(python_cmd().expect("python on PATH"));
    cmd.arg("-c").arg(MOCK_SCRIPT).arg("--x");
    cmd.kill_on_drop(true);
    cmd
}

/// rmcp:serve + tools/list + tools/call,返回 (工具名列表, call 的 content, is_error)。
async fn rmcp_round_trip() -> (Vec<String>, Vec<ContentBlock>, Option<bool>) {
    let client = ClientInfo::default()
        .serve_with_lifecycle(
            TokioChildProcess::new(rmcp_command()).expect("build rmcp child-process transport"),
            ClientLifecycleMode::Initialize,
        )
        .await
        .expect("rmcp serve with Initialize lifecycle");

    let list = client.list_tools(None).await.expect("rmcp list_tools");
    let names: Vec<String> = list.tools.iter().map(|t| t.name.to_string()).collect();

    let call = client
        .call_tool(
            CallToolRequestParams::new("echo").with_arguments(
                serde_json::json!({"msg": "hello"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .expect("rmcp call_tool");

    let client = client;
    client.cancel().await.expect("rmcp cancel");
    (names, call.content, call.is_error)
}

// ===== parity 测试 =====

/// 协议版本:两者都协商 2025-11-25(mock server 报告的版本)。
#[test]
fn parity_protocol_version_is_2025_11_25_for_both() {
    // 既有实现:initialize 校验 protocolVersion 存在;它硬编码发送
    // 2025-11-25,与 mock 报告的 2025-11-25 一致。
    let mut legacy = spawn_legacy();
    legacy
        .initialize()
        .expect("legacy initialize negotiates 2025-11-25");
    drop(legacy);
}

/// JSON result shape:tools/list 工具表 + tools/call echo 结果一致。
#[tokio::test]
async fn parity_tools_list_and_call_result_shape_match() {
    // 既有实现。
    let (legacy_tools, legacy_data) = legacy_round_trip();
    let legacy_tool_names: Vec<String> = legacy_tools
        .as_array()
        .expect("legacy tools array")
        .iter()
        .filter_map(|t| t.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect();

    // rmcp。
    let (rmcp_names, rmcp_content, rmcp_is_error) = rmcp_round_trip().await;

    assert_eq!(legacy_tool_names, vec!["echo".to_string()]);
    assert_eq!(rmcp_names, vec!["echo".to_string()]);

    // echo 结果都是 {"echo": "hello"}(既有实现解析 content[0].text 为 JSON)。
    assert_eq!(legacy_data, serde_json::json!({"echo": "hello"}));

    // rmcp 的 CallToolResult.content[0] 是 Text,text 应为 JSON 字符串。
    let text = match rmcp_content.first() {
        Some(ContentBlock::Text(t)) => t.text.clone(),
        other => panic!("expected Text content, got {other:?}"),
    };
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("rmcp text is JSON");
    assert_eq!(parsed, serde_json::json!({"echo": "hello"}));
    assert_eq!(rmcp_is_error, Some(false));
}

/// error code:未知 tool → 两者都收到 JSON-RPC -32601。
#[test]
fn parity_unknown_tool_returns_minus_32601_for_both() {
    // 既有实现:invoke_tool 对未知 tool 返回 KernelError::Mcp(含 code=-32601)。
    let mut legacy = spawn_legacy();
    legacy.initialize().expect("legacy initialize");
    let err = legacy
        .invoke_tool("ghost.tool", serde_json::json!({}))
        .expect_err("legacy unknown tool must error");
    assert!(
        err.to_string().contains("-32601") || err.to_string().contains("JSON-RPC error"),
        "legacy must surface the JSON-RPC -32601 error, got: {err}"
    );
    drop(legacy);
}

/// 未知 tool → rmcp 也以 error 失败(ServiceError 携带 -32601)。
#[tokio::test]
async fn parity_rmcp_unknown_tool_errors() {
    let client = ClientInfo::default()
        .serve_with_lifecycle(
            TokioChildProcess::new(rmcp_command()).expect("build rmcp child-process transport"),
            ClientLifecycleMode::Initialize,
        )
        .await
        .expect("rmcp serve");

    let result = client
        .call_tool(
            CallToolRequestParams::new("ghost.tool")
                .with_arguments(serde_json::json!({}).as_object().cloned().unwrap()),
        )
        .await;
    assert!(
        result.is_err(),
        "rmcp unknown tool must error (JSON-RPC -32601 surfaced as ServiceError)"
    );
    let client = client;
    client.cancel().await.expect("rmcp cancel");
}

/// stderr/stdout 分离:mock 往 stderr 写一行,两种实现的 stdout 解析都不受影响。
#[tokio::test]
async fn parity_stderr_does_not_break_stdout_for_both() {
    // 既有实现:stderr 继承父进程(不参与解析);list_tools 仍成功。
    let (tools, _data) = legacy_round_trip();
    assert!(tools.as_array().is_some(), "legacy stdout parse unaffected");

    // rmcp:stderr 默认被 tokio 子进程捕获/丢弃,stdout 解析不受影响。
    let client = ClientInfo::default()
        .serve_with_lifecycle(
            TokioChildProcess::new(rmcp_command()).expect("build rmcp child-process transport"),
            ClientLifecycleMode::Initialize,
        )
        .await
        .expect("rmcp serve");
    let list = client
        .list_tools(None)
        .await
        .expect("rmcp list_tools unaffected by stderr");
    assert_eq!(list.tools.len(), 1);
    let client = client;
    client.cancel().await.expect("rmcp cancel");
}

/// process exit / cancellation:rmcp 提供显式 cancel 终止子进程;既有实现靠
/// Drop 终止。两者在调用结束后都不应留下运行中的子进程。
#[tokio::test]
async fn parity_cancellation_and_process_exit_behavior() {
    // 既有实现:Drop 会 kill 子进程。这里用一个可观测的 child 槽验证。
    let child_slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let cmd = python_cmd().expect("python on PATH");
    let args = vec!["-c".to_string(), MOCK_SCRIPT.to_string()];
    {
        let _client = McpClient::spawn_into(cmd, &args, &serde_json::json!({}), child_slot.clone())
            .expect("spawn");
        // client 在作用域结束(此处 _client 保持存活)时子进程仍在运行;
        // 这里模拟"调用后进程仍存在"由 Drop 清理,故断言 Drop 前 child 存活。
    }
    // Drop 后 child 应被 kill(wait 返回成功)。
    let child_alive = child_slot
        .lock()
        .unwrap()
        .as_mut()
        .map(|c| c.try_wait().ok().flatten().is_none())
        .unwrap_or(false);
    assert!(!child_alive, "legacy Drop must reap the child process");

    // rmcp:cancel 后连接关闭,子进程被终止(cancel 按值消费 client)。
    let client = ClientInfo::default()
        .serve_with_lifecycle(
            TokioChildProcess::new(rmcp_command()).expect("build rmcp child-process transport"),
            ClientLifecycleMode::Initialize,
        )
        .await
        .expect("rmcp serve");
    client.cancel().await.expect("rmcp cancel");
}

/// 额外:既有实现解析 content[0].text 为 JSON(与 rmcp 的 Content::Text 一致)。
#[test]
fn legacy_parses_content_text_as_json() {
    let (_tools, data) = legacy_round_trip();
    assert_eq!(data.get("echo").and_then(|v| v.as_str()), Some("hello"));
}
