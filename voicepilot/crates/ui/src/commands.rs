//! Tauri commands —— V1.1 §8.2 webview 与 trust-kernel 之间的 IPC 桥接。
//!
//! 所有 commands 都用 `#[cfg(feature = "tauri")]` 门控。它们接收 `&AppState`
//! (由 Tauri 管理)并返回 `Result<T, String>` 供 webview 消费。

use crate::error::UiResult;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// 镜像 `trust_kernel::voice::router_bridge::RouteOutcome`,但带 Serialize
/// 作为 Tauri command 返回类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteTextResult {
    Routed { skill_id: String },
    Unmatched { text: String },
    Empty,
}

/// 通过 SkillRouter 路由转写文本(或任意文本输入)。
/// V1.1 §5.1 —— 纯关键词匹配(W7 将添加 LLM Planner fallback)。
pub fn route_text(state: &AppState, text: &str) -> UiResult<RouteTextResult> {
    use trust_kernel::skills::manifest::files_organize_manifest;
    use trust_kernel::skills::router::{RouteDecision, SkillRouter};

    // `state` 暂未在路由中使用 —— W7 Planner fallback 会用它访问 TrustKernel。
    let _ = state;

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteTextResult::Empty);
    }
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteTextResult::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteTextResult::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn route_text_command(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<RouteTextResult, String> {
    route_text(&state, &text).map_err(Into::into)
}

// ===== files.organize Skill command (V1.1 §5.2 + §6.2 + §8.2) =====

use std::path::PathBuf;
use trust_kernel::approval::approver::Approver;
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::toolresult::ToolStatus;

/// Webview → Tauri 入参,镜像 `FilesOrganizeInput` 但用 String 路径
/// (serde 友好,跨 IPC 边界无 PathBuf 序列化问题)。
#[derive(Debug, Clone, Deserialize)]
pub struct OrganizeInput {
    pub task_id: String,
    pub step_id: String,
    pub source: String,
    pub filter: String,
    pub destination: String,
}

/// `organize_files` 返回值 —— 桥接 SkillExecution 的 ToolResult V2 字段
/// 到 webview 可消费的扁平结构。
#[derive(Debug, Clone, Serialize)]
pub struct OrganizeResult {
    pub committed: bool,
    pub moved_paths: Vec<[String; 2]>,
    pub evidence_strength: String,
    pub compensation_ref: Option<String>,
    pub error: Option<String>,
}

/// 编排 files.organize Skill 完整管道:
/// prepare → approve → commit → verify → compensate(V1.1 §6.2)。
///
/// 注入的 `approver` 决定 approve 阶段行为:
/// - 测试用 `AutoApprover`(无条件 Allow)
/// - 生产用 `TauriApprover`(通过 IPC 等待 Approval 窗口决定)
///
/// 幂等创建 task/step:若 task_id / step_id 在 kernel 中不存在,则创建
/// (Skill executor 内部第一步就调用 `update_step_status`,要求 step 已存在)。
pub fn organize_files(
    state: &AppState,
    approver: &dyn Approver,
    input: &OrganizeInput,
) -> UiResult<OrganizeResult> {
    let kernel = state.kernel.clone();

    // Idempotent task creation.
    if kernel.get_task(&input.task_id)?.is_none() {
        kernel.create_task(&input.task_id, "files.organize")?;
    }

    // Idempotent step creation.
    if kernel.get_step(&input.step_id)?.is_none() {
        let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
        kernel.create_step(&step)?;
    }

    let skill_input = FilesOrganizeInput {
        task_id: input.task_id.clone(),
        step_id: input.step_id.clone(),
        source: PathBuf::from(&input.source),
        filter: input.filter.clone(),
        destination: PathBuf::from(&input.destination),
    };

    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(kernel.as_ref(), &skill_input, approver)?;

    let moved_paths = execution
        .moved_paths
        .into_iter()
        .map(|(from, to)| {
            [
                from.to_string_lossy().into_owned(),
                to.to_string_lossy().into_owned(),
            ]
        })
        .collect();

    Ok(OrganizeResult {
        committed: execution.tool_result.status == ToolStatus::Succeeded,
        moved_paths,
        evidence_strength: execution.tool_result.evidence_strength.as_str().to_string(),
        compensation_ref: execution.tool_result.compensation_ref.clone(),
        error: None,
    })
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn organize_files_command(
    state: tauri::State<'_, AppState>,
    input: OrganizeInput,
) -> Result<OrganizeResult, String> {
    let approver = crate::approver::TauriApprover::new(state.approval_registry.clone());
    organize_files(&state, &approver, &input).map_err(Into::into)
}

