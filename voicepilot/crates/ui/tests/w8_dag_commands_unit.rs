//! W8 Plan 5 Task 2: list_dag_history + get_dag_plan 集成测试.
//!
//! 验证 DAG 历史列表的分页 + 状态过滤 + node_count + success_rate 计算,
//! 以及单 DAG 详情查询(plan + nodes + edges)。

#![cfg(feature = "tauri")]

use std::collections::HashMap;

use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus,
};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};
use voicepilot_ui::dag_commands::{get_dag_plan, list_dag_history, DagStatusFilter};
use voicepilot_ui::state::AppState;

fn make_test_plan(plan_id: &str, _status: &DagStatus) -> DagPlan {
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

fn seed_plan(state: &AppState, plan_id: &str, status: &DagStatus) {
    let repo = DagRepo::new();
    let conn = state.kernel.conn();
    let plan = make_test_plan(plan_id, status);
    repo.create_plan(&conn, &plan, status, None).unwrap();
    for node in &plan.nodes {
        repo.create_node(&conn, &plan.plan_id, node).unwrap();
    }
}

#[test]
fn list_dag_history_returns_all_when_filter_all() {
    let state = AppState::new_in_memory().unwrap();
    seed_plan(
        &state,
        "p-001",
        &DagStatus::Succeeded,
    );
    seed_plan(
        &state,
        "p-002",
        &DagStatus::Failed {
            failed_node: "n2".into(),
            cause: "permission denied".into(),
        },
    );

    let result = list_dag_history(&state, 20, 0, DagStatusFilter::All).unwrap();
    assert_eq!(result.len(), 2, "should return 2 plans");
    // created_at DESC 排序(后插入的在前)
    assert_eq!(result[0].plan_id, "p-002");
    assert_eq!(result[1].plan_id, "p-001");
}

#[test]
fn list_dag_history_filters_by_status() {
    let state = AppState::new_in_memory().unwrap();
    seed_plan(&state, "p-running", &DagStatus::Running);
    seed_plan(&state, "p-succeeded", &DagStatus::Succeeded);

    let running = list_dag_history(&state, 20, 0, DagStatusFilter::Running).unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].plan_id, "p-running");

    let succeeded = list_dag_history(&state, 20, 0, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(succeeded.len(), 1);
    assert_eq!(succeeded[0].plan_id, "p-succeeded");
}

#[test]
fn list_dag_history_paginates_with_offset_and_limit() {
    let state = AppState::new_in_memory().unwrap();
    for i in 0..5 {
        seed_plan(&state, &format!("p-{:03}", i), &DagStatus::Succeeded);
    }

    let page1 = list_dag_history(&state, 2, 0, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(page1.len(), 2);

    let page2 = list_dag_history(&state, 2, 2, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(page2.len(), 2);

    let page3 = list_dag_history(&state, 2, 4, DagStatusFilter::Succeeded).unwrap();
    assert_eq!(page3.len(), 1, "last page has 1 item");
}

#[test]
fn list_dag_history_computes_node_count_and_success_rate() {
    let state = AppState::new_in_memory().unwrap();
    let repo = DagRepo::new();
    // 用 block 限制 conn 守卫生命周期,避免下方 `list_dag_history(&state, ...)`
    // 内部再次 `state.kernel.conn()` 时触发 Mutex 重入死锁。
    {
        let conn = state.kernel.conn();
        // FK 约束:dag_nodes.task_id → tasks.task_id, dag_nodes.step_id → steps.step_id
        // `update_node_status(..., Some("task-1"), Some("step-1"))` 前必须先建父行。
        let task_repo = TaskRepo::new();
        let step_repo = StepRepo::new();
        let task = TaskRecord::new("task-1".to_string(), "test goal");
        task_repo.create(&conn, &task).unwrap();
        let step = StepRecord::new("step-1", "task-1", 0);
        step_repo.create(&conn, &step).unwrap();

        let plan = make_test_plan(
            "p-rate",
            &DagStatus::PartiallySucceeded {
                succeeded: vec!["n1".into()],
                failed_node: "n2".into(),
                cause: "err".into(),
            },
        );
        repo.create_plan(
            &conn,
            &plan,
            &DagStatus::PartiallySucceeded {
                succeeded: vec!["n1".into()],
                failed_node: "n2".into(),
                cause: "err".into(),
            },
            None,
        )
        .unwrap();
        for node in &plan.nodes {
            repo.create_node(&conn, &plan.plan_id, node).unwrap();
        }
        // 模拟 n1 成功,n2 失败
        repo.update_node_status(
            &conn,
            "p-rate",
            "n1",
            &DagNodeStatus::Succeeded(serde_json::json!({"path": "C:/x.txt"})),
            Some("task-1"),
            Some("step-1"),
        )
        .unwrap();
        repo.update_node_status(
            &conn,
            "p-rate",
            "n2",
            &DagNodeStatus::Failed {
                cause: "permission denied".into(),
            },
            None,
            None,
        )
        .unwrap();
    }

    let result = list_dag_history(&state, 20, 0, DagStatusFilter::All).unwrap();
    let item = result.iter().find(|r| r.plan_id == "p-rate").unwrap();
    assert_eq!(item.node_count, 2, "node_count from plan_json");
    assert!(
        (item.success_rate - 0.5).abs() < 0.01,
        "1/2 = 0.5 success rate"
    );
}

#[test]
fn get_dag_plan_returns_full_detail() {
    let state = AppState::new_in_memory().unwrap();
    seed_plan(&state, "p-detail", &DagStatus::Succeeded);

    let detail = get_dag_plan(&state, "p-detail").unwrap().unwrap();
    assert_eq!(detail.plan_id, "p-detail");
    assert_eq!(detail.user_goal, "打开记事本写 TODO 然后保存到桌面");
    assert_eq!(detail.max_total_steps, 5);
    assert_eq!(detail.nodes.len(), 2);
    assert_eq!(detail.nodes[0].node_id, "n1");
    assert_eq!(detail.nodes[0].skill_id, "note.capture");
    assert_eq!(detail.nodes[1].node_id, "n2");
    assert_eq!(detail.edges.len(), 1);
    assert_eq!(detail.edges[0].from, "n1");
    assert_eq!(detail.edges[0].to, "n2");
}

#[test]
fn get_dag_plan_returns_none_for_unknown_id() {
    let state = AppState::new_in_memory().unwrap();
    let result = get_dag_plan(&state, "nonexistent").unwrap();
    assert!(result.is_none());
}

// ===== W8 Plan 5 Task 3: get_task_explanation =====

use trust_kernel::repo::step_repo::{StepRecord, StepRepo};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};
use trust_kernel::skills::explanation_repo::{
    FailureCategory, TaskExplanationRecord, TaskExplanationRepo,
};
use voicepilot_ui::dag_commands::get_task_explanation;

fn seed_step(state: &AppState, step_id: &str) {
    let conn = state.kernel.conn();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let task_id = format!("task-for-{}", step_id);
    let task = TaskRecord::new(task_id.clone(), "test goal");
    task_repo.create(&conn, &task).unwrap();
    let step = StepRecord::new(step_id, &task_id, 0);
    step_repo.create(&conn, &step).unwrap();
}

#[test]
fn get_task_explanation_returns_record_for_step() {
    let state = AppState::new_in_memory().unwrap();
    seed_step(&state, "step-explain-1");
    let repo = TaskExplanationRepo::new();
    // 用 block 限制 conn 守卫生命周期,避免下方 `get_task_explanation(&state, ...)`
    // 内部再次 `state.kernel.conn()` 时触发 Mutex 重入死锁。
    {
        let conn = state.kernel.conn();
        let rec = TaskExplanationRecord {
            explanation_id: "exp-001".into(),
            step_id: "step-explain-1".into(),
            root_cause_zh: "MCP 服务器未启动,Playwright 工具调用失败".into(),
            category: FailureCategory::McpUnavailable.as_str().into(),
            suggested_fix: Some("请在 Settings 中启动 Playwright MCP".into()),
            confidence: 0.85,
            llm_model: Some("gpt-4o-mini".into()),
            created_at: String::new(),
        };
        repo.create(&conn, &rec).unwrap();
    }

    let result = get_task_explanation(&state, "step-explain-1")
        .unwrap()
        .unwrap();
    assert_eq!(result.explanation_id, "exp-001");
    assert_eq!(result.step_id, "step-explain-1");
    assert_eq!(result.root_cause_zh, "MCP 服务器未启动,Playwright 工具调用失败");
    assert_eq!(result.category, "mcp_unavailable");
    assert!((result.confidence - 0.85).abs() < 0.001);
    assert_eq!(result.llm_model.as_deref(), Some("gpt-4o-mini"));
}

#[test]
fn get_task_explanation_returns_none_for_step_without_record() {
    let state = AppState::new_in_memory().unwrap();
    seed_step(&state, "step-no-explain");
    let result = get_task_explanation(&state, "step-no-explain").unwrap();
    assert!(
        result.is_none(),
        "step without explanation should return None"
    );
}

#[test]
fn get_task_explanation_returns_a_record_for_multiple_records() {
    // 注:`TaskExplanationRepo::create` 内部用 `now_iso()` 覆写 created_at,
    // 测试无法精确控制时间戳。此处仅验证至少能取到一条归因记录。
    let state = AppState::new_in_memory().unwrap();
    seed_step(&state, "step-multi");
    let repo = TaskExplanationRepo::new();
    // 用 block 限制 conn 守卫生命周期,避免下方 `get_task_explanation(&state, ...)`
    // 内部再次 `state.kernel.conn()` 时触发 Mutex 重入死锁。
    {
        let conn = state.kernel.conn();
        let rec1 = TaskExplanationRecord {
            explanation_id: "exp-old".into(),
            step_id: "step-multi".into(),
            root_cause_zh: "旧归因".into(),
            category: FailureCategory::Unknown.as_str().into(),
            suggested_fix: None,
            confidence: 0.3,
            llm_model: None,
            created_at: String::new(),
        };
        let rec2 = TaskExplanationRecord {
            explanation_id: "exp-new".into(),
            step_id: "step-multi".into(),
            root_cause_zh: "新归因".into(),
            category: FailureCategory::PathNotAllowed.as_str().into(),
            suggested_fix: Some("检查 allowed_paths".into()),
            confidence: 0.9,
            llm_model: Some("gpt-4o".into()),
            created_at: String::new(),
        };
        repo.create(&conn, &rec1).unwrap();
        // 微小延迟确保 created_at 不同(repo 内部 now_iso() 用 RFC3339 精确到秒,
        // 若两次 create 在同一秒内,排序不稳定 — 加 1.1s 延迟)
        std::thread::sleep(std::time::Duration::from_millis(1100));
        repo.create(&conn, &rec2).unwrap();
    }

    let result = get_task_explanation(&state, "step-multi")
        .unwrap()
        .unwrap();
    // 期望返回最新一条(exp-new,因 created_at DESC LIMIT 1)
    assert_eq!(result.step_id, "step-multi");
    assert_eq!(result.explanation_id, "exp-new");
    assert_eq!(result.root_cause_zh, "新归因");
}

#[test]
fn get_task_explanation_returns_none_for_unknown_step() {
    let state = AppState::new_in_memory().unwrap();
    let result = get_task_explanation(&state, "step-nonexistent").unwrap();
    assert!(result.is_none());
}

// ===== W8 Plan 5 Task 9: Tauri 命令端到端测试 =====
//
// 注:不在此处直接 `use voicepilot_ui::approver::TauriApprover`。
// `TauriApprover` 持有 `Option<AppHandle>` 字段,会拉入 Tauri runtime → wry → tao
// → user32/comctl32/gdi32 等 GUI DLL,其中 `TaskDialogIndirect` 需 comctl32 v6
// manifest。Rust 测试二进制无 manifest 默认加载 comctl32 v5,触发
// STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)。
// `TauriApprover::create_dag_approval_request_for_test` 的 unique-id 行为已由
// `approver_unit.rs::approval_registry_*` 覆盖(底层调 `ApprovalRegistry::create_request`),
// 本文件其余 4 个测试覆盖 `submit_dag_skeleton_approval` 的真实 IPC 风险。

use voicepilot_ui::dag_commands::{submit_dag_skeleton_approval, DagApprovalDecision};

#[test]
fn full_dag_approval_flow_delivers_allow_decision() {
    // 完整流程:创建 DAG 审批请求 → 提交决策 → 验证 oneshot 收到 Allow
    let state = AppState::new_in_memory().unwrap();
    let (approval_id, rx) = state.approval_registry.create_dag_request();

    // 模拟用户点击 Allow
    let delivered = submit_dag_skeleton_approval(
        &state,
        &approval_id,
        DagApprovalDecision::Allow,
        None,
    )
    .unwrap();
    assert!(delivered, "decision should be delivered");

    // 验证 oneshot 收到 Allow
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let payload = rt.block_on(async move {
        tokio::time::timeout(std::time::Duration::from_millis(100), rx)
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(payload.decision, ApprovalDecision::Allow);
    assert!(payload.modified_plan.is_none());
}

#[test]
fn full_dag_approval_flow_delivers_deny_decision() {
    let state = AppState::new_in_memory().unwrap();
    let (approval_id, rx) = state.approval_registry.create_dag_request();

    let delivered = submit_dag_skeleton_approval(
        &state,
        &approval_id,
        DagApprovalDecision::Deny,
        None,
    )
    .unwrap();
    assert!(delivered, "decision should be delivered");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let payload = rt.block_on(async move {
        tokio::time::timeout(std::time::Duration::from_millis(100), rx)
            .await
            .unwrap()
            .unwrap()
    });
    assert_eq!(payload.decision, ApprovalDecision::Deny);
    assert!(payload.modified_plan.is_none());
}

#[test]
fn submit_dag_skeleton_approval_returns_false_for_consumed_id() {
    // 一次性语义:同一 approval_request_id 第二次提交返回 false
    let state = AppState::new_in_memory().unwrap();
    let (approval_id, _rx) = state.approval_registry.create_dag_request();

    let first = submit_dag_skeleton_approval(
        &state,
        &approval_id,
        DagApprovalDecision::Allow,
        None,
    )
    .unwrap();
    assert!(first, "first submission should succeed");

    let second = submit_dag_skeleton_approval(
        &state,
        &approval_id,
        DagApprovalDecision::Deny,
        None,
    )
    .unwrap();
    assert!(
        !second,
        "second submission should fail (single-use semantic)"
    );
}

#[test]
fn submit_dag_skeleton_approval_returns_false_for_unknown_id() {
    let state = AppState::new_in_memory().unwrap();
    let result = submit_dag_skeleton_approval(
        &state,
        "apr_nonexistent",
        DagApprovalDecision::Allow,
        None,
    )
    .unwrap();
    assert!(!result, "unknown approval_id should return false");
}
