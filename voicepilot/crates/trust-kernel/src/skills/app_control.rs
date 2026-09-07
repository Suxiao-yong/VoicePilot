//! quick.app_control Skill executor — W7 Plan 4 Task 3.
//!
//! Launches, focuses, or closes a Windows application via the UIA adapter.
//! Risk E2 (UIA can drive arbitrary GUI actions). 2026 姿态：免审批直接执行
//! （不在 `approval_required` 白名单），审计留痕由 task/step 落库提供。
//!
//! Pipeline:
//!   1. Validate `app_name` + `action` inputs (non-empty + action ∈
//!      {launch, focus, close}).
//!   2. Validate the full input map against `app_control_manifest`.
//!   3. Create new task + step.
//!   4. Update step → Running.
//!   5. focus/close resolve the window (`find_window_titled`);
//!      not-found fails here with no prompt spent.
//!   6. Branch on action:
//!      - launch → adapter.launch_app(app_name)
//!      - focus  → adapter.click(pre-resolved handle)
//!      - close  → adapter.close_window(pre-resolved handle, discard=false)
//!   7. Finalize step as Succeeded with Weak evidence (no file evidence).
//!
//! W7 Plan 4 Task 5 (review fix): the `allowed_apps` whitelist
//! (`["notepad", "explorer", "calc"]` by default, Settings-configurable)
//! is NO LONGER enforced by the manifest's `app_name` `allowed_values`
//! (the input is now free-form Text). The runtime whitelist lives in
//! `kernel.allowed_apps()` (Settings-persisted in KV "uia.allowed_apps").
//! W7 Plan 4 final review (must-fix #2): per spec §2.6 line 304
//! "超出白名单需 PerStep approval", the executor consults the whitelist
//! for `Action::Launch` — in-whitelist launches SKIP approval (advisory
//! whitelist); out-of-whitelist launches require PerStep approval.
//! `Action::Focus` and `Action::Close` always require approval regardless
//! of whitelist membership (they drive UIA into an existing window,
//! which the whitelist does not cover).

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    finalize_step_success, record_approval_decision, validate_input_against_manifest,
    ApprovalContext,
};
use crate::skills::manifest::app_control_manifest;
use crate::uiautomation::{UiaAdapter, UiaElementHandle};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Input for the `quick.app_control` Skill executor.
#[derive(Debug, Clone)]
pub struct AppControlInput {
    /// New task ID for this app_control operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// Application name (e.g. "notepad", "explorer", "calc").
    pub app_name: String,
    /// Action to perform: "launch" | "focus" | "close".
    pub action: String,
}

/// Allowed action values. Mirrors the `action` SkillInput's `allowed_values`
/// in `app_control_manifest`. Validated here in addition to the manifest so
/// a caller error is surfaced before any DB write.
const ALLOWED_ACTIONS: &[&str] = &["launch", "focus", "close"];

/// 中文应用别名 → 可执行名。
///
/// 与前端 `buildAppControlSlots` 内映射保持一致（改一边必须改另一边）。
/// 归一化发生在白名单/审批之前（见 `execute_app_control` 入口），因此别名目标
/// 与原名享受完全一致的白名单与审批语义；审计与 manifest 记录归一化后的名。
pub fn normalize_app_name(raw: &str) -> String {
    // 唯一来源：`manifest::known_app_aliases`。精确相等（前后 trim）。
    let trimmed = raw.trim();
    for (display, exe) in crate::skills::manifest::known_app_aliases() {
        if trimmed == display {
            return exe.to_string();
        }
    }
    trimmed.to_string()
}

/// Execute the `quick.app_control` Skill.
///
/// Drives the UIA adapter to launch / focus / close a Windows application.
/// Risk E2 + PerStep approval: the approver sees an EffectManifest
/// describing the planned action and must Allow before any UIA call.
///
/// Returns the new task_id on success. On user denial, returns
/// `Err(KernelError::Skill(...))` and marks the step Cancelled. On any
/// other failure (adapter error, finalization failure), returns Err and
/// marks the step Failed.
pub fn execute_app_control(
    kernel: &TrustKernel,
    input: &AppControlInput,
    approver: &dyn Approver,
    adapter: &dyn UiaAdapter,
) -> Result<String> {
    // Step 1: validate input — reject empty app_name/action explicitly,
    // since validate_input_against_manifest only checks presence + type
    // constraints (an empty string passes Text validation, and an
    // out-of-allow-list action is caught here with a clearer message than
    // the manifest's generic one).
    if input.app_name.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for app_name: must not be empty".to_string(),
        ));
    }
    if input.action.trim().is_empty() {
        return Err(KernelError::Skill(
            "validation failed for action: must not be empty".to_string(),
        ));
    }
    if !ALLOWED_ACTIONS.iter().any(|a| *a == input.action) {
        return Err(KernelError::Skill(format!(
            "validation failed for action: '{}' not in allowed_values {:?}",
            input.action, ALLOWED_ACTIONS
        )));
    }
    // 中文别名归一化（`normalize_app_name`）：后续白名单、审批、审计、adapter
    // 全走归一化后的名。`AppControlInput: Clone`，整体替换最省 diff。
    let input_owned = AppControlInput {
        app_name: normalize_app_name(&input.app_name),
        ..input.clone()
    };
    let input = &input_owned;

    // Step 2: validate the full input map against the manifest. With the
    // Task 5 review fix, `app_name` is free-form Text (no allowed_values)
    // — any non-empty string ≤ max_length passes. The runtime whitelist
    // (`kernel.allowed_apps()`) is advisory and does not affect validation;
    // PerStep approval below is the security boundary.
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("app_name".to_string(), serde_json::json!(input.app_name));
    input_map.insert("action".to_string(), serde_json::json!(input.action));
    validate_input_against_manifest(&input_map, &app_control_manifest())?;

    // Step 3: create new task + step.
    kernel.create_task(
        &input.task_id,
        &format!("app_control:{}:{}", input.action, input.app_name),
    )?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 3b: focus/close resolve the window BEFORE the approval prompt,
    // so the approval binds the exact resolved window
    // ({action, app_name, title}) instead of just the query that may have
    // matched several windows. Not-found fails here with the same messages
    // as before — no approval prompt is spent on a missing window.
    // Launch resolves nothing (the window does not exist yet).
    let resolved: Option<(UiaElementHandle, String)> = match input.action.as_str() {
        "focus" | "close" => {
            match adapter
                .find_window_titled(&input.app_name)
                .inspect_err(|_e| {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                })? {
                Some(pair) => Some(pair),
                None => {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                    if input.action == "focus" {
                        return Err(KernelError::Uia(format!(
                            "no window found matching '{}'",
                            input.app_name
                        )));
                    }
                    return Err(KernelError::Uia(
                        "close failed: window not found".to_string(),
                    ));
                }
            }
        }
        _ => None,
    };
    let resolved_title: Option<&str> = resolved.as_ref().map(|(_, t)| t.as_str());

    // Step 4: build EffectManifest for the approval prompt. UIA ops have
    // no file sources — the manifest is purely descriptive; the
    // `destination` carries action+app_name (+ resolved title for
    // focus/close) so the approval record has a non-empty target.
    let effect_manifest =
        build_app_control_effect_manifest(&input.app_name, &input.action, resolved_title);

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: 审批门 —— quick.app_control 不在审批白名单（approval_required 在
    // simple.rs，仅 fs.*/shell.run 保留审批），不弹窗，直接执行。审计留痕仍由
    // 任务/步骤落库提供。保留分支代码便于日后重新加入白名单。
    if crate::skills::simple::approval_required("quick.app_control") {
        // target is in the kernel's `allowed_apps` whitelist. Spec §2.6 line
        // 304: "超出白名单需 PerStep approval" — apps outside the whitelist
        // require PerStep approval; in-whitelist launches are pre-approved
        // and skip the approval gate. Focus and Close always require approval
        // (they drive UIA into an existing window, which the whitelist does
        // not cover). The approver sees the effect_manifest describing what
        // will happen; the preconditions_hash binds this approval to the
        // exact {action, app_name} pair — plus the resolved window title for
        // focus/close (Step 3b) — so post-hoc audit can verify what the user
        // actually approved.
        let preconditions_hash = {
            let mut hasher = Sha256::new();
            hasher.update(input.action.as_bytes());
            hasher.update(b"\x00");
            hasher.update(input.app_name.as_bytes());
            if let Some(title) = resolved_title {
                hasher.update(b"\x00");
                hasher.update(title.as_bytes());
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
        let skip_approval =
            input.action == "launch" && kernel.allowed_apps().iter().any(|a| a == &input.app_name);
        let approval = if skip_approval {
            // App is in the whitelist — approval skipped (advisory whitelist,
            // per spec §2.6 line 304). No approval record is persisted.
            tracing::info!(
                target = "skills.app_control",
                app = %input.app_name,
                "app in whitelist, approval skipped"
            );
            None
        } else {
            Some(record_approval_decision(
                kernel,
                approver,
                &effect_manifest,
                &ctx,
            )?)
        };

        // Branch on user_decision: Allow → proceed; Deny/Modify → cancel.
        // Only consulted when approval was required (skip_approval == false).
        if let Some(approval) = &approval {
            match approval.user_decision {
                ApprovalDecision::Deny => {
                    kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
                    return Err(KernelError::Skill("user denied app_control".to_string()));
                }
                ApprovalDecision::Modify => {
                    kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
                    return Err(KernelError::Skill(
                        "modify not supported for app_control".to_string(),
                    ));
                }
                ApprovalDecision::Allow => { /* proceed to commit */ }
            }
        }
    }

    // Step 7: branch on action.
    //
    // Plan 4 §2.6 specifies an `allowed_apps` whitelist
    // (`["notepad", "explorer", "calc"]` by default; Task 5 makes it
    // Settings-configurable via `kernel.set_allowed_apps()`). The
    // whitelist is consulted in Step 6 above for `Action::Launch` —
    // in-whitelist launches skip approval; out-of-whitelist launches
    // require PerStep approval. `Action::Focus` and `Action::Close`
    // always require approval regardless of whitelist membership.
    match input.action.as_str() {
        "launch" => {
            adapter.launch_app(&input.app_name).inspect_err(|_e| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            })?;
        }
        "focus" => {
            // Handle was resolved pre-approval (Step 3b): act on exactly
            // what the user approved, no second find (no TOCTOU gap).
            let (window, _) = resolved.ok_or_else(|| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                KernelError::Uia("internal error: focus window was not resolved".to_string())
            })?;
            adapter.click(&window).inspect_err(|_e| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            })?;
        }
        "close" => {
            // WM_CLOSE via the adapter (`window_management` close).
            // 标题栏按钮不在现代应用的 UIA 树里（Win11 标签页记事本实测
            // `ui_find` 找不到 Close），按 handle 关是唯一可靠路径。
            // `discard_changes=false`：脏窗口弹保存对话框、后端报失败，
            // 绝不静默丢用户数据。Handle 来自 Step 3b 的审批前解析，
            // 关的正是用户批准的那个窗口。
            let (window, _) = resolved.ok_or_else(|| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                KernelError::Uia("internal error: close window was not resolved".to_string())
            })?;
            adapter.close_window(&window, false).inspect_err(|_e| {
                let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            })?;
        }
        // Unreachable: ALLOWED_ACTIONS check above filters this. Kept for
        // exhaustiveness so future actions force an explicit branch.
        _ => {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Skill(format!(
                "unsupported action: '{}'",
                input.action
            )));
        }
    }

    // Step 8: finalize step as Succeeded with Weak evidence (UIA ops
    // produce no file artifacts — verification is by screenshot, deferred
    // to Plan 5).
    finalize_step_success(kernel, &input.step_id, "weak", None).inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    Ok(input.task_id.clone())
}

/// Build a descriptive EffectManifest for the approval prompt. UIA ops
/// have no file sources — the manifest carries the app_name as the
/// `destination` so the approval record has a non-empty target.
/// For focus/close the pre-resolved window title is appended
/// (`uia:{action}:{app}:{title}`), binding the approval to the exact
/// window (Step 3b); launch passes `None`.
fn build_app_control_effect_manifest(
    app_name: &str,
    action: &str,
    resolved_title: Option<&str>,
) -> EffectManifest {
    let destination = match resolved_title {
        Some(title) => format!("uia:{action}:{app_name}:{title}"),
        None => format!("uia:{action}:{app_name}"),
    };
    EffectManifest {
        sources: vec![],
        destination,
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::kernel::TrustKernel;
    use crate::uiautomation::{UiaElementHandle, UiaSelector};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn normalize_app_name_maps_cjk_aliases() {
        assert_eq!(normalize_app_name("记事本"), "notepad");
        assert_eq!(normalize_app_name("计算器"), "calc");
        assert_eq!(normalize_app_name("资源管理器"), "explorer");
        assert_eq!(normalize_app_name("文件资源管理器"), "explorer");
        assert_eq!(normalize_app_name("Microsoft Edge"), "msedge.exe");
        assert_eq!(normalize_app_name("Edge"), "msedge.exe");
        assert_eq!(normalize_app_name("浏览器"), "msedge.exe");
        assert_eq!(normalize_app_name("飞书"), "Feishu.exe");
        assert_eq!(normalize_app_name(" 飞书 "), "Feishu.exe");
        assert_eq!(normalize_app_name("notepad"), "notepad");
        assert_eq!(normalize_app_name(" 记事本 "), "notepad");
    }

    /// Captures the EffectManifest handed to the approval prompt.
    /// `Mutex` (not `RefCell`): `Approver: Send + Sync`.
    /// 2026 免审批后 app_control 不再提示 approver，本 struct 仅保留供
    /// 未来重新加入白名单时恢复断言。（当前无测试引用时编译器会提示未使用。）

    #[test]
    fn test_app_control_focus_executes_without_approval() {
        // 2026：quick.app_control 不在审批白名单 —— 不弹审批，直接执行。
        // focus 仍先解析窗口（Step 3b），点击为该解析出的 handle（无二次查窗）。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        state.borrow_mut().titled_title = Some("我的记事本".to_string());
        let adapter: &dyn UiaAdapter = &mock;

        let result = execute_app_control(&kernel, &make_input("focus"), &AutoApprover, adapter);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

        // Single find (Step 3b); Step 7 reuses the handle.
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        assert_eq!(state.borrow().click_calls, 1);
        // 免审批：不落审批记录（审计由 task/step 提供）。
        assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
    }

    #[test]
    fn test_app_control_alias_hits_whitelist_before_approval() {
        // 记事本 normalizes to notepad BEFORE the adapter call; 免审批后
        // approver 完全不consulted，adapter 收到规范化名。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let mut input = make_input("launch");
        input.app_name = "记事本".to_string();
        let result = execute_app_control(&kernel, &input, &AutoDenier, adapter);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(state.borrow().launch_calls, vec!["notepad".to_string()]);
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert!(
            approvals.is_empty(),
            "免审批不落审批记录，got {} records",
            approvals.len()
        );
    }

    #[test]
    fn test_app_control_close_executes_without_approval() {
        // 2026：close 免审批 —— 不弹窗，验证 find 一次 + close(discard=false)。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        state.borrow_mut().titled_title = Some("画图".to_string());
        let adapter: &dyn UiaAdapter = &mock;

        let mut input = make_input("close");
        input.app_name = "mspaint".to_string();
        let result = execute_app_control(&kernel, &input, &AutoApprover, adapter);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

        assert_eq!(
            state.borrow().find_window_calls,
            vec!["mspaint".to_string()]
        );
        assert_eq!(state.borrow().close_calls, vec![false]);
        assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
    }

    /// Recorded state shared between the MockAdapter and the test harness.
    /// Mirrors the design in `uiautomation/mod.rs` tests so we can inspect
    /// call counts after the adapter has been boxed as `dyn UiaAdapter`.
    #[derive(Default)]
    struct MockState {
        launch_calls: Vec<String>,
        find_window_calls: Vec<String>,
        find_element_calls: Vec<UiaSelector>,
        click_calls: usize,
        /// When `Some`, every fallible method returns `Err(KernelError::Uia(msg))`.
        next_error: Option<&'static str>,
        /// When `true`, `find_window` returns `Ok(None)` instead of `Ok(Some(handle))`.
        find_window_returns_none: bool,
        /// When `true`, `find_element` returns `Ok(None)` instead of
        /// `Ok(Some(handle))`.
        find_element_returns_none: bool,
        /// Every `close_window` call records its `discard_changes` here.
        close_calls: Vec<bool>,
        /// When `true`, `close_window` returns `Err` (server close failure).
        fail_close: bool,
        /// Title returned by the `find_window_titled` override (`None` →
        /// empty string, same as the default trait body).
        titled_title: Option<String>,
    }

    /// Mock adapter that records calls into a shared `Rc<RefCell<MockState>>`.
    struct MockAdapter {
        state: Rc<RefCell<MockState>>,
    }

    impl MockAdapter {
        fn new() -> Self {
            Self {
                state: Rc::new(RefCell::new(MockState::default())),
            }
        }

        /// Clone of the shared state handle — tests use this to inspect call
        /// counts after the adapter itself has been boxed as `dyn UiaAdapter`.
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

        fn find_window_titled(&self, query: &str) -> Result<Option<(UiaElementHandle, String)>> {
            // find_window call is recorded once here (Step 3b resolves once,
            // Step 7 reuses the handle — no second find).
            let found = self.find_window(query)?;
            let title = self.state.borrow().titled_title.clone().unwrap_or_default();
            Ok(found.map(|h| (h, title)))
        }

        fn find_element(
            &self,
            _root: &UiaElementHandle,
            selector: &UiaSelector,
        ) -> Result<Option<UiaElementHandle>> {
            let mut s = self.state.borrow_mut();
            s.find_element_calls.push(selector.clone());
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            if s.find_element_returns_none {
                Ok(None)
            } else {
                Ok(Some(UiaElementHandle::mock()))
            }
        }

        fn click(&self, _element: &UiaElementHandle) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.click_calls += 1;
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            Ok(())
        }

        fn close_window(&self, _window: &UiaElementHandle, discard_changes: bool) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.close_calls.push(discard_changes);
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
            }
            if s.fail_close {
                return Err(KernelError::Uia("mock close failed".to_string()));
            }
            Ok(())
        }

        fn set_text(&self, _element: &UiaElementHandle, _text: &str) -> Result<()> {
            Ok(())
        }

        fn get_text(&self, _element: &UiaElementHandle) -> Result<String> {
            Ok(String::new())
        }

        fn screenshot(&self, _element: &UiaElementHandle) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }
    }

    fn make_input(action: &str) -> AppControlInput {
        AppControlInput {
            task_id: "t1".to_string(),
            step_id: "s1".to_string(),
            app_name: "notepad".to_string(),
            action: action.to_string(),
        }
    }

    #[test]
    fn test_app_control_launch_action_with_mock_adapter() {
        // launch action with app OUTSIDE the whitelist: PerStep approval
        // (Allow) → adapter.launch_app. Verify launch_app is called once
        // with "notepad", step is Succeeded with weak evidence, and an
        // approval record is persisted. `allowed_apps = []` forces the
        // out-of-whitelist path (default whitelist contains "notepad",
        // which would skip approval — see `launch_in_whitelist_skips_approval`).
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel.set_allowed_apps(vec![]);
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("launch");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "t1");

        // launch_app was called once with "notepad".
        assert_eq!(state.borrow().launch_calls.len(), 1);
        assert_eq!(state.borrow().launch_calls[0], "notepad");
        // No find_window / click calls for launch.
        assert!(state.borrow().find_window_calls.is_empty());
        assert_eq!(state.borrow().click_calls, 0);

        // Step is Succeeded with weak evidence + no compensation.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));
        assert!(step.compensation_ref.is_none());

        // 免审批：不落审批记录。
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert!(approvals.is_empty());
    }

    #[test]
    fn launch_in_whitelist_skips_approval() {
        // 2026：quick.app_control 整体免审批 —— 无论白名单内外都无审批记录。
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel.set_allowed_apps(vec!["notepad".to_string()]);
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("launch");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "t1");

        // launch_app was called once with "notepad".
        assert_eq!(state.borrow().launch_calls.len(), 1);
        assert_eq!(state.borrow().launch_calls[0], "notepad");

        // Step is Succeeded with weak evidence.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));

        // No approval record (免审批)。
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert!(approvals.is_empty(), "got {} records", approvals.len());
    }

    #[test]
    fn launch_outside_whitelist_executes_without_approval() {
        // 2026：白名单内外一律免审批 —— AutoDenier 也不再拦截。
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel.set_allowed_apps(vec![]);
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("launch");
        // AutoDenier：旧姿态下会被拒；免审批后直接执行。
        let result = execute_app_control(&kernel, &input, &AutoDenier, adapter);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

        assert_eq!(state.borrow().launch_calls.len(), 1);
        assert_eq!(state.borrow().launch_calls[0], "notepad");

        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);

        assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
    }

    #[test]
    fn test_app_control_focus_action() {
        // focus action: PerStep approval (Allow) → adapter.find_window →
        // adapter.click on the returned handle. Verify find_window + click
        // are called, launch_app is NOT called, and step is Succeeded.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("focus");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "t1");

        // find_window was called once with "notepad".
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        assert_eq!(state.borrow().find_window_calls[0], "notepad");
        // click was called once on the found window.
        assert_eq!(state.borrow().click_calls, 1);
        // launch_app was NOT called.
        assert!(state.borrow().launch_calls.is_empty());

        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));
    }

    #[test]
    fn test_app_control_close_action() {
        // close action: find_window → close_window(handle, discard=false).
        // WM_CLOSE 按 handle 关（标题栏按钮不在现代应用 UIA 树里）。
        // discard=false：绝不静默丢用户数据。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("close");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

        // find_window was called once with "notepad".
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        assert_eq!(state.borrow().find_window_calls[0], "notepad");
        // close_window was called once with discard_changes=false.
        assert_eq!(state.borrow().close_calls, vec![false]);
        // find_element / click are NOT used by close anymore.
        assert!(state.borrow().find_element_calls.is_empty());
        assert_eq!(state.borrow().click_calls, 0);
        // launch_app was NOT called (close uses find_window, not launch).
        assert!(state.borrow().launch_calls.is_empty());

        // Step is Succeeded with weak evidence.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));

        // 免审批：不落审批记录。
        assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
    }

    #[test]
    fn test_app_control_close_fails_when_window_not_found() {
        // close action with find_window returning Ok(None) → step Failed
        // with "close failed: window not found". close_window is NOT
        // called (window not found short-circuits).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        state.borrow_mut().find_window_returns_none = true;
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("close");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Uia(ref m) if m == "close failed: window not found"),
            "expected Uia 'close failed: window not found' error, got {:?}",
            err
        );

        // find_window was called.
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        // close_window was NOT called (window not found short-circuits).
        assert!(state.borrow().close_calls.is_empty());

        // Step is marked Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);
    }

    #[test]
    fn test_app_control_close_fails_when_backend_close_fails() {
        // close action with close_window returning Err → step Failed,
        // error surfaces verbatim (e.g. dirty-window save dialog timeout).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        state.borrow_mut().fail_close = true;
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("close");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Uia(ref m) if m == "mock close failed"),
            "expected Uia mock close error, got {:?}",
            err
        );

        // find_window was called (returned Some(handle)).
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        // close_window was called once with discard_changes=false.
        assert_eq!(state.borrow().close_calls, vec![false]);

        // Step is marked Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);
    }

    #[test]
    fn test_app_control_invalid_action() {
        // action="invalid" fails validation before any DB or UIA call.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let mut input = make_input("launch");
        input.action = "invalid".to_string();
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Skill(_)),
            "expected Skill error, got {:?}",
            err
        );
        assert!(
            err.to_string().contains("validation failed for action"),
            "expected 'validation failed for action' in error, got: {}",
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
    fn test_app_control_focus_action_returns_err_when_no_window_found() {
        // When find_window returns Ok(None), focus action fails with a
        // Uia error and marks step Failed — no click is attempted.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        state.borrow_mut().find_window_returns_none = true;
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("focus");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Uia(ref m) if m.contains("no window found")),
            "expected Uia 'no window found' error, got {:?}",
            err
        );

        // find_window was called, but click was NOT (window not found).
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        assert_eq!(state.borrow().click_calls, 0);

        // Step is marked Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);
    }
}
