//! W8 Plan 2 Task 4-6: DagExecutor 集成测试.
//!
//! 覆盖:
//! - Task 4: run_simple_node(简单节点成功 / 模板失败 / dispatch 失败)
//! - Task 5: DAG 骨架审批 Deny 短路
//! - Task 6: 失败处理 + PartiallySucceeded 分支

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, DagStatus};
use trust_kernel::skills::template::{
    SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope,
};

fn literal_node(id: &str, skill_id: &str, literal: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal(literal.into()),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn var_node(id: &str, skill_id: &str, scope: VarScope, path: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Var(VarRef { scope, path: path.into() }),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with(nodes: Vec<DagNode>, edges: Vec<DagEdge>) -> DagPlan {
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test goal".into(),
        nodes,
        edges,
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    }
}

fn make_executor(
    kernel: TrustKernel,
    approver: Arc<dyn Approver>,
) -> (Arc<TrustKernel>, DagExecutor) {
    let kernel_arc = Arc::new(kernel);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);
    (kernel_arc, executor)
}

// ===== Task 4: run_simple_node =====

#[test]
fn run_simple_node_succeeds_with_task_explain() {
    // 单节点 DAG,task.explain + limit=5 + AutoApprover → 节点 Succeeded
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = DagNode {
        node_id: "n1".into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Number,
            template: TemplateExpr::Literal("5".into()),
        },
        risk_ceiling: ELevel::E0,
    };
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    let n1_status = result.node_results.get("n1").unwrap();
    assert!(n1_status.is_succeeded(), "got: {:?}", n1_status);
}

#[test]
fn run_simple_node_fails_on_template_resolution_error() {
    // ${prev.output.path} 但 prev_node_id=None → VarNotFound → 节点 Failed
    // 注:模板校验在 LLM 返回后立即做(spec §2.1 双层防御),此处测试 Layer 2
    // (执行前再次校验)。LLM 校验在 Task 8 测试。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = var_node("n1", "task.explain", VarScope::Prev, "output.path");
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    // 模板解析失败 → 节点 Failed → DAG Failed(无成功节点)
    match result.status {
        DagStatus::Failed { ref failed_node, .. } => {
            assert_eq!(failed_node, "n1");
        }
        other => panic!("expected Failed, got {:?}", other),
    }
    let n1_status = result.node_results.get("n1").unwrap();
    assert!(n1_status.is_failed(), "got: {:?}", n1_status);
}

#[test]
fn run_simple_node_fails_on_unknown_skill_id() {
    // skill_id = "nonexistent.skill" → dispatch_skill_executor 返回 Err
    // → 节点 Failed → DAG Failed
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = literal_node("n1", "nonexistent.skill", "test");
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    match result.status {
        DagStatus::Failed { ref failed_node, ref cause } => {
            assert_eq!(failed_node, "n1");
            assert!(cause.contains("unknown skill_id"), "got: {}", cause);
        }
        other => panic!("expected Failed, got {:?}", other),
    }
}

// ===== Task 5: DAG 骨架 Deny 短路 =====

#[test]
fn dag_skeleton_deny_short_circuits_to_cancelled() {
    // AutoDenier → approve_dag_skeleton 返回 Deny
    // → 0 节点执行 + DagResult::cancelled() + dag_plans.status = "cancelled"
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    // 即使 plan 含节点,Deny 也不应执行任何节点
    let n1 = literal_node("n1", "task.explain", "5");
    let n2 = literal_node("n2", "task.explain", "3");
    let plan = plan_with(
        vec![n1, n2],
        vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
    );
    let plan_id = plan.plan_id.clone();
    let result = executor.run(&plan).unwrap();

    // 验证返回值:Cancelled + 空 node_results
    assert_eq!(result.status, DagStatus::Cancelled);
    assert!(
        result.node_results.is_empty(),
        "Deny must not produce any node_results, got: {:?}",
        result.node_results
    );

    // 验证 DB 持久化:dag_plans.status = "cancelled"
    let conn = kernel_arc.conn();
    let plan_record = executor
        .dag_repo()
        .get_plan(&conn, &plan_id)
        .unwrap()
        .expect("dag_plan row must exist after run()");
    assert_eq!(
        plan_record.status, "cancelled",
        "dag_plans.status should be 'cancelled' after Deny, got: {}",
        plan_record.status
    );
    assert!(
        plan_record.completed_at.is_some(),
        "completed_at should be set for Cancelled terminal state"
    );

    // 验证 0 节点执行:dag_nodes 表应为空
    let nodes = executor
        .dag_repo()
        .list_nodes_by_plan(&conn, &plan_id)
        .unwrap();
    assert!(
        nodes.is_empty(),
        "Deny must not create any dag_nodes rows, got: {:?}",
        nodes
    );
}

#[test]
fn dag_skeleton_allow_proceeds_to_node_execution() {
    // 对照测试:AutoApprover → approve_dag_skeleton 返回 Allow
    // → 节点正常执行(单节点 task.explain → Succeeded)
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel_arc, executor) = make_executor(kernel, approver);

    let node = literal_node("n1", "task.explain", "5");
    let plan = plan_with(vec![node], vec![]);
    let result = executor.run(&plan).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    assert_eq!(result.node_results.len(), 1);
    assert!(result.node_results.get("n1").unwrap().is_succeeded());
}

// ===== Task 6: PartiallySucceeded 分支 =====

#[test]
fn run_with_second_node_fails_returns_partially_succeeded() {
    // 2 节点 DAG:n1 = task.explain(成功)+ n2 = nonexistent.skill(失败)
    // → DAG PartiallySucceeded { succeeded: ["n1"], failed_node: "n2" }
    // 决策 #4 + #8:不回滚前序已 commit 节点(n1 的 task/step 不被回滚)
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "task.explain", "5");
    let n2 = literal_node("n2", "nonexistent.skill", "test");
    let plan = plan_with(
        vec![n1, n2],
        vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
    );
    let plan_id = plan.plan_id.clone();
    let result = executor.run(&plan).unwrap();

    match result.status {
        DagStatus::PartiallySucceeded {
            ref succeeded,
            ref failed_node,
            ref cause,
        } => {
            assert_eq!(failed_node, "n2");
            assert_eq!(succeeded.len(), 1);
            assert_eq!(succeeded[0], "n1");
            // cause 来自 dispatch_skill_executor 的 unknown skill_id 错误
            assert!(
                cause.contains("unknown skill_id"),
                "cause should mention unknown skill_id, got: {}",
                cause
            );
        }
        other => panic!("expected PartiallySucceeded, got {:?}", other),
    }

    // 验证 node_results:n1 Succeeded,n2 Failed
    let n1_status = result.node_results.get("n1").unwrap();
    assert!(n1_status.is_succeeded(), "n1 should be Succeeded");
    let n2_status = result.node_results.get("n2").unwrap();
    assert!(n2_status.is_failed(), "n2 should be Failed");

    // 验证 DB 持久化
    let conn = kernel_arc.conn();
    let plan_record = executor
        .dag_repo()
        .get_plan(&conn, &plan_id)
        .unwrap()
        .expect("dag_plan row must exist");
    assert_eq!(
        plan_record.status, "partially_succeeded",
        "dag_plans.status should be 'partially_succeeded', got: {}",
        plan_record.status
    );

    let nodes = executor
        .dag_repo()
        .list_nodes_by_plan(&conn, &plan_id)
        .unwrap();
    assert_eq!(nodes.len(), 2, "both nodes should be persisted");
    let n1_record = nodes.iter().find(|n| n.node_id == "n1").unwrap();
    let n2_record = nodes.iter().find(|n| n.node_id == "n2").unwrap();
    assert_eq!(n1_record.status, "succeeded");
    assert!(
        n1_record.completed_at.is_some(),
        "n1 completed_at should be set"
    );
    assert_eq!(n2_record.status, "failed");
    assert!(
        n2_record.error_message.is_some(),
        "n2 error_message should be set"
    );
    assert!(
        n2_record.completed_at.is_some(),
        "n2 completed_at should be set even on failure"
    );

    // 决策 #4 + #8 验证:不回滚前序已 commit 节点
    // n1 成功后 task/step 已 commit,n2 失败不应影响 n1 的 task/step 持久化
    assert!(
        n1_record.task_id.is_some(),
        "n1 task_id should be persisted (no rollback of committed nodes)"
    );
    assert!(
        n1_record.step_id.is_some(),
        "n1 step_id should be persisted (no rollback of committed nodes)"
    );
    // n2 失败:dispatch_skill_executor 返回 Err,task/step 未创建 → 不写入(避免 FK 违规)
    assert!(
        n2_record.task_id.is_none(),
        "n2 task_id should be None (dispatch failed before task creation)"
    );
    assert!(
        n2_record.step_id.is_none(),
        "n2 step_id should be None (dispatch failed before step creation)"
    );
}

#[test]
fn run_with_first_node_fails_returns_failed_not_partial() {
    // 对照测试:首节点就失败 → DagStatus::Failed(无已成功节点)
    // 区分 Failed vs PartiallySucceeded 的边界:succeeded.is_empty() → Failed
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let n1 = literal_node("n1", "nonexistent.skill", "test");
    let n2 = literal_node("n2", "task.explain", "5");
    let plan = plan_with(
        vec![n1, n2],
        vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
    );
    let plan_id = plan.plan_id.clone();
    let result = executor.run(&plan).unwrap();

    match result.status {
        DagStatus::Failed {
            ref failed_node,
            ref cause,
        } => {
            assert_eq!(failed_node, "n1");
            assert!(cause.contains("unknown skill_id"));
        }
        other => panic!("expected Failed, got {:?}", other),
    }

    // 验证 n2 未执行(dag_nodes 不应有 n2 行)
    let conn = kernel_arc.conn();
    let nodes = executor
        .dag_repo()
        .list_nodes_by_plan(&conn, &plan_id)
        .unwrap();
    // n1 因 dispatch 失败前已 create_node(spec §2.6 start 状态持久化)
    // n2 因 n1 失败短路,未进入 run_simple_node → 不应有 n2 行
    let n2_record = nodes.iter().find(|n| n.node_id == "n2");
    assert!(
        n2_record.is_none(),
        "n2 should not be persisted (short-circuited by n1 failure)"
    );
}
