//! UIA adapter — Windows UI Automation abstraction.
//!
//! W7 Plan 4 Task 2: define a `UiaAdapter` trait + `WindowsUiaAdapter` impl so
//! the trust kernel can drive Windows GUI applications (launch, find, click,
//! read/set text, screenshot) without leaking `uiautomation` crate types to
//! callers. The trait is object-safe so unit tests can swap in a mock without
//! touching real UIA / Windows GUI.
//!
//! Platform / feature gate: this module is only compiled under
//! `cfg(all(windows, feature = "uia"))` (see `lib.rs`). The `uiautomation`
//! crate is Windows-only and `UIAutomation` / `UIElement` are `!Send`/`!Sync`
//! (COM apartment model), so `UiaElementHandle` and `WindowsUiaAdapter` are
//! also `!Send`/`!Sync` — they must live on the thread that created them.

use crate::error::Result;
use uiautomation::core::UIElement;

pub mod adapter;

/// Opaque wrapper around a UI Automation element.
///
/// Holds an `Option<UIElement>` so unit tests can construct a placeholder
/// handle (`None`) without instantiating the real UIA client. The real
/// `WindowsUiaAdapter` always produces handles wrapping `Some(element)`.
///
/// Field is private to avoid leaking `uiautomation` types through the public
/// API; access goes through `as_element` (crate-visible) and `from_element`.
pub struct UiaElementHandle {
    inner: Option<UIElement>,
}

impl std::fmt::Debug for UiaElementHandle {
    /// Manual impl — don't require `UIElement: Debug` (we only surface whether
    /// the handle wraps an element). Needed so `Result<UiaElementHandle>::unwrap_err`
    /// works in tests.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiaElementHandle")
            .field("has_element", &self.inner.is_some())
            .finish()
    }
}

impl UiaElementHandle {
    /// Wrap a real `UIElement` returned by the UIAutomation crate.
    pub fn from_element(element: UIElement) -> Self {
        Self { inner: Some(element) }
    }

    /// Construct a mock handle with no underlying `UIElement`. Test-only —
    /// production code should always obtain a handle from `launch_app` /
    /// `find_window` / `find_element`. Exposed as `pub` (rather than
    /// `pub(crate)`) so integration tests in `tests/` can construct mock
    /// adapters without re-implementing the `UiaElementHandle` wrapper.
    ///
    /// Feature gate: `cfg(any(test, feature = "uia"))` ensures the mock
    /// constructor is only available in test builds or when the `uia` feature
    /// is enabled (follow-up #6). Production builds without `uia` won't link
    /// the symbol.
    #[cfg(any(test, feature = "uia"))]
    pub fn mock() -> Self {
        Self { inner: None }
    }

    /// Borrow the underlying `UIElement`, if any. Crate-visible so the
    /// `WindowsUiaAdapter` can pull the element back out for API calls.
    pub(crate) fn as_element(&self) -> Option<&UIElement> {
        self.inner.as_ref()
    }
}

/// Element selector — chooses which UIA property to match on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiaSelector {
    /// Match by `AutomationId` property.
    ById(String),
    /// Match by `Name` property (case-insensitive contains).
    ByName(String),
    /// Match by localized control type / role name (e.g. "Button", "Edit").
    ByRole(String),
}

/// UI Automation adapter — abstracts Windows UIA so callers (and tests) can
/// mock it. All methods return `crate::error::Result<T>` so errors flow
/// through the trust kernel's unified `KernelError` channel.
pub trait UiaAdapter {
    /// Launch an application by executable name (e.g. `"notepad.exe"`) and
    /// return a handle to its main window element.
    fn launch_app(&self, app_name: &str) -> Result<UiaElementHandle>;

    /// Find the first top-level window whose title contains `title_contains`
    /// (case-insensitive). Returns `Ok(None)` if no such window exists.
    fn find_window(&self, title_contains: &str) -> Result<Option<UiaElementHandle>>;

    /// Find the first descendant element of `root` matching `selector`.
    /// Returns `Ok(None)` if no match is found.
    fn find_element(
        &self,
        root: &UiaElementHandle,
        selector: &UiaSelector,
    ) -> Result<Option<UiaElementHandle>>;

    /// Simulate a left mouse click on `element`.
    fn click(&self, element: &UiaElementHandle) -> Result<()>;

    /// Replace the text value of `element` (uses `UIValuePattern`).
    fn set_text(&self, element: &UiaElementHandle, text: &str) -> Result<()>;

    /// Read the text value of `element` (uses `UIValuePattern`).
    fn get_text(&self, element: &UiaElementHandle) -> Result<String>;

    /// Capture a screenshot of `element` and return it as PNG bytes.
    fn screenshot(&self, element: &UiaElementHandle) -> Result<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Recorded state shared between the `MockAdapter` and the test harness.
    /// Using `Rc<RefCell<MockState>>` lets the test inspect call counts after
    /// the adapter has been moved into a `Box<dyn UiaAdapter>` (which can't
    /// be downcast without an `Any` supertrait).
    #[derive(Default)]
    struct MockState {
        launch_calls: Vec<String>,
        find_window_calls: Vec<String>,
        find_element_calls: Vec<(String, UiaSelector)>,
        click_calls: usize,
        set_text_calls: Vec<String>,
        get_text_calls: usize,
        screenshot_calls: usize,
        /// When `Some`, every method returns `Err(KernelError::Uia(msg))`.
        next_error: Option<&'static str>,
    }

    /// Mock adapter that records calls into a shared `Rc<RefCell<MockState>>`
    /// and returns preset values. Implements `UiaAdapter` so we can exercise
    /// the trait through `Box<dyn UiaAdapter>`.
    struct MockAdapter {
        state: Rc<RefCell<MockState>>,
        fake_text: &'static str,
        fake_screenshot: &'static [u8],
    }

    impl MockAdapter {
        fn new() -> Self {
            Self {
                state: Rc::new(RefCell::new(MockState::default())),
                fake_text: "hello",
                fake_screenshot: &[0u8, 1, 2, 3],
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
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            Ok(UiaElementHandle::mock())
        }

        fn find_window(&self, title_contains: &str) -> Result<Option<UiaElementHandle>> {
            let mut s = self.state.borrow_mut();
            s.find_window_calls.push(title_contains.to_string());
            if let Some(msg) = s.next_error {
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            Ok(Some(UiaElementHandle::mock()))
        }

        fn find_element(
            &self,
            root: &UiaElementHandle,
            selector: &UiaSelector,
        ) -> Result<Option<UiaElementHandle>> {
            let mut s = self.state.borrow_mut();
            // Tag the root by whether it wraps a real element — mocks wrap
            // `None`, real `WindowsUiaAdapter` handles wrap `Some(_)`.
            let root_tag = if root.as_element().is_none() { "mock" } else { "real" };
            s.find_element_calls.push((root_tag.to_string(), selector.clone()));
            if let Some(msg) = s.next_error {
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            Ok(Some(UiaElementHandle::mock()))
        }

        fn click(&self, _element: &UiaElementHandle) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.click_calls += 1;
            if let Some(msg) = s.next_error {
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            Ok(())
        }

        fn set_text(&self, _element: &UiaElementHandle, text: &str) -> Result<()> {
            let mut s = self.state.borrow_mut();
            s.set_text_calls.push(text.to_string());
            if let Some(msg) = s.next_error {
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            Ok(())
        }

        fn get_text(&self, _element: &UiaElementHandle) -> Result<String> {
            let mut s = self.state.borrow_mut();
            s.get_text_calls += 1;
            if let Some(msg) = s.next_error {
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            // Drop the borrow before reading `fake_text` (separate field, no
            // RefCell borrow conflict, but be explicit for clarity).
            drop(s);
            Ok(self.fake_text.to_string())
        }

        fn screenshot(&self, _element: &UiaElementHandle) -> Result<Vec<u8>> {
            let mut s = self.state.borrow_mut();
            s.screenshot_calls += 1;
            if let Some(msg) = s.next_error {
                return Err(crate::error::KernelError::Uia(msg.to_string()));
            }
            drop(s);
            Ok(self.fake_screenshot.to_vec())
        }
    }

    #[test]
    fn uia_selector_construction_and_match() {
        let by_id = UiaSelector::ById("save-btn".to_string());
        let by_name = UiaSelector::ByName("Save".to_string());
        let by_role = UiaSelector::ByRole("Button".to_string());

        // Match on a reference so we don't partially move the enum variants
        // before the later `clone()` / `assert_ne!` calls.
        match &by_id {
            UiaSelector::ById(s) => assert_eq!(s, "save-btn"),
            _ => panic!("expected ById"),
        }
        match &by_name {
            UiaSelector::ByName(s) => assert_eq!(s, "Save"),
            _ => panic!("expected ByName"),
        }
        match &by_role {
            UiaSelector::ByRole(s) => assert_eq!(s, "Button"),
            _ => panic!("expected ByRole"),
        }

        // Clone + Eq so callers can de-dup selectors.
        assert_eq!(by_id.clone(), UiaSelector::ById("save-btn".to_string()));
        assert_ne!(by_id, by_name);
    }

    #[test]
    fn mock_adapter_launch_find_settext_call_chain() {
        // Exercise the trait through a `Box<dyn UiaAdapter>` to prove the
        // mock path works without touching real Windows GUI. State is inspected
        // via a cloned `Rc<RefCell<MockState>>` handle.
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        let adapter: Box<dyn UiaAdapter> = Box::new(mock);

        // 1. launch_app -> returns a handle.
        let app_handle = adapter
            .launch_app("notepad.exe")
            .expect("launch should succeed");
        assert!(app_handle.as_element().is_none(), "mock handle wraps no element");
        assert_eq!(state.borrow().launch_calls.len(), 1);
        assert_eq!(state.borrow().launch_calls[0], "notepad.exe");

        // 2. find_window -> returns Some(handle).
        let window = adapter
            .find_window("Untitled")
            .expect("find_window should succeed")
            .expect("find_window should return Some");
        assert!(window.as_element().is_none());
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        assert_eq!(state.borrow().find_window_calls[0], "Untitled");

        // 3. find_element(root=window, ById) -> returns Some(handle).
        let selector = UiaSelector::ById("edit-area".to_string());
        let edit = adapter
            .find_element(&window, &selector)
            .expect("find_element should succeed")
            .expect("find_element should return Some");
        assert!(edit.as_element().is_none());
        assert_eq!(state.borrow().find_element_calls.len(), 1);
        // Clone the recorded entry out of the RefCell so the borrow is released
        // before we call any further adapter methods (otherwise `borrow_mut`
        // inside `set_text` would panic with "RefCell already borrowed").
        let (root_tag, sel) = state.borrow().find_element_calls[0].clone();
        assert_eq!(root_tag, "mock");
        assert_eq!(sel, selector);

        // 4. set_text on the edit element, then click + screenshot the window.
        adapter
            .set_text(&edit, "hello world")
            .expect("set_text should succeed");
        adapter.click(&window).expect("click should succeed");
        let bytes = adapter
            .screenshot(&window)
            .expect("screenshot should succeed");
        assert_eq!(bytes, vec![0u8, 1, 2, 3]);

        // 5. get_text reads back what set_text wrote (mock returns fake_text).
        let text = adapter.get_text(&edit).expect("get_text should succeed");
        assert_eq!(text, "hello");

        // Call counters prove the chain executed in order.
        assert_eq!(state.borrow().click_calls, 1);
        assert_eq!(state.borrow().set_text_calls.len(), 1);
        assert_eq!(state.borrow().set_text_calls[0], "hello world");
        assert_eq!(state.borrow().get_text_calls, 1);
        assert_eq!(state.borrow().screenshot_calls, 1);
    }

    #[test]
    fn mock_adapter_error_path_propagates_to_caller() {
        // When `next_error` is set, every method returns `Err(KernelError::Uia)`
        // and the caller must see it — not panic, not silently succeed.
        let mock = MockAdapter::new();
        let state = mock.state_handle();
        state.borrow_mut().next_error = Some("simulated UIA failure");
        let adapter: Box<dyn UiaAdapter> = Box::new(mock);

        let launch_err = adapter.launch_app("notepad.exe").unwrap_err();
        assert!(
            matches!(launch_err, crate::error::KernelError::Uia(ref m) if m == "simulated UIA failure"),
            "expected Uia error, got {:?}",
            launch_err
        );

        let find_window_err = adapter.find_window("x").unwrap_err();
        assert!(matches!(find_window_err, crate::error::KernelError::Uia(_)));

        let handle = UiaElementHandle::mock();
        let find_element_err = adapter
            .find_element(&handle, &UiaSelector::ByName("y".to_string()))
            .unwrap_err();
        assert!(matches!(find_element_err, crate::error::KernelError::Uia(_)));

        let click_err = adapter.click(&handle).unwrap_err();
        assert!(matches!(click_err, crate::error::KernelError::Uia(_)));

        let set_text_err = adapter.set_text(&handle, "z").unwrap_err();
        assert!(matches!(set_text_err, crate::error::KernelError::Uia(_)));

        let get_text_err = adapter.get_text(&handle).unwrap_err();
        assert!(matches!(get_text_err, crate::error::KernelError::Uia(_)));

        let screenshot_err = adapter.screenshot(&handle).unwrap_err();
        assert!(matches!(screenshot_err, crate::error::KernelError::Uia(_)));

        // The mock should still have recorded the calls (record-first, error-after).
        assert_eq!(state.borrow().launch_calls.len(), 1);
        assert_eq!(state.borrow().find_window_calls.len(), 1);
        assert_eq!(state.borrow().find_element_calls.len(), 1);
        assert_eq!(state.borrow().click_calls, 1);
        assert_eq!(state.borrow().set_text_calls.len(), 1);
        assert_eq!(state.borrow().get_text_calls, 1);
        assert_eq!(state.borrow().screenshot_calls, 1);
    }
}
