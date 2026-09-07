//! Wave 2 Task 2.1 — snapshot-gated dispatch contract tests (RED→GREEN).
//!
//! `dispatch_skill_executor` must FIRST resolve the skill through the
//! published extension snapshot (`kernel.extension_snapshot()`) and reject
//! unknown / disabled / display-only extensions before any executor or MCP
//! process runs. Builtin targets route to the existing typed executors;
//! MCP tool targets route through the existing `invoke_mcp_tool` security
//! chain. Outgoing MCP calls run under `McpCallLimits` (fixed defaults,
//! constants only — no DB columns) and oversized tool results are rejected.

use std::fs;
use std::time::Duration;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::client::McpCallLimits;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::repo::step_repo::StepRepo;
use trust_kernel::skills::common::enforce_mcp_output_limit;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;
use trust_kernel::skills::repo::SkillRecord;
use uuid::Uuid;

/// A complete MCP tool contract Skill manifest file (same shape as the
/// `extensions_registry.rs` fixture).
fn mcp_tool_skill_md(skill_id: &str, server_id: &str, tool_name: &str) -> String {
    format!(
        r#"---
name: "{skill_id}"
description: "A complete MCP tool contract fixture. Runs {tool_name} on {server_id}."
metadata:
  voicepilot:
    execution:
      type: mcp_tool
      server_id: "{server_id}"
      tool_name: "{tool_name}"
---

# MCP tool contract fixture
"#,
        skill_id = skill_id,
        server_id = server_id,
        tool_name = tool_name,
    )
}

/// A display-only user Skill manifest — same shape as the MCP fixture but
/// with no `execution:` block (execution resolves to None).
fn display_only_skill_md(skill_id: &str) -> String {
    format!(
        r#"---
name: "{skill_id}"
description: "A display-only skill fixture (no execution binding)."
---

# Display-only skill fixture
"#,
        skill_id = skill_id,
    )
}

/// Write an MCP-backed user Skill file + an enabled/trusted server record
/// (command as given — `None` keeps the record structurally valid while
/// making any actual invocation fail with a deterministic MCP-chain error).
/// Returns the kernel and the temp dir guard (kept alive so later catalog
/// reads still work).
fn open_kernel_with_mcp_skill(
    skill_id: &str,
    server_id: &str,
    tool_name: &str,
    command: Option<String>,
) -> (TrustKernel, tempfile::TempDir) {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone())
        .expect("open in-memory kernel with temporary user Skills directory");
    let skill_path = skills_dir.join(format!("{skill_id}/SKILL.md"));
    std::fs::create_dir_all(skill_path.parent().expect("skill dir parent")).expect("create skill dir");
    fs::write(
        &skill_path,
        mcp_tool_skill_md(skill_id, server_id, tool_name),
    )
    .expect("write MCP-backed user Skill");
    {
        let conn = kernel.conn();
        McpServerRepo::new()
            .create(
                &conn,
                &McpServerRecord {
                    server_id: server_id.to_string(),
                    name: format!("Gate {server_id}"),
                    version: "1.0.0".to_string(),
                    transport: "stdio".to_string(),
                    enabled: true,
                    trusted: true,
                    protocol_version: Some("2025-11-25".to_string()),
                    allowed_origins: None,
                    allowed_paths: None,
                    command,
                    args: None,
                    env: None,
                },
            )
            .expect("create trusted enabled MCP server");
    }
    (kernel, temp_root)
}

fn assert_no_task_or_step_side_effects(kernel: &TrustKernel, task_id: &str, step_id: &str) {
    assert!(
        kernel.get_task(task_id).expect("query task row").is_none(),
        "rejected dispatch must not create a task row"
    );
    {
        let conn = kernel.conn();
        assert!(
            StepRepo::new()
                .get(&conn, step_id)
                .expect("query step row")
                .is_none(),
            "rejected dispatch must not create a step row"
        );
    }
}

#[test]
fn dispatch_rejects_unknown_extension() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let result = dispatch_skill_executor(
        "no.such.extension",
        &kernel,
        &serde_json::json!({}),
        &AutoApprover,
        "task-unknown",
        "step-unknown",
    );
    let err = result.expect_err("unknown extension must be rejected before any executor runs");
    assert!(
        err.to_string().contains("not found"),
        "error should mention 'not found', got: {err}"
    );
    assert_no_task_or_step_side_effects(&kernel, "task-unknown", "step-unknown");
}

#[test]
fn dispatch_rejects_disabled_extension_direct_call() {
    let skill_id = format!("gate.disabled.{}", Uuid::new_v4());
    let server_id = format!("gate-disabled-server-{}", Uuid::new_v4());
    let tool_name = "gate.disabled.tool";
    let (kernel, _temp_root) = open_kernel_with_mcp_skill(&skill_id, &server_id, tool_name, None);

    // Disable the Skill row BEFORE the catalog loads so the published
    // snapshot registers a disabled descriptor (extensions_registry fixture
    // style: disabled entries stay in the catalog, explicitly rejectable).
    {
        let conn = kernel.conn();
        kernel
            .skill_repo()
            .upsert(
                &conn,
                &SkillRecord {
                    skill_id: skill_id.clone(),
                    version: 1,
                    manifest_json: "{}".to_string(),
                    enabled: false,
                    success_count: 0,
                    avg_latency_ms: 0.0,
                },
            )
            .expect("upsert disabled Skill row");
    }
    kernel.load_user_skills().expect("load user Skills");

    let snapshot = kernel.extension_snapshot();
    assert!(
        snapshot.resolve_candidate(&skill_id).is_none(),
        "disabled Skill must not be a planner candidate"
    );
    assert!(snapshot
        .resolve_execution_target(&skill_id)
        .expect_err("disabled Skill must not resolve an execution target")
        .to_string()
        .contains("disabled"));

    let result = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"x": 1}),
        &AutoApprover,
        "task-disabled",
        "step-disabled",
    );
    let err = result.expect_err("disabled extension must be rejected before any executor runs");
    assert!(
        err.to_string().contains("disabled"),
        "error should mention 'disabled', got: {err}"
    );
    // No executor side effects: the MCP chain must never have been reached.
    assert_no_task_or_step_side_effects(&kernel, "task-disabled", "step-disabled");
}

#[test]
fn dispatch_rejects_display_only_extension_before_executor_runs() {
    let skill_id = format!("gate.display.{}", Uuid::new_v4());
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone())
        .expect("open in-memory kernel with temporary user Skills directory");
    std::fs::create_dir_all(
        skills_dir.join(format!("{skill_id}")).as_path(),
    )
    .expect("create skill dir");
    fs::write(
        skills_dir.join(format!("{skill_id}/SKILL.md")),
        display_only_skill_md(&skill_id),
    )
    .expect("write display-only user Skill");
    kernel.load_user_skills().expect("load user Skills");

    // Display-only extensions stay in the catalog (planner-readable) but
    // must never resolve to an execution target.
    let snapshot = kernel.extension_snapshot();
    assert!(
        snapshot.resolve_candidate(&skill_id).is_none(),
        "display-only Skill must not be an execution candidate"
    );
    assert!(snapshot
        .resolve_execution_target(&skill_id)
        .expect_err("display-only Skill must not resolve an execution target")
        .to_string()
        .contains("has no execution target"));

    let result = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({}),
        &AutoApprover,
        "task-display-only",
        "step-display-only",
    );
    let err = result.expect_err("display-only extension must be rejected before any executor runs");
    assert!(
        err.to_string().contains("has no execution target"),
        "error should mention 'has no execution target', got: {err}"
    );
    // No executor side effects: nothing may run for a display-only skill.
    assert_no_task_or_step_side_effects(&kernel, "task-display-only", "step-display-only");
}

#[test]
fn dispatch_builtin_routes_to_typed_executor() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let outcome = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &serde_json::json!({"limit": 3}),
        &AutoApprover,
        "task-builtin",
        "step-builtin",
    )
    .expect("builtin task.explain must route to the typed executor");
    assert!(outcome.succeeded, "executor must report success");
    assert_eq!(outcome.task_id, "task-builtin");
    assert_eq!(outcome.step_id, "step-builtin");
    assert_eq!(
        outcome.output.get("task_id").and_then(|v| v.as_str()),
        Some("task-builtin"),
        "output must carry the executor-created task_id"
    );
    // The typed executor really ran: task/step rows were persisted.
    assert!(
        kernel
            .get_task("task-builtin")
            .expect("query task row")
            .is_some(),
        "typed executor must create the task row"
    );
    assert!(
        kernel
            .get_step("step-builtin")
            .expect("query step row")
            .is_some(),
        "typed executor must create the step row"
    );
}

/// Wave 2 Task 2.3: a successful builtin dispatch must append the extension
/// audit metadata (manifest_hash, extension source, catalog snapshot_id) to
/// the existing taint_propagated audit details.
#[test]
fn builtin_dispatch_audit_details_include_extension_metadata() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let snapshot_id = kernel.extension_snapshot().id().to_string();

    let outcome = dispatch_skill_executor(
        "task.explain",
        &kernel,
        &serde_json::json!({"limit": 3}),
        &AutoApprover,
        "task-builtin-audit",
        "step-builtin-audit",
    )
    .expect("builtin task.explain must dispatch successfully");
    assert!(outcome.succeeded, "executor must report success");

    let events = kernel
        .list_audit_for_task("task-builtin-audit")
        .expect("list audit events");
    let taint = events
        .iter()
        .find(|event| event.event_type == "taint_propagated")
        .expect("successful dispatch must emit taint_propagated");
    let extension = taint
        .details
        .get("extension")
        .expect("taint_propagated details must carry extension metadata");

    let manifest_hash = extension
        .get("manifest_hash")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(
        !manifest_hash.is_empty() && manifest_hash.starts_with("sha256:"),
        "details must carry the resolved descriptor's manifest_hash, got: {extension}"
    );
    assert_eq!(
        extension.get("source").and_then(|value| value.as_str()),
        Some("builtin"),
        "builtin dispatch must carry source=builtin, got: {extension}"
    );
    assert_eq!(
        extension.get("snapshot_id").and_then(|value| value.as_str()),
        Some(snapshot_id.as_str()),
        "details must carry the catalog snapshot_id, got: {extension}"
    );
}

#[test]
fn dispatch_mcp_target_goes_through_mcp_chain() {
    let skill_id = format!("gate.mcp.{}", Uuid::new_v4());
    let server_id = format!("gate-mcp-server-{}", Uuid::new_v4());
    let tool_name = "gate.mcp.tool";
    let (kernel, _temp_root) = open_kernel_with_mcp_skill(&skill_id, &server_id, tool_name, None);
    kernel.load_user_skills().expect("load user Skills");

    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "enabled+trusted MCP-backed Skill must be a dispatch candidate"
    );

    let result = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"x": 1}),
        &AutoApprover,
        "task-mcp",
        "step-mcp",
    );
    // Deterministic MCP-chain outcome: the server record is registered and
    // enabled but has no command, so invoke_mcp_tool fails with its own
    // "missing command" error — NOT a dispatch-gate rejection.
    let err = result.expect_err("MCP target must route through the MCP security chain");
    let msg = err.to_string();
    assert!(
        msg.contains("missing command"),
        "expected the MCP-chain 'missing command' error, got: {msg}"
    );
    assert!(
        !msg.contains("unknown skill_id"),
        "must not be a dispatch gate rejection: {msg}"
    );
    assert!(
        !msg.contains("was not found"),
        "must not be a catalog miss: {msg}"
    );
}

#[test]
fn mcp_result_above_max_output_bytes_is_rejected() {
    let limits = McpCallLimits::default();
    // Fixed defaults, constants only (no DB columns).
    assert_eq!(limits.timeout, Duration::from_secs(60));
    assert_eq!(limits.max_output_bytes, 4 * 1024 * 1024);

    let oversized =
        serde_json::json!({ "payload": "x".repeat(limits.max_output_bytes as usize + 1) });
    let err = enforce_mcp_output_limit(&oversized, limits.max_output_bytes)
        .expect_err("result above max_output_bytes must be rejected");
    assert!(
        err.to_string().contains("output limit"),
        "error should mention the output limit, got: {err}"
    );

    let near_limit =
        serde_json::json!({ "payload": "x".repeat(limits.max_output_bytes as usize - 64) });
    enforce_mcp_output_limit(&near_limit, limits.max_output_bytes)
        .expect("result below the cap must pass");

    let small = serde_json::json!({ "payload": "ok" });
    enforce_mcp_output_limit(&small, limits.max_output_bytes).expect("small result must pass");
}
