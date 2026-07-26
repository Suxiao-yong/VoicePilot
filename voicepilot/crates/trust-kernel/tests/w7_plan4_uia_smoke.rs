//! W7 Plan 4 Task 7 — end-to-end smoke tests for the UIA Skill pipeline.
//!
//! Exercises the full pipeline (route_text → router → executor → adapter →
//! filesystem) using mock adapters for headless CI, plus `#[ignore]`-tagged
//! real-GUI tests that launch actual Notepad on Windows (manually runnable).
//!
//! Test 1 (`smoke_app_control_launch_notepad_with_mock_adapter`):
//!   `quick.app_control` launch action → MockAdapter records
//!   `launch_app("notepad")` → step Succeeded with weak evidence → approval
//!   record persisted (E2 + PerStep).
//!
//! Test 2 (`smoke_note_capture_writes_content_with_mock_adapter`):
//!   `note.capture` → MockAdapter records `launch_app("notepad")` +
//!   `set_text("hello")` → file written to disk at `<temp>/Documents/test.txt`
//!   → step Succeeded with strong evidence → approval record persisted.
//!
//! Test 3 (`real_gui_notepad_launch_settext_close`):
//!   Real `WindowsUiaAdapter` launches actual Notepad, sets text on its edit
//!   control, (cleanup is manual). `#[ignore]`-tagged — run with
//!   `cargo test -p trust-kernel --features uia --test w7_plan4_uia_smoke --
//!   --ignored real_gui`.
//!
//! Platform / feature gate: every item is `#[cfg(all(windows, feature = "uia"))]`-gated
//! so the default build (no `uia` feature, or non-Windows) compiles cleanly.

#[cfg(all(windows, feature = "uia"))]
mod common {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Mutex;

    use trust_kernel::error::{KernelError, Result};
    use trust_kernel::uiautomation::{UiaAdapter, UiaElementHandle, UiaSelector};

    /// Recorded state shared between the `MockAdapter` and the test harness.
    /// Mirrors the design in `note_capture.rs` tests so we can inspect call
    /// counts after the adapter has been passed to the executor as
    /// `&dyn UiaAdapter`.
    #[derive(Default)]
    pub struct MockState {
        pub launch_calls: Vec<String>,
        pub find_window_calls: Vec<String>,
        pub click_calls: usize,
        pub set_text_calls: Vec<String>,
        /// When `Some`, every fallible method returns `Err(KernelError::Uia(msg))`.
        pub next_error: Option<&'static str>,
        /// When `true`, `find_window` returns `Ok(None)` instead of `Ok(Some(handle))`.
        pub find_window_returns_none: bool,
    }

    /// Mock adapter that records calls into a shared `Rc<RefCell<MockState>>`.
    pub struct MockAdapter {
        state: Rc<RefCell<MockState>>,
    }

    impl MockAdapter {
        pub fn new() -> Self {
            Self {
                state: Rc::new(RefCell::new(MockState::default())),
            }
        }

        /// Clone of the shared state handle — tests use this to inspect call
        /// counts after the adapter itself has been passed to the executor.
        pub fn state_handle(&self) -> Rc<RefCell<MockState>> {
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
            // Unused by app_control / note_capture executors; return a mock
            // handle for trait completeness.
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

        fn set_text(&self, _element: &UiaElementHandle, text: &str) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.set_text_calls.push(text.to_string());
            if let Some(msg) = s.next_error {
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

    /// Global mutex serializing tests that change CWD. Without this, parallel
    /// test threads racing on `set_current_dir` would corrupt each other's
    /// file writes (CWD is process-global).
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    /// RAII guard that restores the original CWD on drop. Paired with
    /// `CWD_MUTEX` to ensure tests don't race on process-global CWD.
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
    /// exists. Use a relative `save_path` like `"Documents/<uuid>.txt"` inside
    /// `body` so it satisfies the manifest's `allowed_roots: ["Documents",
    /// "Desktop"]` constraint AND writes to `<temp>/Documents/<uuid>.txt`
    /// (a real temp location).
    ///
    /// The `CWD_MUTEX` serializes all callers so parallel test threads don't
    /// race on `set_current_dir`. We recover from poison
    /// (`unwrap_or_else(|e| e.into_inner())`) so a panicking sibling test
    /// doesn't mask the real failure in this test. The `CwdGuard` restores
    /// CWD on drop, even if the test body panics. The `TempDir` cleans up the
    /// temp directory on drop.
    pub fn with_temp_cwd<F, R>(body: F) -> R
    where
        F: FnOnce(&std::path::Path) -> R,
    {
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join("Documents")).expect("create Documents dir");
        let _cwd = CwdGuard::enter(temp.path());
        body(temp.path())
        // `_cwd` drops here → restore CWD.
        // `_guard` drops here → release mutex.
        // `temp` drops here → cleanup temp dir.
    }
}

#[cfg(all(windows, feature = "uia"))]
mod smoke {
    use super::common::{MockAdapter, with_temp_cwd};
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::kernel::TrustKernel;
    use trust_kernel::policy::types::ELevel;
    use trust_kernel::repo::step_repo::StepStatus;
    use trust_kernel::skills::app_control::{execute_app_control, AppControlInput};
    use trust_kernel::skills::note_capture::{execute_note_capture, NoteCaptureInput};
    use trust_kernel::uiautomation::UiaAdapter;

    /// Test 1: `quick.app_control` launch notepad (mock adapter).
    ///
    /// End-to-end pipeline:
    /// 1. `TrustKernel::open_in_memory()`
    /// 2. `MockAdapter` records calls into `Rc<RefCell<MockState>>`
    /// 3. `AutoApprover`
    /// 4. `AppControlInput { task_id, step_id, app_name: "notepad", action: "launch" }`
    /// 5. `execute_app_control(kernel, input, approver, adapter)`
    /// 6. Assert `Ok(task_id)`
    /// 7. Assert mock recorded `launch_app("notepad")` once
    /// 8. Assert no `find_window` / `click` calls (launch only)
    /// 9. Assert step status is `Succeeded` with weak evidence
    /// 10. Assert an approval record was persisted (E2 + PerStep)
    #[test]
    fn smoke_app_control_launch_notepad_with_mock_adapter() {
        let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
        let approver = AutoApprover;
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: &dyn UiaAdapter = &mock;

        let input = AppControlInput {
            task_id: "smoke-task-1".to_string(),
            step_id: "smoke-step-1".to_string(),
            app_name: "notepad".to_string(),
            action: "launch".to_string(),
        };
        let result = execute_app_control(&kernel, &input, &approver, adapter);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "smoke-task-1");

        // launch_app was called once with "notepad".
        assert_eq!(state.borrow().launch_calls.len(), 1);
        assert_eq!(state.borrow().launch_calls[0], "notepad");
        // No find_window / click calls for launch action.
        assert!(state.borrow().find_window_calls.is_empty());
        assert_eq!(state.borrow().click_calls, 0);

        // Step is Succeeded with weak evidence + no compensation.
        let step = kernel
            .get_step("smoke-step-1")
            .expect("get_step")
            .expect("step must exist");
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));
        assert!(step.compensation_ref.is_none());

        // Approval was recorded (PerStep, E2).
        let approvals = kernel
            .list_approvals_for_task("smoke-task-1")
            .expect("list_approvals_for_task");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].e_level, ELevel::E2);
    }

    /// Test 2: `note.capture` writes "hello" (mock adapter).
    ///
    /// End-to-end pipeline:
    /// 1. `TrustKernel::open_in_memory()`
    /// 2. `with_temp_cwd` sets CWD to a temp dir with `Documents/` subdir
    /// 3. `MockAdapter` + `AutoApprover`
    /// 4. `NoteCaptureInput { task_id, step_id, content: "hello", save_path: "Documents/test.txt" }`
    /// 5. `execute_note_capture(kernel, input, approver, adapter)`
    /// 6. Assert `Ok(task_id)`
    /// 7. Assert mock recorded `launch_app("notepad")` + `set_text("hello")`
    /// 8. Assert file exists at `<temp>/Documents/test.txt` with content "hello"
    /// 9. Assert step status is `Succeeded` with strong evidence (real file artifact)
    /// 10. Assert an approval record was persisted
    #[test]
    fn smoke_note_capture_writes_content_with_mock_adapter() {
        with_temp_cwd(|temp| {
            let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
            let approver = AutoApprover;
            let mock = MockAdapter::new();
            let state = mock.state_handle();
            let adapter: &dyn UiaAdapter = &mock;

            let input = NoteCaptureInput {
                task_id: "smoke-task-2".to_string(),
                step_id: "smoke-step-2".to_string(),
                content: "hello".to_string(),
                save_path: "Documents/test.txt".to_string(),
            };
            let result = execute_note_capture(&kernel, &input, &approver, adapter);

            assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
            assert_eq!(result.unwrap(), "smoke-task-2");

            // launch_app was called once with "notepad".
            assert_eq!(state.borrow().launch_calls.len(), 1);
            assert_eq!(state.borrow().launch_calls[0], "notepad");
            // set_text was called once with "hello".
            assert_eq!(state.borrow().set_text_calls.len(), 1);
            assert_eq!(state.borrow().set_text_calls[0], "hello");

            // File exists on disk with correct content.
            let file_path = temp.join("Documents/test.txt");
            let on_disk = std::fs::read_to_string(&file_path)
                .expect("file should exist at <temp>/Documents/test.txt");
            assert_eq!(on_disk, "hello");

            // Step is Succeeded with strong evidence (real file artifact).
            let step = kernel
                .get_step("smoke-step-2")
                .expect("get_step")
                .expect("step must exist");
            assert_eq!(step.status, StepStatus::Succeeded);
            assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
            assert!(step.compensation_ref.is_none());

            // Approval was recorded (PerStep, E2).
            let approvals = kernel
                .list_approvals_for_task("smoke-task-2")
                .expect("list_approvals_for_task");
            assert_eq!(approvals.len(), 1);
            assert_eq!(approvals[0].e_level, ELevel::E2);
        });
    }
}

#[cfg(all(windows, feature = "uia"))]
mod real_gui {
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::kernel::TrustKernel;
    use trust_kernel::uiautomation::adapter::WindowsUiaAdapter;
    use trust_kernel::uiautomation::{UiaAdapter, UiaSelector};

    /// Test 3: real-GUI Notepad launch + set_text + close (`#[ignore]`).
    ///
    /// This test launches a REAL Notepad process via `WindowsUiaAdapter`. It
    /// is `#[ignore]`-tagged so CI doesn't run it; run manually:
    ///   `cargo test -p trust-kernel --features uia --test w7_plan4_uia_smoke -- --ignored real_gui`
    ///
    /// Steps:
    /// 1. `TrustKernel::open_in_memory()` — verify kernel constructs OK.
    /// 2. `WindowsUiaAdapter::new()` — real adapter (initializes COM).
    /// 3. `AutoApprover` — verify it coexists with the real adapter.
    /// 4. `adapter.launch_app("notepad")` — spawns real notepad.exe.
    /// 5. `adapter.find_window("Notepad")` — waits for the window to appear.
    /// 6. `adapter.find_element(window, ByRole("Edit"))` — locate the text
    ///    area. `set_text` requires an element supporting `UIValuePattern`;
    ///    the window itself doesn't, but the Edit control does.
    /// 7. `adapter.set_text(edit, "VoicePilot UIA smoke test")` — write text.
    /// 8. Assert `set_text` succeeded.
    /// 9. (Cleanup) Print "please close Notepad manually" — the `close`
    ///    action is not yet implemented (see `app_control.rs`).
    ///
    /// NOTE: This test will fail if Notepad isn't available or if the test
    /// runs without a desktop session (e.g., over SSH without an interactive
    /// Windows session). The `#[ignore]` tag makes this explicit.
    #[test]
    #[ignore = "requires real Windows GUI; run with --ignored --features uia manually"]
    fn real_gui_notepad_launch_settext_close() {
        // 1. Kernel — verify it constructs alongside the real adapter.
        let _kernel = TrustKernel::open_in_memory().expect("open_in_memory");
        // 2. Real adapter (initializes COM for the calling thread).
        let adapter = WindowsUiaAdapter::new().expect("WindowsUiaAdapter::new");
        // 3. AutoApprover — unused here (we call the adapter directly), but
        //    constructed to verify it coexists with the real adapter.
        let _approver = AutoApprover;

        // 4. Launch real Notepad. `Command::new("notepad")` resolves via PATH
        //    on Windows (→ C:\Windows\System32\notepad.exe). The adapter waits
        //    up to 3s for the window to appear.
        let _app_handle = adapter
            .launch_app("notepad")
            .expect("launch_app('notepad') should succeed");

        // 5. Find the Notepad window by title. `find_window` returns
        //    `Ok(None)` if no window matches within timeout(0).
        let window = adapter
            .find_window("Notepad")
            .expect("find_window('Notepad') should not error")
            .expect("Notepad window should be found after launch");

        // 6. Locate the Edit control inside the Notepad window. `set_text`
        //    requires an element supporting `UIValuePattern`; the window
        //    itself doesn't, but its Edit child does.
        let edit = adapter
            .find_element(&window, &UiaSelector::ByRole("Edit".to_string()))
            .expect("find_element(ByRole('Edit')) should not error")
            .expect("Notepad window should contain an Edit control");

        // 7. Set the text. This calls `UIValuePattern::set_value` on the
        //    Edit control.
        adapter
            .set_text(&edit, "VoicePilot UIA smoke test")
            .expect("set_text on Edit control should succeed");

        // 8. Assert set_text succeeded (implicit — expect didn't panic).
        //    Optionally read back the text to verify.
        let read_back = adapter
            .get_text(&edit)
            .expect("get_text should succeed after set_text");
        assert_eq!(
            read_back, "VoicePilot UIA smoke test",
            "text read back from Notepad should match what we set"
        );

        // 9. Cleanup: the `close` action is not yet implemented (see
        //    `app_control.rs`). Print a manual cleanup message.
        eprintln!("[real_gui_notepad_launch_settext_close] please close Notepad manually");
    }
}
