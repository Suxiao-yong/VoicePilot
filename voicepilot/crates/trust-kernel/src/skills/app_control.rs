//! quick.app_control Skill executor — W7 Plan 4 Task 3.
//!
//! Launches, focuses, or closes a Windows application via the UIA adapter.
//! Risk E2 (UIA can drive arbitrary GUI actions), approval PerStep —
//! the approver sees an EffectManifest describing the planned action and
//! must Allow before the adapter is invoked.
//!
//! Pipeline:
//!   1. Validate `app_name` + `action` inputs (non-empty + action ∈
//!      {launch, focus, close}).
//!   2. Validate the full input map against `app_control_manifest`.
//!   3. Create new task + step.
//!   4. Update step → Running.
//!   5. Record approval decision (E2 + PerStep). Branch on Allow/Deny/Modify.
//!   6. Branch on action:
//!      - launch → adapter.launch_app(app_name)
//!      - focus  → adapter.find_window(app_name) → adapter.click(handle)
//!      - close  → TODO: not yet implemented (returns Uia error)
//!   7. Finalize step as Succeeded with Weak evidence (no file evidence).
//!
//! W7 Plan 4 Task 5 (review fix): the `allowed_apps` whitelist
//! (`["notepad", "explorer", "calc"]` by default, Settings-configurable)
//! is NO LONGER enforced by the manifest's `app_name` `allowed_values`
//! (the input is now free-form Text). The runtime whitelist lives in
//! `kernel.allowed_apps()` (Settings-persisted in KV "uia.allowed_apps").
//! Per Plan 4 §2.6, PerStep approval is mandatory for ALL launches
//! regardless of whitelist membership — the approval gate is the
//! security boundary, never skipped. The whitelist is advisory: it
//! tells the user "these apps are pre-approved" but does not change
//! the approval flow. A future enhancement could skip approval for
//! in-whitelist apps (out of scope for Plan 4 Task 5).

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
use crate::uiautomation::UiaAdapter;
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

    // Step 4: build EffectManifest for the approval prompt. UIA ops have
    // no file sources — the manifest is purely descriptive; the
    // `destination` carries the action+app_name so the approval record
    // has a non-empty target.
    let effect_manifest = build_app_control_effect_manifest(&input.app_name, &input.action);

    // Step 5: update step → Running.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 6: record approval decision (E2 + PerStep). The approver sees
    // the effect_manifest describing what will happen. The
    // preconditions_hash binds this approval to the exact {action, app_name}
    // pair so post-hoc audit can verify what the user actually approved.
    let preconditions_hash = {
        let mut hasher = Sha256::new();
        hasher.update(input.action.as_bytes());
        hasher.update(b"\x00");
        hasher.update(input.app_name.as_bytes());
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

    // Step 7: branch on action.
    //
    // Plan 4 §2.6 specifies an `allowed_apps` whitelist
    // (`["notepad", "explorer", "calc"]` by default; Task 5 makes it
    // Settings-configurable via `kernel.set_allowed_apps()`). The
    // whitelist is ADVISORY — PerStep approval above is mandatory for
    // ALL launches regardless of whitelist membership, so no separate
    // whitelist branch is needed here. (Task 5 review fix: manifest
    // `app_name` is now free-form Text; the whitelist lives in
    // `kernel.allowed_apps()` and is checked nowhere in the executor —
    // a future enhancement could skip approval for in-whitelist apps.)
    match input.action.as_str() {
        "launch" => {
            adapter
                .launch_app(&input.app_name)
                .inspect_err(|_e| {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                })?;
        }
        "focus" => {
            let window = adapter
                .find_window(&input.app_name)
                .inspect_err(|_e| {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                })?;
            let window = match window {
                Some(h) => h,
                None => {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                    return Err(KernelError::Uia(format!(
                        "no window found matching '{}'",
                        input.app_name
                    )));
                }
            };
            adapter
                .click(&window)
                .inspect_err(|_e| {
                    let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
                })?;
        }
        "close" => {
            // TODO(Plan 5+): implement close — either send a WM_CLOSE
            // event via `uiautomation::UIElement::send_close` (if added
            // to the trait), or locate the window's Close button via
            // `find_element(root, ByName("Close"))` and `click` it. For
            // now, return a Uia error so callers can detect the
            // unimplemented path without a panic. find_window is NOT
            // called — the close action is a stub and would waste a
            // UIA round-trip.
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
            return Err(KernelError::Uia(
                "close action not yet implemented".to_string(),
            ));
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
    finalize_step_success(kernel, &input.step_id, "weak", None)
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    Ok(input.task_id.clone())
}

/// Build a descriptive EffectManifest for the approval prompt. UIA ops
/// have no file sources — the manifest carries the app_name as the
/// `destination` so the approval record has a non-empty target.
fn build_app_control_effect_manifest(app_name: &str, action: &str) -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: format!("uia:{}:{}", action, app_name),
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::uiautomation::{UiaElementHandle, UiaSelector};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Recorded state shared between the MockAdapter and the test harness.
    /// Mirrors the design in `uiautomation/mod.rs` tests so we can inspect
    /// call counts after the adapter has been boxed as `dyn UiaAdapter`.
    #[derive(Default)]
    struct MockState {
        launch_calls: Vec<String>,
        find_window_calls: Vec<String>,
        click_calls: usize,
        /// When `Some`, every fallible method returns `Err(KernelError::Uia(msg))`.
        next_error: Option<&'static str>,
        /// When `true`, `find_window` returns `Ok(None)` instead of `Ok(Some(handle))`.
        find_window_returns_none: bool,
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

        fn find_element(
            &self,
            _root: &UiaElementHandle,
            _selector: &UiaSelector,
        ) -> Result<Option<UiaElementHandle>> {
            // Unused by app_control executor; return a mock handle for
            // trait completeness.
            Ok(Some(UiaElementHandle::mock()))
        }

        fn click(&self, _element: &UiaElementHandle) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.click_calls += 1;
            if let Some(msg) = s.next_error {
                return Err(KernelError::Uia(msg.to_string()));
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
        // launch action: PerStep approval (Allow) → adapter.launch_app.
        // Verify launch_app is called once with "notepad", step is
        // Succeeded with weak evidence, and an approval record is
        // persisted.
        let kernel = TrustKernel::open_in_memory().unwrap();
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

        // Approval was recorded (PerStep).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].e_level, ELevel::E2);
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
        // close action is not yet implemented — returns Uia error and
        // marks step Failed. find_window is NOT called (close is a stub;
        // calling find_window would waste a UIA round-trip).
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = make_input("close");
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        let err = result.unwrap_err();
        assert!(
            matches!(err, KernelError::Uia(ref m) if m.contains("close action not yet implemented")),
            "expected Uia 'close action not yet implemented' error, got {:?}",
            err
        );

        // Step is marked Failed.
        let step = kernel.get_step("s1").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Failed);

        // find_window was NOT called (close stub returns early).
        assert!(state.borrow().find_window_calls.is_empty());
        // launch_app and click were NOT called either.
        assert!(state.borrow().launch_calls.is_empty());
        assert_eq!(state.borrow().click_calls, 0);

        // Approval was still recorded (PerStep — approval happens before
        // the action branch, so the user's Allow decision is persisted
        // even though the action failed).
        let approvals = kernel.list_approvals_for_task("t1").unwrap();
        assert_eq!(approvals.len(), 1);
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
