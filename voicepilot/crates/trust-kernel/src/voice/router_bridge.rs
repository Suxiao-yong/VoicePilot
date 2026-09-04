//! RouterBridge — wires transcribed text → PlannerPipeline → Skill execution.
//!
//! V1.1 §2.1 + §5.1: voice input pipeline terminal stage.
//! Returns RouteOutcome so caller (CLI) can decide UI feedback.
//!
//! W1 Task 1.3: 三个真实路由入口(router_bridge / UI commands / UI voice)统一
//! 委托给 `PlannerPipeline` 纯规划层。本模块只保留 facade 职责:
//!   1. `RouteOutcome` / `RouteTextResult` 映射(等价于既有行为)
//!   2. DAG 成功路径的副作用(仅当 LLM 实际产出并通过校验的 DAG):
//!      `task-llm-{uuid}` 占位 task + `llm_decompose_called` 审计(仅一次,
//!      硬约束 plan_id / llm_model / latency_ms / token_count)+
//!      `llm_output` taint 标记 —— 全部在 facade 层,不在 planner 内。

use std::sync::Arc;

use crate::approval::approver::Approver;
use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::planner::{PlannerInput, PlannerPipeline, PlannerSource, PlanResult, PlannerTrace};

#[derive(Debug)]
pub enum RouteOutcome {
    /// Skill matched. Caller (CLI) prompts user for args, then invokes
    /// `FilesOrganizeSkill::execute` to run the prepare→approve→commit flow.
    Routed {
        skill_id: String,
    },
    /// W8 Plan 4 新增:LLM 拆解为多步 DAG。Caller 应弹 DAG 骨架审批 UI
    /// (Plan 5 实现),Allow 后调 `DagExecutor::run` 执行。
    #[cfg(feature = "llm")]
    DagPlan(crate::skills::dag_types::DagPlan),
    /// No skill matched; caller should fall back to LLM Planner (W7).
    Unmatched { text: String },
    /// Input was empty/whitespace.
    Empty,
}

/// 在同步上下文驱动 async 规划(route_text / UI voice 路径用)。
///
/// 新线程 + current-thread runtime:在已有 tokio runtime 上下文(如 Tauri
/// async command)内外调用都安全 —— 直接 `Runtime::new().block_on` 在 runtime
/// 内会 panic("Cannot start a runtime from within a runtime")。
pub fn block_on_planner<F, T>(future: F) -> crate::error::Result<T>
where
    F: std::future::Future<Output = crate::error::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| crate::error::KernelError::Skill(format!("planner runtime: {e}")))?;
        runtime.block_on(future)
    })
    .join()
    .map_err(|_| crate::error::KernelError::Skill("planner thread panicked".to_string()))?
}

/// Route transcribed text through PlannerPipeline.
///
/// If a Skill is matched, this fn does NOT execute the Skill — execution
/// requires user-supplied arguments (source, filter, destination) that
/// aren't derivable from the voice text alone in W5. W5 PoC: caller (CLI)
/// prompts user for missing args. W7 LLM Planner will extract args from
/// text automatically.
///
/// The `_approver` param is unused in W5 (routing only); it exists so W7 can
/// extend this fn to invoke the Skill executor directly without changing the
/// call signature.
pub fn route_text(
    kernel: &TrustKernel,
    _approver: &dyn Approver,
    text: &str,
) -> Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    // W1 Task 1.3: 委托 PlannerPipeline(纯规划),同步上下文经 block_on_planner
    // 驱动。keyword 命中路径不产生任何 task / audit / taint(既有行为)。
    let kernel = kernel.clone_arc();
    let text = trimmed.to_string();
    block_on_planner(async move { route_via_pipeline(kernel, text, PlannerSource::Text, None).await })
}

/// W8 Plan 4: 三级路由策略(spec §2.8)的 facade 入口。
///
/// 规划本身(关键词优先 → LLM classify → LLM DAG 拆解 → 双层防御校验)全部
/// 在 `PlannerPipeline` 内完成(spec §2.8 语义不变);本函数只做:
///   1. `PlanResult` → `RouteOutcome` 映射
///   2. DAG 成功路径的副作用(task-llm-{uuid} 占位 task + `llm_decompose_called`
///      审计 + `llm_output` taint,仅当 LLM 实际产出并通过校验的 DAG,仅一次)
///
/// 与 W7 `route_text` 的区别:
/// - async(LLM 调用是 async)
/// - 不接收 `approver` 参数(本函数只路由不执行,DAG 执行时由 DagExecutor 自带 approver)
/// - 返回 `RouteOutcome` 而非 `RouteDecision`(便于调用方直接处理 DAG / Routed / Unmatched)
///
/// # Errors
/// - `KernelError::Db` 当 KV 读取 privacy_mode 失败时(保守策略:返回 Err 让上层处理)
/// - `KernelError::Db` 当 `create_task` 失败时(LLM 成功路径需预创建 task 满足审计 FK)
/// - 规划错误均由 pipeline catch 并收敛到 `Unmatched`(回退语义与 W8 一致)
#[cfg(feature = "voice")]
pub async fn route_text_with_dag(
    kernel: &TrustKernel,
    text: &str,
) -> crate::error::Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    route_via_pipeline(kernel.clone_arc(), trimmed.to_string(), PlannerSource::Voice, None).await
}

/// 带实时快照的语音路由入口（UI voice 路径用）。
/// 快照由调用方现采现传；`None` 时行为等价 `route_text_with_dag`。
#[cfg(feature = "voice")]
pub async fn route_text_with_snapshot(
    kernel: &TrustKernel,
    text: &str,
    snapshot: Option<crate::planner::RealtimeSnapshot>,
) -> crate::error::Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    route_via_pipeline(
        kernel.clone_arc(),
        trimmed.to_string(),
        PlannerSource::Voice,
        snapshot,
    )
    .await
}

/// PlannerPipeline facade 的共享实现(route_text / route_text_with_dag 共用)。
///
/// 纯规划由 pipeline 完成;facade 层负责 `RouteOutcome` 映射与 DAG 副作用
/// (task-llm-{uuid} 占位 task + llm_decompose_called 审计 + llm_output taint)。
/// keyword / classify 命中路径无任何 DB 副作用(既有行为)。
async fn route_via_pipeline(
    kernel: Arc<TrustKernel>,
    text: String,
    source: PlannerSource,
    snapshot: Option<crate::planner::RealtimeSnapshot>,
) -> Result<RouteOutcome> {
    let pipeline = PlannerPipeline::new(kernel.clone(), kernel.extension_snapshot());
    let (plan, trace) = pipeline
        .plan(PlannerInput {
            text: text.clone(),
            source,
            snapshot,
        })
        .await?;

    match plan {
        PlanResult::Empty => Ok(RouteOutcome::Empty),
        PlanResult::Skill { extension_id, .. } => Ok(RouteOutcome::Routed {
            skill_id: extension_id,
        }),
        PlanResult::Dag(dag) => {
            #[cfg(feature = "llm")]
            {
                // 仅 LLM 实际产出并通过校验的 DAG 才落副作用(不重复记录:
                // classify-only 计划走 Skill 分支,不产生 llm_decompose_called)。
                if trace.used_llm {
                    apply_dag_side_effects(&kernel, &text, &dag, &trace)?;
                }
                Ok(RouteOutcome::DagPlan(dag))
            }
            #[cfg(not(feature = "llm"))]
            {
                // 无 llm feature 时 pipeline 从不返回 Dag;防御性映射为 Unmatched。
                let _ = (dag, trace);
                Ok(RouteOutcome::Unmatched { text })
            }
        }
        PlanResult::Unmatched { text } => Ok(RouteOutcome::Unmatched { text }),
    }
}

/// DAG 成功路径的 facade 级副作用(spec §6.1)。
///
/// - `task-llm-{uuid}` 占位 task:audit_logs.task_id NOT NULL + FK 约束要求
///   task 先存在(DagExecutor::run 创建自己的 root_task_id,不复用此临时 task)。
/// - `record_llm_decompose_called` 审计(硬约束 4 字段:plan_id / llm_model /
///   latency_ms / token_count,全部来自 PlannerTrace)。
/// - `tag_dag_plan_literals`:`llm_output` taint(仅校验通过的 DAG 才标记;
///   校验失败的计划已被 pipeline 丢弃,无孤儿记录)。taint 失败不阻断流程。
#[cfg(feature = "llm")]
fn apply_dag_side_effects(
    kernel: &TrustKernel,
    trimmed: &str,
    dag: &crate::skills::dag_types::DagPlan,
    trace: &PlannerTrace,
) -> Result<()> {
    let llm_task_id = format!("task-llm-{}", uuid::Uuid::new_v4());
    kernel.create_task(&llm_task_id, &format!("LLM decompose for: {}", trimmed))?;

    let stats = crate::llm::types::DecomposeStats {
        llm_model: trace.llm_model.clone().unwrap_or_default(),
        latency_ms: trace.latency_ms,
        token_count: trace.token_count.unwrap_or(0) as u32,
    };
    let _ = crate::llm::client::record_llm_decompose_called(
        kernel,
        &llm_task_id,
        &dag.plan_id,
        &stats,
    );

    if let Err(e) = crate::llm::client::tag_dag_plan_literals(kernel, &llm_task_id, dag) {
        tracing::warn!(
            error = %e,
            "tag_dag_plan_literals failed; continuing without llm_output taints"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;

    /// W7 Plan 4 Task 6: `quick.app_control` 注册后,`route_text` 应将
    /// "打开记事本" 路由到该 Skill(intent_example 命中)。仅在 `uia` feature
    /// 开启时验证 —— 无 uia 时 manifest 未注册,路由会落到 Planner。
    #[cfg(all(windows, feature = "uia"))]
    #[test]
    fn route_text_recognizes_app_control_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "打开记事本").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "quick.app_control"),
            "expected Routed to quick.app_control, got {:?}",
            result
        );
    }

    /// W7 Plan 4 Task 6: `note.capture` 注册后,`route_text` 应将
    /// "用记事本记录这个想法" 路由到该 Skill(intent_example "用记事本记录" 命中)。
    #[cfg(all(windows, feature = "uia"))]
    #[test]
    fn route_text_recognizes_note_capture_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "用记事本记录这个想法").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "note.capture"),
            "expected Routed to note.capture, got {:?}",
            result
        );
    }

    /// W7 Plan 5 Task 6: `research.save_markdown` 注册后,`route_text` 应将
    /// "把这个网页存为 Markdown" 路由到该 Skill(keyword "网页" + "Markdown" 命中)。
    /// 跨平台,无 cfg 门控。
    #[test]
    fn route_text_recognizes_research_save_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "把这个网页存为 Markdown").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "research.save_markdown"),
            "expected Routed to research.save_markdown, got {:?}",
            result
        );
    }

    /// W7 Plan 5 Task 6: `form.prepare` 注册后,`route_text` 应将
    /// "帮我填这个表单" 路由到该 Skill(keyword "表单" + "填" 命中)。跨平台。
    #[test]
    fn route_text_recognizes_form_prepare_intent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let approver = AutoApprover;
        let result = route_text(&kernel, &approver, "帮我填这个表单").unwrap();
        assert!(
            matches!(result, RouteOutcome::Routed { ref skill_id } if skill_id == "form.prepare"),
            "expected Routed to form.prepare, got {:?}",
            result
        );
    }
}
