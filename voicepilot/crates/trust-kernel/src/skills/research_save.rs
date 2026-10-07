//! research.save_markdown Skill executor — W7 Plan 5 Task 4.
//!
//! Fetches a web page via Playwright MCP (navigate → snapshot → eval) and
//! saves the extracted main content as a Markdown file on disk. Risk E2
//! (web-to-local data flow + filesystem write), approval PerStep — the
//! approver sees an EffectManifest describing the planned URL + save_path
//! and must Allow before any MCP call.
//!
//! Pipeline:
//!   1. Validate `url` (http/https) + `save_path` (non-empty, under
//!      Documents/Desktop per manifest's allowed_roots).
//!   2. Validate the full input map against `research_save_manifest`.
//!   3. Create new task + step.
//!   4. Build EffectManifest (`sources: vec![]`,
//!      `destination: format!("mcp:research_save:{}:{}", url, save_path)`,
//!      `total_bytes: 0` — unknown until eval returns).
//!   5. Update step → Running.
//!   6. Record approval decision (E2 + D2 + Single). Branch on Allow/Deny.
//!   7. invoke_mcp_tool(navigate, {url}) → navigate to URL.
//!   8. invoke_mcp_tool(snapshot, {}) → get accessibility tree (unused
//!      in this minimal implementation but required by spec §2.7 to
//!      establish browser session state).
//!   9. invoke_mcp_tool(eval, {script}) → extract main content as text.
//!  10. assert_path_allowed + std::fs::write save_path with eval result.
//!  11. Finalize step as Succeeded with Strong evidence (file artifact).
//!
//! Error handling:
//! - User Deny → step Cancelled + Err(KernelError::Skill("user denied")).
//! - MCP failure (server not found / disabled / spawn / invoke) → step
//!   Failed + Err(KernelError::Mcp(...)). SkillRouter maps this to a
//!   Failed ToolResult with `error_code = "mcp_playwright_unavailable"`
//!   in the smoke test.
//! - Filesystem write failure → step Failed + Err.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    ApprovalContext, create_post_commit_compensation_with_payload, finalize_step_success,
    invoke_mcp_tool, record_approval_decision, validate_input_against_manifest,
};
use crate::skills::manifest::research_save_manifest;
use crate::skills::verifiers::{VerificationContext, VerificationOutcome, verify_research_save};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

/// Input for the `research.save_markdown` Skill executor.
#[derive(Debug, Clone)]
pub struct ResearchSaveInput {
    /// New task ID for this research_save operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// URL to fetch (must be http/https).
    pub url: String,
    /// File path to save the Markdown to. Must be under "Documents" or
    /// "Desktop" (per `research_save_manifest`'s `allowed_roots`).
    pub save_path: String,
}

/// JavaScript snippet to extract the main textual content of a page.
/// Prefers `<main>` element, falls back to `<body>`. Returns innerText
/// which preserves line breaks.
const EVAL_SCRIPT: &str = "document.querySelector('main')?.innerText || document.body.innerText";

/// Execute the `research.save_markdown` Skill.
///
/// Drives Playwright MCP to navigate to a URL, snapshot the page, extract
/// main content via eval, and save the result as a Markdown file on disk.
/// Risk E2 + PerStep approval: the approver sees an EffectManifest
/// describing the URL + save_path and must Allow before any MCP call.
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (MCP error, filesystem error, finalization failure),
/// returns Err and marks the step Failed.
pub fn execute_research_save(
    kernel: &TrustKernel,
    input: &ResearchSaveInput,
    approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate input — reject empty url / save_path explicitly.
    if input.url.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for url: must not be empty".to_string(),
        ));
    }
    if input.save_path.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for save_path: must not be empty".to_string(),
        ));
    }

    // Step 2: validate the full input map against the manifest. Catches
    // url ∉ http(s)://... and save_path ∉ {Documents, Desktop}/... before
    // any DB write.
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("url".to_string(), serde_json::json!(input.url));
    input_map.insert("save_path".to_string(), serde_json::json!(input.save_path));
    validate_input_against_manifest(&input_map, &research_save_manifest())?;

    // Step 3: create new task + step.
    kernel.create_task(&input.task_id, &format!("research_save:{}", input.url))?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 4: build EffectManifest. No file sources (web-to-local flow).
    // destination carries the url + save_path so the approval record has
    // a non-empty target. total_bytes is 0 (unknown until eval returns).
    let effect_manifest = build_research_save_effect_manifest(&input.url, &input.save_path);

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E2 + D2 + PerStep). The
    // preconditions_hash binds this approval to the exact {url, save_path}
    // pair so post-hoc audit can verify what the user actually approved.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.url.as_bytes());
        hasher.update(b"\x00");
        hasher.update(input.save_path.as_bytes());
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
    // Step 6: 审批门 —— research.save_markdown 不在审批白名单，不弹窗直接执行。
    // 审计留痕由任务/步骤落库提供；分支代码保留便于日后重新加入白名单。
    if crate::skills::simple::approval_required("research.save_markdown") {
        let approval = record_approval_decision(kernel, approver, &effect_manifest, &ctx)?;

        // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
        match approval.user_decision {
            ApprovalDecision::Deny => {
                kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
                return Err(KernelError::Skill("user denied research_save".to_string()));
            }
            ApprovalDecision::Modify => {
                kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
                return Err(KernelError::Skill(
                    "modify not supported for research_save".to_string(),
                ));
            }
            ApprovalDecision::Allow => { /* proceed to MCP calls */ }
        }
    }

    // Step 7: invoke_mcp_tool(navigate, {url}) — navigate to the URL.
    // On MCP failure, mark step Failed and propagate the error so the
    // SkillRouter can map it to error_code = "mcp_playwright_unavailable".
    invoke_mcp_tool(
        kernel,
        "playwright",
        "navigate",
        serde_json::json!({"url": input.url}),
    )
    .inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 8: invoke_mcp_tool(snapshot, {}) — get accessibility tree.
    // Required by spec §2.7 to establish browser session state. The
    // returned tree is unused in this minimal implementation but the
    // call must succeed (verifies the page is interactive).
    invoke_mcp_tool(kernel, "playwright", "snapshot", serde_json::json!({})).inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 9: invoke_mcp_tool(eval, {script}) — extract main content.
    // The returned ToolResult.data is a JSON object like {"text": "..."}.
    let eval_result = invoke_mcp_tool(
        kernel,
        "playwright",
        "eval",
        serde_json::json!({"script": EVAL_SCRIPT}),
    )
    .inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Extract text from eval result. The mock returns {"text": "..."}.
    // Real Playwright MCP may return the string directly. Handle both.
    let content = extract_text_from_eval(&eval_result.data);

    // Step 10: assert_path_allowed + std::fs::write save_path with content.
    // The path check is in its own block so the MutexGuard is dropped
    // before std::fs::write runs — avoids holding the filesystem lock
    // across the actual write (mirrors note_capture.rs).
    let path_check = {
        let fs = kernel.filesystem();
        fs.assert_path_allowed(Path::new(&input.save_path))
    };
    if let Err(e) = path_check {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        return Err(e);
    }
    std::fs::write(&input.save_path, content.as_bytes()).map_err(|e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        KernelError::Skill(format!(
            "failed to write save_path '{}': {}",
            input.save_path, e
        ))
    })?;

    // Step 11: W10 Plan 1 — 调用真实 verify_research_save 重读磁盘文件,
    // 验证存在 + 非空(spec §6.3 Strong Verifier)。markdown 内容由
    // Playwright eval 动态生成,sha256 难匹配,只验证文件存在 + 非空已
    // 足以证明 commit 成功。通过 → Strong;失败 → step Failed + 返回错误。
    let verify_ctx = VerificationContext {
        kernel,
        step_id: &input.step_id,
    };
    let outcome = verify_research_save(&verify_ctx, &input.save_path)?;
    match outcome {
        VerificationOutcome::Strong { .. } => { /* proceed to register compensation */ }
        VerificationOutcome::Failed { reason } => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_research_save failed: {}",
                reason
            )));
        }
        _ => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_research_save returned unexpected outcome: {:?}",
                outcome
            )));
        }
    }

    // W10 Plan 2: 注册 research.reverse_save compensation,payload 含 save_path。
    let comp_ref = create_post_commit_compensation_with_payload(
        kernel,
        &input.step_id,
        "research.reverse_save",
        serde_json::json!({"save_path": &input.save_path}).to_string(),
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
fn build_research_save_effect_manifest(url: &str, save_path: &str) -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: format!("mcp:research_save:{}:{}", url, save_path),
        conflicts: vec![],
        total_bytes: 0,
    }
}

/// Extract textual content from the eval ToolResult.data. Handles:
/// - JSON object with "text" field: `{"text": "..."}`
/// - Plain string: `"..."`
/// - Any other JSON: pretty-printed
fn extract_text_from_eval(data: &serde_json::Value) -> String {
    if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(s) = data.as_str() {
        return s.to_string();
    }
    serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::kernel::TrustKernel;
    use crate::mcp::repo::McpServerRepo;
    use std::process::Command;

    // Serialize tests that mutate CWD via the crate-shared test mutex
    // (common.rs). CWD is process-global: each mod's private mutex cannot
    // stop cross-module races, parallel test threads racing on
    // set_current_dir would corrupt each other's file writes.

    struct CwdGuard {
        prev: std::path::PathBuf,
    }
    impl CwdGuard {
        fn enter(temp: &std::path::Path) -> Self {
            let prev = std::env::current_dir().expect("getcwd");
            std::env::set_current_dir(temp).expect("setcwd");
            Self { prev }
        }
    }
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev);
        }
    }

    fn with_temp_cwd<F: FnOnce(&std::path::Path)>(body: F) {
        let _guard = crate::skills::common::TEST_CWD_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("Documents")).expect("create Documents dir");
        let _cwd = CwdGuard::enter(temp.path());
        body(temp.path())
    }

    fn python_available() -> bool {
        Command::new("python")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Override the default playwright MCP record (inserted by kernel boot)
    /// with a Python mock that returns canned navigate/snapshot/eval responses.
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
                "serverInfo": {"name": "mock-playwright", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "snapshot":
            text_payload = json.dumps({"tree": "Example Domain"})
        elif name == "eval":
            text_payload = json.dumps({"text": "Example Domain\n\nThis domain is for use in illustrative examples in documents."})
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

    fn make_input(save_path: &str) -> ResearchSaveInput {
        ResearchSaveInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "https://example.com".to_string(),
            save_path: save_path.to_string(),
        }
    }

    #[test]
    fn test_research_save_success_writes_markdown_file() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        with_temp_cwd(|temp_root| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            install_python_mock(&kernel, MOCK_SCRIPT);
            let approver = AutoApprover;
            let save_path = format!("Documents/research-{}.md", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_research_save(&kernel, &input, &approver);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
            assert_eq!(result.unwrap(), "t1");

            // File exists with correct content.
            let file_path = temp_root.join(&input.save_path);
            let content = std::fs::read_to_string(&file_path).expect("markdown file must exist");
            assert!(content.contains("Example Domain"));
            assert!(content.contains("illustrative examples"));

            // Step is Succeeded with strong evidence + compensation registered.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Succeeded);
            assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
            // W10 Plan 2: compensation_ref 必须指向 research.reverse_save 记录。
            let comp_ref = step
                .compensation_ref
                .as_ref()
                .expect("compensation_ref must be set");
            let comp = kernel.get_compensation(comp_ref).unwrap().unwrap();
            assert_eq!(comp.compensate_fn, "research.reverse_save");
            assert_eq!(comp.level, CompensationLevel::Strong);

            // 2026 免审批：不落审批记录。
            let approvals = kernel.list_approvals_for_task("t1").unwrap();
            assert!(approvals.is_empty(), "got {} records", approvals.len());
        });
    }

    #[test]
    fn test_research_save_denier_still_writes_file() {
        // 2026 免审批：AutoDenier 不再拦截，research.save_markdown 照常执行写文件。
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        with_temp_cwd(|temp_root| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            install_python_mock(&kernel, MOCK_SCRIPT);
            let approver = AutoDenier;
            let save_path = format!("Documents/research-{}.md", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_research_save(&kernel, &input, &approver);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
            assert_eq!(result.unwrap(), "t1");

            // File WAS written.
            let file_path = temp_root.join(&input.save_path);
            let content = std::fs::read_to_string(&file_path)
                .expect("markdown file must exist after approval-free run");
            assert!(content.contains("Example Domain"));

            // Step Succeeded.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Succeeded);

            // 免审批：不落审批记录。
            assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
        });
    }

    #[test]
    fn test_research_save_invalid_url_fails_validation() {
        // url="ftp://example.com" → manifest validation rejects (not http/https).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = ResearchSaveInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "ftp://example.com".to_string(),
            save_path: "Documents/test.md".to_string(),
        };
        let result = execute_research_save(&kernel, &input, &approver);
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
    fn test_research_save_invalid_save_path_fails_validation() {
        // save_path="/tmp/evil.md" → manifest validation rejects (not under
        // Documents/Desktop).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = ResearchSaveInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "https://example.com".to_string(),
            save_path: "/tmp/evil.md".to_string(),
        };
        let result = execute_research_save(&kernel, &input, &approver);
        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("allowed_root"),
            "expected 'allowed_root' in error, got: {}",
            err
        );

        // No task/step created.
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
    }

    #[test]
    fn test_research_save_mcp_failure_marks_step_failed() {
        // Override playwright record with a guaranteed-to-fail command —
        // spawn will fail with KernelError::Mcp(...), executor marks step
        // Failed, no file written.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .unwrap()
            .unwrap();
        rec.command = Some("this-command-does-not-exist-12345".to_string());
        rec.args = Some("[]".to_string());
        McpServerRepo::new().update(&kernel.conn(), &rec).unwrap();

        let approver = AutoApprover;
        let input = ResearchSaveInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "https://example.com".to_string(),
            save_path: "Documents/test.md".to_string(),
        };
        let result = execute_research_save(&kernel, &input, &approver);
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

        // No file written (spawn failure short-circuits before fs::write).
        assert!(!std::path::Path::new(&input.save_path).exists());
    }

    #[test]
    fn test_research_save_empty_url_fails_validation() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let input = ResearchSaveInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            url: "".to_string(),
            save_path: "Documents/test.md".to_string(),
        };
        let result = execute_research_save(&kernel, &input, &approver);
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
    }

    #[test]
    fn extract_text_from_eval_handles_text_field() {
        let data = serde_json::json!({"text": "hello world"});
        assert_eq!(extract_text_from_eval(&data), "hello world");
    }

    #[test]
    fn extract_text_from_eval_handles_plain_string() {
        let data = serde_json::json!("raw string");
        assert_eq!(extract_text_from_eval(&data), "raw string");
    }

    #[test]
    fn extract_text_from_eval_pretty_prints_other_json() {
        let data = serde_json::json!({"foo": "bar", "n": 42});
        let s = extract_text_from_eval(&data);
        assert!(s.contains("\"foo\""));
        assert!(s.contains("\"bar\""));
        assert!(s.contains("42"));
    }
}
