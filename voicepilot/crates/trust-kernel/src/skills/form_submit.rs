//! form.submit Skill executor — W8 Plan 3 Task 5.
//!
//! 通过 Playwright MCP navigate + click 提交表单。Risk E3(提交不可逆),
//! approval PerStep — approver 看到 EffectManifest(url + submit_selector)
//! 必须 Allow 才执行。无补偿(CompensationLevel::None)。
//!
//! Pipeline:
//!   1. Validate `url`(http/https)+ `submit_selector`(非空)。
//!   2. validate_input_against_manifest(url + submit_selector)。
//!   3. Create new task + step.
//!   4. Build EffectManifest(`mcp:form_submit:{url}:{submit_selector}`)。
//!   5. Update step → Running.
//!   6. record_approval_decision(E3 + D2 + PerStep + Single)。
//!      Branch on Allow/Deny/Modify:
//!        - Allow → proceed to MCP calls
//!        - Deny  → step Cancelled + Err("user denied form_submit")
//!        - Modify → step Cancelled + Err("modify not supported for form_submit")
//!   7. invoke_mcp_tool(navigate, {url}) — navigate to URL。
//!   8. invoke_mcp_tool(click, {selector: submit_selector}) — click submit。
//!   9. finalize_step_success(evidence="weak", compensation_ref=None)。
//!
//! 关键约束(spec §2.4):
//! - risk_ceiling = E3(提交不可逆,与 form.prepare 的 E2 区分)
//! - compensation.level = None(不可逆,与决策 #4 一致)
//! - approval.mode = PerStep(强制每步审批)
//! - verifier.strategy = Weak(浏览器无文件 evidence)
//!
//! Error handling:
//! - Empty url → Err(KernelError::Skill("validation failed for url: must not be empty"))
//! - Empty submit_selector → 用 manifest default "button[type=submit]"(不报错)
//! - manifest validation failure → propagated as-is
//! - User Deny → step Cancelled + Err(KernelError::Skill("user denied form_submit"))
//! - User Modify → step Cancelled + Err(KernelError::Skill("modify not supported for form_submit"))
//! - MCP failure on navigate/click → step Failed + Err(KernelError::Mcp(...))
//! - finalization failure → step Failed + Err propagated

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    finalize_step_success, invoke_mcp_tool, record_approval_decision,
    validate_input_against_manifest, ApprovalContext,
};
use crate::skills::manifest::form_submit_manifest;
use crate::skills::verifiers::{verify_form_submit, VerificationContext, VerificationOutcome};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Input for the `form.submit` Skill executor.
#[derive(Debug, Clone)]
pub struct FormSubmitInput {
    /// New task ID for this form_submit operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// URL to navigate to (must be http/https).
    pub url: String,
    /// CSS selector for the submit button. If empty, defaults to "button[type=submit]".
    pub submit_selector: String,
}

/// Execute the `form.submit` Skill.
///
/// Drives Playwright MCP to navigate to a URL and click the submit button.
/// Risk E3 + PerStep approval: the approver sees an EffectManifest describing
/// the URL + submit_selector and must Allow before any MCP call. **No
/// compensation**(提交不可逆,决策 #4)。
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (MCP error, finalization failure), returns Err and
/// marks the step Failed.
pub fn execute_form_submit(
    kernel: &TrustKernel,
    input: &FormSubmitInput,
    approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate input — reject empty url explicitly. Empty
    // submit_selector falls back to the manifest default ("button[type=submit]").
    let submit_selector = if input.submit_selector.trim().is_empty() {
        "button[type=submit]".to_string()
    } else {
        input.submit_selector.trim().to_string()
    };
    if input.url.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for url: must not be empty".to_string(),
        ));
    }

    // Step 2: validate the full input map against the manifest. Catches
    // url ∉ http(s)://... before any DB write.
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("url".to_string(), serde_json::json!(input.url));
    input_map.insert(
        "submit_selector".to_string(),
        serde_json::json!(submit_selector),
    );
    validate_input_against_manifest(&input_map, &form_submit_manifest())?;

    // Step 3: create new task + step.
    kernel.create_task(&input.task_id, &format!("form_submit:{}", input.url))?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 4: build EffectManifest. No file sources (web-only flow).
    // destination carries the url + submit_selector so the approval record
    // has a non-empty target.
    let effect_manifest = build_form_submit_effect_manifest(&input.url, &submit_selector);

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E3 + D2 + PerStep). The
    // preconditions_hash binds this approval to the exact {url, submit_selector}
    // pair so post-hoc audit can verify what the user actually approved.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.url.as_bytes());
        hasher.update(b"\x00");
        hasher.update(submit_selector.as_bytes());
        hasher.update(b"\x00");
        format!("{:x}", hasher.finalize())
    };
    let ctx = ApprovalContext {
        task_id: &input.task_id,
        step_id: &input.step_id,
        destination: &effect_manifest.destination,
        preconditions_hash: &preconditions_hash,
        e_level: ELevel::E3,
        d_level: DLevel::D2,
        approval_scope: ApprovalScope::Single,
    };
    let approval = record_approval_decision(kernel, approver, &effect_manifest, &ctx)?;

    // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
    match approval.user_decision {
        ApprovalDecision::Deny => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill("user denied form_submit".to_string()));
        }
        ApprovalDecision::Modify => {
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            return Err(KernelError::Skill(
                "modify not supported for form_submit".to_string(),
            ));
        }
        ApprovalDecision::Allow => { /* proceed to MCP calls */ }
    }

    // Step 7: invoke_mcp_tool(navigate, {url}) — navigate to the URL.
    // On MCP failure, mark step Failed and propagate the error.
    invoke_mcp_tool(
        kernel,
        "playwright",
        "navigate",
        serde_json::json!({"url": input.url}),
    )
    .inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 8: invoke_mcp_tool(click, {selector: submit_selector}) — click submit.
    invoke_mcp_tool(
        kernel,
        "playwright",
        "click",
        serde_json::json!({"selector": submit_selector}),
    )
    .inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 9: W10 Plan 1 — 调用真实 verify_form_submit 通过 Playwright eval
    // 查 document.URL 变更 或 success 元素存在,验证提交确实发生(spec §6.3 Strong Verifier)。
    // 提交不可逆,verifier 必须验证副作用真实发生。通过 → Strong;失败 → step Failed + 返回错误。
    let verify_ctx = VerificationContext {
        kernel,
        step_id: &input.step_id,
    };
    let outcome = verify_form_submit(&verify_ctx, &input.url)?;
    match outcome {
        VerificationOutcome::Strong { .. } => {
            finalize_step_success(kernel, &input.step_id, "strong", None).inspect_err(|_e| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            })?;
        }
        VerificationOutcome::Failed { reason } => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_form_submit failed: {}",
                reason
            )));
        }
        _ => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_form_submit returned unexpected outcome: {:?}",
                outcome
            )));
        }
    }

    Ok(input.task_id.clone())
}

/// Build a descriptive EffectManifest for the approval prompt.
fn build_form_submit_effect_manifest(url: &str, submit_selector: &str) -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: format!("mcp:form_submit:{}:{}", url, submit_selector),
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
    use crate::repo::step_repo::StepStatus;
    use std::process::Command;
    use std::sync::Mutex;

    // Serialize tests that mutate CWD via a global mutex (mirrors form_prepare.rs).
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    fn python_available() -> bool {
        Command::new("python")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Override the default playwright MCP record with a Python mock that
    /// records every tools/call name to a JSON file at
    /// `$FORM_SUBMIT_CALLS_PATH` and returns canned navigate/click responses.
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

    /// Mock script: records every tools/call `name` to a JSON file whose
    /// path is taken from the FORM_SUBMIT_CALLS_PATH env var. Returns
    /// canned navigate/click responses.
    const MOCK_SCRIPT: &str = r#"
import sys, json, os
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
calls_path = os.environ.get("FORM_SUBMIT_CALLS_PATH")
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
        elif name == "click":
            text_payload = json.dumps({"clicked": True})
        elif name == "eval":
            # W10 Plan 1: verify_form_submit 通过 eval 查 URL 变更 / success 元素。
            # 默认 current_url = .../success(与测试 input.url .../com 不同)→ url_changed → Strong。
            # 测试可通过 MOCK_CURRENT_URL / MOCK_HAS_SUCCESS 环境变量覆盖。
            current_url = os.environ.get("MOCK_CURRENT_URL", "https://example.com/success")
            has_success = os.environ.get("MOCK_HAS_SUCCESS", "false") == "true"
            text_payload = json.dumps({"url": current_url, "success": has_success})
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

    fn make_input(url: &str, submit_selector: &str) -> FormSubmitInput {
        FormSubmitInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: url.to_string(),
            submit_selector: submit_selector.to_string(),
        }
    }

    fn set_calls_env(temp_root: &std::path::Path) -> std::path::PathBuf {
        let path = temp_root.join(format!("calls-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, "[]").expect("seed calls file");
        // SAFETY: tests guarded by CWD_MUTEX serialize env mutations process-wide.
        unsafe { std::env::set_var("FORM_SUBMIT_CALLS_PATH", &path); }
        path
    }

    fn clear_calls_env() {
        // SAFETY: see set_calls_env.
        unsafe { std::env::remove_var("FORM_SUBMIT_CALLS_PATH"); }
    }

    #[test]
    fn test_form_submit_success_navigate_and_click() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let calls_path = set_calls_env(temp.path());

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoApprover;
        let input = make_input("https://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "t1");

        // Step is Succeeded with strong evidence (W10 Plan 1: verify_form_submit 通过), no compensation.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
        assert!(
            step.compensation_ref.is_none(),
            "form.submit must have no compensation"
        );

        // Approval was recorded (E3 + PerStep).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].e_level, ELevel::E3);

        // CRITICAL: navigate + click + eval were called (in order).
        // W10 Plan 1: verify_form_submit 追加 eval 调用查 URL 变更。
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        assert_eq!(
            recorded,
            vec![
                "navigate".to_string(),
                "click".to_string(),
                "eval".to_string(),
            ],
            "expected navigate then click then eval, got {:?}",
            recorded
        );

        clear_calls_env();
    }

    #[test]
    fn test_form_submit_user_denies_cancels_step() {
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
        let input = make_input("https://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);

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
    fn test_form_submit_empty_url_fails_validation() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = make_input("", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(err.to_string().contains("url"));
        assert!(err.to_string().contains("empty"));

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_form_submit_invalid_url_fails_manifest_validation() {
        // url="ftp://example.com" → manifest Url validation rejects (not http/https).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = make_input("ftp://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(err.to_string().contains("url"));

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_form_submit_empty_submit_selector_uses_default() {
        // submit_selector="" → 用 manifest default "button[type=submit]"
        // 验证:不报错,且 click 调用的 selector 是 default 值
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        let calls_path = set_calls_env(temp.path());

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_python_mock(&kernel, MOCK_SCRIPT);
        let approver = AutoApprover;
        let input = make_input("https://example.com", ""); // 空提交选择器
        let result = execute_form_submit(&kernel, &input, &approver);

        assert!(
            result.is_ok(),
            "expected Ok with default selector, got {:?}",
            result.err()
        );

        // 验证 click 被调用(selector 在 mock 中不记录参数,但调用次数 = 1)
        let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        assert!(
            recorded.contains(&"click".to_string()),
            "click must be called with default selector"
        );

        clear_calls_env();
    }

    #[test]
    fn test_form_submit_mcp_failure_marks_step_failed() {
        // Override playwright record with a guaranteed-to-fail command.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .unwrap()
            .unwrap();
        rec.command = Some("this-command-does-not-exist-12345".to_string());
        rec.args = Some("[]".to_string());
        McpServerRepo::new().update(&kernel.conn(), &rec).unwrap();

        let approver = AutoApprover;
        let input = make_input("https://example.com", "button[type=submit]");
        let result = execute_form_submit(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Mcp(_)),
            "expected KernelError::Mcp, got {:?}",
            err
        );

        // Step is Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);
    }
}
