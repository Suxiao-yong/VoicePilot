//! W8 Plan 6 — End-to-end DAG orchestration smoke tests.
//!
//! 8 个场景覆盖 W8 spec §7.3 功能门禁 + §7.4 安全门禁:
//!   1. LLM 拆解 → DAG [task.explain, task.explain] 成功执行
//!   2. form.submit E3 PerStep 审批被调用(approvals 表有 E3 记录)
//!   3. DAG 骨架审批 Deny → 0 节点执行 + 审计完整
//!   4. DAG 中某步 Failed → PartiallySucceeded + 前序已 commit 无回滚
//!   5. task.explain 调 LLM → 输出含 root_cause_zh + category
//!   6. 循环节点 break_condition 不触发(字符串 item 无数值字段)→ 全循环
//!   7. LLM 拆解返回非法 skill_id → DAG 被拒绝,回退单 Skill 路由
//!   8. 循环 max_iterations > 50 → 强制截断到 50
//!
//! 测试设计(适配实际 API,非 spec 假设的 MockSkillDispatcher):
//! - 场景 1/2/7 用 wiremock mock LLM HTTP,验证 decompose_to_dag + route_text_with_dag
//! - 场景 3/4/5/6/8 直接构造 DagPlan / 预置 step,聚焦 DagExecutor 调度逻辑
//! - 所有场景用真实 dispatch_skill_executor:`task.explain`(读 audit log,无副作用)
//!   用于成功路径;`nonexistent.skill` 用于失败路径;`form.submit` 用于 E3 审批验证
//! - 所有场景用 TrustKernel::open_in_memory() + AutoApprover / AutoDenier
//! - 断言:DagStatus + DagNodeStatus + audit_log 事件类型 + DB 表状态

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::sync::Arc;

use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::policy::types::ELevel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{
    DagEdge, DagNode, DagNodeStatus, DagPlan, DagStatus, IterableSource, LoopSpec,
};
use trust_kernel::skills::explanation_repo::TaskExplanationRepo;
#[cfg(feature = "voice")]
use trust_kernel::skills::task_explain::{execute_task_explain_with_llm, TaskExplainInput};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ===== 共享 helpers =====

/// 构造一个 literal SlotTemplate(用于直接构造 DagPlan)。
fn literal_text_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Text,
        template: TemplateExpr::Literal(value.to_string()),
    }
}

/// 构造一个 number kind 的 literal SlotTemplate。
fn literal_number_template(value: &str) -> SlotTemplate {
    SlotTemplate {
        kind: SlotKind::Number,
        template: TemplateExpr::Literal(value.to_string()),
    }
}

/// 构造 task.explain 节点。template 用 JSON 字符串字面量,
/// dispatcher 会通过 `serde_json::from_str` 把 String 解析回 Object,
/// 让 `extract_u32("limit")` 能命中。
fn explain_node(id: &str, limit: u32) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "task.explain".into(),
        input_template: literal_number_template(&format!("{{\"limit\": {}}}", limit)),
        risk_ceiling: ELevel::E0,
    }
}

/// 构造 wiremock mock 返回的 OpenAI-compatible decompose_to_dag 响应 body。
fn llm_decompose_response_body(plan_json: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": plan_json,
                    }
                }]
            }
        }],
        "usage": {"total_tokens": 42}
    })
}

/// 构造 wiremock mock 返回的 OpenAI-compatible explain_failure 响应 body。
fn llm_explain_response_body(root_cause_zh: &str, category: &str, confidence: f32) -> serde_json::Value {
    let arguments = serde_json::json!({
        "root_cause_zh": root_cause_zh,
        "category": category,
        "suggested_fix": null,
        "confidence": confidence,
    });
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_explain_1",
                    "type": "function",
                    "function": {
                        "name": "explain_failure",
                        "arguments": arguments.to_string(),
                    }
                }]
            }
        }]
    })
}

/// Mount a wiremock returning `body` for POST /chat/completions.
async fn mount_chat_completions(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// 统计 audit_logs 中某 event_type 的数量。
fn count_audit_events(kernel: &TrustKernel, event_type: &str) -> i64 {
    let conn = kernel.conn();
    conn.query_row(
        "SELECT COUNT(*) FROM audit_logs WHERE event_type = ?1",
        rusqlite::params![event_type],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// 列出 audit_logs 中某 event_type 的所有 details(JSON 字符串)。
fn list_audit_details(kernel: &TrustKernel, event_type: &str) -> Vec<String> {
    let conn = kernel.conn();
    let mut stmt = conn
        .prepare("SELECT details FROM audit_logs WHERE event_type = ?1 ORDER BY timestamp ASC")
        .unwrap();
    stmt.query_map(rusqlite::params![event_type], |row| row.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

// ===== 场景 1: LLM 拆解 → DAG [task.explain, task.explain] 成功执行 =====
//
// 适配说明:spec 假设用 [note.capture, files.move],但这两个 Skill 分别需要
// UIA / 文件系统。本测试用 [task.explain, task.explain](读 audit log,无副作用)
// 验证 LLM 拆解 + DagExecutor 串联 + 2 节点拓扑序执行。
#[cfg(feature = "voice")]
#[tokio::test]
async fn scenario_1_llm_decomposes_two_node_dag_succeeds() {
    // ===== Setup: 构造 2 节点 DagPlan(task.explain × 2)并序列化为 LLM 响应 =====
    let plan = DagPlan {
        plan_id: format!("plan-1-{}", uuid::Uuid::new_v4()),
        user_goal: "explain recent then explain more".into(),
        nodes: vec![explain_node("n1", 5), explain_node("n2", 10)],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    };
    let plan_json = serde_json::to_string(&plan).unwrap();
    let body = llm_decompose_response_body(&plan_json);

    let server = MockServer::start().await;
    mount_chat_completions(&server, body).await;

    // ===== Setup: kernel + LlmClient(注入 mock URL) =====
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    // ===== Act: route_text_with_dag → DagPlan =====
    let outcome = trust_kernel::voice::router_bridge::route_text_with_dag(
        &kernel,
        "请帮我处理这个多步任务",
    )
    .await
    .unwrap();
    let dag_plan = match outcome {
        trust_kernel::voice::router_bridge::RouteOutcome::DagPlan(p) => p,
        other => panic!("expected DagPlan, got {:?}", other),
    };
    assert_eq!(dag_plan.nodes.len(), 2);
    assert_eq!(dag_plan.nodes[0].skill_id, "task.explain");
    assert_eq!(dag_plan.nodes[1].skill_id, "task.explain");

    // ===== Act: DagExecutor::run =====
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let kernel_arc = Arc::new(kernel);
    let executor = DagExecutor::new(kernel_arc.clone(), approver, dag_repo);
    let result = executor.run(&dag_plan, &[]).expect("DagExecutor::run must succeed");

    // ===== Assert: DagStatus::Succeeded + 2 节点都 Succeeded =====
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected Succeeded, got {:?}",
        result.status
    );
    let n1_status = result.node_results.get("n1").expect("n1 must have status");
    let n2_status = result.node_results.get("n2").expect("n2 must have status");
    assert!(
        n1_status.is_succeeded(),
        "n1 must be Succeeded, got {:?}",
        n1_status
    );
    assert!(
        n2_status.is_succeeded(),
        "n2 must be Succeeded, got {:?}",
        n2_status
    );

    // ===== Assert: audit_log 事件链完整 =====
    assert!(
        count_audit_events(&kernel_arc, "dag_plan_created") >= 1,
        "dag_plan_created must be logged"
    );
    assert!(
        count_audit_events(&kernel_arc, "dag_node_started") >= 2,
        "2 dag_node_started events expected"
    );
    assert!(
        count_audit_events(&kernel_arc, "dag_node_succeeded") >= 2,
        "2 dag_node_succeeded events expected"
    );
    assert!(
        count_audit_events(&kernel_arc, "dag_completed") >= 1,
        "dag_completed must be logged"
    );

    // ===== Assert: dag_plans 表 status=succeeded + dag_nodes 表 2 行 =====
    let dag_repo = DagRepo::new();
    let persisted_plan = dag_repo
        .get_plan(&kernel_arc.conn(), &dag_plan.plan_id)
        .unwrap()
        .expect("plan must be persisted");
    assert_eq!(persisted_plan.status, "succeeded");
    let persisted_nodes = dag_repo
        .list_nodes_by_plan(&kernel_arc.conn(), &dag_plan.plan_id)
        .unwrap();
    assert_eq!(persisted_nodes.len(), 2);
}

// ===== 场景 2: form.submit E3 PerStep 审批被调用 =====
//
// 适配说明:spec 假设 [form.prepare, form.submit] 全成功,但 form.prepare / form.submit
// 都需要 Playwright MCP(测试环境不可用)。本测试直接构造 form.submit 单节点 DAG,
// 验证 E3 PerStep 审批在 MCP 调用前已记录到 approvals 表(form.submit 失败于 MCP,
// 但审批记录已落盘)— 这正是 spec §7.4 "form.submit 必须 PerStep 审批(E3)" 的核心。
#[tokio::test]
async fn scenario_2_form_submit_e3_perstep_approval_recorded() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    // form.submit input_template:JSON 字符串 {"url": "https://example.com"}
    // dispatcher 会 normalize 为 Object,extract_string("url") 命中。
    let dag_plan = DagPlan {
        plan_id: format!("plan-2-{}", uuid::Uuid::new_v4()),
        user_goal: "test form.submit E3 approval".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "form.submit".into(),
            input_template: literal_text_template(r#"{"url": "https://example.com"}"#),
            risk_ceiling: ELevel::E3,
        }],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };

    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    // Act: DagExecutor::run — form.submit 会在 MCP 调用处失败,但 E3 审批已记录
    let result = executor.run(&dag_plan, &[]).expect("run must not infra-error");

    // ===== Assert: form.submit 节点 Failed(MCP 不可用)→ DAG Failed =====
    // 注:单节点失败 + 无已成功节点 → DagStatus::Failed
    match &result.status {
        DagStatus::Failed { failed_node, .. } => {
            assert_eq!(failed_node, "n1");
        }
        other => panic!(
            "expected Failed (MCP unavailable), got {:?}",
            other
        ),
    }
    let n1_status = result.node_results.get("n1").expect("n1 must have status");
    assert!(
        matches!(n1_status, DagNodeStatus::Failed { .. }),
        "n1 must be Failed (MCP unavailable), got {:?}",
        n1_status
    );

    // ===== Assert: E3 PerStep 审批已记录到 approvals 表 =====
    // form.submit executor 在 invoke_mcp_tool 之前调 record_approval_decision(E3),
    // 即使 MCP 失败,审批记录已落盘。
    let e3_approval_count: i64 = {
        let conn = kernel.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM approvals WHERE e_level = 'E3'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    };
    assert!(
        e3_approval_count >= 1,
        "form.submit E3 PerStep approval must be recorded before MCP failure, got {}",
        e3_approval_count
    );

    // ===== Assert: audit_log 含 dag_node_started(n1) + dag_node_failed(n1) =====
    let n1_started_details = list_audit_details(&kernel, "dag_node_started");
    assert!(
        n1_started_details
            .iter()
            .any(|d| d.contains("\"node_id\":\"n1\"")),
        "dag_node_started for n1 must be logged, got: {:?}",
        n1_started_details
    );
    let n1_failed_details = list_audit_details(&kernel, "dag_node_failed");
    assert!(
        n1_failed_details
            .iter()
            .any(|d| d.contains("\"node_id\":\"n1\"")),
        "dag_node_failed for n1 must be logged, got: {:?}",
        n1_failed_details
    );
}

// ===== 场景 3: DAG 骨架审批 Deny → 0 节点执行 + 审计完整 =====
//
// 适配说明:用 AutoDenier 让 approve_dag_skeleton 返回 Deny,
// 验证 DagStatus::Cancelled + 0 节点执行 + dag_plans.status=cancelled
// + audit_log 含 dag_skeleton_approved(Deny) + dag_completed(cancelled) + 0 个 dag_node_started。
#[tokio::test]
async fn scenario_3_dag_skeleton_deny_cancels_execution_zero_nodes_run() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    // 构造 2 节点 DAG(绕过 LLM,聚焦审批门禁)
    let dag_plan = DagPlan {
        plan_id: format!("plan-deny-{}", uuid::Uuid::new_v4()),
        user_goal: "测试 Deny 路径".into(),
        nodes: vec![
            DagNode {
                node_id: "n1".into(),
                skill_id: "task.explain".into(),
                input_template: literal_number_template(r#"{"limit": 5}"#),
                risk_ceiling: ELevel::E0,
            },
            DagNode {
                node_id: "n2".into(),
                skill_id: "task.explain".into(),
                input_template: literal_number_template(r#"{"limit": 10}"#),
                risk_ceiling: ELevel::E0,
            },
        ],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    };

    // AutoDenier → approve_dag_skeleton 返回 Deny
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    // Act: DagExecutor::run — Deny 应让 0 节点执行
    let result = executor.run(&dag_plan, &[]).expect("DagExecutor::run must succeed even on Deny");

    // ===== Assert: DagStatus::Cancelled =====
    assert!(
        matches!(result.status, DagStatus::Cancelled),
        "expected Cancelled, got {:?}",
        result.status
    );

    // ===== Assert: 0 节点执行 =====
    assert!(
        result.node_results.is_empty(),
        "no nodes should have run, got {:?}",
        result.node_results
    );

    // ===== Assert: dag_plans 表 status=cancelled =====
    let dag_repo = DagRepo::new();
    let persisted_plan = dag_repo
        .get_plan(&kernel.conn(), &dag_plan.plan_id)
        .unwrap()
        .expect("plan must be persisted");
    assert_eq!(persisted_plan.status, "cancelled");

    // ===== Assert: dag_nodes 表 0 行(骨架 Deny 时不创建 node 行) =====
    let persisted_nodes = dag_repo
        .list_nodes_by_plan(&kernel.conn(), &dag_plan.plan_id)
        .unwrap();
    assert!(
        persisted_nodes.is_empty(),
        "dag_nodes table must have 0 rows on skeleton Deny, got {} rows",
        persisted_nodes.len()
    );

    // ===== Assert: audit_log 含 dag_skeleton_approved Deny + dag_completed(cancelled) =====
    // W9 Plan 4:`decision` 字段从 `format!("{:?}", decision)`(W8 "Deny")
    // 改为 `outcome.as_str()`(W9 "deny"),闭合 W8 spec §11 format→字符串。
    let skeleton_details = list_audit_details(&kernel, "dag_skeleton_approved");
    assert!(
        skeleton_details
            .iter()
            .any(|d| d.contains("\"decision\":\"deny\"")),
        "dag_skeleton_approved deny must be logged, got: {:?}",
        skeleton_details
    );
    let completed_details = list_audit_details(&kernel, "dag_completed");
    assert!(
        completed_details
            .iter()
            .any(|d| d.contains("\"final_status\":\"cancelled\"")),
        "dag_completed with final_status=cancelled must be logged, got: {:?}",
        completed_details
    );
    // 0 节点执行 → 0 个 dag_node_started
    assert_eq!(
        count_audit_events(&kernel, "dag_node_started"),
        0,
        "no dag_node_started on Deny"
    );
}

// ===== 场景 4: DAG 中某步 Failed → PartiallySucceeded + 前序已 commit 无回滚 =====
//
// 适配说明:n1=task.explain(succeeds,读 audit log),n2=nonexistent.skill(fails)。
// 验证 DagStatus::PartiallySucceeded + n1 Succeeded + n2 Failed
// + n1 的 task/step 在 DB 中仍为 Succeeded(未回滚)。
#[tokio::test]
async fn scenario_4_node_failed_yields_partially_succeeded_no_rollback_of_committed() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    let dag_plan = DagPlan {
        plan_id: format!("plan-partial-{}", uuid::Uuid::new_v4()),
        user_goal: "测试 PartiallySucceeded".into(),
        nodes: vec![
            DagNode {
                node_id: "n1".into(),
                skill_id: "task.explain".into(),
                input_template: literal_number_template(r#"{"limit": 5}"#),
                risk_ceiling: ELevel::E0,
            },
            DagNode {
                node_id: "n2".into(),
                skill_id: "nonexistent.skill".into(), // 必失败 → DagNodeStatus::Failed
                input_template: literal_text_template(r#"{"x": 1}"#),
                risk_ceiling: ELevel::E0,
            },
        ],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    };

    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    // Act
    let result = executor
        .run(&dag_plan, &[])
        .expect("DagExecutor::run must succeed even with node failure");

    // ===== Assert: DagStatus::PartiallySucceeded =====
    match &result.status {
        DagStatus::PartiallySucceeded { succeeded, failed_node, .. } => {
            assert_eq!(succeeded, &vec!["n1".to_string()], "n1 must be in succeeded list");
            assert_eq!(failed_node, "n2");
        }
        other => panic!("expected PartiallySucceeded, got {:?}", other),
    }

    // ===== Assert: n1 Succeeded,n2 Failed =====
    let n1_status = result.node_results.get("n1").expect("n1 must have status");
    let n2_status = result.node_results.get("n2").expect("n2 must have status");
    assert!(n1_status.is_succeeded(), "n1 must be Succeeded, got {:?}", n1_status);
    assert!(
        n2_status.is_failed(),
        "n2 must be Failed, got {:?}",
        n2_status
    );

    // ===== Assert: n1 的 dag_nodes 行 task_id/step_id 已落盘(committed,未回滚) =====
    let dag_repo = DagRepo::new();
    let persisted_nodes = dag_repo
        .list_nodes_by_plan(&kernel.conn(), &dag_plan.plan_id)
        .unwrap();
    let n1_row = persisted_nodes
        .iter()
        .find(|n| n.node_id == "n1")
        .expect("n1 row must exist");
    assert_eq!(n1_row.status, "succeeded");
    assert!(
        n1_row.task_id.is_some(),
        "n1 task_id must be set (committed, not rolled back)"
    );
    assert!(
        n1_row.step_id.is_some(),
        "n1 step_id must be set (committed, not rolled back)"
    );

    // 验证 n1 的 step 在 steps 表中状态为 Succeeded(未回滚)
    let n1_step_id = n1_row.step_id.as_ref().unwrap();
    let n1_step_status: String = {
        let conn = kernel.conn();
        conn.query_row(
            "SELECT status FROM steps WHERE step_id = ?1",
            rusqlite::params![n1_step_id],
            |row| row.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        n1_step_status, "SUCCEEDED",
        "n1 step must remain SUCCEEDED (no rollback)"
    );

    // ===== Assert: dag_plans 表 status=partially_succeeded =====
    let persisted_plan = dag_repo
        .get_plan(&kernel.conn(), &dag_plan.plan_id)
        .unwrap()
        .expect("plan must be persisted");
    assert_eq!(persisted_plan.status, "partially_succeeded");

    // ===== Assert: audit_log 含 dag_node_failed(n2) =====
    let failed_details = list_audit_details(&kernel, "dag_node_failed");
    assert!(
        failed_details
            .iter()
            .any(|d| d.contains("\"node_id\":\"n2\"")),
        "dag_node_failed for n2 must be logged, got: {:?}",
        failed_details
    );
    let completed_details = list_audit_details(&kernel, "dag_completed");
    assert!(
        completed_details
            .iter()
            .any(|d| d.contains("\"final_status\":\"partially_succeeded\"")),
        "dag_completed with final_status=partially_succeeded must be logged, got: {:?}",
        completed_details
    );
}

// ===== 场景 5: task.explain 调 LLM → 输出含 root_cause_zh + category =====
//
// 适配说明:直接调用 execute_task_explain_with_llm,验证 LLM 归因结果
// + task_explanations 表持久化 + llm_explain_called 审计事件。
// 注:category 字符串使用 snake_case("path_not_allowed"),非 PascalCase。
// 注:本测试调用 execute_task_explain_with_llm / TaskExplainInput,二者均
// `#[cfg(feature = "voice")]` 门控(task_explain 模块只在 voice feature 下编译
// 这些 API)。因此 scenario_5 必须 voice-gated,否则 `cargo test --features llm`
// 单独跑会编译失败。
#[cfg(feature = "voice")]
#[tokio::test]
async fn scenario_5_task_explain_calls_llm_yields_root_cause_zh_and_category() {
    // ===== Setup: kernel + 预置一个 Failed step + audit_logs =====
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = format!("task-explain-{}", uuid::Uuid::new_v4());
    let step_id = format!("step-explain-{}", uuid::Uuid::new_v4());
    kernel.create_task(&task_id, "test task for explain").unwrap();
    kernel
        .create_step(&StepRecord::new(&step_id, &task_id, 1))
        .unwrap();
    // 标记 step 为 Failed(模拟失败,触发 LLM 归因)
    kernel.update_step_status(&step_id, StepStatus::Failed).unwrap();
    // 写几条 audit_logs 给 LLM 归因用
    kernel
        .audit_append_external(&task_id, Some(&step_id), "STEP_PREPARED", serde_json::json!({}))
        .unwrap();
    kernel
        .audit_append_external(
            &task_id,
            Some(&step_id),
            "STEP_FAILED",
            serde_json::json!({"error": "path not allowed"}),
        )
        .unwrap();

    // ===== Setup: wiremock LLM 返回 explain_failure 归因 =====
    let server = MockServer::start().await;
    let body = llm_explain_response_body(
        "目标路径不在 allowed_paths 白名单内,FilesystemTool 拒绝写入。",
        "path_not_allowed", // snake_case,匹配 FailureCategory::parse_str
        0.92,
    );
    mount_chat_completions(&server, body).await;

    // ===== Act: 调 task.explain executor(注入 LLM) =====
    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let explain_input = TaskExplainInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        limit: 10,
    };
    let explanation = execute_task_explain_with_llm(
        &kernel,
        &explain_input,
        approver.as_ref(),
        Some(llm.as_ref()),
    )
    .await
    .expect("task.explain must succeed");

    // ===== Assert: llm_analysis 含 root_cause_zh + category + confidence =====
    let analysis = explanation
        .llm_analysis
        .as_ref()
        .expect("llm_analysis must be Some when LLM succeeds");
    assert!(
        analysis.root_cause_zh.contains("allowed_paths"),
        "root_cause_zh must mention allowed_paths, got: {}",
        analysis.root_cause_zh
    );
    assert_eq!(
        analysis.category,
        trust_kernel::skills::explanation_repo::FailureCategory::PathNotAllowed
    );
    assert!(
        (analysis.confidence - 0.92_f32).abs() < 1e-6,
        "confidence must be 0.92, got {}",
        analysis.confidence
    );

    // ===== Assert: task_explanations 表有记录,category=path_not_allowed(snake_case) =====
    let explanation_repo = TaskExplanationRepo::new();
    let persisted = explanation_repo
        .get_by_step_id(&kernel.conn(), &step_id)
        .unwrap()
        .expect("task_explanations row must be persisted");
    assert_eq!(persisted.step_id, step_id);
    assert_eq!(persisted.category, "path_not_allowed");
    assert!((persisted.confidence - 0.92).abs() < 1e-6);

    // ===== Assert: audit_log 含 llm_explain_called + category=path_not_allowed + token_count 字段 =====
    let explain_events = count_audit_events(&kernel, "llm_explain_called");
    assert!(explain_events >= 1, "llm_explain_called must be logged");
    let explain_details = list_audit_details(&kernel, "llm_explain_called");
    assert!(
        explain_details
            .iter()
            .any(|d| d.contains("\"category\":\"path_not_allowed\"")),
        "llm_explain_called must contain category=path_not_allowed, got: {:?}",
        explain_details
    );
    // spec §7.5 LLM 成本门禁:token_count 字段必须存在
    assert!(
        explain_details.iter().any(|d| d.contains("\"token_count\":")),
        "llm_explain_called must contain token_count field, got: {:?}",
        explain_details
    );
}

// ===== 场景 6: 循环节点 break_condition 不触发(字符串 item 无数值字段)→ 全循环 =====
//
// 适配说明:IterableSource::Literal 只支持 Vec<String>,字符串 item 无数值字段,
// break_condition "item.size > 1000" 永远返回 false → 循环完整执行 3 次。
// 验证:循环 Succeeded(Array len=3) + dag_node_succeeded 含 n1。
// break_condition 触发场景已由 dag_executor.rs 单元测试
// (evaluate_break_condition_object_item_matches)覆盖。
#[tokio::test]
async fn scenario_6_loop_break_condition_not_triggered_for_string_items_completes_full() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    // 循环节点:input_template 引用 ${item},break_condition 永远不触发
    let dag_plan = DagPlan {
        plan_id: format!("plan-loop-{}", uuid::Uuid::new_v4()),
        user_goal: "测试循环 break 不触发".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(), // 真实 skill,读 audit log 无副作用
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal(r#"{"limit": 5}"#.to_string()),
            },
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    loop_var: "item".to_string(),
                    iterable_source: IterableSource::Literal(vec![
                        "C:/file1.txt".to_string(),
                        "C:/file2.txt".to_string(),
                        "C:/file3.txt".to_string(),
                    ]),
                    max_iterations: 10,
                    // 字符串 item 无 .size 字段 → break_condition 永远 false
                    break_condition: Some("item.size > 1000".to_string()),
                },
            );
            m
        },
        max_total_steps: 20,
    };

    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    // Act
    let result = executor.run(&dag_plan, &[]).expect("DagExecutor::run must succeed");

    // ===== Assert: DagStatus::Succeeded =====
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected Succeeded, got {:?}",
        result.status
    );

    // ===== Assert: 循环节点 Succeeded(Array len=3) =====
    let n1_status = result.node_results.get("n1").expect("n1 must have status");
    match n1_status {
        DagNodeStatus::Succeeded(value) => {
            let arr = value.as_array().expect("loop node output must be array");
            assert_eq!(
                arr.len(),
                3,
                "loop must execute exactly 3 iterations (no break triggered), got {}",
                arr.len()
            );
        }
        other => panic!("expected Succeeded(Array), got {:?}", other),
    }

    // ===== Assert: dag_node_succeeded 含 n1 =====
    let n1_succeeded_details = list_audit_details(&kernel, "dag_node_succeeded");
    assert!(
        n1_succeeded_details
            .iter()
            .any(|d| d.contains("\"node_id\":\"n1\"")),
        "dag_node_succeeded for n1 must be logged, got: {:?}",
        n1_succeeded_details
    );

    // ===== Assert: dag_completed final_status=succeeded =====
    let completed_details = list_audit_details(&kernel, "dag_completed");
    assert!(
        completed_details
            .iter()
            .any(|d| d.contains("\"final_status\":\"succeeded\"")),
        "dag_completed with final_status=succeeded must be logged, got: {:?}",
        completed_details
    );
}

// ===== 场景 7: LLM 拆解返回非法 skill_id → DAG 被拒绝,回退单 Skill 路由 =====
//
// 适配说明:wiremock 返回含 `unknown.skill` 的 DAG。LlmClient::decompose_to_dag_traced
// 在校验阶段(校验 2:所有 skill_id 在 candidate_skills 中)拒绝该 plan,返回 Err。
// router_bridge::route_text_with_dag catch 该 Err,回退到第 3 级 route_with_llm。
// route_with_llm 调同一 mock → classify_and_extract 期望 `route_skill` tool_call,
// 实际拿到 `decompose_to_dag` → 解析失败 → 关键词不命中 → Planner → Unmatched。
//
// 关键断言:
// - RouteOutcome 不是 DagPlan(DAG 被拒绝)
// - audit_log 无 dag_plan_created(DagExecutor 未执行)
// - audit_log 无 llm_decompose_called(LLM 调用未成功,不记审计事件)
#[cfg(feature = "voice")]
#[tokio::test]
async fn scenario_7_llm_returns_invalid_skill_id_falls_back_to_single_skill_routing() {
    // ===== Setup: wiremock LLM 返回含 unknown.skill 的 DAG =====
    // 用 DagPlan 序列化作为 LLM 响应,skill_id = "unknown.skill" 不在 registered skills 中。
    let invalid_plan = DagPlan {
        plan_id: format!("plan-invalid-{}", uuid::Uuid::new_v4()),
        user_goal: "测试非法 skill_id".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "unknown.skill".into(), // 非法 skill_id
            input_template: literal_text_template(r#"{"x": 1}"#),
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    let plan_json = serde_json::to_string(&invalid_plan).unwrap();
    let body = llm_decompose_response_body(&plan_json);
    let server = MockServer::start().await;
    mount_chat_completions(&server, body).await;

    // ===== Setup: kernel + LlmClient(注入 mock URL) =====
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    // ===== Act: route_text_with_dag =====
    let outcome = trust_kernel::voice::router_bridge::route_text_with_dag(
        &kernel,
        "做一些不认识的事情",
    )
    .await
    .unwrap();

    // ===== Assert: RouteOutcome 不是 DagPlan(DAG 被拒绝) =====
    // 期望路径:LLM 校验失败 → 回退 route_with_llm → LLM classify_and_extract 解析失败
    // → 关键词不命中 → Planner → Unmatched
    match &outcome {
        trust_kernel::voice::router_bridge::RouteOutcome::Unmatched { .. } => {
            // 期望路径
        }
        trust_kernel::voice::router_bridge::RouteOutcome::Routed { .. } => {
            // 关键词命中场景也合法(spec §2.8:回退后若关键词命中则 Routed)
        }
        trust_kernel::voice::router_bridge::RouteOutcome::DagPlan(p) => {
            panic!(
                "DagPlan must be rejected when LLM returns invalid skill_id, got: {:?}",
                p
            );
        }
        trust_kernel::voice::router_bridge::RouteOutcome::Empty => {
            panic!("Empty outcome not expected for non-empty input");
        }
    }

    // ===== Assert: audit_log 无 dag_plan_created(DagExecutor 未执行) =====
    assert_eq!(
        count_audit_events(&kernel, "dag_plan_created"),
        0,
        "dag_plan_created must NOT be logged when validation rejects the DAG"
    );

    // ===== Assert: audit_log 无 llm_decompose_called =====
    // 注:router_bridge::route_text_with_dag 只在 LLM 成功路径调用
    // record_llm_decompose_called。decompose_to_dag_traced 校验失败时返回 Err,
    // 不记录审计事件(spec §6.1 + router_bridge.rs:216-228 注释)。
    assert_eq!(
        count_audit_events(&kernel, "llm_decompose_called"),
        0,
        "llm_decompose_called must NOT be logged when LLM validation fails (decompose_to_dag_traced returned Err)"
    );
}

// ===== 场景 8: 循环 max_iterations > 50 → 强制截断到 50 =====
//
// 适配说明:DagExecutor::run_loop_node 用 `max_iterations.min(50)` 强制截断,
// 取前 50 个 item 执行。DagExecutor::run 不调 validate_loop_specs,所以
// max_iterations=60 不会触发校验错误(校验只在 LLM 路径的 decompose_to_dag_traced 中)。
//
// 关键断言:
// - DagStatus::Succeeded
// - 循环节点 Succeeded(Array len=50)
// - dag_completed final_status=succeeded
// - 无 dag_node_failed(截断不是失败)
#[tokio::test]
async fn scenario_8_loop_max_iterations_above_50_clamped_to_50() {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());

    // 100 个 item,但 max_iterations=60 → 强制截断到 50
    let hundred_items: Vec<String> = (1..=100).map(|i| format!("C:/file{}.txt", i)).collect();
    let dag_plan = DagPlan {
        plan_id: format!("plan-clamp-{}", uuid::Uuid::new_v4()),
        user_goal: "测试循环截断".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(), // 真实 skill,读 audit log 无副作用
            input_template: literal_number_template(r#"{"limit": 5}"#),
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: {
            let mut m = HashMap::new();
            m.insert(
                "n1".to_string(),
                LoopSpec {
                    loop_var: "item".to_string(),
                    iterable_source: IterableSource::Literal(hundred_items),
                    max_iterations: 60, // 超过 50,应被截断到 50
                    break_condition: None,
                },
            );
            m
        },
        max_total_steps: 20,
    };

    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);

    // Act
    let result = executor.run(&dag_plan, &[]).expect("DagExecutor::run must succeed");

    // ===== Assert: DagStatus::Succeeded =====
    assert!(
        matches!(result.status, DagStatus::Succeeded),
        "expected Succeeded, got {:?}",
        result.status
    );

    // ===== Assert: 循环节点 Succeeded(Array len=50) =====
    let n1_status = result.node_results.get("n1").expect("n1 must have status");
    match n1_status {
        DagNodeStatus::Succeeded(value) => {
            let arr = value.as_array().expect("loop output must be array");
            assert_eq!(
                arr.len(),
                50,
                "loop must execute exactly 50 iterations (clamped from 60), got {}",
                arr.len()
            );
        }
        other => panic!("expected Succeeded(Array len=50), got {:?}", other),
    }

    // ===== Assert: audit_log 无 dag_node_failed(截断不是失败) =====
    assert_eq!(
        count_audit_events(&kernel, "dag_node_failed"),
        0,
        "no dag_node_failed events expected (clamp is not a failure)"
    );

    // ===== Assert: dag_completed final_status=succeeded =====
    assert!(
        count_audit_events(&kernel, "dag_completed") >= 1,
        "dag_completed must be logged"
    );
    let completed_details = list_audit_details(&kernel, "dag_completed");
    assert!(
        completed_details
            .iter()
            .any(|d| d.contains("\"final_status\":\"succeeded\"")),
        "dag_completed final_status must be succeeded, got: {:?}",
        completed_details
    );
}
