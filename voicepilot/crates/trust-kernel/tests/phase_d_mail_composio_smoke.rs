//! Phase D — IM/邮件试点（Composio MCP）mock 集成测试。
//!
//! 抄 `w7_plan5_task0_mcp_client_smoke` 的 python mock 模式：spawn 一个
//! stdio mock MCP server（tools/list 广播 GMAIL_FETCH_EMAILS，tools/call
//! 返回 canned 邮件列表），验证 mail.list_recent 捆绑技能全链路：
//!
//! 1. 捆绑技能经 seed → load_user_skills 注册，execution 绑定 composio。
//! 2. `dispatch_skill_executor` 走 mcp_tool_call_checked（tools/list 预检
//!    + schema baseline + tools/call），mock 返回进 output。
//! 3. taint 溯源断言：`mcp_tool:composio` 进 TaintRepo（spec §2.3）+
//!    `taint_propagated` 审计事件的 extension.server_id == "composio"。
//!
//! Python 缺失时短路 passing（同 w7 模式，mock 不可得 ≠ 回归）。

use std::process::Command;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::policy::taint_repo::TaintRepo;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;

const MOCK_SCRIPT: &str = r#"
import sys, json
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if msg.get("method") == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "serverInfo": {"name": "mock-composio", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/list":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "tools": [
                    {
                        "name": "GMAIL_FETCH_EMAILS",
                        "description": "mock gmail fetch",
                        "inputSchema": {"type": "object"}
                    }
                ]
            }
        })
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "GMAIL_FETCH_EMAILS":
            payload = json.dumps({
                "emails": [
                    {"from": "a@b.com", "subject": "周报", "snippet": "本周进展..."},
                    {"from": "c@d.com", "subject": "发票", "snippet": "发票已开"}
                ]
            })
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "result": {"content": [{"type": "text", "text": payload}], "isError": False}
            })
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": f"unknown tool {name}"}
            })
    else:
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

const MAIL_SKILL_MD: &str = r#"---
name: mail.list_recent
description: 读收件箱：用 Composio MCP 拉取 Gmail 最近邮件列表（只读，测试固件）。
metadata:
  version: "1.0.0"
  voicepilot:
    execution:
      type: mcp_tool
      server_id: composio
      tool_name: GMAIL_FETCH_EMAILS
---

# mail.list_recent（测试固件）
"#;

fn python_available() -> bool {
    Command::new("python")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn phase_d_mail_list_recent_dispatch_and_taint() {
    if !python_available() {
        eprintln!("skipping phase_d_mail_list_recent_dispatch_and_taint: python not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let skills_dir = tmp.path().join("skills");
    std::fs::create_dir_all(skills_dir.join("mail.list_recent")).unwrap();
    std::fs::write(
        skills_dir.join("mail.list_recent").join("SKILL.md"),
        MAIL_SKILL_MD,
    )
    .unwrap();
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir).unwrap();

    let rec = McpServerRecord {
        server_id: "composio".to_string(),
        name: "Composio MCP".to_string(),
        version: "external".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some("[]".to_string()),
        command: Some("python".to_string()),
        args: Some(serde_json::json!(["-c", MOCK_SCRIPT]).to_string()),
        env: Some("{}".to_string()),
    };
    McpServerRepo::new().create(&kernel.conn(), &rec).unwrap();
    kernel.load_user_skills().unwrap();

    let task_id = format!("task-mail-{}", uuid::Uuid::new_v4());
    let step_id = format!("step-mail-{}", uuid::Uuid::new_v4());
    kernel
        .create_task(&task_id, "mail.list_recent smoke")
        .unwrap();
    kernel
        .create_step(&trust_kernel::repo::step_repo::StepRecord::new(
            step_id.clone(),
            task_id.clone(),
            1,
        ))
        .unwrap();

    let outcome = dispatch_skill_executor(
        "mail.list_recent",
        &kernel,
        &serde_json::json!({"query": "is:unread", "max_results": 5}),
        &AutoApprover,
        &task_id,
        &step_id,
    )
    .expect("dispatch must succeed via mock composio server");

    // 4. output 断言：mock 邮件列表原样进 output。
    let emails = outcome
        .output
        .get("emails")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(emails.len(), 2, "mock returns 2 emails");
    assert_eq!(emails[0]["subject"], "周报");

    // 5. taint 溯源断言（计划 D 验证门）：mcp_tool:composio 进 TaintRepo。
    {
        let conn = kernel.conn();
        let taints = TaintRepo::new()
            .list_by_provenance(&conn, "mcp_tool:composio")
            .expect("list_by_provenance");
        assert!(
            !taints.is_empty(),
            "mcp_tool:composio taint must be recorded after dispatch"
        );
    }

    // 6. 审计断言：taint_propagated 事件带 extension.server_id = composio。
    let events = kernel.list_audit_for_task(&task_id).unwrap();
    let taint_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "taint_propagated")
        .collect();
    assert_eq!(taint_events.len(), 1, "exactly one taint_propagated event");
    let details = &taint_events[0].details;
    assert_eq!(
        details
            .pointer("/extension/server_id")
            .and_then(|v| v.as_str()),
        Some("composio"),
        "audit must attribute the call to the composio server"
    );
    let taints_list = details["taints"].as_array().unwrap();
    assert!(
        taints_list
            .iter()
            .any(|t| t.as_str() == Some("mcp_tool:composio")),
        "taints list must contain mcp_tool:composio, got {taints_list:?}"
    );
}
