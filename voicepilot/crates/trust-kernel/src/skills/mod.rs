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
