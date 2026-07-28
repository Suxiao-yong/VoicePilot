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
use crate::kernel::TrustKernel;
use crate::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill, SkillExecution};
use crate::skills::form_prepare::{execute_form_prepare, FormPrepareInput};
use crate::skills::form_submit::{execute_form_submit, FormSubmitInput};
use crate::skills::research_save::{execute_research_save, ResearchSaveInput};
use crate::skills::task_compensate::{execute_compensate, TaskCompensateInput};
use crate::skills::task_explain::{execute_explain, TaskExplainInput};
use crate::skills::task_repeat::{execute_repeat_verified, TaskRepeatVerifiedInput};
use crate::toolresult::ToolStatus;

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
    /// 节点 output,作为下游模板 resolve 的 node_outputs[node_id]。
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
        "note.capture" => {
            dispatch_note_capture(kernel, resolved_input, approver, task_id, step_id)
        }
        "research.save_markdown" => {
            dispatch_research_save(kernel, resolved_input, approver, task_id, step_id)
        }
        "form.prepare" => {
            dispatch_form_prepare(kernel, resolved_input, approver, task_id, step_id)
        }
        "form.submit" => dispatch_form_submit(kernel, resolved_input, approver, task_id, step_id),
        _ => Err(KernelError::Skill(format!("unknown skill_id: {}", skill_id))),
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
    // W8 Plan 4 修复:`execute_app_control` 需要 `adapter: &dyn UiaAdapter`
    // 参数(W7 Plan 4 加入),但 `dispatch_skill_executor` 签名不携带 adapter。
    // Plan 2 原始代码漏传 adapter,在 `voice,tauri,llm,uia` feature 组合下
    // 编译失败(此前 W8 验收门禁只跑 `--no-default-features` 未暴露)。
    //
    // 本 Plan 4 修复策略:保留字段校验(extract_string),不调 execute_*,
    // 返回 Err 表明 UIA-in-DAG 待 Plan 6 集成时通过 DagExecutor::new 加
    // `Option<Arc<dyn UiaAdapter>>` 字段并透传到 dispatch_*。UiaAdapter 是
    // `!Send + !Sync`(COM apartment 模型),Plan 6 需评估 DagExecutor 是否
    // 改为 `!Send` 或用 thread-local adapter。
    let _ = (kernel, approver, task_id, step_id);
    let _action = extract_string(resolved_input, "action")
        .ok()
        .unwrap_or_else(|| "launch".into());
    let _app_name = extract_string(resolved_input, "app_name")?;
    // 校验通过 → 构造 input 仅为了失败信息更清晰(不实际执行)。
    let _input = AppControlInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        app_name: _app_name,
        action: _action,
    };
    Err(KernelError::Skill(
        "quick.app_control in DagExecutor requires UiaAdapter plumbing — Plan 6 work".into(),
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
    // W8 Plan 4 修复:同 dispatch_app_control,execute_note_capture 需要
    // `adapter: &dyn UiaAdapter`,Plan 2 漏传。保留字段校验,返回 Err。
    // Plan 6 集成时通过 DagExecutor 透传 adapter。
    let _ = (kernel, approver, task_id, step_id);
    let _content = extract_string(resolved_input, "content")?;
    let _save_path = extract_string(resolved_input, "save_path")?;
    let _input = NoteCaptureInput {
        task_id: task_id.to_string(),
        step_id: step_id.to_string(),
        content: _content,
        save_path: _save_path,
    };
    Err(KernelError::Skill(
        "note.capture in DagExecutor requires UiaAdapter plumbing — Plan 6 work".into(),
    ))
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

// ===== JSON 提取 helper =====

fn extract_string(v: &serde_json::Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            KernelError::Skill(format!("dispatch: missing or invalid string field '{}'", key))
        })
}

fn extract_u32(v: &serde_json::Value, key: &str) -> Result<Option<u32>> {
    match v.get(key) {
        None => Ok(None),
        Some(serde_json::Value::Null) => Ok(None),
        Some(x) => x.as_u64().map(|n| Some(n as u32)).ok_or_else(|| {
            KernelError::Skill(format!("dispatch: field '{}' must be u32", key))
        }),
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
fn extract_string_map(v: &serde_json::Value, key: &str) -> Result<std::collections::HashMap<String, String>> {
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
