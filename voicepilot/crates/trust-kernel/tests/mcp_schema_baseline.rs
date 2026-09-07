//! Wave 2 Task 2.3 — User-Skill MCP tool pre-check + session schema-hash
//! baseline (RED→GREEN).
//!
//! Contract (plan Task 2.3):
//! - Before `tools/call` for a User-Skill MCP target, the dispatch path must
//!   confirm the target tool is advertised via the existing `tools/list`
//!   capability, and maintain an in-process (session) schema-hash baseline
//!   per (server_id, tool_name):
//!   - first call records the sha256 of the canonicalized tool schema JSON,
//!   - subsequent calls must match, else the call is rejected with an error
//!     telling the user to re-plan/re-approve (the tool schema changed).
//! - This applies ONLY to the `McpTool` dispatch arm; builtin MCP executors
//!   (research.save_markdown …) keep their existing path untouched.
//!
//! Variant chosen: the real `tools/list` pre-check (deterministic python
//! one-shot MCP mock, same pattern as `dispatch_snapshot_gate` /
//! `w11_malicious_server_smoke`) — NOT the catalog-level manifest.tools
//! fallback, because the schema-change test needs a live list_tools round
//! trip to prove the baseline mismatch rejection.
//!
//! The mock increments a counter in a state file (path passed via the
//! server record's env) on every process start and advertises tool `echo`
//! with `inputSchema.properties.version.const = "v{N}"`. Each dispatch
//! spawns the server exactly once (preflight + tools/call share one
//! process), so the first dispatch records v1 and the second sees v2 →
//! baseline mismatch.

use std::fs;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::skills::dispatcher::dispatch_skill_executor;
use uuid::Uuid;

/// The one-shot python MCP mock: advertises exactly one tool `echo` whose
/// inputSchema embeds a monotonic version read from `SCHEMA_STATE`; tools/call
/// `echo` echoes `arguments.msg` back as `{"echo": <msg>}`.
const MOCK_SCRIPT: &str = r#"
import sys, json, os
state_path = os.environ.get("SCHEMA_STATE", "")
count = 0
if state_path:
    try:
        with open(state_path) as f:
            count = int(f.read().strip() or "0")
    except Exception:
        count = 0
count += 1
if state_path:
    try:
        with open(state_path, "w") as f:
            f.write(str(count))
    except Exception:
        pass

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
            "result": {
                "tools": [{
                    "name": "echo",
                    "description": "echo tool",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "version": {"type": "string", "const": "v%d" % count},
                            "msg": {"type": "string"}
                        }
                    }
                }]
            }
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

/// Same manifest fixture shape as `dispatch_snapshot_gate.rs`.
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

fn python_cmd() -> Option<&'static str> {
    ["python", "python3"]
        .into_iter()
        .find(|candidate| {
            std::process::Command::new(candidate)
                .arg("--version")
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        })
}

/// Kernel + temp skills dir + a User Skill bound to `tool_name` on
/// `server_id`, plus a trusted+enabled python-mock server record whose env
/// points `SCHEMA_STATE` at `state_file`.
fn open_kernel_with_mock_server(
    skill_id: &str,
    server_id: &str,
    tool_name: &str,
    state_file: &std::path::Path,
) -> (TrustKernel, tempfile::TempDir) {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir.clone())
        .expect("open in-memory kernel with temporary user Skills directory");
    let skill_md = skills_dir.join(format!("{skill_id}/SKILL.md"));
    std::fs::create_dir_all(skill_md.parent().expect("skill dir parent")).expect("create skill dir");
    fs::write(
        skill_md,
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
                    name: format!("Baseline {server_id}"),
                    version: "1.0.0".to_string(),
                    transport: "stdio".to_string(),
                    enabled: true,
                    trusted: true,
                    protocol_version: Some("2025-11-25".to_string()),
                    allowed_origins: None,
                    allowed_paths: None,
                    command: Some(python_cmd().expect("python interpreter on PATH").to_string()),
                    args: Some(serde_json::json!(["-c", MOCK_SCRIPT]).to_string()),
                    env: Some(
                        serde_json::json!({ "SCHEMA_STATE": state_file.to_string_lossy() })
                            .to_string(),
                    ),
                },
            )
            .expect("create trusted enabled python mock MCP server");
    }
    kernel.load_user_skills().expect("load user Skills");
    (kernel, temp_root)
}

#[test]
fn mcp_tool_schema_change_rejected_on_second_call() {
    if python_cmd().is_none() {
        eprintln!("skipping mcp_tool_schema_change_rejected_on_second_call: python not on PATH");
        return;
    }

    let skill_id = format!("baseline.skill.{}", Uuid::new_v4());
    let server_id = format!("baseline-server-{}", Uuid::new_v4());
    let temp_root = tempfile::tempdir().expect("create temporary root");
    let state_file = temp_root.path().join("schema_state.txt");
    let (kernel, _guard) =
        open_kernel_with_mock_server(&skill_id, &server_id, "echo", &state_file);

    // The McpTool dispatch arm emits the taint_propagated audit event on
    // success; audit_logs.step_id REFERENCES steps(step_id), so create the
    // task + step rows up front (the MCP arm itself creates neither).
    kernel
        .create_task("task-schema-1", "schema baseline test")
        .expect("create task");
    kernel
        .create_step(&StepRecord::new("step-schema-1", "task-schema-1", 1))
        .expect("create step");

    // First call: preflight records the v1 baseline, tools/call echoes back.
    let first = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"msg": "hello"}),
        &AutoApprover,
        "task-schema-1",
        "step-schema-1",
    )
    .expect("first dispatch must pass the schema pre-check and invoke the tool");
    assert!(first.succeeded, "first dispatch must succeed");
    assert_eq!(first.output, serde_json::json!({"echo": "hello"}));

    // Second call: the mock now advertises a changed inputSchema (v2 vs the
    // recorded v1) → session baseline mismatch → rejected BEFORE tools/call.
    let err = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"msg": "again"}),
        &AutoApprover,
        "task-schema-1",
        "step-schema-1",
    )
    .expect_err("schema change between calls must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("schema changed"),
        "error must say the schema changed, got: {msg}"
    );
    assert!(
        msg.to_ascii_lowercase().contains("re-plan"),
        "error must tell the user to re-plan/re-approve, got: {msg}"
    );
}

/// Wave 2 Task 2.3: a schema-change rejection is NOT permanent — an explicit
/// server toggle (config change = re-approval boundary) clears the session
/// baseline so the next dispatch re-records instead of failing forever.
#[test]
fn mcp_tool_schema_baseline_resets_on_server_toggle() {
    if python_cmd().is_none() {
        eprintln!("skipping mcp_tool_schema_baseline_resets_on_server_toggle: python not on PATH");
        return;
    }

    let skill_id = format!("baseline.reset.{}", Uuid::new_v4());
    let server_id = format!("baseline-reset-server-{}", Uuid::new_v4());
    let temp_root = tempfile::tempdir().expect("create temporary root");
    let state_file = temp_root.path().join("reset_state.txt");
    let (kernel, _guard) =
        open_kernel_with_mock_server(&skill_id, &server_id, "echo", &state_file);
    kernel
        .create_task("task-reset-1", "baseline reset test")
        .expect("create task");
    kernel
        .create_step(&StepRecord::new("step-reset-1", "task-reset-1", 1))
        .expect("create step");

    // First dispatch records the v1 baseline and succeeds.
    dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"msg": "one"}),
        &AutoApprover,
        "task-reset-1",
        "step-reset-1",
    )
    .expect("first dispatch must succeed");

    // Second dispatch sees a changed schema (v2) → rejected.
    let err = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"msg": "two"}),
        &AutoApprover,
        "task-reset-1",
        "step-reset-1",
    )
    .expect_err("schema change between calls must be rejected");
    assert!(
        err.to_string().contains("schema changed"),
        "second dispatch must be rejected for the schema change, got: {err}"
    );

    // Toggling the server is an explicit re-approval boundary: it clears the
    // stale baseline, so the next dispatch re-records and succeeds.
    kernel
        .toggle_mcp_server(&server_id, false)
        .expect("disable server clears baseline");
    kernel
        .toggle_mcp_server(&server_id, true)
        .expect("re-enable server");

    dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"msg": "three"}),
        &AutoApprover,
        "task-reset-1",
        "step-reset-1",
    )
    .expect("after a toggle the baseline is re-established and the dispatch succeeds");
}

#[test]
fn mcp_tool_missing_from_tools_list_rejected_before_call() {
    if python_cmd().is_none() {
        eprintln!("skipping mcp_tool_missing_from_tools_list_rejected_before_call: python not on PATH");
        return;
    }

    let skill_id = format!("baseline.ghost.{}", Uuid::new_v4());
    let server_id = format!("baseline-ghost-server-{}", Uuid::new_v4());
    let temp_root = tempfile::tempdir().expect("create temporary root");
    let state_file = temp_root.path().join("ghost_state.txt");
    // The mock only advertises `echo`; the Skill is bound to `ghost.tool`.
    let (kernel, _guard) =
        open_kernel_with_mock_server(&skill_id, &server_id, "ghost.tool", &state_file);

    let err = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"msg": "hi"}),
        &AutoApprover,
        "task-missing-tool",
        "step-missing-tool",
    )
    .expect_err("a tool missing from tools/list must be rejected before tools/call");
    let msg = err.to_string();
    assert!(
        msg.contains("not advertised"),
        "error must say the tool is not advertised, got: {msg}"
    );
    assert!(
        msg.to_ascii_lowercase().contains("re-plan"),
        "error must tell the user to re-plan/re-approve, got: {msg}"
    );
    assert!(
        !msg.contains("was not found"),
        "must not be a catalog miss: {msg}"
    );
    assert!(
        !msg.contains("missing command"),
        "must not be the MCP-chain spawn error: {msg}"
    );
}
