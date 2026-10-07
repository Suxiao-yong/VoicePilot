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
// Daisy 移植：共用骨架 + 系统组。
pub mod clip_ops;
pub mod doc_office;
pub mod fs_ops;
pub mod media_ops;
pub mod pim;
pub mod shell_run;
pub mod simple;
pub mod sys_ops;
pub mod web_ops;

// W10 Plan 1: Strong Verifier 函数集合(6 个有副作用 Skill 的 verify 函数)。
// files.organize 已在 W3a 实现 verify_move,task.explain 是只读 Skill(strategy="none"),
// 均不在此模块。
pub mod verifiers;

// W8 Plan 1 Task 2: DAG orchestration core data structures (no business logic).
pub mod dag_types;
// W8 Plan 1 Task 3: SlotTemplateEngine — parse ${prev}/${user}/${item}/${step} placeholders.
pub mod template;
// W8 Plan 1 Task 6: DagRepo — CRUD for dag_plans + dag_nodes tables.
pub mod dag_repo;
// W8 Plan 1 Task 7: TaskExplanationRepo — CRUD for task_explanations table.
pub mod explanation_repo;
// W8 Plan 2 Task 2: dispatch_skill_executor 路由 + DispatchOutcome 适配器
pub mod dispatcher;
// W8 Plan 2 Task 3: DagExecutor 简单节点调度
pub mod dag_executor;
// 原子快路由：意图匹配 + 参数提取一步完成（对标 Daisy tryLocalCommand）。
pub mod fastroute;

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

// W8 Plan 3 Task 5: form.submit executor (navigate + click submit).
// Cross-platform — only depends on the MCP client.
pub mod form_submit;

// W10 Plan 2: reverse function implementations for non-move Skills.
// Cross-platform — reverse_note_capture / reverse_research_save only use
// std::fs; reverse_form_prepare uses invoke_mcp_tool (Playwright MCP).
pub mod reverse_fns;

// W11 Plan 2: red team 恶意输入分类器(评测用确定性拦截,default-gated)。
pub mod redteam;
