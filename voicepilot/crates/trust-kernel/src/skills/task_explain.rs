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
