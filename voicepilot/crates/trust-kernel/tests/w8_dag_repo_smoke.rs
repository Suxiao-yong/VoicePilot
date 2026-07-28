//! W8 Plan 1 Task 8: DagRepo + TaskExplanationRepo 端到端冒烟测试.

use std::collections::HashMap;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::types::ELevel;
use trust_kernel::repo::step_repo::{StepRecord, StepRepo, StepStatus};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus};
use trust_kernel::skills::explanation_repo::{
    FailureCategory, TaskExplanationRecord, TaskExplanationRepo,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};
use trust_kernel::state::TaskState;

fn make_plan(plan_id: &str) -> DagPlan {
    let n1 = DagNode {
        node_id: "n1".into(),
        skill_id: "note.capture".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Text,
            template: TemplateExpr::Literal("notepad".into()),
        },
        risk_ceiling: ELevel::E1,
    };
    let n2 = DagNode {
        node_id: "n2".into(),
        skill_id: "files.move".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Path,
            template: TemplateExpr::Var(VarRef {
                scope: VarScope::Prev,
                path: "output.path".into(),
            }),
        },
        risk_ceiling: ELevel::E2,
    };
    DagPlan {
        plan_id: plan_id.into(),
        user_goal: "打开记事本写 TODO 然后保存到桌面".into(),
        nodes: vec![n1, n2],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: Some("output.path -> input.source".into()),
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    }
}

#[test]
fn dag_repo_create_and_get_plan() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-001");

    repo.create_plan(&conn, &plan, &DagStatus::Pending, None)
        .unwrap();

    let got = repo.get_plan(&conn, "plan-001").unwrap().unwrap();
    assert_eq!(got.plan_id, "plan-001");
    assert_eq!(got.status, "pending");
    assert_eq!(got.root_task_id, None);
    assert!(got.plan_json.contains("note.capture"));
    assert!(got.plan_json.contains("files.move"));
}

#[test]
fn dag_repo_update_plan_status() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-002");

    repo.create_plan(&conn, &plan, &DagStatus::Pending, None)
        .unwrap();
    repo.update_plan_status(&conn, "plan-002", &DagStatus::Succeeded, true)
        .unwrap();

    let got = repo.get_plan(&conn, "plan-002").unwrap().unwrap();
    assert_eq!(got.status, "succeeded");
    assert!(got.completed_at.is_some());
}

#[test]
fn dag_repo_list_plans_by_status() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    repo.create_plan(&conn, &make_plan("p-a"), &DagStatus::Pending, None)
        .unwrap();
    repo.create_plan(&conn, &make_plan("p-b"), &DagStatus::Succeeded, None)
        .unwrap();
    repo.create_plan(&conn, &make_plan("p-c"), &DagStatus::Pending, None)
        .unwrap();

    let pending = repo.list_plans_by_status(&conn, "pending").unwrap();
    assert_eq!(pending.len(), 2);
    let succeeded = repo.list_plans_by_status(&conn, "succeeded").unwrap();
    assert_eq!(succeeded.len(), 1);
}

#[test]
fn dag_repo_create_and_update_nodes() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-003");

    repo.create_plan(&conn, &plan, &DagStatus::Running, None)
        .unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }

    // 预创建 parent task + step(FK 约束:dag_nodes.task_id / step_id REFERENCES tasks / steps)
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let mut task_rec = TaskRecord::new("task-001", "test dag node");
    task_rec.status = TaskState::Idle;
    task_repo.create(&conn, &task_rec).unwrap();
    let mut step_rec = StepRecord::new("step-001", "task-001", 0);
    step_rec.status = StepStatus::Pending;
    step_repo.create(&conn, &step_rec).unwrap();

    // 模拟 n1 执行成功,n2 执行失败
    repo.update_node_status(
        &conn,
        "plan-003",
        "n1",
        &DagNodeStatus::Succeeded(serde_json::json!({"output": {"path": "C:/test.txt"}})),
        Some("task-001"),
        Some("step-001"),
    )
    .unwrap();
    repo.update_node_status(
        &conn,
        "plan-003",
        "n2",
        &DagNodeStatus::Failed {
            cause: "permission denied".into(),
        },
        None,
        None,
    )
    .unwrap();

    let nodes = repo.list_nodes_by_plan(&conn, "plan-003").unwrap();
    assert_eq!(nodes.len(), 2);
    let n1_rec = nodes.iter().find(|n| n.node_id == "n1").unwrap();
    assert_eq!(n1_rec.status, "succeeded");
    assert!(n1_rec.output_json.is_some());
    assert_eq!(n1_rec.task_id.as_deref(), Some("task-001"));
    let n2_rec = nodes.iter().find(|n| n.node_id == "n2").unwrap();
    assert_eq!(n2_rec.status, "failed");
    assert_eq!(n2_rec.error_message.as_deref(), Some("permission denied"));
}

#[test]
fn dag_repo_cascade_delete() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = DagRepo::new();
    let plan = make_plan("plan-004");
    repo.create_plan(&conn, &plan, &DagStatus::Pending, None)
        .unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }
    // 确认节点已创建
    let nodes_before = repo.list_nodes_by_plan(&conn, "plan-004").unwrap();
    assert_eq!(nodes_before.len(), 2);

    repo.delete_plan_cascade(&conn, "plan-004").unwrap();

    // 确认 plan + nodes 全删
    assert!(repo.get_plan(&conn, "plan-004").unwrap().is_none());
    let nodes_after = repo.list_nodes_by_plan(&conn, "plan-004").unwrap();
    assert_eq!(nodes_after.len(), 0);
}

#[test]
fn task_explanation_repo_create_and_get() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();

    // 必须先创建 parent task + step(FK 约束:task_explanations.step_id REFERENCES steps)
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let mut task_rec = TaskRecord::new("task-explain-test", "test goal");
    task_rec.status = TaskState::Idle;
    task_repo.create(&conn, &task_rec).unwrap();
    let mut step_rec = StepRecord::new("step-001", "task-explain-test", 0);
    step_rec.status = StepStatus::Pending;
    step_repo.create(&conn, &step_rec).unwrap();

    let rec = TaskExplanationRecord {
        explanation_id: "exp-001".into(),
        step_id: "step-001".into(),
        root_cause_zh: "MCP 服务器未启动".into(),
        category: FailureCategory::McpUnavailable.as_str().into(),
        suggested_fix: Some("请在 Settings 中启动 Playwright MCP".into()),
        confidence: 0.85,
        llm_model: Some("gpt-4o-mini".into()),
        created_at: String::new(),
    };
    repo.create(&conn, &rec).unwrap();

    let got = repo.get_by_id(&conn, "exp-001").unwrap().unwrap();
    assert_eq!(got.step_id, "step-001");
    assert_eq!(got.root_cause_zh, "MCP 服务器未启动");
    assert_eq!(got.category, "mcp_unavailable");
    assert!((got.confidence - 0.85).abs() < 0.001);
    assert_eq!(got.llm_model.as_deref(), Some("gpt-4o-mini"));

    let by_step = repo.get_by_step_id(&conn, "step-001").unwrap().unwrap();
    assert_eq!(by_step.explanation_id, "exp-001");
}

#[test]
fn task_explanation_repo_delete() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let mut task_rec = TaskRecord::new("task-del-test", "test");
    task_rec.status = TaskState::Idle;
    task_repo.create(&conn, &task_rec).unwrap();
    let mut step_rec = StepRecord::new("step-del", "task-del-test", 0);
    step_rec.status = StepStatus::Pending;
    step_repo.create(&conn, &step_rec).unwrap();
    let rec = TaskExplanationRecord {
        explanation_id: "exp-del".into(),
        step_id: "step-del".into(),
        root_cause_zh: "test".into(),
        category: FailureCategory::Unknown.as_str().into(),
        suggested_fix: None,
        confidence: 0.1,
        llm_model: None,
        created_at: String::new(),
    };
    repo.create(&conn, &rec).unwrap();
    assert!(repo.get_by_id(&conn, "exp-del").unwrap().is_some());
    repo.delete(&conn, "exp-del").unwrap();
    assert!(repo.get_by_id(&conn, "exp-del").unwrap().is_none());
}

#[test]
fn failure_category_round_trip() {
    for cat in [
        FailureCategory::McpUnavailable,
        FailureCategory::PathNotAllowed,
        FailureCategory::ApprovalDenied,
        FailureCategory::NetworkError,
        FailureCategory::Unknown,
    ] {
        let s = cat.as_str();
        let back = FailureCategory::from_str(s).unwrap();
        assert_eq!(cat, back);
    }
}
