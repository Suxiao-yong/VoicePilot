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
//! Test 3 (`live_mcp_windows_launch_find_close`):
//!   Live round trip through `McpUiaAdapter` against a real mcp-windows
//!   server: launch notepad → find window → close tab. Auto-skips when
//!   `Sbroenne.WindowsMcp.exe` is absent (CI) or the user already has
//!   Notepad open (never touches чужі windows).
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
    use super::common::{with_temp_cwd, MockAdapter};
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::compensation::types::CompensationLevel;
    use trust_kernel::kernel::TrustKernel;
    use trust_kernel::repo::step_repo::StepStatus;
    use trust_kernel::skills::app_control::{execute_app_control, AppControlInput};
    use trust_kernel::skills::note_capture::{execute_note_capture, NoteCaptureInput};
    use trust_kernel::uiautomation::UiaAdapter;

    /// Test 1: `quick.app_control` launch notepad (mock adapter).
    ///
    /// End-to-end pipeline:
    /// 1. `TrustKernel::open_in_memory()` then `kernel.set_allowed_apps(vec![])`
    ///    to force out-of-whitelist path so PerStep approval is exercised
    ///    (default whitelist contains "notepad", which would skip approval
    ///    per spec §2.6 line 304).
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
        // Force out-of-whitelist path so PerStep approval is exercised.
        kernel.set_allowed_apps(vec![]);
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

        // 2026 免审批：不落审批记录（审计由 task/step 提供）。
        let approvals = kernel
            .list_approvals_for_task("smoke-task-1")
            .expect("list_approvals_for_task");
        assert!(approvals.is_empty(), "got {} records", approvals.len());
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
            // W10 Plan 2: note.capture 成功必须注册 note.reverse_capture 补偿
            // （旧断言 is_none() 是 Plan 2 之前的残留，已过期）。
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
                Some("Documents/test.txt")
            );

            // 2026 免审批：不落审批记录（审计由 task/step 提供）。
            let approvals = kernel
                .list_approvals_for_task("smoke-task-2")
                .expect("list_approvals_for_task");
            assert!(approvals.is_empty(), "got {} records", approvals.len());
        });
    }
}

#[cfg(all(windows, feature = "uia"))]
mod live_mcp {
    //! Test 3: live round trip through `McpUiaAdapter` (auto-skip, not ignore).
    //!
    //! Needs `Sbroenne.WindowsMcp.exe`: `MCP_WINDOWS_EXE` env or the repo
    //! `tools/mcp-windows/server/` copy. Skips (green) when absent — CI has
    //! no exe — and when the user already runs Notepad (we never close
    //! чужі tabs; the launched fresh tab is closed by its own handle).

    use std::path::PathBuf;
    use std::sync::Arc;
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::kernel::TrustKernel;
    use trust_kernel::mcp::repo::McpServerRepo;
    use trust_kernel::skills::app_control::{execute_app_control, AppControlInput};
    use trust_kernel::skills::dag_executor::set_thread_local_uia_adapter;
    use trust_kernel::uiautomation::adapter::McpUiaAdapter;
    use trust_kernel::uiautomation::UiaAdapter;

    fn exe_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("MCP_WINDOWS_EXE") {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Some(p);
            }
        }
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // crates/trust-kernel → crates → voicepilot/ → repo root → tools/…
        // （曾少一层 `..`，导致恒 SKIP、活覆盖静默丢失）。
        p.push("..");
        p.push("..");
        p.push("..");
        p.push("tools");
        p.push("mcp-windows");
        p.push("server");
        p.push("Sbroenne.WindowsMcp.exe");
        p.is_file().then_some(p)
    }

    fn register_server(kernel: &TrustKernel, exe: &PathBuf) {
        // conn guard 不得活过本函数：executor 内部还要锁 conn（同线程重锁会死锁）。
        let repo = McpServerRepo::new();
        let conn = kernel.conn();
        repo.delete(&conn, "mcp-windows-test").unwrap();
        let mut rec = repo.get(&conn, "mcp-windows").unwrap().unwrap();
        rec.server_id = "mcp-windows-test".to_string();
        rec.command = Some(exe.to_string_lossy().into_owned());
        rec.enabled = true;
        repo.create(&conn, &rec).unwrap();
    }

    #[test]
    fn live_mcp_windows_launch_find_close() {
        let Some(exe) = exe_path() else {
            eprintln!("SKIP: Sbroenne.WindowsMcp.exe not found (set MCP_WINDOWS_EXE)");
            return;
        };
        let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
        register_server(&kernel, &exe);
        let adapter = McpUiaAdapter::for_server(&kernel, "mcp-windows-test").expect("resolve");
        if adapter.find_window("Notepad").expect("pre-check").is_some() {
            eprintln!("SKIP: user already runs Notepad, refusing to touch it");
            return;
        }
        set_thread_local_uia_adapter(Some(Arc::new(adapter)));
        let adapter_ref =
            trust_kernel::skills::dag_executor::thread_local_uia_adapter().expect("injected");
        let launch = AppControlInput {
            task_id: "live-launch".to_string(),
            step_id: "live-launch-s".to_string(),
            app_name: "notepad".to_string(),
            action: "launch".to_string(),
        };
        execute_app_control(&kernel, &launch, &AutoApprover, adapter_ref.as_ref())
            .expect("launch must succeed");
        // Fresh tab is empty → WM_CLOSE needs no save dialog.
        let close = AppControlInput {
            task_id: "live-close".to_string(),
            step_id: "live-close-s".to_string(),
            app_name: "notepad".to_string(),
            action: "close".to_string(),
        };
        execute_app_control(&kernel, &close, &AutoApprover, adapter_ref.as_ref())
            .expect("close must succeed");
        set_thread_local_uia_adapter(None);
        // Window really gone (new client to avoid adapter borrow issues).
        let adapter2 = McpUiaAdapter::for_server(&kernel, "mcp-windows-test").expect("resolve");
        assert!(
            adapter2.find_window("Notepad").expect("verify").is_none(),
            "notepad window must be gone after close"
        );
    }
}
