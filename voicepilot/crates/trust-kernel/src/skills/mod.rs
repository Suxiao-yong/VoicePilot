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

// W8 Plan 1 Task 2: DAG orchestration core data structures (no business logic).
pub mod dag_types;
// W8 Plan 1 Task 3: SlotTemplateEngine — parse ${prev}/${user}/${item}/${step} placeholders.
pub mod template;
// W8 Plan 1 Task 6: DagRepo — CRUD for dag_plans + dag_nodes tables.
pub mod dag_repo;
// W8 Plan 1 Task 7: TaskExplanationRepo — CRUD for task_explanations table.
pub mod explanation_repo;

// W7 Plan 4 Task 3: quick.app_control executor (launch/focus/close).
// `cfg(all(windows, feature = "uia"))` (rather than just `feature = "uia"`)
// for the same reason as `crate::uiautomation` (see lib.rs): the file
// `use`s `crate::uiautomation::UiaAdapter`, which only exists on Windows
// with the `uia` feature. Project is Windows-only (user decision 2026-07-26);
// the `all(windows, ...)` gate is retained as a compile-time guard.
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
