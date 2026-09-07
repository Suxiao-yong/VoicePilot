//! dispatch_skill_executor — W8 §2.3.
//!
//! 9 路 skill_id 分发器:把 DagExecutor 解析后的 JSON input 路由到对应
//! W7 既有 executor,统一返回 `DispatchOutcome` 适配器。
//!
//! 设计权衡(见 plan Self-Review §2 已知偏离):
//!
//!   - spec §2.3 期望返回 `SkillExecution`,但 W7 既有 8 个 executor 返回类型
//!     异构(`SkillExecution` vs `String` task_id),强行统一需重构 7 个 executor
//!   - 本 plan 引入 `DispatchOutcome` 适配器,各分支按 executor 原生返回类型
//!     适配,`output` 字段为 `serde_json::Value`(供下游模板 `${prev.output.xxx}` 解析)
//!   - `form.submit` 在 Plan 3 实现,本 plan 占位返回 Err
//!   - `task.explain` 路由到 W7 既有 `execute_explain`,Plan 3 改为 `execute_task_explain_with_llm`

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::extensions::types::{ExecutionTarget, ExtensionSource};
use crate::kernel::TrustKernel;
use crate::mcp::client::{McpCallLimits, McpClient};
use crate::mcp::repo::McpServerRepo;
use crate::policy::taint_repo::{compute_value_hash, make_taint_record, TaintRepo};
use crate::skills::common::enforce_mcp_output_limit;
use crate::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill, SkillExecution};
use crate::skills::form_prepare::{execute_form_prepare, FormPrepareInput};
use crate::skills::form_submit::{execute_form_submit, FormSubmitInput};
use crate::skills::research_save::{execute_research_save, ResearchSaveInput};
use crate::skills::task_compensate::{execute_compensate, TaskCompensateInput};
use crate::skills::task_explain::{execute_explain, TaskExplainInput};
use crate::skills::task_repeat::{execute_repeat_verified, TaskRepeatVerifiedInput};
use crate::toolresult::ToolStatus;
use sha2::{Digest, Sha256};
use std::process::Child;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};

#[cfg(all(windows, feature = "uia"))]
use crate::skills::app_control::AppControlInput;
#[cfg(all(windows, feature = "uia"))]
use crate::skills::note_capture::NoteCaptureInput;

/// dispatch_skill_executor 的统一返回值。
///
/// `output` 是节点的 output(供下游 `${prev.output.xxx}` 模板解析):
///
///   - files.organize → `tool_result.data`(含 `moved_count` / `destination` / `approval_id`)
///   - 其他 executor → `{"task_id": "...", "step_id": "..."}`(executor 仅返回 task_id)
///
/// `succeeded` 字段让 DagExecutor 判定 `DagNodeStatus::Succeeded` / `Failed`。
/// 注意:executor 返回 `Err` 时本函数直接传播 `Err`(不构造 `DispatchOutcome`),
/// DagExecutor 在 `run_simple_node` 中 catch 并标记节点 Failed。
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub task_id: String,
    pub step_id: String,
    /// 节点 output,作为下游模板 resolve 的 `node_outputs[node_id]`。
    pub output: serde_json::Value,
    /// 是否成功(executor 返回 Ok 但 ToolStatus 可能是 Cancelled)。
    pub succeeded: bool,
    /// 失败原因(若 succeeded=false)。
    pub error_cause: Option<String>,
}

impl DispatchOutcome {
    /// files.organize 分支:从 SkillExecution 提取 outcome。
    pub fn from_skill_execution(exec: SkillExecution, task_id: String, step_id: String) -> Self {
        let succeeded = matches!(exec.tool_result.status, ToolStatus::Succeeded);
        let error_cause = if succeeded {
            None
        } else {
            Some(format!("tool_status: {:?}", exec.tool_result.status))
        };
        Self {
            task_id,
            step_id,
            output: exec.tool_result.data,
            succeeded,
            error_cause,
        }
    }

    /// 其余 8 个 executor 分支:仅返回 task_id,构造最小 outcome。
    /// Daisy 移植 Skill：人类可读输出直接装入 output（下游模板可用，
    /// 语音回读同一份文本）。
    pub fn from_text_result(task_id: String, step_id: String, text: String) -> Self {
        Self {
            task_id,
            step_id,
            output: serde_json::json!({ "result": text }),
            succeeded: true,
            error_cause: None,
        }
    }
    pub fn from_task_id(returned_task_id: String, step_id: String) -> Self {
        let output = serde_json::json!({
            "task_id": returned_task_id.clone(),
            "step_id": step_id.clone(),
        });
        Self {
            task_id: returned_task_id,
            step_id,
            output,
            succeeded: true,
            error_cause: None,
        }
    }
}

/// dispatch_skill_executor — 9 路 skill_id 分发。
///
/// 参数:
///
/// - `skill_id`:DagNode.skill_id(必须命中 9 路之一,否则 Err)
/// - `kernel`:TrustKernel 引用(传给 executor)
/// - `resolved_input`:`SlotTemplateEngine::resolve` 渲染后的 JSON value
/// - `approver`:节点级审批器(传给 executor,与 DAG 骨架审批独立)
/// - `task_id`:DagExecutor 生成的新 task_id(每个节点独立 task)
/// - `step_id`:DagExecutor 生成的新 step_id
///
/// 返回 `Result<DispatchOutcome>`:
///
/// - Ok(outcome) — executor 成功执行(ToolStatus 可能是 Succeeded / Cancelled)
/// - Err(KernelError::Skill(...)) — executor 失败 / 未知 skill_id / form.submit 占位
///
/// DagExecutor 在 `run_simple_node` 中:
///
///   1. 调本函数
///   2. Ok + outcome.succeeded=true → DagNodeStatus::Succeeded(outcome.output)
///   3. Ok + outcome.succeeded=false → DagNodeStatus::Failed{cause: outcome.error_cause}
///   4. Err(e) → DagNodeStatus::Failed{cause: e.to_string()}
pub fn dispatch_skill_executor(
    skill_id: &str,
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // Wave 2 Task 2.1: snapshot-gated dispatch. Every direct dispatch must
    // first resolve through the published extension snapshot; unknown,
    // disabled, and display-only extensions are rejected BEFORE any executor
    // or MCP process runs (no side effects). The legacy "unknown skill_id"
    // phrasing is preserved for unknown ids (w8 dispatcher/DAG tests pin it)
    // while keeping the catalog's "not found" wording (dispatch_snapshot_gate
    // tests pin it).
    let snapshot = kernel.extension_snapshot();
    let execution_target = snapshot.resolve_execution_target(skill_id).map_err(|e| {
        let message = e.to_string();
        if message.contains("was not found") {
            KernelError::Skill(format!("unknown skill_id: {message}"))
        } else {
            e
        }
    })?;

    // Wave 2 Task 2.3: capture the executed extension's audit metadata
    // (manifest_hash, extension source, catalog snapshot_id).
    // resolve_execution_target already proved the entry is enabled with a
    // target, so resolve_candidate returns the same descriptor here.
    let mut extension_meta = snapshot.resolve_candidate(skill_id).map(|descriptor| {
        serde_json::json!({
            "manifest_hash": descriptor.manifest_hash,
            "source": extension_source_label(&descriptor.source),
            "snapshot_id": snapshot.snapshot_id(),
        })
    });

    // Normalize: 若 resolved_input 是 String,尝试解析为 JSON 对象。
    // LLM 拆解时 input_template.template 常返回 JSON 字符串(如 '{"limit": 5}'),
    // SlotTemplateEngine::resolve 把 Literal 编译为 Value::String,
    // 此处把 String 解析回 Object,让 dispatch_* 的 extract_* 能正常工作。
    // 解析失败则保留原值(dispatch_* 会按字段缺失报错)。
    let normalized: serde_json::Value = if let Some(s) = resolved_input.as_str() {
        serde_json::from_str(s).unwrap_or_else(|_| resolved_input.clone())
    } else {
        resolved_input.clone()
    };
    let resolved_input = &normalized;

    // W9 Plan 3: 记录输入 taint(查表驱动传播)。
    // compute_value_hash 接收 &serde_json::Value,resolved_input 已是 Value。
    let input_hash = compute_value_hash(resolved_input);
    let input_taints: Vec<String> = {
        let conn = kernel.conn();
        TaintRepo::new()
            .find_by_hash(&conn, &input_hash)?
            .map(|r| r.taints)
            .unwrap_or_default()
    };

    // Wave 2 Task 2.1: dispatch by execution target. Builtin targets route
    // to the existing 9-way typed executor map (builtin-only); MCP tool
    // targets route through mcp_tool_call_checked, which enforces the
    // per-call boundary here: server lookup + enabled check + tools/list
    // pre-check + schema-hash baseline + McpCallLimits (timeout / max output
    // bytes on both the tools/list and the tools/call payload). The
    // enabled+trusted server filter was already applied at the snapshot
    // registration gate (registry.load).
    //
    // Approved deviation (Wave 2 closeout, user decision): the plan's
    // "manifest/risk → Gateway::decide" steps are NOT enforced on this path —
    // the pre-existing MCP chain (invoke_mcp_tool, also used by builtin MCP
    // executors) never had them, and MCP tool calls produce no effect
    // manifest for a Cedar decision. Recorded in progress.md as a follow-up
    // design item, not a silent gap.
    let mut mcp_server_taint: Option<String> = None;
    let outcome = match execution_target {
        ExecutionTarget::Builtin { executor_id } => {
            if executor_id != skill_id {
                return Err(KernelError::Skill(format!(
                    "extension target executor_id '{executor_id}' does not match skill '{skill_id}'"
                )));
            }
            dispatch_builtin(skill_id, kernel, resolved_input, approver, task_id, step_id)?
        }
        ExecutionTarget::McpTool {
            server_id,
            tool_name,
        } => {
            if let Some(serde_json::Value::Object(fields)) = extension_meta.as_mut() {
                fields.insert("server_id".to_string(), serde_json::json!(server_id));
                fields.insert("tool_name".to_string(), serde_json::json!(tool_name));
            }
            // Phase D: 记下 server 源 taint（mcp_tool:<server_id>）——MCP 返回
            // 值携带服务器溯源（spec §2.3 传播规则），审计与 TaintRepo 均可见。
            mcp_server_taint = Some(format!("mcp_tool:{server_id}"));
            // Wave 2 Task 2.3: single-process tools/list pre-check + session
            // schema-hash baseline + tools/call, all under McpCallLimits. The
            // schema hash is derived from and verified against the SAME
            // subprocess that executes the call, so a changed tool schema (or
            // a tool missing from tools/list) is rejected BEFORE any call side
            // effect. Builtin MCP executors (research.save_markdown …) keep
            // their existing invoke_mcp_tool path — this arm is the only
            // place the pre-check runs.
            let data =
                mcp_tool_call_checked(kernel, &server_id, &tool_name, resolved_input.clone())?;
            DispatchOutcome {
                task_id: task_id.to_string(),
                step_id: step_id.to_string(),
                output: data,
                succeeded: true,
                error_cause: None,
            }
        }
    };

    // W9 Plan 3: 记录输出 taint(继承输入 taints + 加 executor_output:<skill_id>)。
    // 仅当 outcome.succeeded 且 output 非 Null 时 upsert(spec §6.2:不存储原始 value)。
    if outcome.succeeded && !outcome.output.is_null() {
        let output_hash = compute_value_hash(&outcome.output);
        let mut output_taints = input_taints.clone();
        // Phase D: MCP 工具返回值额外携带 mcp_tool:<server_id> 源 taint
        // （与 spec §2.3 传播规则一致；mcp_tool:composio 等进审计 + TaintRepo）。
        if let Some(server_taint) = &mcp_server_taint {
            if !output_taints.contains(server_taint) {
                output_taints.push(server_taint.clone());
            }
        }
        let executor_taint = format!("executor_output:{}", skill_id);
        if !output_taints.contains(&executor_taint) {
            output_taints.push(executor_taint.clone());
        }
        let record = make_taint_record(
            output_hash.clone(),
            // Phase D：MCP 工具返回值的 provenance = mcp_tool:<server_id>
            // （spec §2.3 溯源规则，w9 e2e 同口径）；builtin 路径维持
            // executor_output:<skill_id>。
            mcp_server_taint
                .clone()
                .unwrap_or_else(|| executor_taint.clone()),
            output_taints.clone(),
            Some(format!("{}:{}", task_id, step_id)),
        );
        {
            let conn = kernel.conn();
            TaintRepo::new().upsert(&conn, &record)?;
        }
        // 审计:taint_propagated(details 仅含 hash + 标签,不含原始 value,spec §6.2)。
        // Wave 2 Task 2.3:details 追加 `extension` 审计元数据 —— 本次执行的是
        // 哪个扩展 manifest(manifest_hash)、来源(source)、以及从哪个 catalog
        // snapshot 解析(snapshot_id;MCP target 另有 server_id + tool_name)。
        // 复用既有事件类型 + details 字段,避免 AUDIT_EVENT_TYPE_REGISTRY 变更。
        let mut details = serde_json::json!({
            "source_ref": format!("{}:{}", task_id, step_id),
            "input_hash": input_hash,
            "output_hash": output_hash,
            "taints": output_taints,
        });
        if let Some(meta) = extension_meta {
            details["extension"] = meta;
        }
        kernel.audit_append_external(task_id, Some(step_id), "taint_propagated", details)?;
    }

    Ok(outcome)
}

/// The builtin-only typed executor map: exact existing arms plus the
/// Daisy-ported `text_out`/`text_in` arms — no MCP/UserSkill arms belong here.
type TextExec = fn(&TrustKernel, &dyn Approver, &str, &str, &serde_json::Value) -> Result<String>;

/// 无槽位 Skill：直接包装 execute 的文本输出。
fn text_out(
    _kernel: &TrustKernel,
    _approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    result: Result<String>,
) -> Result<DispatchOutcome> {
    Ok(DispatchOutcome::from_text_result(
        task_id.into(),
        step_id.into(),
        result?,
    ))
}

/// 有槽位 Skill：把 resolved_input 透传给 execute。
fn text_in(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
    run: TextExec,
) -> Result<DispatchOutcome> {
    let text = run(kernel, approver, task_id, step_id, inputs)?;
    Ok(DispatchOutcome::from_text_result(
        task_id.into(),
        step_id.into(),
        text,
    ))
}

fn dispatch_builtin(
    skill_id: &str,
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    match skill_id {
        "files.organize" => {
            dispatch_files_organize(kernel, resolved_input, approver, task_id, step_id)
        }
        "task.repeat_verified" => {
            dispatch_task_repeat(kernel, resolved_input, approver, task_id, step_id)
        }
        "task.explain" => dispatch_task_explain(kernel, resolved_input, approver, task_id, step_id),
        "task.compensate" => {
            dispatch_task_compensate(kernel, resolved_input, approver, task_id, step_id)
        }
        #[cfg(all(windows, feature = "uia"))]
        "quick.app_control" => {
            dispatch_app_control(kernel, resolved_input, approver, task_id, step_id)
        }
        #[cfg(all(windows, feature = "uia"))]
        "note.capture" => dispatch_note_capture(kernel, resolved_input, approver, task_id, step_id),
        "research.save_markdown" => {
            dispatch_research_save(kernel, resolved_input, approver, task_id, step_id)
        }
        "form.prepare" => dispatch_form_prepare(kernel, resolved_input, approver, task_id, step_id),
        "form.submit" => dispatch_form_submit(kernel, resolved_input, approver, task_id, step_id),
        // Phase B：长期记忆的用户否决面（可看可删；纯 DB，只读免审批）。
        "memory.view" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            execute_memory_view(kernel, task_id, step_id),
        ),
        "memory.forget" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            execute_memory_forget,
        ),
        // Phase C：进程内后台作业（创建/列表/停用；纯 DB，免审批）。
        // executor 签名与 scheduler.rs 内的 5 参形态匹配（含 approver）。
        "task.schedule" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::scheduler::execute_task_schedule,
        ),
        "task.jobs" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::scheduler::execute_task_jobs(kernel, task_id, step_id),
        ),
        "task.unschedule" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::scheduler::execute_task_unschedule,
        ),
        // Daisy 移植 Skill（只读免审批/写入 PerStep，见 skills/simple.rs
        // 姿态矩阵）：人类可读输出直接装入 output。
        "sys.datetime" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::sys_ops::execute_datetime(kernel, approver, task_id, step_id),
        ),
        "sys.frontmost" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::sys_ops::execute_frontmost(kernel, approver, task_id, step_id),
        ),
        "sys.open_url" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_open_url,
        ),
        "sys.quit_all" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::sys_ops::execute_quit_all(kernel, approver, task_id, step_id),
        ),
        "sys.diagnose_app" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_diagnose_app,
        ),
        "sys.timer_set" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_timer_set,
        ),
        "sys.alarm_set" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_alarm_set,
        ),
        "sys.maps_search" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_maps_search,
        ),
        "sys.quit_browsers" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::sys_ops::execute_quit_browsers(kernel, approver, task_id, step_id),
        ),
        "sys.volume" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_sys_volume,
        ),
        "sys.playback" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::sys_ops::execute_sys_playback,
        ),
        "sys.lock_screen" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::sys_ops::execute_lock_screen(kernel, approver, task_id, step_id),
        ),
        "clip.read" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::clip_ops::execute_clip_read(kernel, approver, task_id, step_id),
        ),
        "clip.write" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::clip_ops::execute_clip_write,
        ),
        "clip.selected" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::clip_ops::execute_clip_selected(kernel, approver, task_id, step_id),
        ),
        "clip.type_text" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::clip_ops::execute_clip_type_text,
        ),
        "clip.press_keys" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::clip_ops::execute_clip_press_keys,
        ),
        "clip.save_image" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::clip_ops::execute_clip_save_image(kernel, approver, task_id, step_id),
        ),
        "clip.selected_files" => text_out(
            kernel,
            approver,
            task_id,
            step_id,
            crate::skills::clip_ops::execute_clip_selected_files(
                kernel, approver, task_id, step_id,
            ),
        ),
        "fs.read_file" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::fs_ops::execute_fs_read,
        ),
        "fs.list_dir" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::fs_ops::execute_fs_list,
        ),
        "fs.write_file" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::fs_ops::execute_fs_write,
        ),
        "fs.create_file" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::fs_ops::execute_fs_create,
        ),
        "fs.delete_file" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::fs_ops::execute_fs_delete,
        ),
        "web.search" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::web_ops::execute_web_search,
        ),
        "web.scrape" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::web_ops::execute_web_scrape,
        ),
        "web.fetch_file" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::web_ops::execute_web_fetch_file,
        ),
        "web.wallpapers" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::web_ops::execute_web_wallpapers,
        ),
        "web.weather" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::web_ops::execute_web_weather,
        ),
        "web.sports" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::web_ops::execute_web_sports,
        ),
        "shell.run" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::shell_run::execute_shell_run,
        ),
        "media.download" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::media_ops::execute_media_download,
        ),
        "media.trim_video" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::media_ops::execute_media_trim,
        ),
        "media.convert_video" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::media_ops::execute_media_convert,
        ),
        "media.clip_chorus" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::media_ops::execute_media_clip_chorus,
        ),
        "doc.convert" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::media_ops::execute_doc_convert,
        ),
        "doc.office" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::doc_office::execute_doc_office,
        ),
        "mail.compose" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::pim::execute_mail_compose,
        ),
        "pim.note_create" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::pim::execute_pim_note,
        ),
        "pim.notes_search" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::pim::execute_pim_search,
        ),
        "pim.reminder_create" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::pim::execute_pim_reminder,
        ),
        "pim.calendar_create" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::pim::execute_pim_calendar,
        ),
        "pim.calendar_list" => text_in(
            kernel,
            approver,
            task_id,
            step_id,
            resolved_input,
            crate::skills::pim::execute_pim_calendar_list,
        ),
        // Unreachable through the snapshot gate (builtin targets are only
        // registered for manifest ids in the map above); kept as a defensive
        // fallback for mismatched catalogs.
        _ => Err(KernelError::Skill(format!(
            "unknown skill_id: {}",
            skill_id
        ))),
    }
}

fn dispatch_files_organize(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = FilesOrganizeInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        source: extract_path(resolved_input, "source")?,
        filter: extract_string(resolved_input, "filter")?,
        destination: extract_path(resolved_input, "destination")?,
    };
    let exec = FilesOrganizeSkill::new().execute(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_skill_execution(
        exec,
        task_id.into(),
        step_id.into(),
    ))
}

fn dispatch_task_repeat(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = TaskRepeatVerifiedInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        target_task_id: extract_string(resolved_input, "target_task_id")?,
        source_filter: extract_string(resolved_input, "source_filter")?,
    };
    let returned = execute_repeat_verified(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_task_explain(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let limit = extract_u32(resolved_input, "limit")?.unwrap_or(10);
    let input = TaskExplainInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        limit,
    };
    let returned = execute_explain(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_task_compensate(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = TaskCompensateInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        target_step_id: extract_string(resolved_input, "target_step_id")?,
    };
    let returned = execute_compensate(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

#[cfg(all(windows, feature = "uia"))]
fn dispatch_app_control(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // W9 Plan 6 Task 6:从 thread-local 取 UiaAdapter(由 DagExecutor 调用方
    // 在 run() 前通过 set_thread_local_uia_adapter 注入)。
    // UiaAdapter 是 `!Send + !Sync`(COM apartment 模型),不能用字段持有,
    // 改用 thread-local 透传(见 dag_executor.rs THREAD_LOCAL_UIA_ADAPTER)。
    let action = extract_string(resolved_input, "action")
        .ok()
        .unwrap_or_else(|| "launch".into());
    let app_name = extract_string(resolved_input, "app_name")?;
    let input = AppControlInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        app_name,
        action,
    };

    let adapter = crate::skills::dag_executor::thread_local_uia_adapter().ok_or_else(|| {
        KernelError::Skill(
            "quick.app_control in DagExecutor requires thread-local UiaAdapter — call set_thread_local_uia_adapter before run()".into(),
        )
    })?;

    let returned_task_id = crate::skills::app_control::execute_app_control(
        kernel,
        &input,
        approver,
        adapter.as_ref(),
    )?;

    Ok(DispatchOutcome::from_task_id(
        returned_task_id,
        step_id.into(),
    ))
}

#[cfg(all(windows, feature = "uia"))]
fn dispatch_note_capture(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // W9 Plan 6 Task 6:从 thread-local 取 UiaAdapter(由 DagExecutor 调用方
    // 在 run() 前通过 set_thread_local_uia_adapter 注入)。
    let content = extract_string(resolved_input, "content")?;
    let save_path = extract_string(resolved_input, "save_path")?;
    let input = NoteCaptureInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        content: content.clone(),
        save_path: save_path.clone(),
    };

    let adapter = crate::skills::dag_executor::thread_local_uia_adapter().ok_or_else(|| {
        KernelError::Skill(
            "note.capture in DagExecutor requires thread-local UiaAdapter — call set_thread_local_uia_adapter before run()".into(),
        )
    })?;

    let returned_task_id = crate::skills::note_capture::execute_note_capture(
        kernel,
        &input,
        approver,
        adapter.as_ref(),
    )?;

    // W9 Plan 6 Task 6:output 含 save_path(供下游 ${prev.output.save_path}
    // Slot 流水解析)。既有 from_task_id 只返回 {task_id, step_id},这里
    // 自定义 output 追加 save_path + content。
    let output = serde_json::json!({
        "task_id": returned_task_id,
        "step_id": step_id,
        "save_path": save_path,
        "content": content,
    });
    Ok(DispatchOutcome {
        task_id: returned_task_id,
        step_id: step_id.to_string(),
        output,
        succeeded: true,
        error_cause: None,
    })
}

fn dispatch_research_save(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let input = ResearchSaveInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        url: extract_string(resolved_input, "url")?,
        save_path: extract_string(resolved_input, "save_path")?,
    };
    let returned = execute_research_save(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_form_prepare(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    // FormPrepareInput.fields 是 HashMap<String, String>。SlotTemplateEngine
    // 渲染后的 JSON 可能是 object 或 string(JSON-encoded object)。
    let fields = extract_string_map(resolved_input, "fields")?;
    let input = FormPrepareInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        url: extract_string(resolved_input, "url")?,
        fields,
    };
    let returned = execute_form_prepare(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

fn dispatch_form_submit(
    kernel: &TrustKernel,
    resolved_input: &serde_json::Value,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<DispatchOutcome> {
    let url = extract_string(resolved_input, "url")?;
    // submit_selector 可选 — 缺失时用空串,executor 内部会 fallback 到 default
    let submit_selector = match resolved_input.get("submit_selector") {
        None => String::new(),
        Some(serde_json::Value::Null) => String::new(),
        Some(v) => v.as_str().map(|s| s.to_string()).unwrap_or_default(),
    };
    let input = FormSubmitInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        url,
        submit_selector,
    };
    let returned = execute_form_submit(kernel, &input, approver)?;
    Ok(DispatchOutcome::from_task_id(returned, step_id.into()))
}

// ===== Wave 2 Task 2.3: MCP tool pre-check + schema-hash baseline =====

/// Human-readable label for an extension's provenance (audit metadata).
fn extension_source_label(source: &ExtensionSource) -> String {
    match source {
        ExtensionSource::Builtin => "builtin".to_string(),
        ExtensionSource::UserSkill { path } => {
            format!("user_skill:{}", path.display())
        }
        ExtensionSource::McpPlugin { server_id } => format!("mcp_plugin:{server_id}"),
    }
}

/// Wave 2 Task 2.3: single-process MCP dispatch for User-Skill MCP targets.
///
/// Spawns the server ONCE, then runs initialize → tools/list → schema-hash
/// baseline check → tools/call, all under `McpCallLimits`. The schema hash is
/// derived from and verified against the SAME subprocess that executes the
/// call, so:
///   - a tool missing from tools/list is rejected before any call,
///   - a schema change between dispatches rejects with a re-plan hint
///     (verified against the shared session baseline),
///   - the tools/list payload and the tool result are both bounded by
///     `max_output_bytes` at the process/tool boundary.
///
/// The whole round trip runs on a worker thread with a shared subprocess slot
/// so a hung server is reaped on timeout (same pattern as
/// `skills::common::run_mcp_call_limited`). Returns the parsed tool result
/// data on success.
///
/// Applies ONLY to the `McpTool` dispatch arm; builtin MCP executors
/// (research.save_markdown …) keep their existing `invoke_mcp_tool` path
/// untouched.
fn mcp_tool_call_checked(
    kernel: &TrustKernel,
    server_id: &str,
    tool_name: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value> {
    // Shared lookup (same deterministic failures as invoke_mcp_tool).
    let (command, args_vec, env_json) = {
        let conn = kernel.conn();
        McpServerRepo::new().spawn_config(&conn, server_id)?
    };
    // Phase A 密钥收编：spawn 前解析 env 里的 keyring 引用（fail-closed）。
    let env_json = crate::skills::common::resolve_mcp_env(kernel, &env_json)?;

    let limits = McpCallLimits::default();
    // Shared session baseline: the worker verifies the schema hash against
    // this map, then — only on match — executes tools/call in the same
    // subprocess. A mismatch never reaches the call.
    let baseline = kernel.mcp_tool_schema_baseline();
    let (tx, rx) = std::sync::mpsc::channel();
    // Shared subprocess slot: on timeout the caller reaps the child so a hung
    // server cannot outlive the call (same pattern as common.rs).
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let worker_slot = child_slot.clone();
    let command = command.clone();
    // Worker-side clones for the closure; the originals stay in this thread
    // for the timeout / baseline error messages.
    let worker_server_id = server_id.to_string();
    let worker_tool_name = tool_name.to_string();
    std::thread::spawn(move || {
        let outcome = (|| -> Result<serde_json::Value> {
            let mut client = McpClient::spawn_into(&command, &args_vec, &env_json, worker_slot)?;
            client.initialize()?;
            let tools = client.list_tools()?;
            enforce_mcp_output_limit(&tools, limits.max_output_bytes).map_err(|e| {
                KernelError::Mcp(format!(
                    "MCP tools/list preflight for tool '{worker_tool_name}' on server '{worker_server_id}': {e}"
                ))
            })?;
            let tools_arr = tools.as_array().ok_or_else(|| {
                KernelError::Mcp(format!(
                    "MCP server '{worker_server_id}' tools/list returned no tools array"
                ))
            })?;
            let tool = tools_arr
                .iter()
                .find(|entry| {
                    entry.get("name").and_then(|v| v.as_str()) == Some(worker_tool_name.as_str())
                })
                .ok_or_else(|| {
                    KernelError::Mcp(format!(
                        "MCP tool '{worker_tool_name}' is not advertised by server '{worker_server_id}' in tools/list; re-plan/re-approve before calling this tool"
                    ))
                })?;
            let schema_hash = tool_schema_hash(tool);
            verify_tool_schema_baseline(
                &mut baseline.lock().unwrap(),
                &worker_server_id,
                &worker_tool_name,
                &schema_hash,
            )?;
            let result = client.invoke_tool(&worker_tool_name, args)?;
            enforce_mcp_output_limit(&result, limits.max_output_bytes).map_err(|e| {
                KernelError::Mcp(format!(
                    "MCP tool call '{worker_tool_name}' on server '{worker_server_id}': {e}"
                ))
            })?;
            Ok(result)
        })();
        if tx.send(outcome).is_err() {
            tracing::warn!(
                server_id = %worker_server_id,
                tool_name = %worker_tool_name,
                "MCP dispatch finished after caller timed out"
            );
        }
    });

    rx.recv_timeout(limits.timeout).map_err(|e| match e {
        RecvTimeoutError::Timeout => {
            drop(rx);
            reap_preflight_child(&child_slot);
            KernelError::Mcp(format!(
                "MCP tool call '{tool_name}' on server '{server_id}' exceeded timeout of {}s",
                limits.timeout.as_secs()
            ))
        }
        RecvTimeoutError::Disconnected => {
            reap_preflight_child(&child_slot);
            KernelError::Mcp(format!(
                "MCP worker for tool '{tool_name}' on server '{server_id}' disconnected (worker thread panicked or channel dropped)"
            ))
        }
    })?
}

/// Wave 2 Task 2.3: maintain the in-process (session) schema-hash baseline
/// for a User-Skill MCP tool target.
///
/// First call records `schema_hash` for (server_id, tool_name) and returns
/// Ok; later calls must match the recorded hash, otherwise the dispatch is
/// rejected with an error telling the user to re-plan/re-approve (the tool
/// schema changed under the approved Skill binding). A mismatch never updates
/// the baseline; recovery is an explicit configuration change — re-enabling
/// or re-registering the server (or restarting the process) re-records it.
fn verify_tool_schema_baseline(
    hashes: &mut std::collections::HashMap<String, String>,
    server_id: &str,
    tool_name: &str,
    schema_hash: &str,
) -> Result<()> {
    let key = format!("{server_id}\u{0}{tool_name}");
    match hashes.get(&key) {
        None => {
            hashes.insert(key, schema_hash.to_string());
            Ok(())
        }
        Some(previous) if previous == schema_hash => Ok(()),
        Some(previous) => Err(KernelError::Mcp(format!(
            "MCP tool schema changed for '{tool_name}' on server '{server_id}' (recorded {previous}, now {schema_hash}); re-plan/re-approve before calling this tool again"
        ))),
    }
}

/// sha256 of the canonicalized tool schema JSON advertised via tools/list.
/// Canonicalization (sorted object keys) keeps the hash stable across
/// servers that reorder JSON keys between calls, so only real schema changes
/// trip the baseline.
fn tool_schema_hash(tool: &serde_json::Value) -> String {
    let canonical = crate::extensions::registry::canonicalize_json(tool.clone());
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

/// Kill and reap the shared preflight subprocess if it is still alive.
fn reap_preflight_child(child_slot: &Arc<Mutex<Option<Child>>>) {
    if let Some(child) = child_slot.lock().unwrap().as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

// ===== JSON 提取 helper =====

fn extract_string(v: &serde_json::Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            KernelError::Skill(format!(
                "dispatch: missing or invalid string field '{}'",
                key
            ))
        })
}

fn extract_u32(v: &serde_json::Value, key: &str) -> Result<Option<u32>> {
    match v.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(None),
        Some(x) => x
            .as_u64()
            .map(|n| Some(n as u32))
            .ok_or_else(|| KernelError::Skill(format!("dispatch: field '{}' must be u32", key))),
    }
}

fn extract_path(v: &serde_json::Value, key: &str) -> Result<std::path::PathBuf> {
    let s = extract_string(v, key)?;
    Ok(std::path::PathBuf::from(s))
}

/// 提取 `HashMap<String, String>` — 接受 JSON object 或 JSON-encoded string。
///
/// form.prepare 的 `fields` 在 manifest 中是 Text(max_length=5000),executor
/// 接收 JSON-encoded string;但 SlotTemplateEngine 渲染后可能是 object。
/// 两种形式都支持。
fn extract_string_map(
    v: &serde_json::Value,
    key: &str,
) -> Result<std::collections::HashMap<String, String>> {
    match v.get(key) {
        None => Err(KernelError::Skill(format!(
            "dispatch: missing field '{}'",
            key
        ))),
        Some(serde_json::Value::Null) => Err(KernelError::Skill(format!(
            "dispatch: field '{}' is null",
            key
        ))),
        Some(serde_json::Value::Object(obj)) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in obj {
                let s = v.as_str().ok_or_else(|| {
                    KernelError::Skill(format!(
                        "dispatch: field '{}' has non-string value for key '{}'",
                        key, k
                    ))
                })?;
                map.insert(k.clone(), s.to_string());
            }
            Ok(map)
        }
        Some(serde_json::Value::String(s)) => {
            // JSON-encoded object string
            let parsed: serde_json::Value = serde_json::from_str(s).map_err(|e| {
                KernelError::Skill(format!(
                    "dispatch: field '{}' is not valid JSON: {}",
                    key, e
                ))
            })?;
            extract_string_map(&parsed, key)
        }
        Some(_) => Err(KernelError::Skill(format!(
            "dispatch: field '{}' must be object or JSON string",
            key
        ))),
    }
}

// ===== Phase B：长期记忆的用户否决面（memory.view / memory.forget）=====

/// memory.view：列出活跃长期记忆事实，人类可读文本输出。
fn execute_memory_view(kernel: &TrustKernel, _task_id: &str, _step_id: &str) -> Result<String> {
    let facts = crate::memory::view(kernel)?;
    if facts.is_empty() {
        return Ok("长期记忆为空（跨会话蒸馏尚未产生事实）。".to_string());
    }
    let mut lines = vec![format!("长期记忆共 {} 条：", facts.len())];
    for f in facts {
        lines.push(format!(
            "#{} [{}] {}",
            f.id,
            f.category,
            f.content.chars().take(160).collect::<String>()
        ));
    }
    Ok(lines.join("\n"))
}

/// memory.forget：按 fact_id 硬删一条（用户否决权）。
fn execute_memory_forget(
    kernel: &TrustKernel,
    _approver: &dyn Approver,
    _task_id: &str,
    _step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let fact_id = inputs
        .get("fact_id")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            inputs
                .get("fact_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        })
        .ok_or_else(|| KernelError::Skill("dispatch: missing field 'fact_id'".to_string()))?;
    crate::memory::forget(kernel, fact_id)?;
    Ok(format!("已删除记忆 #{fact_id}。"))
}
