//! W9 Plan 4:DAG Modify 闭环集成测试。
//!
//! 测试 `DagExecutor::run` 处理 `DagApprovalOutcome::Modify` 分支:
//! 审计 `dag_skeleton_modified` + 重新校验 + `run_modified` 第二次审批。
//! Modify 只允许一次,第二次 Modify 返回 `KernelError::DagModifyLimitExceeded`。

#![cfg(test)]

use std::sync::{Arc, Mutex};

use trust_kernel::approval::approver::{Approver, DagApprovalOutcome};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagNode, DagPlan};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, SlotTemplateEngine, TemplateExpr};

/// 可编程 Approver:按预设序列返回决策(第一次 Modify → 第二次 Allow)。
struct ScriptedApprover {
    outcomes: Mutex<Vec<DagApprovalOutcome>>,
}

impl ScriptedApprover {
    fn new(outcomes: Vec<DagApprovalOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes),
        }
    }
}

impl Approver for ScriptedApprover {
    fn prompt(&self, _manifest: &trust_kernel::policy::transaction::EffectManifest) -> trust_kernel::approval::types::ApprovalDecision {
        trust_kernel::approval::types::ApprovalDecision::Allow
    }

    fn approve_dag_skeleton(&self, _plan: &DagPlan) -> trust_kernel::error::Result<DagApprovalOutcome> {
        let mut guard = self.outcomes.lock().unwrap();
        if guard.is_empty() {
            return Ok(DagApprovalOutcome::Deny);
        }
        Ok(guard.remove(0))
    }
}

fn literal_text_node(node_id: &str, skill_id: &str, text: &str) -> DagNode {
    DagNode {
        node_id: node_id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Number,
            template: TemplateExpr::Literal(text.into()),
        },
        risk_ceiling: trust_kernel::policy::types::ELevel::E0,
    }
}

fn one_node_plan(plan_id: &str, node: DagNode) -> DagPlan {
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "W9 Plan 4 测试".into(),
        nodes: vec![node],
        edges: vec![],
        loop_specs: std::collections::HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn modify_then_approve_allow_runs_modified_plan() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    // 使用 task.explain + SlotKind::Number + 数字字面量(与 W8 既有测试同模式,
    // 不依赖 uia feature;note.capture 需要 cfg(all(windows, feature = "uia")))
    let original_node = literal_text_node("n1", "task.explain", "5");
    let original_plan = one_node_plan("w9p4-modify-allow", original_node);

    let modified_node = literal_text_node("n1", "task.explain", "7");
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    // 第一次 Modify → 第二次 Allow
    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan.clone()) },
        DagApprovalOutcome::Allow,
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan).unwrap();

    // 验证:DagStatus::Succeeded(modified_plan 被执行)
    assert!(matches!(result.status, trust_kernel::skills::dag_types::DagStatus::Succeeded),
        "expected Succeeded, got {:?}", result.status);
}

#[test]
fn modify_with_invalid_modified_plan_fails_validation() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    // 沿用 Task 2 模式:task.explain + SlotKind::Number + E0(测试在 dispatcher 前 fail-fast,
    // skill_id 不重要,但保持与 W8 既有测试一致)
    let original_node = literal_text_node("n1", "task.explain", "5");
    let original_plan = one_node_plan("w9p4-invalid-modify", original_node);

    // modified_plan 引用未知 node_id n99(违反 validate_dag)
    let bad_expr = SlotTemplateEngine::parse("${n99.output.path}").unwrap();
    let modified_node = DagNode {
        node_id: "n1".into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Number,
            template: bad_expr,
        },
        risk_ceiling: trust_kernel::policy::types::ELevel::E0,
    };
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan);

    // 验证:返回 Err(KernelError::Skill("modified_plan validate_dag failed: ..."))
    assert!(result.is_err(), "expected Err for invalid modified_plan");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("validate_dag failed"), "got: {}", err_msg);
}

#[test]
fn modify_with_escalated_risk_ceiling_rejected() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    // 原 plan:n1 = E0(helper 默认)
    let original_node = literal_text_node("n1", "task.explain", "5");
    let original_plan = one_node_plan("w9p4-escalation", original_node);

    // modified_plan:n1 = E3(提权 E0 → E3)
    let modified_node = DagNode {
        node_id: "n1".into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Number,
            template: TemplateExpr::Literal("7".into()),
        },
        risk_ceiling: trust_kernel::policy::types::ELevel::E3, // 提权 E0 → E3
    };
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan);

    // 验证:返回 Err(KernelError::Skill("risk ceiling escalated ..."))
    assert!(result.is_err(), "expected Err for escalated risk_ceiling");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("risk ceiling escalated"), "got: {}", err_msg);
}
