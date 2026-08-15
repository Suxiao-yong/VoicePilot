//! form.prepare Skill executor — W7 Plan 5 Task 5.
//!
//! Navigates to a URL via Playwright MCP, snapshots the page, and fills
//! a caller-supplied set of (selector, value) pairs into the form — but
//! **does not** click submit. Risk E2 (local-to-web draft flow), approval
//! PerStep — the approver sees an EffectManifest describing the planned
//! URL + field count and must Allow before any MCP call.
//!
//! Pipeline:
//!   1. Validate `url` (http/https) + `fields` (non-empty HashMap).
//!   2. Validate the full input map against `form_prepare_manifest`.
//!      The manifest's `fields` SkillInput is `Text` (the SkillInputType
//!      enum has no Object variant) with `max_length: 5000`; the executor
//!      passes the JSON-encoded fields map as a string so the manifest's
//!      Text validation applies cleanly.
//!   3. Create new task + step.
//!   4. Build EffectManifest (`sources: vec![]`,
//!      `destination: format!("mcp:form_prepare:{}:{}", url, fields.len())`,
//!      `total_bytes: 0` — unknown until fills complete).
//!   5. Update step → Running.
//!   6. Record approval decision (E2 + D2 + Single). Branch on Allow/Deny.
//!   7. invoke_mcp_tool(navigate, {url}) → navigate to URL.
//!   8. invoke_mcp_tool(snapshot, {}) → establish browser session state.
//!   9. For each (selector, value) in `fields`, call invoke_mcp_tool(fill,
//!      {selector, value}).
//!  10. Finalize step as Succeeded with Weak evidence (no file artifact —
//!      only web state change) and no compensation_ref.
//!
//! Critical invariant: the executor MUST NOT call `playwright.click`.
//! The acceptance gate (Plan 5 §"Acceptance Gates") asserts this in the
//! success unit test by reading the recorded tool-call list from the
//! mock MCP server.
//!
//! Error handling:
//! - Empty url → Err(KernelError::Skill("validation failed for url: must not be empty")).
//! - Empty fields → Err(KernelError::Skill("validation failed for fields: must not be empty")).
//! - manifest validation failure → propagated as-is.
//! - User Deny → step Cancelled + Err(KernelError::Skill("user denied form_prepare")).
//! - User Modify → step Cancelled + Err(KernelError::Skill("modify not supported for form_prepare")).
//! - MCP failure on navigate/snapshot/fill → step Failed + Err(KernelError::Mcp(...)).
//! - finalization failure → step Failed + Err propagated.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    create_post_commit_compensation_with_payload, finalize_step_success, invoke_mcp_tool,
    record_approval_decision, validate_input_against_manifest, ApprovalContext,
};
use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::skills::manifest::form_prepare_manifest;
use crate::skills::verifiers::{verify_form_prepare, VerificationContext, VerificationOutcome};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// Input for the `form.prepare` Skill executor.
#[derive(Debug, Clone)]
pub struct FormPrepareInput {
    /// New task ID for this form_prepare operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// URL to navigate to (must be http/https).
    pub url: String,
    /// Map of CSS selector → value to fill. Must be non-empty.
    pub fields: HashMap<String, String>,
}

/// Execute the `form.prepare` Skill.
///
/// Drives Playwright MCP to navigate to a URL, snapshot the page, and fill
/// each (selector, value) pair. Risk E2 + PerStep approval: the approver
/// sees an EffectManifest describing the URL + field count and must Allow
/// before any MCP call. **Does not** click submit.
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (MCP error, finalization failure), returns Err and
/// marks the step Failed.
pub fn execute_form_prepare(
    kernel: &TrustKernel,
    input: &FormPrepareInput,
    approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate input — reject empty url / fields explicitly.
    if input.url.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for url: must not be empty".to_string(),
        ));
    }
    if input.fields.is_empty() {
        return Err(KernelError::Skill(
            "validation failed for fields: must not be empty".to_string(),
        ));
    }

    // Step 2: validate the full input map against the manifest. Catches
    // url ∉ http(s)://... before any DB write. The manifest's `fields`
    // input is `Text` with `max_length: 5000` (SkillInputType has no
    // Object variant) — pass the JSON-encoded fields map as a string so
    // the manifest's Text + max_length validation applies cleanly.
    let fields_json = serde_json::to_string(&input.fields).map_err(|e| {
        KernelError::Skill(format!("failed to serialize fields: {e}"))
    })?;
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("url".to_string(), serde_json::json!(input.url));
    input_map.insert("fields".to_string(), serde_json::json!(fields_json));
    validate_input_against_manifest(&input_map, &form_prepare_manifest())?;

    // Step 3: create new task + step.
    kernel.create_task(
        &input.task_id,
        &format!("form_prepare:{}", input.url),
    )?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 4: build EffectManifest. No file sources (web-only flow).
    // destination carries the url + field count so the approval record
    // has a non-empty target. total_bytes is 0 (unknown until fills
    // complete). Mirrors research_save.rs.
    let effect_manifest = build_form_prepare_effect_manifest(&input.url, input.fields.len());

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E2 + D2 + PerStep). The
    // preconditions_hash binds this approval to the exact {url, fields}
    // pair so post-hoc audit can verify what the user actually approved.
    // Use BTreeMap for deterministic iteration over the fields map.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.url.as_bytes());
        hasher.update(b"\x00");
        let sorted: BTreeMap<&String, &String> = input.fields.iter().collect();
        for (k, v) in sorted {
            hasher.update(k.as_bytes());
            hasher.update(b"\x00");
            hasher.update(v.as_bytes());
            hasher.update(b"\x00");
        }
        format!("{:x}", hasher.finalize())
    };
    let ctx = ApprovalContext {
        task_id: &input.task_id,
        step_id: &input.step_id,
        destination: &effect_manifest.destination,
        preconditions_hash: &preconditions_hash,
        e_level: ELevel::E2,
        d_level: DLevel::D2,
        approval_scope: ApprovalScope::Single,
    };
    let approval = record_approval_decision(kernel, approver, &effect_manifest, &ctx)?;

    // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
    match approval.user_decision {
        ApprovalDecision::Deny => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill("user denied form_prepare".to_string()));
        }
        ApprovalDecision::Modify => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill(
                "modify not supported for form_prepare".to_string(),
            ));
        }
        ApprovalDecision::Allow => { /* proceed to MCP calls */ }
    }

    // Step 7: invoke_mcp_tool(navigate, {url}) — navigate to the URL.
    // On MCP failure, mark step Failed and propagate the error so the
    // SkillRouter can map it to error_code = "mcp_playwright_unavailable".
    invoke_mcp_tool(kernel, "playwright", "navigate", serde_json::json!({"url": input.url}))
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 8: invoke_mcp_tool(snapshot, {}) — establish browser session
    // state. Required by spec §2.7 to establish browser session state
    // before fills. The returned tree is unused in this minimal
    // implementation but the call must succeed.
    invoke_mcp_tool(kernel, "playwright", "snapshot", serde_json::json!({}))
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 9: for each (selector, value) in `fields`, call fill. Use
    // BTreeMap for deterministic iteration order so test assertions on
    // the recorded call sequence are stable. CRITICAL: do NOT call
    // playwright.click anywhere in this loop — the acceptance gate
    // asserts this.
    let sorted_fields: BTreeMap<&String, &String> = input.fields.iter().collect();
    for (selector, value) in sorted_fields {
        invoke_mcp_tool(
            kernel,
            "playwright",
            "fill",
            serde_json::json!({"selector": selector, "value": value}),
        )
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;
    }

    // Step 10: W10 Plan 1 — 调用真实 verify_form_prepare 通过 Playwright eval
    // 重查每个 selector 的当前值,与 input.fields 比对(spec §6.3 Strong Verifier)。
    // 全部匹配 → Strong evidence;任一不匹配 → step Failed + 返回错误(说明 fill
    // 阶段未真正写入,commit 阶段出错)。
    let verify_ctx = VerificationContext {
        kernel,
        step_id: &input.step_id,
    };
    let outcome = verify_form_prepare(&verify_ctx, &input.fields)?;
    match outcome {
        VerificationOutcome::Strong { .. } => { /* proceed to register compensation */ }
        VerificationOutcome::Failed { reason } => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_form_prepare failed: {}",
                reason
            )));
        }
        _ => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_form_prepare returned unexpected outcome: {:?}",
                outcome
            )));
        }
    }

    // W10 Plan 2: 注册 form.reverse_prepare compensation,payload 含 fields 映射。
    // reverse_form_prepare 对每个 selector 调用 Playwright eval 清空 value。
    let comp_ref = create_post_commit_compensation_with_payload(
        kernel,
        &input.step_id,
        "form.reverse_prepare",
        serde_json::json!({"fields": &input.fields}).to_string(),
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    finalize_step_success(kernel, &input.step_id, "strong", Some(&comp_ref)).inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    Ok(input.task_id.clone())
}

/// Build a descriptive EffectManifest for the approval prompt.
fn build_form_prepare_effect_manifest(url: &str, field_count: usize) -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: format!("mcp:form_prepare:{}:{}", url, field_count),
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::kernel::TrustKernel;
    use crate::mcp::repo::McpServerRepo;
    use std::process::Command;
    use std::sync::Mutex;

    // Serialize tests that mutate CWD via a global mutex. CWD is
    // process-global, so parallel test threads racing on
    // set_current_dir would corrupt each other's file writes.
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    fn python_available() -> bool {
        Command::new("python")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Override the default playwright MCP record (inserted by kernel boot)
    /// with a Python mock that records every tools/call name to a JSON file
    /// at `$FORM_PREPARE_CALLS_PATH` and returns canned navigate/snapshot/
    /// fill responses.
    fn install_python_mock(kernel: &TrustKernel, mock_script: &str) {
        let args_json = serde_json::to_string(&vec!["-c".to_string(), mock_script.to_string()])
            .expect("serialize args");
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .expect("playwright row must exist (kernel boot seeds it)")
            .expect("playwright row must exist");
        rec.command = Some("python".to_string());
        rec.args = Some(args_json);
        rec.env = Some("{}".to_string());
        McpServerRepo::new()
            .update(&kernel.conn(), &rec)
            .expect("update mock playwright record");
    }

    /// Mock script that records every tools/call `name` to a JSON file
    /// whose path is taken from the FORM_PREPARE_CALLS_PATH env var. The
    /// test reads this file after the executor returns to assert that
    /// `click` was never called.
    ///
    /// The script reads the env var on each invocation (cheap) so that
    /// each spawned subprocess picks up the path the test set before
    /// calling the executor.
    const MOCK_SCRIPT: &str = r#"
import sys, json, os
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
calls_path = os.environ.get("FORM_PREPARE_CALLS_PATH")
calls = []
if calls_path and os.path.exists(calls_path):
    try:
        with open(calls_path, "r", encoding="utf-8") as f:
            calls = json.load(f)
    except Exception:
        calls = []
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
                "serverInfo": {"name": "mock-playwright", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        calls.append(name)
        if calls_path:
            try:
                with open(calls_path, "w", encoding="utf-8") as f:
                    json.dump(calls, f)
            except Exception:
                pass
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "snapshot":
            text_payload = json.dumps({"tree": "form"})
        elif name == "fill":
            text_payload = json.dumps({"filled": True})
        elif name == "eval":
            # W10 Plan 1: verify_form_prepare 通过 eval 重查字段值。
            # 从 FORM_PREPARE_VALUES_PATH 读取预存 JSON {selector: value} 返回。
            # 用独立 env var 名(不与 verifiers::form_prepare_tests 的
            # FORM_VALUES_PATH 冲突),避免并行测试相互覆盖 env var。
            values_path = os.environ.get("FORM_PREPARE_VALUES_PATH")
            values = {}
            if values_path and os.path.exists(values_path):
                try:
                    with open(values_path, "r", encoding="utf-8") as f:
                        values = json.load(f)
                except Exception:
                    values = {}
            text_payload = json.dumps(values)
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": f"unknown tool {name}"}
            })
            continue
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
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

    fn make_input(fields: HashMap<String, String>) -> FormPrepareInput {
        FormPrepareInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "https://example.com".to_string(),
            fields,
        }
    }

    fn sample_fields() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("#username".to_string(), "alice".to_string());
        m.insert("#email".to_string(), "alice@example.com".to_string());
        m.insert("#phone".to_string(), "555-1234".to_string());
        m
    }

    /// Set FORM_PREPARE_CALLS_PATH to a temp file path inside the test's
    /// tempdir so the mock can write to it. Returns the path.
    fn set_calls_env(temp_root: &std::path::Path) -> std::path::PathBuf {
        let path = temp_root.join(format!("calls-{}.json", uuid::Uuid::new_v4()));
        // Pre-create an empty list so the mock's "file exists" branch
        // triggers on the first call.
        std::fs::write(&path, "[]").expect("seed calls file");
        // SAFETY: tests guarded by CWD_MUTEX serialize env mutations
        // process-wide; no other test thread is reading this var.
        unsafe { std::env::set_var("FORM_PREPARE_CALLS_PATH", &path); }
        path
    }

    fn clear_calls_env() {
        // SAFETY: see set_calls_env.
        unsafe { std::env::remove_var("FORM_PREPARE_CALLS_PATH"); }
    }

    #[test]
    fn test_form_prepare_success_fills_all_fields() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = temp.path().to_path_buf();
        let calls_path = set_calls_env(&temp_root);

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoApprover;
        let input = make_input(sample_fields());
        let fields_count = input.fields.len();

        // W10 Plan 1: verify_form_prepare 通过 eval 重查字段值,mock 从
        // FORM_PREPARE_VALUES_PATH 读取预存 JSON。写入与 input.fields 一致的值 → Strong。
        let values_path = temp_root.join(format!("values-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&values_path, serde_json::to_string(&input.fields).unwrap()).unwrap();
        // SAFETY: tests guarded by CWD_MUTEX serialize env mutations process-wide.
        unsafe { std::env::set_var("FORM_PREPARE_VALUES_PATH", &values_path); }

        let result = execute_form_prepare(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "t1");

        // Step is Succeeded with strong evidence (W10 Plan 1: verify_form_prepare 通过)。
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
        // W10 Plan 2: compensation_ref 必须指向 form.reverse_prepare 记录。
        let comp_ref = step.compensation_ref.as_ref().expect("compensation_ref must be set");
        let comp = kernel.get_compensation(comp_ref).unwrap().unwrap();
        assert_eq!(comp.compensate_fn, "form.reverse_prepare");
        assert_eq!(comp.level, CompensationLevel::Strong);

        // Approval was recorded (PerStep).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].e_level, ELevel::E2);

        // CRITICAL acceptance gate: assert that the mock MCP server
        // received navigate + snapshot + fill × N calls and NO click.
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        assert!(
            recorded.contains(&"navigate".to_string()),
            "expected navigate in recorded calls, got {:?}",
            recorded
        );
        assert!(
            recorded.contains(&"snapshot".to_string()),
            "expected snapshot in recorded calls, got {:?}",
            recorded
        );
        let fill_count = recorded.iter().filter(|n| *n == "fill").count();
        assert_eq!(
            fill_count,
            fields_count,
            "expected {} fill calls, got {} (full list: {:?})",
            fields_count,
            fill_count,
            recorded
        );
        // Acceptance gate: click MUST NOT be called.
        assert!(
            !recorded.iter().any(|n| n == "click"),
            "playwright.click was called — form.prepare MUST NOT click submit. recorded: {:?}",
            recorded
        );

        clear_calls_env();
        // SAFETY: see set_calls_env.
        unsafe { std::env::remove_var("FORM_PREPARE_VALUES_PATH"); }
    }

    #[test]
    fn test_form_prepare_user_denies_cancels_step() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let calls_path = set_calls_env(temp.path());

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoDenier;
        let input = make_input(sample_fields());
        let result = execute_form_prepare(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(ref m) if m.contains("user denied")),
            "expected Skill 'user denied' error, got {:?}",
            err
        );

        // Step is Cancelled, not Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Cancelled);

        // Approval was still recorded (user saw the prompt).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);

        // No MCP calls were made (denial short-circuits before navigate).
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        assert!(
            recorded.is_empty(),
            "expected no MCP calls after denial, got {:?}",
            recorded
        );

        clear_calls_env();
    }

    #[test]
    fn test_form_prepare_invalid_url_fails_validation() {
        // url="ftp://example.com" → manifest validation rejects (not http/https).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = FormPrepareInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "ftp://example.com".to_string(),
            fields: sample_fields(),
        };
        let result = execute_form_prepare(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("url"),
            "expected 'url' in error, got: {}",
            err
        );

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_form_prepare_empty_fields_fails_validation() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = FormPrepareInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "https://example.com".to_string(),
            fields: HashMap::new(),
        };
        let result = execute_form_prepare(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("fields"),
            "expected 'fields' in error, got: {}",
            err
        );
        assert!(
            err.to_string().contains("empty"),
            "expected 'empty' in error, got: {}",
            err
        );

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_form_prepare_mcp_failure_marks_step_failed() {
        // Override playwright record with a guaranteed-to-fail command —
        // spawn will fail with KernelError::Mcp(...), executor marks step
        // Failed.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .unwrap()
            .unwrap();
        rec.command = Some("this-command-does-not-exist-12345".to_string());
        rec.args = Some("[]".to_string());
        McpServerRepo::new().update(&kernel.conn(), &rec).unwrap();

        let approver = AutoApprover;
        let input = FormPrepareInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "https://example.com".to_string(),
            fields: sample_fields(),
        };
        let result = execute_form_prepare(&kernel, &input, &approver);
        let err = result.unwrap_err();
        // MCP spawn failure maps to KernelError::Mcp.
        assert!(
            matches!(err, KernelError::Mcp(_)),
            "expected KernelError::Mcp, got {:?}",
            err
        );

        // Step is Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);
    }

    #[test]
    fn test_form_prepare_empty_url_fails_validation() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = FormPrepareInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "".to_string(),
            fields: sample_fields(),
        };
        let result = execute_form_prepare(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("url"),
            "expected 'url' in error, got: {}",
            err
        );
        assert!(
            err.to_string().contains("empty"),
            "expected 'empty' in error, got: {}",
            err
        );

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }
}
