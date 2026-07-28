//! W8 Plan 5: DAG 相关 Tauri 命令 —— V1.1.2 §8.2 + W8 spec §2.7.
//!
//! 4 个命令:
//! - `approve_dag_skeleton_command`:提交 DAG 骨架审批决策(Allow/Deny/Modify)
//! - `list_dag_history_command`:分页 + 状态过滤查询历史 DAG
//! - `get_dag_plan_command`:查询单个 DAG 完整详情(plan + nodes)
//! - `get_task_explanation_command`:查询 step 的 LLM 失败归因
//!
//! 安全规则(参考 project_memory.md "Tauri IPC 三安全规则"):
//! - WebView 不直接访问 filesystem(本模块只读 DagRepo / TaskExplanationRepo)
//! - UI 不直接调用 MCP(DAG 审批走 oneshot channel,不触发 MCP)
//! - `approval_request_id` 单次使用(`take_sender` 移除 sender,防重放)

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;
use trust_kernel::approval::types::ApprovalDecision;

/// W8 §2.7:DAG 骨架审批决策。
/// 与 `ApprovalDecision` 一致,但单独定义以便未来扩展 Modify payload。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DagApprovalDecision {
    Allow,
    Deny,
    /// W9+ 实现(spec §8 延后项):用户调整 input_template
    Modify,
}

impl From<DagApprovalDecision> for ApprovalDecision {
    fn from(d: DagApprovalDecision) -> Self {
        match d {
            DagApprovalDecision::Allow => ApprovalDecision::Allow,
            DagApprovalDecision::Deny => ApprovalDecision::Deny,
            DagApprovalDecision::Modify => ApprovalDecision::Modify,
        }
    }
}

/// 提交 DAG 骨架审批决策。
///
/// 由 webview `DagApprovalDialog` 在用户点击 Allow/Deny 后调用。
/// 通过 `ApprovalRegistry::take_sender` 取出 oneshot sender,发送决策。
/// 返回 true = 投递成功,false = 请求已被消费 / 已过期 / 不存在(一次性语义)。
pub fn submit_dag_skeleton_approval(
    state: &AppState,
    approval_request_id: &str,
    decision: DagApprovalDecision,
) -> UiResult<bool> {
    let sender = match state.approval_registry.take_sender(approval_request_id) {
        Some(s) => s,
        None => return Ok(false),
    };
    let _ = sender.send(decision.into());
    Ok(true)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn approve_dag_skeleton_command(
    state: State<'_, AppState>,
    approval_request_id: String,
    decision: DagApprovalDecision,
) -> Result<bool, String> {
    submit_dag_skeleton_approval(&state, &approval_request_id, decision).map_err(Into::into)
}

// ===== W8 Plan 5 Task 2: list_dag_history + get_dag_plan =====

use trust_kernel::skills::dag_repo::{DagPlanRecord, DagRepo};

/// W8 §2.7:DAG 历史列表项(webview 表格行)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlanSummaryDto {
    pub plan_id: String,
    pub user_goal: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub root_task_id: Option<String>,
    /// 节点数(从 plan_json 解析,前端显示 "N 个节点")
    pub node_count: usize,
    /// 成功率(succeeded 节点数 / 总节点数,0.0-1.0)
    pub success_rate: f32,
}

/// W8 §2.7:DAG 详情(webview 点击行展开)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPlanDetailDto {
    pub plan_id: String,
    pub user_goal: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub max_total_steps: u32,
    pub nodes: Vec<DagNodeDetailDto>,
    pub edges: Vec<DagEdgeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNodeDetailDto {
    pub node_id: String,
    pub skill_id: String,
    pub risk_ceiling: String,
    pub status: String,
    pub input_template_json: String,
    pub output_json: Option<String>,
    pub error_message: Option<String>,
    pub task_id: Option<String>,
    pub step_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagEdgeDto {
    pub from: String,
    pub to: String,
    pub port_binding: Option<String>,
}

/// 状态过滤选项(webview 下拉框)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DagStatusFilter {
    All,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl DagStatusFilter {
    fn to_status_str(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Running => Some("running"),
            Self::Succeeded => Some("succeeded"),
            Self::Failed => Some("failed"),
            Self::Cancelled => Some("cancelled"),
        }
    }
}

/// 从 DagPlanRecord 解析 node_count。
/// plan_json 是序列化的 DagPlan,nodes 字段含节点列表;
/// success_rate 需查 dag_nodes 表(从 plan_json 无法获取节点状态)。
fn parse_node_count_from_plan_json(plan_json: &str) -> usize {
    #[derive(serde::Deserialize)]
    struct PlanShell {
        nodes: Vec<serde_json::Value>,
    }
    serde_json::from_str::<PlanShell>(plan_json)
        .map(|p| p.nodes.len())
        .unwrap_or(0)
}

/// 逻辑函数:分页 + 状态过滤查询 DAG 历史列表。
///
/// - `limit`:每页数量(默认 20,硬上限 100)
/// - `offset`:分页偏移
/// - `filter`:状态过滤(All / Running / Succeeded / Failed / Cancelled)
pub fn list_dag_history(
    state: &AppState,
    limit: usize,
    offset: usize,
    filter: DagStatusFilter,
) -> UiResult<Vec<DagPlanSummaryDto>> {
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let limit_clamped = limit.clamp(1, 100);
    let records: Vec<DagPlanRecord> = match filter.to_status_str() {
        Some(status) => repo.list_plans_by_status(&conn, status)?,
        None => {
            // All:逐个 status 查询后合并(Plan 1 未实现 list_all_plans,W8 简化)
            let mut all = Vec::new();
            for s in [
                "pending",
                "running",
                "succeeded",
                "failed",
                "partially_succeeded",
                "cancelled",
            ] {
                all.extend(repo.list_plans_by_status(&conn, s)?);
            }
            // 按 created_at DESC 排序
            all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            all
        }
    };

    // 分页(offset + limit)
    let paged: Vec<DagPlanRecord> = records
        .into_iter()
        .skip(offset)
        .take(limit_clamped)
        .collect();

    let mut summaries = Vec::with_capacity(paged.len());
    for rec in paged {
        let node_count = parse_node_count_from_plan_json(&rec.plan_json);
        // success_rate 需查 dag_nodes 表
        let nodes = repo.list_nodes_by_plan(&conn, &rec.plan_id)?;
        let total = nodes.len();
        let succeeded = nodes.iter().filter(|n| n.status == "succeeded").count();
        let success_rate = if total == 0 {
            0.0
        } else {
            succeeded as f32 / total as f32
        };
        summaries.push(DagPlanSummaryDto {
            plan_id: rec.plan_id,
            user_goal: rec.user_goal,
            status: rec.status,
            created_at: rec.created_at,
            completed_at: rec.completed_at,
            root_task_id: rec.root_task_id,
            node_count,
            success_rate,
        });
    }
    Ok(summaries)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn list_dag_history_command(
    state: State<'_, AppState>,
    limit: Option<usize>,
    offset: Option<usize>,
    filter: Option<DagStatusFilter>,
) -> Result<Vec<DagPlanSummaryDto>, String> {
    let limit = limit.unwrap_or(20);
    let offset = offset.unwrap_or(0);
    let filter = filter.unwrap_or(DagStatusFilter::All);
    list_dag_history(&state, limit, offset, filter).map_err(Into::into)
}

/// 逻辑函数:查询单个 DAG 完整详情(plan + nodes + edges)。
///
/// 返回 `None`(包在 `Option` 中)若 plan_id 不存在。
/// webview 点击历史表格行时调用,展开节点详情。
pub fn get_dag_plan(state: &AppState, plan_id: &str) -> UiResult<Option<DagPlanDetailDto>> {
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let plan_rec = match repo.get_plan(&conn, plan_id)? {
        Some(r) => r,
        None => return Ok(None),
    };

    // 从 plan_json 解析 max_total_steps + edges
    #[derive(serde::Deserialize)]
    struct PlanShell {
        max_total_steps: u32,
        edges: Vec<DagEdgeDto>,
    }
    let shell: PlanShell = serde_json::from_str(&plan_rec.plan_json).unwrap_or(PlanShell {
        max_total_steps: 0,
        edges: vec![],
    });

    let node_recs = repo.list_nodes_by_plan(&conn, plan_id)?;
    let nodes: Vec<DagNodeDetailDto> = node_recs
        .into_iter()
        .map(|n| DagNodeDetailDto {
            node_id: n.node_id,
            skill_id: n.skill_id,
            risk_ceiling: n.risk_ceiling,
            status: n.status,
            input_template_json: n.input_template_json,
            output_json: n.output_json,
            error_message: n.error_message,
            task_id: n.task_id,
            step_id: n.step_id,
            started_at: n.started_at,
            completed_at: n.completed_at,
        })
        .collect();

    Ok(Some(DagPlanDetailDto {
        plan_id: plan_rec.plan_id,
        user_goal: plan_rec.user_goal,
        status: plan_rec.status,
        created_at: plan_rec.created_at,
        completed_at: plan_rec.completed_at,
        max_total_steps: shell.max_total_steps,
        nodes,
        edges: shell.edges,
    }))
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn get_dag_plan_command(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<Option<DagPlanDetailDto>, String> {
    get_dag_plan(&state, &plan_id).map_err(Into::into)
}

// ===== W8 Plan 5 Task 3: get_task_explanation =====

use trust_kernel::skills::explanation_repo::{TaskExplanationRecord, TaskExplanationRepo};

/// W8 §2.5:task.explain 输出(webview 显示 LLM 失败归因)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExplanationDto {
    pub explanation_id: String,
    pub step_id: String,
    pub root_cause_zh: String,
    pub category: String,
    pub suggested_fix: Option<String>,
    pub confidence: f32,
    pub llm_model: Option<String>,
    pub created_at: String,
}

impl From<TaskExplanationRecord> for TaskExplanationDto {
    fn from(r: TaskExplanationRecord) -> Self {
        Self {
            explanation_id: r.explanation_id,
            step_id: r.step_id,
            root_cause_zh: r.root_cause_zh,
            category: r.category,
            suggested_fix: r.suggested_fix,
            confidence: r.confidence,
            llm_model: r.llm_model,
            created_at: r.created_at,
        }
    }
}

/// 逻辑函数:查询 step 的最新 LLM 失败归因。
///
/// 返回 `None` 若该 step 无归因记录(LLM 未启用 / step 非 Failed / 尚未调用 task.explain)。
/// webview `TaskExplainPanel` 据此显示 "未启用 LLM 归因" 提示。
pub fn get_task_explanation(
    state: &AppState,
    step_id: &str,
) -> UiResult<Option<TaskExplanationDto>> {
    let repo = TaskExplanationRepo::new();
    let conn = state.kernel.conn();
    let rec = repo.get_by_step_id(&conn, step_id)?;
    Ok(rec.map(TaskExplanationDto::from))
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn get_task_explanation_command(
    state: State<'_, AppState>,
    step_id: String,
) -> Result<Option<TaskExplanationDto>, String> {
    get_task_explanation(&state, &step_id).map_err(Into::into)
}
