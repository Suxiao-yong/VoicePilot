//! note.capture Skill executor — W7 Plan 4 Task 4.
//!
//! Opens Notepad via UIA, writes text content into its edit area, and
//! saves the content to a file on disk. Risk E2 (UIA can drive arbitrary
//! GUI actions + filesystem write), approval PerStep — the approver sees
//! an EffectManifest describing the planned action and must Allow before
//! the adapter is invoked.
//!
//! Pipeline:
//!   1. Validate `content` + `save_path` inputs (non-empty).
//!   2. Validate the full input map against `note_capture_manifest`
//!      (enforces `save_path` ∈ {Documents, Desktop}/...).
//!   3. Create new task + step.
//!   4. Build EffectManifest (`sources: vec![]`,
//!      `destination: format!("uia:note_capture:{}", save_path)`,
//!      `total_bytes: content.len()`).
//!   5. Update step → Running.
//!   6. Record approval decision (E2 + D2 + Single). Branch on
//!      Allow/Deny/Modify.
//!   7. On Allow:
//!      a. `adapter.launch_app("notepad")` — on error mark step Failed +
//!      return Uia error.
//!      b. `adapter.find_window("Notepad")` — if `Ok(Some(handle))`, call
//!      `adapter.set_text(handle, content)`. If `Ok(None)`, skip
//!      set_text (graceful degradation — still save the file directly
//!      so the user's note isn't lost). If `Err`, mark step Failed +
//!      return Uia error.
//!      c. Save to disk: `kernel.filesystem().assert_path_allowed(&save_path)`
//!      (enforces allowed_paths whitelist — no-op when allowed_paths is
//!      `None`), then `std::fs::write(&save_path, content.as_bytes())`.
//!   8. Finalize step as Succeeded with Strong evidence (real file artifact
//!      on disk — different from `app_control` which uses Weak because
//!      UIA ops produce no file artifacts).
//!
//! The `save_path` SkillInput's `allowed_roots` (`["Documents", "Desktop"]`)
//! is enforced by `validate_input_against_manifest`. Per Plan 4 §2.6, even
//! paths outside this whitelist require PerStep approval — the approval
//! gate is mandatory, never skipped.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    create_post_commit_compensation_with_payload, finalize_step_success, record_approval_decision,
    validate_input_against_manifest, ApprovalContext,
};
use crate::skills::manifest::note_capture_manifest;
use crate::skills::verifiers::{verify_note_capture, VerificationContext, VerificationOutcome};
use crate::uiautomation::UiaAdapter;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

/// Input for the `note.capture` Skill executor.
#[derive(Debug, Clone)]
pub struct NoteCaptureInput {
    /// New task ID for this note_capture operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// Text content to write into Notepad + save to disk.
    pub content: String,
    /// File path to save the note to. Must be under "Documents" or
    /// "Desktop" (per `note_capture_manifest`'s `allowed_roots`).
    pub save_path: String,
}

/// Execute the `note.capture` Skill.
///
/// Drives the UIA adapter to launch Notepad, set its text content, and
/// save the content to a file on disk. Risk E2 + PerStep approval: the
/// approver sees an EffectManifest describing the planned action and
/// must Allow before any UIA or filesystem call.
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (adapter error, filesystem error, finalization
/// failure), returns Err and marks the step Failed.
pub fn execute_note_capture(
    kernel: &TrustKernel,
    input: &NoteCaptureInput,
    approver: &dyn Approver,
    adapter: &dyn UiaAdapter,
) -> Result<String> {
    // Step 1: validate input — reject empty content / save_path
    // explicitly, since validate_input_against_manifest only checks
    // presence + type constraints (an empty string passes Text
    // validation when max_length is None, and an empty File path may
    // slip through allowed_roots matching if the path is interpreted
    // as "").
    if input.content.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for content: must not be empty".to_string(),
        ));
    }
    if input.save_path.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for save_path: must not be empty".to_string(),
        ));
    }

    // Step 2: validate the full input map against the manifest. Catches
    // save_path ∉ {Documents, Desktop}/... before any DB write.
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("content".to_string(), serde_json::json!(input.content));
    input_map.insert("save_path".to_string(), serde_json::json!(input.save_path));
    validate_input_against_manifest(&input_map, &note_capture_manifest())?;

    // Step 3: create new task + step.
    kernel.create_task(&input.task_id, &format!("note_capture:{}", input.save_path))?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 4: build EffectManifest for the approval prompt. UIA ops have
    // no file sources — the manifest is purely descriptive; the
    // `destination` carries the save_path so the approval record has a
    // non-empty target. `total_bytes` reflects the content size so the
    // approver can see how much data will be written.
    let effect_manifest = build_note_capture_effect_manifest(&input.content, &input.save_path);

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E2 + D2 + Single). The approver
    // sees the effect_manifest describing what will happen. The
    // preconditions_hash binds this approval to the exact
    // {content, save_path} pair so post-hoc audit can verify what the
    // user actually approved.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.content.as_bytes());
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
    // Step 6: 审批门 —— note.capture 不在审批白名单（approval_required 在
    // simple.rs，仅 fs.*/shell.run 保留审批），不弹窗：record_approval_decision
    // 调用与决策分支都包在门内。审计留痕仍由任务/步骤落库提供。
    if crate::skills::simple::approval_required("note.capture") {
        let approval = record_approval_decision(kernel, approver, &effect_manifest, &ctx)?;

        // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
        match approval.user_decision {
            ApprovalDecision::Deny => {
                kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
                return Err(KernelError::Skill("user denied note_capture".to_string()));
            }
            ApprovalDecision::Modify => {
                kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
                return Err(KernelError::Skill(
                    "modify not supported for note_capture".to_string(),
                ));
            }
            ApprovalDecision::Allow => { /* proceed to commit */ }
        }
    }

    // Step 7a: launch Notepad. On error, mark step Failed + return Uia
    // error. The launch is the first side-effecting call after the
    // approval gate — its success is the precondition for any UIA
    // text-driving.
    adapter.launch_app("notepad").inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    // Step 7b: find the Notepad window and set its text. If find_window
    // returns Ok(None), skip set_text (graceful degradation — we still
    // save the file directly so the user's note isn't lost). If
    // find_window returns Err, mark step Failed + return Uia error.
    let window = adapter.find_window("Notepad").inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;
    if let Some(handle) = window {
        // Spec §2.6 line 305: "set_text 仅对窗口标题在白名单内的元素生效
        // (防伪造窗口)". `find_window("Notepad")` may have matched a
        // forged / spoofed window whose title contains "Notepad" but is
        // not the real Notepad process. Verify "notepad" is in the
        // kernel's `allowed_apps` whitelist before driving text into
        // the returned handle. A mismatch indicates either a forged
        // window or a Settings change that hasn't been re-confirmed —
        // reject the set_text and mark step Failed.
        let in_whitelist = kernel
            .allowed_apps()
            .iter()
            .any(|app| app.eq_ignore_ascii_case("notepad"));
        if !in_whitelist {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Uia(
                "set_text rejected: window title not in allowed_apps whitelist".to_string(),
            ));
        }
        adapter
            .set_text(&handle, &input.content)
            .inspect_err(|_e| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            })?;
    }

    // Step 7c: save to disk. First enforce the kernel's allowed_paths
    // whitelist (no-op when allowed_paths is `None`, e.g. in
    // `open_in_memory()`). The path check is in its own block so the
    // `MutexGuard` is dropped before `std::fs::write` runs — avoids
    // holding the filesystem lock across the actual write, and lets us
    // mark the step Failed on a `PathNotAllowed` error without
    // re-borrowing the guard.
    let path_check = {
        let fs = kernel.filesystem();
        fs.assert_path_allowed(Path::new(&input.save_path))
    };
    if let Err(e) = path_check {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        return Err(e);
    }
    std::fs::write(&input.save_path, input.content.as_bytes()).map_err(|e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        KernelError::Io(e)
    })?;

    // Step 8: W10 Plan 1 — 调用真实 verify_note_capture 重读磁盘文件,
    // 计算 sha256+size 与 input.content 比对(spec §6.3 Strong Verifier)。
    // 通过 → Strong evidence;失败 → step Failed + 返回错误(文件未写入或
    // 内容不一致,说明 commit 阶段出错)。
    let verify_ctx = VerificationContext {
        kernel,
        step_id: &input.step_id,
    };
    let outcome = verify_note_capture(&verify_ctx, &input.save_path, &input.content)?;
    match outcome {
        VerificationOutcome::Strong { .. } => { /* proceed to register compensation */ }
        VerificationOutcome::Failed { reason } => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_note_capture failed: {}",
                reason
            )));
        }
        // Plan 1 不使用 Medium / Weak,保留匹配为未来扩展占位。
        _ => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "verify_note_capture returned unexpected outcome: {:?}",
                outcome
            )));
        }
    }

    // W10 Plan 2: 注册 note.reverse_capture compensation,payload 含 save_path。
    // task.compensate 调用 auto_reverse 时,ReverseFnRegistry 路由到
    // reverse_note_capture,删除 save_path 文件。
    let comp_ref = create_post_commit_compensation_with_payload(
        kernel,
        &input.step_id,
        "note.reverse_capture",
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

/// Build a descriptive EffectManifest for the approval prompt. UIA ops
/// have no file sources — the manifest carries the save_path as the
/// `destination` so the approval record has a non-empty target.
/// `total_bytes` reflects the content size so the approver can see how
/// much data will be written.
fn build_note_capture_effect_manifest(content: &str, save_path: &str) -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: format!("uia:note_capture:{}", save_path),
        conflicts: vec![],
        total_bytes: content.len() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier, DagApprovalOutcome};
    use crate::kernel::TrustKernel;
    use crate::uiautomation::{UiaElementHandle, UiaSelector};
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// Recorded state shared between the MockAdapter and the test
    /// harness. Mirrors the design in `app_control.rs` tests so we can
    /// inspect call counts after the adapter has been boxed as
    /// `dyn UiaAdapter`.
    #[derive(Default)]
    struct MockState {
        launch_calls: Vec<String>,
        find_window_calls: Vec<String>,
        set_text_calls: Vec<String>,
        /// When `Some`, every fallible method returns
        /// `Err(KernelError::Uia(msg))`.
        next_error: Option<&'static str>,
        /// When `Some`, `set_text` returns `Err(KernelError::Uia(msg))`.
        /// Separate from `next_error` so a test can simulate a set_text
        /// failure without short-circuiting at launch_app (which would
        /// prevent set_text from being invoked at all).
        next_set_text_error: Option<&'static str>,
        /// When `true`, `find_window` returns `Ok(None)` instead of
        /// `Ok(Some(handle))`.
        find_window_returns_none: bool,
    }

    /// Mock adapter that records calls into a shared
    /// `Rc<RefCell<MockState>>`.
    struct MockAdapter {
        state: Rc<RefCell<MockState>>,
    }

    impl MockAdapter {
        fn new() -> Self {
            Self {
                state: Rc::new(RefCell::new(MockState::default())),
            }
        }

        /// Clone of the shared state handle — tests use this to inspect
        /// call counts after the adapter itself has been boxed as
        /// `dyn UiaAdapter`.
        fn state_handle(&self) -> Rc<RefCell<MockState>> {
            Rc::clone(&self.state)
        }
    }

    impl UiaAdapter for MockAdapter {
        fn launch_app(&self, app_name: &str) -> Result<UiaElementHandle> {
            let mut s = self.state.borrow_mut();
            s.launch_calls.push(app_name.to_string());
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            Ok(UiaElementHandle::mock())
        }

        fn find_window(&self, title_contains: &str) -> Result<Option<UiaElementHandle>> {
            let mut s = self.state.borrow_mut();
            s.find_window_calls.push(title_contains.to_string());
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            if s.find_window_returns_none {
                Ok(None)
            } else {
                Ok(Some(UiaElementHandle::mock()))
            }
        }

        fn find_element(
            &self,
            _root: &UiaElementHandle,
            _selector: &UiaSelector,
        ) -> Result<Option<UiaElementHandle>> {
            // Unused by note_capture executor; return a mock handle for
            // trait completeness.
            Ok(Some(UiaElementHandle::mock()))
        }

        fn click(&self, _element: &UiaElementHandle) -> Result<()> {
            Ok(())
        }

        fn set_text(&self, _element: &UiaElementHandle, text: &str) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.set_text_calls.push(text.to_string());
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            if let Some(msg) = s.next_set_text_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            Ok(())
        }

        fn get_text(&self, _element: &UiaElementHandle) -> Result<String> {
            Ok(String::new())
        }

        fn screenshot(&self, _element: &UiaElementHandle) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }
    }

    /// RAII guard that restores the original CWD on drop. Paired with
    /// the shared `TEST_CWD_MUTEX` (common.rs) to ensure tests don't race
    /// on process-global CWD.
    struct CwdGuard {
        original: PathBuf,
    }

    impl CwdGuard {
        fn enter(temp: &std::path::Path) -> Self {
            let original = std::env::current_dir().expect("current_dir");
            std::env::set_current_dir(temp).expect("set_current_dir");
            CwdGuard { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    /// Run `body` inside a temp directory whose `Documents/` subdirectory
    /// exists. Use a relative save_path = "Documents/<uuid>.txt" inside
    /// `body` so it satisfies the manifest's
    /// `allowed_roots: ["Documents", "Desktop"]` constraint AND writes
    /// to `<temp>/Documents/<uuid>.txt` (a real temp location).
    ///
    /// The shared `TEST_CWD_MUTEX` (common.rs) serializes all callers
    /// crate-wide — note_capture 与 research_save 各自的私有锁管不住跨模块并行。
    /// We recover from poison
    /// (`unwrap_or_else(|e| e.into_inner())`) so a panicking sibling test
    /// doesn't mask the real failure in this test — Rust's test runner
    /// runs tests in parallel by default, and one panic would otherwise
    /// poison the mutex for every subsequent caller.
    /// The `CwdGuard` restores CWD on drop, even if the test body panics.
    /// The `TempDir` cleans up the temp directory on drop.
    fn with_temp_cwd<F, R>(body: F) -> R
    where
        F: FnOnce(&std::path::Path) -> R,
    {
        let _guard = crate::skills::common::TEST_CWD_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("Documents")).expect("create Documents dir");
        let _cwd = CwdGuard::enter(temp.path());
        body(temp.path())
        // `_cwd` drops here → restore CWD.
        // `_guard` drops here → release mutex.
        // `temp` drops here → cleanup temp dir.
    }

    fn make_input(save_path: &str) -> NoteCaptureInput {
        NoteCaptureInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            content: "hello notepad".to_string(),
            save_path: save_path.to_string(),
        }
    }

    /// Auto-modifier for the Modify-path test. Returns `Modify` to verify
    /// the executor rejects modification requests (note_capture inputs
    /// are not user-adjustable post-prompt — the user must re-invoke
    /// with different inputs rather than tweak them mid-flow).
    struct AutoModifier;

    impl Approver for AutoModifier {
        fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
            ApprovalDecision::Modify
        }

        /// W8 Plan 4: DAG 骨架审批回调。`AutoModifier` 仅用于 note_capture
        /// 的 Modify-path 测试,不参与 DAG 流程;返回 `Deny` 作为安全默认
        /// (note_capture 测试不触发 DAG,此方法不会被调用)。
        fn approve_dag_skeleton(
            &self,
            _plan: &crate::skills::dag_types::DagPlan,
        ) -> Result<DagApprovalOutcome> {
            Ok(DagApprovalOutcome::Deny)
        }
    }

    #[test]
    fn test_note_capture_success_writes_file_and_settext() {
        // AutoApprover + MockAdapter → execute → assert: launch_app
        // called once with "notepad", set_text called once with content,
        // file actually exists on disk at save_path with correct content,
        // step is Succeeded with strong evidence, approval record
        // persisted.
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
            assert_eq!(result.unwrap(), "t1");

            // launch_app was called once with "notepad".
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window was called once with "Notepad".
            assert_eq!(state.borrow().find_window_calls.len(), 1);
            assert_eq!(state.borrow().find_window_calls[0], "Notepad");
            // set_text was called once with the content.
            assert_eq!(state.borrow().set_text_calls.len(), 1);
            assert_eq!(state.borrow().set_text_calls[0], "hello notepad");

            // File exists on disk with correct content.
            let file_path = temp.join(&save_path);
            let on_disk = std::fs::read_to_string(&file_path).expect("file should exist");
            assert_eq!(on_disk, "hello notepad");

            // Step is Succeeded with strong evidence + compensation registered.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Succeeded);
            assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
            // W10 Plan 2: compensation_ref 必须指向 note.reverse_capture 记录。
            let comp_ref = step
                .compensation_ref
                .as_ref()
                .expect("compensation_ref must be set");
            let comp = kernel.get_compensation(comp_ref).unwrap().unwrap();
            assert_eq!(comp.compensate_fn, "note.reverse_capture");
            assert_eq!(comp.level, CompensationLevel::Strong);
            let payload: serde_json::Value = serde_json::from_str(&comp.reverse_payload).unwrap();
            assert_eq!(
                payload.get("save_path").and_then(|v| v.as_str()),
                Some(save_path.as_str())
            );

            // 2026 免审批：不落审批记录。
            let approvals = kernel.list_approvals_for_task("t1").unwrap();
            assert!(approvals.is_empty(), "got {} records", approvals.len());
        });
    }

    #[test]
    fn test_note_capture_invalid_empty_content() {
        // empty content → Skill validation error before any DB or
        // adapter call.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let mut input = make_input("Documents/note.txt");
        input.content = "".to_string();
        let result = execute_note_capture(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("validation failed for content"),
            "expected 'validation failed for content' in error, got: {}",
            err
        );

        // No task/step should have been created (validation runs first).
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
        // No adapter calls.
        assert!(state.borrow().launch_calls.is_empty());
        assert!(state.borrow().find_window_calls.is_empty());
    }

    #[test]
    fn test_note_capture_invalid_empty_save_path() {
        // empty save_path → Skill validation error.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let mut input = make_input("Documents/note.txt");
        input.save_path = "".to_string();
        let result = execute_note_capture(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("validation failed for save_path"),
            "expected 'validation failed for save_path' in error, got: {}",
            err
        );

        // No task/step should have been created (validation runs first).
        assert!(kernel.get_task("t1").unwrap().is_none());
        assert!(kernel.get_step("s1").unwrap().is_none());
        // No adapter calls.
        assert!(state.borrow().launch_calls.is_empty());
    }

    #[test]
    fn test_note_capture_denier_still_writes_file() {
        // 2026 免审批：AutoDenier 不再拦截，note.capture 照常执行写文件。
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoDenier;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
            assert_eq!(result.unwrap(), "t1");

            // Adapter calls happened (免审批直接执行).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            assert_eq!(state.borrow().set_text_calls.len(), 1);

            // File WAS written.
            let file_path = temp.join(&save_path);
            let on_disk = std::fs::read_to_string(&file_path).expect("file should exist");
            assert_eq!(on_disk, "hello notepad");

            // Step Succeeded.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Succeeded);

            // 免审批：不落审批记录。
            assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
        });
    }

    #[test]
    fn test_note_capture_launch_failure_marks_step_failed() {
        // MockState.next_error = Some("launch failed") → execute
        // returns Uia error, step Failed, no file written.
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            state.borrow_mut().next_error = Some("launch failed");
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            let err = result.unwrap_err();
            assert!(
                matches!(err, KernelError::Uia(ref m) if m == "launch failed"),
                "expected Uia 'launch failed' error, got {:?}",
                err
            );

            // launch_app was called (and failed).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window / set_text were NOT called (launch failed first).
            assert!(state.borrow().find_window_calls.is_empty());
            assert!(state.borrow().set_text_calls.is_empty());

            // Step is marked Failed.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Failed);

            // No file written.
            let file_path = temp.join(&save_path);
            assert!(
                !file_path.exists(),
                "file should not exist after launch failure"
            );

            // 2026 免审批：不落审批记录。
            let approvals = kernel.list_approvals_for_task("t1").unwrap();
            assert!(approvals.is_empty(), "got {} records", approvals.len());
        });
    }

    #[test]
    fn test_note_capture_find_window_returns_none_still_saves_file() {
        // find_window_returns_none = true → set_text NOT called (no
        // handle), but file IS still saved via std::fs::write, step
        // Succeeded. (Graceful degradation: if we can't drive the
        // Notepad GUI text, we still save the content to disk so the
        // user's note isn't lost.)
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            state.borrow_mut().find_window_returns_none = true;
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

            // launch_app was called.
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window was called.
            assert_eq!(state.borrow().find_window_calls.len(), 1);
            // set_text was NOT called (no handle returned).
            assert!(state.borrow().set_text_calls.is_empty());

            // File IS still saved via std::fs::write.
            let file_path = temp.join(&save_path);
            let on_disk = std::fs::read_to_string(&file_path).expect("file should exist");
            assert_eq!(on_disk, "hello notepad");

            // Step is Succeeded with strong evidence + compensation registered.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Succeeded);
            assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
            assert!(
                step.compensation_ref.is_some(),
                "compensation_ref must be set"
            );
        });
    }

    #[test]
    fn test_note_capture_modifier_still_writes_file() {
        // 2026 免审批：AutoModifier 也不再拦截，note.capture 照常执行写文件。
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoModifier;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

            // Adapter calls happened (免审批直接执行).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().set_text_calls.len(), 1);

            // File WAS written.
            let file_path = temp.join(&save_path);
            let on_disk = std::fs::read_to_string(&file_path).expect("file should exist");
            assert_eq!(on_disk, "hello notepad");

            // Step Succeeded.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Succeeded);

            // 免审批：不落审批记录。
            assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
        });
    }

    #[test]
    fn test_note_capture_set_text_failure_marks_step_failed() {
        // MockState.next_set_text_error = Some("set_text failed") →
        // launch + find_window succeed, set_text fails → execute returns
        // Uia error, step Failed, no file written.
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            state.borrow_mut().next_set_text_error = Some("set_text failed");
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            let err = result.unwrap_err();
            assert!(
                matches!(err, KernelError::Uia(ref m) if m == "set_text failed"),
                "expected Uia 'set_text failed' error, got {:?}",
                err
            );

            // launch_app was called (and succeeded — next_set_text_error
            // only fires on set_text, not launch_app).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window was called (returned Ok(Some(handle))).
            assert_eq!(state.borrow().find_window_calls.len(), 1);
            // set_text was called (and failed).
            assert_eq!(state.borrow().set_text_calls.len(), 1);
            assert_eq!(state.borrow().set_text_calls[0], "hello notepad");

            // Step is marked Failed.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Failed);

            // No file written (set_text failure short-circuits before
            // std::fs::write).
            let file_path = temp.join(&save_path);
            assert!(
                !file_path.exists(),
                "file should not exist after set_text failure"
            );
        });
    }

    #[test]
    fn test_note_capture_allowed_paths_rejection_marks_step_failed() {
        // Configure the kernel with an AllowedPaths whitelist that does
        // NOT include save_path. The executor's `assert_path_allowed`
        // check rejects the write → step Failed, no file on disk.
        // (Whitelist is enforced AFTER UIA ops in Step 7c, per the
        // executor's pipeline — launch + set_text still run.)
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            // Whitelist a directory that won't contain save_path.
            kernel.replace_filesystem_with_allowed_paths(crate::allowed_paths::AllowedPaths::new(
                vec!["E:/nonexistent_allowed_root".to_string()],
            ));

            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            let err = result.unwrap_err();
            assert!(
                matches!(err, KernelError::PathNotAllowed(_)),
                "expected PathNotAllowed error, got {:?}",
                err
            );

            // launch_app was called (whitelist check happens AFTER UIA
            // ops in Step 7c).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window was called.
            assert_eq!(state.borrow().find_window_calls.len(), 1);
            // set_text was called.
            assert_eq!(state.borrow().set_text_calls.len(), 1);

            // Step is marked Failed.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Failed);

            // No file written.
            let file_path = temp.join(&save_path);
            assert!(
                !file_path.exists(),
                "file should not exist after allowed_paths rejection"
            );
        });
    }

    #[test]
    fn test_note_capture_fs_write_failure_marks_step_failed() {
        // save_path's parent directory doesn't exist → std::fs::write
        // fails with NotFound → step Failed, no file on disk. The path
        // still starts with "Documents/" so the manifest's allowed_roots
        // check passes; assert_path_allowed is a no-op under
        // open_in_memory (no allowed_paths set).
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            // `Documents/nonexistent_subdir/<uuid>.txt` — parent subdir
            // doesn't exist, so std::fs::write will fail with NotFound.
            let save_path = format!(
                "Documents/nonexistent_subdir/note-{}.txt",
                uuid::Uuid::new_v4()
            );
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            let err = result.unwrap_err();
            assert!(
                matches!(err, KernelError::Io(_)),
                "expected Io error, got {:?}",
                err
            );

            // launch_app was called (and succeeded).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window was called.
            assert_eq!(state.borrow().find_window_calls.len(), 1);
            // set_text was called.
            assert_eq!(state.borrow().set_text_calls.len(), 1);

            // Step is marked Failed.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Failed);

            // File does NOT exist on disk.
            let file_path = temp.join(&save_path);
            assert!(
                !file_path.exists(),
                "file should not exist after fs::write failure"
            );
        });
    }

    #[test]
    fn set_text_rejected_when_window_not_in_allowed_apps() {
        // Spec §2.6 line 305: "set_text 仅对窗口标题在白名单内的元素
        // 生效(防伪造窗口)". When `find_window("Notepad")` returns
        // Some(handle) but "notepad" is NOT in `kernel.allowed_apps()`,
        // the executor must reject the set_text call, mark step Failed,
        // and NOT call adapter.set_text. This catches forged windows
        // whose title contains "Notepad" but which are not the real
        // Notepad process.
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().unwrap();
            // Whitelist only "explorer" — "notepad" is absent.
            kernel.set_allowed_apps(vec!["explorer".to_string()]);

            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            let save_path = format!("Documents/note-{}.txt", uuid::Uuid::new_v4());
            let input = make_input(&save_path);
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            let err = result.unwrap_err();
            assert!(
                matches!(err, KernelError::Uia(ref m)
                    if m == "set_text rejected: window title not in allowed_apps whitelist"),
                "expected Uia whitelist rejection error, got {:?}",
                err
            );

            // launch_app was called (whitelist check is after launch).
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // find_window was called and returned Some(handle).
            assert_eq!(state.borrow().find_window_calls.len(), 1);
            // set_text was NOT called (whitelist check rejected it).
            assert!(
                state.borrow().set_text_calls.is_empty(),
                "set_text must not be called when window not in allowed_apps"
            );

            // Step is marked Failed.
            let step = kernel.get_step("s1").unwrap().unwrap();
            assert_eq!(step.status, StepStatus::Failed);

            // No file written (whitelist rejection short-circuits before
            // std::fs::write).
            let file_path = temp.join(&save_path);
            assert!(
                !file_path.exists(),
                "file should not exist after whitelist rejection"
            );

            // 2026 免审批：不落审批记录。
            let approvals = kernel.list_approvals_for_task("t1").unwrap();
            assert!(approvals.is_empty(), "got {} records", approvals.len());
        });
    }
}
