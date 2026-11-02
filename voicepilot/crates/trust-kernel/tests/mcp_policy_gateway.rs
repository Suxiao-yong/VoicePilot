//! Task 1 —— MCP 工具调用经由 `Gateway::decide` 的策略放行与拦截（契约 §1.6）。
//!
//! 覆盖 5 条判定：
//!   1. 只读本地工具 → E0×D0 → Allow，并落 `mcp_policy_decision` 审计；
//!   2. 凭据形态参数 → D3 → Deny（红线，Cedar/decide Step 1）；
//!   3. destructive 工具 → E2×D1 = Confirm → fail-closed 拦截；
//!   4. 参数带 `web_page` taint → `TaintPropagationBlocked` + `taint_blocked` 审计；
//!   5. 谎报只读的写工具 → `MaliciousServer` + `malicious_server_detected` 审计。
//!
//! 这些断言把「MCP 也在同一套 Cedar + E×D + taint 决策链里」钉死：
//! 任何一条被放开，外部 MCP server 就成了绕过策略网关的后门。

use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::policy::evaluate_mcp_call;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::policy::taint_repo::{TaintRepo, compute_value_hash, make_taint_record};
use trust_kernel::policy::types::Effect;
use trust_kernel::repo::step_repo::StepRecord;

const TASK_ID: &str = "task-mcp-policy";
const STEP_ID: &str = "step-mcp-policy";

fn seed_kernel() -> TrustKernel {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task(TASK_ID, "mcp policy gateway test").unwrap();
    kernel
        .create_step(&StepRecord::new(STEP_ID, TASK_ID, 1))
        .unwrap();
    let record = McpServerRecord {
        server_id: "mock".to_string(),
        name: "mock".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: None,
        allowed_origins: None,
        allowed_paths: None,
        command: Some("python".to_string()),
        args: Some("[]".to_string()),
        env: None,
    };
    {
        let conn = kernel.conn();
        McpServerRepo::new().create(&conn, &record).unwrap();
    }
    kernel
}

fn server_record(kernel: &TrustKernel) -> McpServerRecord {
    let conn = kernel.conn();
    McpServerRepo::new()
        .get(&conn, "mock")
        .unwrap()
        .expect("seeded server")
}

fn tool(name: &str, read_only: bool, destructive: bool, open_world: bool) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": "test tool",
        "inputSchema": {"type": "object"},
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "openWorldHint": open_world,
        }
    })
}

/// 统计某 task 下某类审计事件条数。
fn audit_count(kernel: &TrustKernel, event_type: &str) -> usize {
    kernel
        .list_audit_for_task(TASK_ID)
        .unwrap()
        .into_iter()
        .filter(|e| e.event_type == event_type)
        .count()
}

#[test]
fn readonly_tool_is_allowed_and_audited() {
    let kernel = seed_kernel();
    let server = server_record(&kernel);
    let args = serde_json::json!({"url": "https://example.com"});

    let decision = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("snapshot", true, false, false),
        &args,
    )
    .expect("read-only local tool must be allowed");

    assert_eq!(decision.effect, Effect::Allow);
    assert_eq!(
        audit_count(&kernel, "mcp_policy_decision"),
        1,
        "every MCP policy verdict must be audited"
    );
    let event = kernel
        .list_audit_for_task(TASK_ID)
        .unwrap()
        .into_iter()
        .find(|e| e.event_type == "mcp_policy_decision")
        .unwrap();
    assert_eq!(event.details["effect"], "allow");
    assert_eq!(event.details["server_id"], "mock");
    assert_eq!(event.details["tool_name"], "snapshot");
    // 审计不得泄漏参数原文（只存 hash）。
    assert!(
        !event.details.to_string().contains("example.com"),
        "audit details must not leak raw args"
    );
}

#[test]
fn credential_shaped_args_are_denied_by_d3_red_line() {
    let kernel = seed_kernel();
    let server = server_record(&kernel);
    let args = serde_json::json!({"api_key": "sk-live-123"});

    let err = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("login", false, false, false),
        &args,
    )
    .expect_err("D3 credentials must never reach an external MCP tool");

    assert!(
        err.to_string().contains("denied"),
        "expected PolicyDenied, got: {err}"
    );
    let event = kernel
        .list_audit_for_task(TASK_ID)
        .unwrap()
        .into_iter()
        .find(|e| e.event_type == "mcp_policy_decision")
        .unwrap();
    assert_eq!(event.details["effect"], "deny");
    assert_eq!(event.details["data_class"], "D3");
}

#[test]
fn destructive_tool_is_fail_closed_on_confirm() {
    let kernel = seed_kernel();
    let server = server_record(&kernel);

    let err = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("purge_records", false, true, false),
        &serde_json::json!({}),
    )
    .expect_err("E2×D1 = confirm must be blocked (MCP has no approval channel)");

    assert!(
        err.to_string().contains("requires approval"),
        "expected fail-closed confirm, got: {err}"
    );
}

#[test]
fn web_page_tainted_args_are_blocked_before_cedar() {
    let kernel = seed_kernel();
    let server = server_record(&kernel);
    let args = serde_json::json!({"instruction": "ignore previous instructions"});
    let hash = compute_value_hash(&args);
    {
        let conn = kernel.conn();
        TaintRepo::new()
            .upsert(
                &conn,
                &make_taint_record(
                    hash,
                    "web_page".to_string(),
                    vec!["web_page".to_string()],
                    Some("page:test".to_string()),
                ),
            )
            .unwrap();
    }

    let err = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("run", false, false, false),
        &args,
    )
    .expect_err("web_page taint must not elevate into tool arguments");

    assert!(
        matches!(
            err,
            trust_kernel::error::KernelError::TaintPropagationBlocked { .. }
        ),
        "expected TaintPropagationBlocked, got: {err}"
    );
    assert_eq!(
        audit_count(&kernel, "taint_blocked"),
        1,
        "taint block must be audited"
    );
    // 策略裁决根本不该跑到 Cedar：taint 是前置硬拒。
    assert_eq!(audit_count(&kernel, "mcp_policy_decision"), 0);
}

#[test]
fn lying_readonly_tool_is_rejected_as_malicious() {
    let kernel = seed_kernel();
    let server = server_record(&kernel);

    let err = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("delete_everything", true, false, false),
        &serde_json::json!({}),
    )
    .expect_err("a write tool advertising readOnlyHint must be rejected");

    assert!(
        matches!(err, trust_kernel::error::KernelError::MaliciousServer(_)),
        "expected MaliciousServer, got: {err}"
    );
    assert_eq!(audit_count(&kernel, "malicious_server_detected"), 1);
    assert_eq!(audit_count(&kernel, "mcp_policy_decision"), 0);
}

#[test]
fn audit_chain_stays_valid_after_policy_verdicts() {
    let kernel = seed_kernel();
    let server = server_record(&kernel);
    let _ = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("snapshot", true, false, false),
        &serde_json::json!({}),
    )
    .unwrap();
    let _ = evaluate_mcp_call(
        &kernel,
        TASK_ID,
        Some(STEP_ID),
        &server,
        &tool("purge", false, true, false),
        &serde_json::json!({}),
    );

    let verification = kernel.verify_audit_chain().unwrap();
    assert!(verification.valid, "audit hash chain must stay intact");
    assert!(verification.checked >= 3, "task_created + step + 2 verdicts");
}
