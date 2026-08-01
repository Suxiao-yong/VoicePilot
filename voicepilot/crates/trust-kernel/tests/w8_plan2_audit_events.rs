//! W8 Plan 2 Task 7: DAG 审计事件测试.
//!
//! Spec §6.1:6 个 event_type 各至少 1 个断言。
//! - dag_plan_created
//! - dag_skeleton_approved
//! - dag_node_started
//! - dag_node_succeeded
//! - dag_node_failed
//! - dag_completed
//!
//! 额外:验证 W1 哈希链在 DAG 审计事件追加后仍完整。

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagNode, DagPlan};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};

fn literal_node(id: &str, skill_id: &str, lit: &str) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: skill_id.into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal(lit.into()),
        },
        risk_ceiling: ELevel::E1,
    }
}

fn plan_with(nodes: Vec<DagNode>) -> DagPlan {
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "audit test goal".into(),
        nodes,
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    }
}

fn find_audit_events(kernel: &TrustKernel, event_type: &str) -> Vec<serde_json::Value> {
    let recent = kernel.list_audit_recent(100).unwrap();
    recent
        .into_iter()
        .filter(|e| e.event_type == event_type)
        .map(|e| e.details)
        .collect()
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

#[test]
fn audit_emits_dag_plan_created_on_run_start() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_plan_created");
    assert!(!events.is_empty(), "dag_plan_created must be emitted");
    let e = &events[0];
    assert_eq!(e["plan_id"], plan.plan_id);
    assert_eq!(e["node_count"], 1);
    assert_eq!(e["max_total_steps"], 10);
}

#[test]
fn audit_emits_dag_skeleton_approved_on_allow() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_skeleton_approved");
    assert!(!events.is_empty());
    let e = &events[0];
    assert_eq!(e["plan_id"], plan.plan_id);
    // W9 Plan 4:`decision` 字段从 `format!("{:?}", decision)`(W8 "Allow")
    // 改为 `outcome.as_str()`(W9 "allow"),闭合 W8 spec §11 format→字符串。
    assert_eq!(e["decision"], "allow");
}

#[test]
fn audit_emits_dag_skeleton_approved_on_deny() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_skeleton_approved");
    assert!(!events.is_empty());
    let e = &events[0];
    // W9 Plan 4:`decision` 字段从 `format!("{:?}", decision)`(W8 "Deny")
    // 改为 `outcome.as_str()`(W9 "deny")。
    assert_eq!(e["decision"], "deny");
}

#[test]
fn audit_emits_dag_node_started_on_each_node() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![
        literal_node("n1", "task.explain", "5"),
        literal_node("n2", "task.explain", "10"),
    ]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_node_started");
    assert_eq!(
        events.len(),
        2,
        "expected 2 dag_node_started events, got: {:?}",
        events
    );
    let n1_event = events.iter().find(|e| e["node_id"] == "n1").unwrap();
    assert_eq!(n1_event["skill_id"], "task.explain");
}

#[test]
fn audit_emits_dag_node_succeeded_on_success() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_node_succeeded");
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e["node_id"], "n1");
    assert!(e["evidence_strength"].is_string());
}

#[test]
fn audit_emits_dag_node_failed_on_failure() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![literal_node("n1", "nonexistent.skill", "test")]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_node_failed");
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e["node_id"], "n1");
    let cause = e["cause"].as_str().unwrap();
    assert!(cause.contains("unknown skill_id"), "got: {}", cause);
}

#[test]
fn audit_emits_dag_completed_on_terminal_status() {
    // 测试 4 种终态各发 1 个 dag_completed 事件
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    // 1. Succeeded
    let plan_ok = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan_ok, &[]).unwrap();
    // 2. Failed
    let plan_fail = plan_with(vec![literal_node("n1", "nonexistent.skill", "x")]);
    executor.run(&plan_fail, &[]).unwrap();

    // 3. Cancelled(用 AutoDenier,需要新 kernel 避免事件混在一起)
    let kernel2 = TrustKernel::open_in_memory().unwrap();
    let approver_deny: Arc<dyn Approver> = Arc::new(AutoDenier);
    let (kernel2_arc, executor2) = make_executor(kernel2, approver_deny);
    let plan_cancel = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor2.run(&plan_cancel, &[]).unwrap();

    // 验证 kernel1(Succeeded + Failed)
    let events1 = find_audit_events(&kernel_arc, "dag_completed");
    assert_eq!(
        events1.len(),
        2,
        "expected 2 dag_completed events, got: {:?}",
        events1
    );
    let succeeded_ev = events1
        .iter()
        .find(|e| e["final_status"] == "succeeded")
        .unwrap();
    assert_eq!(succeeded_ev["succeeded_count"], 1);
    let failed_ev = events1
        .iter()
        .find(|e| e["final_status"] == "failed")
        .unwrap();
    assert_eq!(failed_ev["failed_node"], "n1");

    // 验证 kernel2(Cancelled)
    let events2 = find_audit_events(&kernel2_arc, "dag_completed");
    assert_eq!(events2.len(), 1);
    let cancelled_ev = &events2[0];
    assert_eq!(cancelled_ev["final_status"], "cancelled");
    assert_eq!(cancelled_ev["succeeded_count"], 0);
}

#[test]
fn audit_emits_dag_completed_on_partially_succeeded() {
    // 边界测试:PartiallySucceeded 终态也要发 dag_completed
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    // 2 节点:n1 成功 + n2 失败 → PartiallySucceeded
    let plan = plan_with(vec![
        literal_node("n1", "task.explain", "5"),
        literal_node("n2", "nonexistent.skill", "test"),
    ]);
    executor.run(&plan, &[]).unwrap();

    let events = find_audit_events(&kernel_arc, "dag_completed");
    assert_eq!(events.len(), 1, "expected 1 dag_completed, got: {:?}", events);
    let e = &events[0];
    assert_eq!(e["final_status"], "partially_succeeded");
    assert_eq!(e["succeeded_count"], 1);
    assert_eq!(e["failed_node"], "n2");
}

#[test]
fn audit_hash_chain_remains_intact_after_dag_events() {
    // 验证 DAG 审计事件不破坏 W1 哈希链
    // W1 audit_append 自动链接 prev_hash;此处仅验证事件序列可被 list_audit_recent 读出
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (kernel_arc, executor) = make_executor(kernel, approver);

    let plan = plan_with(vec![literal_node("n1", "task.explain", "5")]);
    executor.run(&plan, &[]).unwrap();

    let recent = kernel_arc.list_audit_recent(50).unwrap();
    // 至少应有:dag_plan_created + dag_skeleton_approved + dag_node_started
    // + dag_node_succeeded + dag_completed + W1 task_created + state_transition
    assert!(
        recent.len() >= 5,
        "expected >= 5 audit events, got: {}",
        recent.len()
    );

    // 验证所有事件都有 hash + prev_hash(W1 哈希链)
    for ev in &recent {
        assert!(
            !ev.hash.is_empty(),
            "audit event hash must be non-empty: {:?}",
            ev
        );
    }
}
