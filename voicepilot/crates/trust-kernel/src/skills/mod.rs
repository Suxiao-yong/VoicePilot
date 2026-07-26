//! Skills layer — V1.1 §5.
//!
//! W3b: SkillManifest struct + SkillRouter + files.organize executor.

pub mod common;
pub mod executor;
pub mod manifest;
pub mod repo;
pub mod router;
pub mod task_compensate;
pub mod task_explain;
pub mod task_repeat;
pub mod user_loader;

// W7 Plan 4 Task 3: quick.app_control executor (launch/focus/close).
// `cfg(all(windows, feature = "uia"))` (rather than just `feature = "uia"`)
// for the same reason as `crate::uiautomation` (see lib.rs): the file
// `use`s `crate::uiautomation::UiaAdapter`, which only exists on Windows
// with the `uia` feature. Enabling `--features uia` on Linux/macOS must
// not try to compile this module.
#[cfg(all(windows, feature = "uia"))]
pub mod app_control;

// W7 Plan 4 Task 4: note.capture executor (notepad + set_text + save).
// Same cfg gate as app_control — uses `crate::uiautomation::UiaAdapter`.
#[cfg(all(windows, feature = "uia"))]
pub mod note_capture;

// W7 Plan 5 Task 4: research.save_markdown executor (navigate + snapshot
// + eval + write). Cross-platform — only depends on the MCP client.
pub mod research_save;

// W7 Plan 5 Task 5: form.prepare executor (navigate + snapshot + fill,
// no submit). Cross-platform — only depends on the MCP client.
pub mod form_prepare;
