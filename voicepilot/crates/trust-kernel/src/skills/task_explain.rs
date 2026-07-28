//! task.explain Skill executor — W7 Plan 2 Task 4.
//!
//! Reads the audit log and shows the most recent N operation records.
//! Per spec §2.4: simplest Skill — risk E0 (read-only), approval None,
//! tools=`audit.read`. No prepare/commit/approval/compensation happens.
//!
//! The Skill's value:
//!   1. Validates the `limit` input (1..=100, manifest default 10).
//!   2. Creates a new task + step recording the user's explain request
//!      (this itself emits audit events — the request is auditable).
//!   3. Calls `kernel.list_audit_recent(limit)` to read recent events.
//!   4. Finalizes the step as Succeeded with Weak evidence.
//!
//! The audit log IS the explanation data. UI/CLI reads
//! `kernel.list_audit_recent(limit)` directly to render the explanation;
//! this Skill does not persist a summary (Option A from the design brief).

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{finalize_step_success, validate_input_against_manifest};
use crate::skills::explanation_repo::FailureCategory;
#[cfg(feature = "llm")]
use crate::skills::explanation_repo::{TaskExplanationRecord, TaskExplanationRepo};
use crate::skills::manifest::task_explain_manifest;
use std::collections::HashMap;

/// Input for the `task.explain` Skill executor.
#[derive(Debug, Clone)]
pub struct TaskExplainInput {
    /// New task ID for this explain operation.
    pub task_id: String,
    /// New step ID.
    pub step_id: String,
    /// Max audit events to read (1..=100; manifest default is 10).
    pub limit: u32,
}

/// W8 Plan 3 Task 7-8:LLM 失败归因结果(spec §2.5)。
///
/// 由 `LlmClient::explain_failure` 返回,被 `execute_task_explain_with_llm`
/// 持久化到 `task_explanations` 表(Task 8)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmAnalysis {
    /// 中文归因,≤ 200 字,仅基于 audit_logs 事实
    pub root_cause_zh: String,
    /// 失败分类(与 `task_explanations.category` 列一致)
    pub category: FailureCategory,
    /// 可选的修复建议
    pub suggested_fix: Option<String>,
    /// LLM 置信度 [0.0, 1.0]
    pub confidence: f32,
}

/// W8 §2.5: task.explain LLM 增强的输出结构。
///
/// 包含静态部分(step_id / status / failed_tool_calls,从 audit_logs 提取)
/// 和可选的 LLM 归因(llm_analysis)。LLM 未配置 / step 非 Failed / LLM
/// 失败回退时 llm_analysis=None。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskExplanation {
    pub step_id: String,
    pub status: StepStatus,
    pub failed_tool_calls: Vec<FailedToolCallSummary>,
    pub llm_analysis: Option<LlmAnalysis>,
}

/// W8 §2.5: 从 audit_logs 提取的失败 tool call 摘要。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FailedToolCallSummary {
    pub tool_name: String,
    pub args: serde_json::Value,
    pub error_message: String,
}

impl TaskExplanation {
    /// 构造无 LLM 归因的解释(LLM 未配置 / step 非 Failed / LLM 失败回退)。
    pub fn structured_only(
        step: &StepRecord,
        audit_logs: &[crate::audit::AuditEvent],
    ) -> Self {
        Self {
            step_id: step.step_id.clone(),
            status: step.status,
            failed_tool_calls: extract_failed_tool_calls(audit_logs),
            llm_analysis: None,
        }
    }
}

/// 从 audit_logs 中提取 `MCP_CALL_FAILED` 事件的 tool_name / args / error_message。
fn extract_failed_tool_calls(
    audit_logs: &[crate::audit::AuditEvent],
) -> Vec<FailedToolCallSummary> {
    audit_logs
        .iter()
        .filter(|e| e.event_type == "MCP_CALL_FAILED")
        .map(|e| FailedToolCallSummary {
            tool_name: e
                .details
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            args: e.details.get("args").cloned().unwrap_or(serde_json::Value::Null),
            error_message: e
                .details
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        })
        .collect()
}

/// W8 §2.5: task.explain LLM 增强主入口。
///
/// 与 W7 既有 `execute_explain` 并存:
/// - W7 `execute_explain`:读 audit_recent,创建新 task/step,返回 task_id(dispatcher 已路由)
/// - W8 `execute_task_explain_with_llm`:针对指定 step_id 做失败归因,可选调 LLM,
///   持久化到 `task_explanations` 表,返回 `TaskExplanation` 内存对象
///
/// 调用方(CLI/UI)自行决定何时切换到本函数(Plan 4 Router Bridge 集成时决定)。
///
/// 参数:
/// - `kernel`:用于读 step / audit_logs / 持久化 / 审计
/// - `input`:复用 W7 既有 `TaskExplainInput`(task_id / step_id / limit)
/// - `_approver`:未使用(task.explain risk=E0,approval=None)
/// - `llm`:`Option<&LlmClient>`;`None` 或 `llm.is_enabled()==false` → 走 structured_only
///
/// 返回:`Result<TaskExplanation>` — 持久化失败 / step 不存在 → `Err`
#[cfg(feature = "llm")]
pub async fn execute_task_explain_with_llm(
    kernel: &TrustKernel,
    input: &TaskExplainInput,
    _approver: &dyn Approver,
    llm: Option<&crate::llm::client::LlmClient>,
) -> Result<TaskExplanation> {
    // Step 1: 读 step(必须存在)。step.status 决定是否调 LLM。
    // KernelError 没有 StepNotFound 变体,用 Skill 字符串包装。
    let step = kernel
        .get_step(&input.step_id)?
        .ok_or_else(|| KernelError::Skill(format!("step not found: {}", input.step_id)))?;

    // Step 2: 读 step 关联的 audit_logs(kernel 仅暴露 list_audit_for_task,
    // 客户端按 step_id 过滤)。
    let all_logs = kernel.list_audit_for_task(&step.task_id)?;
    let audit_logs: Vec<crate::audit::AuditEvent> = all_logs
        .into_iter()
        .filter(|e| e.step_id.as_deref() == Some(&step.step_id))
        .collect();

    // Step 3: 决定是否调 LLM。
    // - llm 为 None → 不调
    // - llm.is_enabled()==false → 不调
    // - step.status != Failed → 不调(spec §7.5:仅 Failed 时调 LLM)
    let should_call_llm = llm
        .map(|l| l.is_enabled() && step.status == StepStatus::Failed)
        .unwrap_or(false);

    let mut llm_analysis: Option<LlmAnalysis> = None;
    let mut llm_model_used: Option<String> = None;

    if should_call_llm {
        if let Some(l) = llm {
            match l.explain_failure(&step, &audit_logs).await {
                Ok(analysis) => {
                    llm_model_used = Some(l.model().to_string());
                    llm_analysis = Some(analysis);
                }
                Err(e) => {
                    // spec §2.5:LLM 失败 → 回退 structured_only,不阻塞。
                    tracing::warn!(
                        error = ?e,
                        step_id = %step.step_id,
                        "LLM explain_failure failed; falling back to structured_only"
                    );
                }
            }
        }
    }

    // Step 4: 构造 TaskExplanation。
    let explanation = if let Some(analysis) = llm_analysis.clone() {
        TaskExplanation {
            step_id: step.step_id.clone(),
            status: step.status,
            failed_tool_calls: extract_failed_tool_calls(&audit_logs),
            llm_analysis: Some(analysis),
        }
    } else {
        TaskExplanation::structured_only(&step, &audit_logs)
    };

    // Step 5: 持久化 LLM 归因到 task_explanations 表(仅当有 llm_analysis)。
    if let Some(ref analysis) = explanation.llm_analysis {
        let rec = TaskExplanationRecord {
            explanation_id: uuid::Uuid::new_v4().to_string(),
            step_id: step.step_id.clone(),
            root_cause_zh: analysis.root_cause_zh.clone(),
            category: analysis.category.as_str().to_string(),
            suggested_fix: analysis.suggested_fix.clone(),
            confidence: analysis.confidence,
            llm_model: llm_model_used.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        {
            let conn = kernel.conn();
            TaskExplanationRepo::new().create(&conn, &rec)?;
        }
    }

    // Step 6: 发 llm_explain_called 审计事件(仅当 LLM 实际成功调用)。
    if let Some(ref analysis) = explanation.llm_analysis {
        kernel.audit_append_external(
            &step.task_id,
            Some(&step.step_id),
            "llm_explain_called",
            serde_json::json!({
                "step_id": step.step_id,
                "llm_model": llm_model_used,
                "category": analysis.category.as_str(),
                "token_count": 0i64,
            }),
        )?;
    }

    Ok(explanation)
}

/// Execute the `task.explain` Skill.
///
/// Read-only: reads the most recent `limit` audit events. No
/// prepare/commit/approval/compensation happens. Returns the new task_id
/// on success. The `_approver` parameter is unused (approval=None in the
/// manifest).
pub fn execute_explain(
    kernel: &TrustKernel,
    input: &TaskExplainInput,
    _approver: &dyn Approver,
) -> Result<String> {
    // Step 1: validate limit bounds explicitly. The manifest declares
    // `limit` as a Number with max_length=100, but
    // `validate_input_against_manifest` only enforces max_length on Text
    // inputs — it does not interpret max_length as a numeric upper bound.
    // So we enforce 1..=100 here.
    if input.limit == 0 || input.limit > 100 {
        return Err(KernelError::Skill(format!(
            "validation failed for limit: must be in 1..=100, got {}",
            input.limit
        )));
    }
    let mut input_map: HashMap<String, serde_json::Value> = HashMap::new();
    input_map.insert("limit".to_string(), serde_json::json!(input.limit));
    validate_input_against_manifest(&input_map, &task_explain_manifest())?;

    // Step 2: create new task + step recording the user's explain request.
    kernel.create_task(&input.task_id, "task.explain")?;
    let step = StepRecord::new(input.step_id.clone(), input.task_id.clone(), 1);
    kernel.create_step(&step)?;

    // Step 3: mark step Running. If this fails, the step stays Pending —
    // no compensation to attempt.
    kernel.update_step_status(&input.step_id, StepStatus::Running)?;

    // Step 4: read the most recent N audit events. The events themselves
    // are the explanation data — UI reads `kernel.list_audit_recent(limit)`
    // directly to render. We read here to validate the operation succeeds
    // and to surface any DB read failure as a Failed step.
    let _ = kernel
        .list_audit_recent(input.limit as usize)
        .inspect_err(|_e| {
            let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
        })?;

    // Step 5: finalize step as Succeeded with Weak evidence (read-only,
    // no compensation).
    finalize_step_success(kernel, &input.step_id, "weak", None).inspect_err(|_e| {
        let _ = kernel.update_step_status(&input.step_id, StepStatus::Failed);
    })?;

    Ok(input.task_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::repo::step_repo::{StepRecord, StepStatus};

    /// Seed the kernel with a few prior audit events by creating tasks +
    /// steps. Each `create_task` / `create_step` emits an audit event.
    fn seed_audit_events(kernel: &TrustKernel, n: usize) {
        for i in 0..n {
            let task_id = format!("seed-task-{i}");
            let step_id = format!("seed-step-{i}");
            kernel.create_task(&task_id, &format!("seed {i}")).unwrap();
            kernel
                .create_step(&StepRecord::new(step_id, task_id, 1))
                .unwrap();
        }
    }

    #[test]
    fn execute_explain_returns_task_id_and_logs_audit() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // Seed 3 prior tasks — each emits TASK_CREATED + STEP_CREATED.
        seed_audit_events(&kernel, 3);

        let before_count = kernel.list_audit_recent(1000).unwrap().len();

        let input = TaskExplainInput {
            task_id: "explain-task".to_string(),
            step_id: "explain-step".to_string(),
            limit: 5,
        };
        let approver = AutoApprover;
        let result = execute_explain(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "explain-task");

        // New task + step were created; step is Succeeded with weak
        // evidence and no compensation.
        assert!(kernel.get_task("explain-task").unwrap().is_some());
        let step = kernel.get_step("explain-step").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
        assert_eq!(step.evidence_strength.as_deref(), Some("weak"));
        assert!(step.compensation_ref.is_none());

        // The new task + step creation emitted more audit events.
        let after_count = kernel.list_audit_recent(1000).unwrap().len();
        assert!(
            after_count > before_count,
            "expected audit log to grow, before={before_count}, after={after_count}"
        );
    }

    #[test]
    fn execute_explain_fails_when_limit_exceeds_100() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let input = TaskExplainInput {
            task_id: "explain-task".to_string(),
            step_id: "explain-step".to_string(),
            limit: 101,
        };
        let approver = AutoApprover;
        let result = execute_explain(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("validation failed"),
            "expected 'validation failed' in error, got: {}",
            err
        );
    }

    #[test]
    fn execute_explain_fails_when_limit_is_zero() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let input = TaskExplainInput {
            task_id: "explain-task".to_string(),
            step_id: "explain-step".to_string(),
            limit: 0,
        };
        let approver = AutoApprover;
        let result = execute_explain(&kernel, &input, &approver);

        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::Skill(_)));
        assert!(
            err.to_string().contains("validation failed"),
            "expected 'validation failed' in error, got: {}",
            err
        );
    }

    #[test]
    fn execute_explain_succeeds_with_empty_audit_log() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // No prior events — reading an empty log is not an error.
        let input = TaskExplainInput {
            task_id: "explain-task".to_string(),
            step_id: "explain-step".to_string(),
            limit: 10,
        };
        let approver = AutoApprover;
        let result = execute_explain(&kernel, &input, &approver);

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        assert_eq!(result.unwrap(), "explain-task");
        let step = kernel.get_step("explain-step").unwrap().unwrap();
        assert_eq!(step.status, StepStatus::Succeeded);
    }
}
