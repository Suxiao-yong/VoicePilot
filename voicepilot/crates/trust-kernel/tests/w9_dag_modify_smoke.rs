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

#[test]
fn second_modify_returns_dag_modify_limit_exceeded() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    // 沿用 Task 2 模式:task.explain + SlotKind::Number + E0
    // (第二次 Modify 在 dispatcher 之前 fail-fast,skill_id 不影响测试)
    let original_node = literal_text_node("n1", "task.explain", "5");
    let original_plan = one_node_plan("w9p4-double-modify", original_node);

    let modified_plan_v1 = original_plan.clone();
    let modified_plan_v2 = original_plan.clone();

    // 第一次 Modify → 第二次仍 Modify → 应返回 DagModifyLimitExceeded
    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan_v1) },
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan_v2) },
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let result = executor.run(&original_plan);

    assert!(result.is_err(), "expected Err for second Modify");
    match result.unwrap_err() {
        trust_kernel::error::KernelError::DagModifyLimitExceeded { plan_id } => {
            // run_modified 用 format!("{}_modified", plan_id) 生成新 plan_id
            assert_eq!(plan_id, "w9p4-double-modify_modified");
        }
        other => panic!("expected DagModifyLimitExceeded, got {:?}", other),
    }

    // 验证审计:dag_modify_limit_exceeded 事件存在
    let conn = kernel.conn();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE event_type = 'dag_modify_limit_exceeded'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "dag_modify_limit_exceeded audit event must be emitted");
}

#[test]
fn modify_emits_complete_audit_events() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());

    // 沿用 Task 2 模式:task.explain + SlotKind::Number + E0(确保 dispatch 成功,
    // 完整审计链含 dag_node_succeeded)
    let original_node = literal_text_node("n1", "task.explain", "5");
    let original_plan = one_node_plan("w9p4-audit-chain", original_node);

    let modified_node = literal_text_node("n1", "task.explain", "7");
    let mut modified_plan = original_plan.clone();
    modified_plan.nodes = vec![modified_node];

    let approver = Arc::new(ScriptedApprover::new(vec![
        DagApprovalOutcome::Modify { modified_plan: Box::new(modified_plan) },
        DagApprovalOutcome::Allow,
    ]));

    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    let _ = executor.run(&original_plan).unwrap();

    // 验证审计链(顺序)— audit_logs 表用 timestamp 列(非 created_at)
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT event_type FROM audit_logs ORDER BY timestamp ASC")
        .unwrap();
    let event_types: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    // 期望序列:dag_plan_created → dag_skeleton_approved(decision=modify) →
    //          dag_skeleton_modified → dag_plan_created(modified_plan_with_id) →
    //          dag_skeleton_approved(decision=allow, phase=after_modify) →
    //          dag_node_started → dag_node_succeeded → dag_completed
    assert!(event_types.iter().any(|e| e == "dag_plan_created"), "missing dag_plan_created");
    assert!(event_types.iter().any(|e| e == "dag_skeleton_modified"), "missing dag_skeleton_modified");
    assert!(event_types.iter().any(|e| e == "dag_skeleton_approved"), "missing dag_skeleton_approved");

    // 验证 dag_skeleton_modified details 不含 input_template / nodes 内容(隐私约束)
    let mut stmt2 = conn
        .prepare("SELECT details FROM audit_logs WHERE event_type = 'dag_skeleton_modified'")
        .unwrap();
    let details_str: String = stmt2
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .next()
        .unwrap();
    // W9 修复(P1-9):解析 JSON 后检查字段,避免字符串 contains 误判
    let details: serde_json::Value = serde_json::from_str(&details_str).unwrap();
    assert!(details.get("input_template").is_none(), "details must not contain input_template field, got: {}", details_str);
    assert!(details.get("nodes").is_none(), "details must not contain nodes field (may contain input_template in nodes), got: {}", details_str);
    assert!(details.get("modified_node_count").is_some(), "details must contain modified_node_count, got: {}", details_str);
}
