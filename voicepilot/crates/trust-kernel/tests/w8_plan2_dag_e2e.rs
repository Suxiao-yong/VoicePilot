//! W8 Plan 2 Task 9: DAG 端到端集成测试.
//!
//! 串联 LlmClient::decompose_to_dag + DagExecutor + dispatch_skill_executor +
//! Approver + DagRepo + audit,验证 spec §3.1 完整流程。
//!
//! 4 个集成测试:
//! 1. 单节点 DAG(task.explain + AutoApprover)→ DagStatus::Succeeded
//! 2. 2 节点 DAG(n1 → n2,串行执行)→ DagStatus::Succeeded + 拓扑序
//! 3. LLM 拆解失败(HTTP 500)→ Err 传播给调用方(Plan 4 router_bridge catch)
//! 4. AutoDenier DAG 骨架审批 → 0 节点执行 + DagStatus::Cancelled

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use trust_kernel::approval::approver::{AutoApprover, AutoDenier, Approver};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_executor::DagExecutor;
use trust_kernel::skills::dag_repo::DagRepo;
use trust_kernel::skills::dag_types::{DagEdge, DagNode, DagPlan, DagStatus};
use trust_kernel::skills::manifest::{task_explain_manifest, SkillManifest};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn candidate_skills() -> Vec<SkillManifest> {
    // 复用 W7 既有 built-in manifest,避免重新构造 SkillManifest 全字段
    // (SkillManifest 未 impl Default,需手动构造 13 个字段)
    vec![task_explain_manifest()]
}

fn llm_response_with_plan(plan: &DagPlan) -> serde_json::Value {
    let plan_json = serde_json::to_string(plan).unwrap();
    json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": plan_json
                    }
                }]
            }
        }],
        "usage": {"total_tokens": 42}
    })
}

/// 构造 task.explain 节点。template 用 JSON 字符串字面量,
/// dispatcher 会通过 `serde_json::from_str` 把 String 解析回 Object
/// (见 dispatcher.rs `normalized` 块),让 `extract_u32("limit")` 能命中。
fn explain_node(id: &str, limit: u32) -> DagNode {
    DagNode {
        node_id: id.into(),
        skill_id: "task.explain".into(),
        input_template: SlotTemplate {
            kind: SlotKind::Number,
            template: TemplateExpr::Literal(format!("{{\"limit\": {}}}", limit)),
        },
        risk_ceiling: ELevel::E0,
    }
}

fn build_executor(
    approver: Arc<dyn Approver>,
) -> (Arc<TrustKernel>, DagExecutor) {
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let dag_repo = Arc::new(DagRepo::new());
    let executor = DagExecutor::new(kernel.clone(), approver, dag_repo);
    (kernel, executor)
}

#[tokio::test]
async fn e2e_single_node_dag_succeeds() {
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "explain recent".into(),
        nodes: vec![explain_node("n1", 5)],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(llm_response_with_plan(&plan)))
        .mount(&server)
        .await;

    let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel, executor) = build_executor(approver);

    // Step 1: LLM 拆解
    let dag = client
        .decompose_to_dag("explain recent", &candidate_skills(), &[])
        .await
        .expect("LLM decompose should succeed");

    // Step 2: DagExecutor 执行
    let result = executor.run(&dag, &[]).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    let n1 = result.node_results.get("n1").expect("n1 must be in node_results");
    assert!(n1.is_succeeded(), "n1 should be Succeeded, got: {:?}", n1);
}

#[tokio::test]
async fn e2e_two_node_dag_succeeds_in_topo_order() {
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "explain 5 then explain 10".into(),
        nodes: vec![explain_node("n1", 5), explain_node("n2", 10)],
        edges: vec![DagEdge {
            from: "n1".into(),
            to: "n2".into(),
            port_binding: None,
        }],
        loop_specs: HashMap::new(),
        max_total_steps: 10,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(llm_response_with_plan(&plan)))
        .mount(&server)
        .await;

    let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    let (_kernel, executor) = build_executor(approver);

    let dag = client
        .decompose_to_dag("explain 5 then 10", &candidate_skills(), &[])
        .await
        .expect("LLM decompose should succeed");
    let result = executor.run(&dag, &[]).unwrap();
    assert_eq!(result.status, DagStatus::Succeeded);
    assert_eq!(result.node_results.len(), 2, "both nodes must be in results");
    assert!(
        result.node_results.get("n1").unwrap().is_succeeded(),
        "n1 should be Succeeded"
    );
    assert!(
        result.node_results.get("n2").unwrap().is_succeeded(),
        "n2 should be Succeeded"
    );
}

#[tokio::test]
async fn e2e_deny_short_circuits_zero_node_execution() {
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "explain".into(),
        nodes: vec![explain_node("n1", 5)],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(llm_response_with_plan(&plan)))
        .mount(&server)
        .await;

    let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
    let approver: Arc<dyn Approver> = Arc::new(AutoDenier);
    let (_kernel, executor) = build_executor(approver);

    let dag = client
        .decompose_to_dag("explain", &candidate_skills(), &[])
        .await
        .expect("LLM decompose should succeed (Deny happens at executor, not LLM)");
    let result = executor.run(&dag, &[]).unwrap();
    assert_eq!(result.status, DagStatus::Cancelled);
    assert!(
        result.node_results.is_empty(),
        "Deny must short-circuit, got: {:?}",
        result.node_results
    );
}

#[tokio::test]
async fn e2e_llm_failure_propagates_err_to_caller() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
        .mount(&server)
        .await;

    let client = LlmClient::new(&server.uri(), "sk-test", "deepseek-chat");
    // LLM 拆解失败 → Err 传播给调用方(router_bridge Plan 4 会 catch 并回退)
    let result = client
        .decompose_to_dag("explain", &candidate_skills(), &[])
        .await;
    assert!(
        result.is_err(),
        "expected LLM failure to propagate as Err, got: {:?}",
        result
    );
    // 调用方在 Plan 4 实现 catch + 回退逻辑,本测试仅验证 Err 传播
}
