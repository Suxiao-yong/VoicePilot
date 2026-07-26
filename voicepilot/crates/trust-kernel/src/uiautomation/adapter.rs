//! `WindowsUiaAdapter` — concrete `UiaAdapter` backed by the `uiautomation` crate.
//!
//! Implementation notes:
//!
//! * `UIAutomation` and `UIElement` are `!Send`/`!Sync` (COM apartment model),
//!   so this adapter is also `!Send`/`!Sync`. It must be created and used on
//!   the same thread. `UIAutomation::new()` initializes COM with
//!   `COINIT_MULTITHREADED` for the current thread.
//! * `UIAutomation` is `Clone` (COM reference counting), so we store it by
//!   value and clone for each `UIMatcher` operation — no `Rc<RefCell<…>>`
//!   needed despite the `&self` receiver on trait methods.
//! * `find_window` / `find_element` use `UIMatcher` with `timeout(0)` (no
//!   retry) and treat any `find_first` error as "not found" → `Ok(None)`.
//!   W7 Plan 4 final review (follow-up #4): swallowed errors are now
//!   logged via `tracing::warn!` so they're observable. A future task can
//!   differentiate "no match" from "real UIA error" by inspecting
//!   `uiautomation::Error` variants.
//! * `set_text` / `get_text` go through `UIValuePattern`. Elements that don't
//!   support the pattern (e.g. static labels) yield `KernelError::Uia`.
//! * `screenshot` returns `KernelError::Uia` because the `uiautomation`
//!   crate's `screenshot` feature is not enabled in `trust-kernel/Cargo.toml`
//!   (Task 1 only added the base dependency). A future task can enable that
//!   feature or implement a Win32 GDI capture path.

use std::process::Command;

use uiautomation::core::UIElement;
use uiautomation::patterns::UIValuePattern;

use crate::error::{KernelError, Result};

use super::{UiaAdapter, UiaElementHandle, UiaSelector};

/// Windows UIA-backed adapter. Holds a clone-able `UIAutomation` client.
pub struct WindowsUiaAdapter {
    automation: uiautomation::core::UIAutomation,
}

impl WindowsUiaAdapter {
    /// Create a new adapter. Initializes COM (multithreaded apartment) for the
    /// calling thread via `UIAutomation::new()`.
    pub fn new() -> Result<Self> {
        let automation =
            uiautomation::core::UIAutomation::new().map_err(uia_err)?;
        Ok(Self { automation })
    }
}

/// Convert a `uiautomation::Error` into a `KernelError::Uia` by stringifying
/// it. We don't use `#[from]` because the `uiautomation` crate is an optional
/// dependency and we want `KernelError` to stay buildable without it.
fn uia_err(e: uiautomation::Error) -> KernelError {
    KernelError::Uia(e.to_string())
}

/// Pull the underlying `UIElement` out of a handle, or return an error if the
/// handle is a mock (no element). Real `WindowsUiaAdapter` callers always
/// receive handles wrapping `Some(element)`.
fn require_element(handle: &UiaElementHandle) -> Result<&UIElement> {
    handle
        .as_element()
        .ok_or_else(|| KernelError::Uia("handle wraps no UIElement (mock handle?)".to_string()))
}

impl UiaAdapter for WindowsUiaAdapter {
    fn launch_app(&self, app_name: &str) -> Result<UiaElementHandle> {
        // 1. Spawn the process. `Command::new` resolves via PATH on Windows
        //    (e.g. "notepad.exe" → C:\Windows\System32\notepad.exe).
        let child = Command::new(app_name).spawn().map_err(|e| {
            KernelError::Uia(format!("launch '{}' failed: {}", app_name, e))
        })?;
        let pid = child.id() as i32;
        // Hold on to the child so we don't accidentally kill it by dropping
        // (the default Drop kills the child on some platforms). We don't need
        // to wait on it — the OS keeps the process alive after Drop on
        // Windows, but be explicit by detaching via `forget`-free ownership.
        // Actually `std::process::Child::id` doesn't kill; only `kill()` does.
        // Drop away — the child outlives the handle on Windows.
        drop(child);

        // 2. Find the first top-level window whose process_id matches `pid`.
        //    Use a 3s timeout so the matcher retries while the app boots.
        let matcher = self
            .automation
            .create_matcher()
            .timeout(3000)
            .filter_fn(Box::new(move |e: &UIElement| {
                let elem_pid = e.get_process_id().unwrap_or(-1);
                Ok(elem_pid == pid)
            }));
        match matcher.find_first() {
            Ok(element) => Ok(UiaElementHandle::from_element(element)),
            Err(e) => Err(KernelError::Uia(format!(
                "launch_app: no window found for pid {} ({}): {}",
                pid, app_name, e
            ))),
        }
    }

    fn find_window(&self, title_contains: &str) -> Result<Option<UiaElementHandle>> {
        // Search from desktop root for a window whose name contains the
        // given substring (case-insensitive). timeout(0) → no retry.
        let matcher = self
            .automation
            .create_matcher()
            .contains_name(title_contains.to_string())
            .timeout(0);
        match matcher.find_first() {
            Ok(element) => Ok(Some(UiaElementHandle::from_element(element))),
            Err(e) => {
                // W7 Plan 4 final review (follow-up #4): previously all
                // `find_first` errors were silently mapped to `Ok(None)`,
                // making "no match" indistinguishable from COM failure /
                // permission denied / etc. Log the swallowed error so it
                // is observable in tracing. We still return `Ok(None)`
                // (not `Err`) to preserve the existing contract: callers
                // treat `Ok(None)` as "window not found" and `Err` as a
                // hard failure — a UIA `find_first` error is recoverable
                // (the window may appear later), so we don't escalate it.
                tracing::warn!(
                    target = "uiautomation",
                    error = %e,
                    title_contains = %title_contains,
                    "find_window: find_first failed; returning None"
                );
                Ok(None)
            }
        }
    }

    fn find_element(
        &self,
        root: &UiaElementHandle,
        selector: &UiaSelector,
    ) -> Result<Option<UiaElementHandle>> {
        let root_element = require_element(root)?;
        let matcher = self.automation.create_matcher().from(root_element.clone()).timeout(0);
        // `filter_fn` requires a `'static` closure, so for `ById`/`ByRole` we
        // clone the inner `String` out of the borrowed `&UiaSelector` before
        // moving it into the closure. `ByName` uses the matcher's built-in
        // `contains_name` filter which takes an owned `String` directly.
        let matcher = match selector {
            UiaSelector::ById(id) => {
                let id = id.clone();
                matcher.filter_fn(Box::new(move |e: &UIElement| {
                    let elem_id = e.get_automation_id().unwrap_or_default();
                    Ok(elem_id == id)
                }))
            }
            UiaSelector::ByName(name) => matcher.contains_name(name.clone()),
            UiaSelector::ByRole(role) => {
                let role = role.clone();
                matcher.filter_fn(Box::new(move |e: &UIElement| {
                    let ct = e.get_localized_control_type().unwrap_or_default();
                    Ok(ct.eq_ignore_ascii_case(&role))
                }))
            }
        };
        match matcher.find_first() {
            Ok(element) => Ok(Some(UiaElementHandle::from_element(element))),
            Err(e) => {
                // W7 Plan 4 final review (follow-up #4): same rationale as
                // `find_window` — log the swallowed error so it's observable
                // without changing the `Ok(None)` contract.
                tracing::warn!(
                    target = "uiautomation",
                    error = %e,
                    selector = ?selector,
                    "find_element: find_first failed; returning None"
                );
                Ok(None)
            }
        }
    }

    fn click(&self, element: &UiaElementHandle) -> Result<()> {
        let elem = require_element(element)?;
        elem.click().map_err(uia_err)
    }

    fn set_text(&self, element: &UiaElementHandle, text: &str) -> Result<()> {
        let elem = require_element(element)?;
        let pattern: UIValuePattern = elem.get_pattern().map_err(uia_err)?;
        pattern.set_value(text).map_err(uia_err)
    }

    fn get_text(&self, element: &UiaElementHandle) -> Result<String> {
        let elem = require_element(element)?;
        let pattern: UIValuePattern = elem.get_pattern().map_err(uia_err)?;
        pattern.get_value().map_err(uia_err)
    }

    fn screenshot(&self, _element: &UiaElementHandle) -> Result<Vec<u8>> {
        // The `uiautomation` crate's `screenshot` feature is not enabled in
        // `trust-kernel/Cargo.toml` (Task 1 only added the base dep). Return
        // a clear error rather than silently no-op'ing. A future task can
        // enable the feature or implement a Win32 GDI capture path.
        Err(KernelError::Uia(
            "screenshot requires the `screenshot` feature on the `uiautomation` crate, \
             which is not enabled in trust-kernel/Cargo.toml"
                .to_string(),
        ))
    }
}
