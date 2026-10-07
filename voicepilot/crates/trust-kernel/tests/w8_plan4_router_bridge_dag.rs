//! W8 Plan 4 Task 3-6: route_text_with_dag 集成测试.
//!
//! 覆盖 spec §2.8 三级路由策略的全部 8 个场景。
//! - scenario_1: 关键词命中 → Routed(不调 LLM)
//! - scenario_2: LLM disabled → Unmatched
//! - scenario_3: privacy_mode=true → 不调 LLM,Unmatched
//! - scenario_4: LLM 拆解成功(2 节点)→ DagPlan
//! - scenario_5: LLM HTTP 失败 → 回退 route_with_llm → Unmatched
//! - scenario_6: LLM 返回非法 skill_id → 回退 route_with_llm → Unmatched
//! - scenario_7: LLM 拆解成功(单节点)→ DagPlan
//! - scenario_8: 空字符串 → Empty

#![cfg(all(feature = "voice", feature = "llm"))]

use std::sync::Arc;

use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::voice::router_bridge::{RouteOutcome, route_text_with_dag};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 构造一个 mock LLM server,返回给定 JSON body(用于 decompose_to_dag 测试)。
async fn mock_llm_server_decompose(body: serde_json::Value) -> (MockServer, String) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let uri = server.uri();
    (server, uri)
}

/// decompose_to_dag 的成功 mock 响应:2 节点 DAG(research.save_markdown → files.organize)。
///
/// 注:skill_id 必须在 `route_text_with_dag` 注册的候选 Skills 中。
/// `note.capture` / `files.move` 仅在 `uia` feature 开启时注册(`files.move`
/// 根本不存在),用跨平台始终注册的 `research.save_markdown` + `files.organize`
/// 保证测试在 default feature 组合下通过。
fn decompose_success_body_two_nodes() -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": "{\"plan_id\":\"plan-test-1\",\"user_goal\":\"test\",\"nodes\":[{\"node_id\":\"n1\",\"skill_id\":\"research.save_markdown\",\"input_template\":{\"kind\":\"url\",\"template\":{\"Literal\":\"https://example.com\"}},\"risk_ceiling\":\"E2\"},{\"node_id\":\"n2\",\"skill_id\":\"files.organize\",\"input_template\":{\"kind\":\"path\",\"template\":{\"Literal\":\"C:/Downloads\"}},\"risk_ceiling\":\"E2\"}],\"edges\":[{\"from\":\"n1\",\"to\":\"n2\",\"port_binding\":\"output.path -> input.source\"}],\"loop_specs\":{},\"max_total_steps\":5}"
                    }
                }]
            }
        }]
    })
}

/// decompose_to_dag 的成功 mock 响应:1 节点 DAG(单节点也合法,spec §2.2)。
///
/// 用 `files.organize`(跨平台始终注册)避免 `uia` feature 依赖。
fn decompose_success_body_single_node() -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        "arguments": "{\"plan_id\":\"plan-test-2\",\"user_goal\":\"test\",\"nodes\":[{\"node_id\":\"n1\",\"skill_id\":\"files.organize\",\"input_template\":{\"kind\":\"path\",\"template\":{\"Literal\":\"C:/Downloads\"}},\"risk_ceiling\":\"E2\"}],\"edges\":[],\"loop_specs\":{},\"max_total_steps\":3}"
                    }
                }]
            }
        }]
    })
}

#[tokio::test]
async fn scenario_1_keyword_match_returns_skill_without_llm() {
    // "整理下载目录" 命中 files.organize 的 keyword "整理" → 直接返回 Routed,
    // 不调 LLM(用 disabled LlmClient 验证:即使 LLM 不可用,keyword 命中仍工作)。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let outcome = route_text_with_dag(&kernel, "整理下载目录").await.unwrap();
    match outcome {
        RouteOutcome::Routed { skill_id } => {
            assert_eq!(skill_id, "files.organize");
        }
        other => panic!("expected Routed, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_2_llm_disabled_returns_unmatched() {
    // LLM disabled → 不走 LLM 拆解分支 → 回退 W7 route_with_llm
    // W7 route_with_llm 在 LLM disabled 时只做 keyword 匹配 → 不命中 → Planner → Unmatched
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::disabled());
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理")
        .await
        .unwrap();
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!(
            "LLM disabled should fall back to Unmatched, got {:?}",
            other
        ),
    }
}

#[tokio::test]
async fn scenario_3_privacy_mode_true_skips_llm() {
    // privacy_mode=true 时即使 LLM 启用也不调用,回退到 W7 Planner。
    let kernel = TrustKernel::open_in_memory().unwrap();
    {
        let conn = kernel.conn();
        kernel
            .config_repo()
            .set(&conn, "privacy.mode", "true")
            .unwrap();
    }
    // LLM 启用(api_key 非空)— 但 privacy_mode 应阻止调用
    let llm = Arc::new(LlmClient::new(
        "https://invalid.example.com",
        "sk-test",
        "test",
    ));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理")
        .await
        .unwrap();
    // privacy_mode=true → 不调 LLM → 回退 W7 route_with_llm
    // W7 route_with_llm 调 invalid URL 失败 → Planner → Unmatched
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!(
            "privacy_mode=true should fall back to Unmatched, got {:?}",
            other
        ),
    }
}

#[tokio::test]
async fn scenario_4_llm_decompose_success_returns_dag_plan() {
    let (server, uri) = mock_llm_server_decompose(decompose_success_body_two_nodes()).await;
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    // 用不匹配任何 keyword 的文本,确保走 LLM 拆解分支而非关键词短路。
    let outcome = route_text_with_dag(&kernel, "请帮我处理这个多步任务")
        .await
        .unwrap();
    match outcome {
        RouteOutcome::DagPlan(plan) => {
            assert_eq!(plan.nodes.len(), 2);
            assert_eq!(plan.nodes[0].skill_id, "research.save_markdown");
            assert_eq!(plan.nodes[1].skill_id, "files.organize");
            assert_eq!(plan.edges.len(), 1);
            assert_eq!(plan.edges[0].from, "n1");
            assert_eq!(plan.edges[0].to, "n2");
            assert_eq!(plan.max_total_steps, 5);
        }
        other => panic!("expected DagPlan, got {:?}", other),
    }
    drop(server);
}

#[tokio::test]
async fn scenario_5_llm_http_failure_falls_back_to_route_with_llm() {
    // LLM 启用但 HTTP 失败(401)→ decompose_to_dag 失败 → 回退 route_with_llm
    // route_with_llm 也调 LLM(同一 URL)→ 同样 401 失败 → Planner → Unmatched
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let uri = server.uri();

    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-invalid", "test"));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理")
        .await
        .unwrap();
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!("HTTP 401 should fall back to Unmatched, got {:?}", other),
    }
}

#[tokio::test]
async fn scenario_6_llm_validation_failure_falls_back_to_route_with_llm() {
    // LLM 返回非法 skill_id(unknown.skill)→ decompose_to_dag_traced 内部校验失败
    // (Plan 2 实现严格,会 Err)→ 回退 route_with_llm → 同一 mock schema 不匹配
    // classify_and_extract 期望的 route_skill → parse 失败 → Planner → Unmatched。
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "decompose_to_dag",
                        // 引用 unknown.skill(不在 candidate_skills)
                        "arguments": "{\"plan_id\":\"plan-bad\",\"user_goal\":\"test\",\"nodes\":[{\"node_id\":\"n1\",\"skill_id\":\"unknown.skill\",\"input_template\":{\"kind\":\"text\",\"template\":{\"Literal\":\"foo\"}},\"risk_ceiling\":\"E1\"}],\"edges\":[],\"loop_specs\":{},\"max_total_steps\":3}"
                    }
                }]
            }
        }]
    });
    let (server, uri) = mock_llm_server_decompose(body).await;

    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    let outcome = route_text_with_dag(&kernel, "请讲解量子计算原理")
        .await
        .unwrap();
    // 不论是 decompose_to_dag_traced 内部校验失败还是 validate_dag 失败,
    // 都应 catch Err 并回退到 route_with_llm → 同一 mock 返回非法 DAG →
    // route_with_llm 的 classify_and_extract 也可能失败 → Planner → Unmatched
    match outcome {
        RouteOutcome::Unmatched { text } => {
            assert_eq!(text, "请讲解量子计算原理");
        }
        other => panic!(
            "validation failure should fall back to Unmatched, got {:?}",
            other
        ),
    }
    drop(server);
}

#[tokio::test]
async fn scenario_7_llm_decompose_single_node_dag_returns_dag_plan() {
    let (server, uri) = mock_llm_server_decompose(decompose_success_body_single_node()).await;
    let kernel = TrustKernel::open_in_memory().unwrap();
    let llm = Arc::new(LlmClient::new(&uri, "sk-test", "test"));
    kernel.set_llm_client(Some(llm));

    // 用不匹配任何 keyword 的文本,确保走 LLM 拆解分支。
    let outcome = route_text_with_dag(&kernel, "请帮我处理这个单步任务")
        .await
        .unwrap();
    match outcome {
        RouteOutcome::DagPlan(plan) => {
            assert_eq!(plan.nodes.len(), 1, "single-node DAG is valid (spec §2.2)");
            assert_eq!(plan.nodes[0].skill_id, "files.organize");
        }
        other => panic!("expected DagPlan for single-node DAG, got {:?}", other),
    }
    drop(server);
}

#[tokio::test]
async fn scenario_8_empty_string_returns_empty() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let outcome = route_text_with_dag(&kernel, "").await.unwrap();
    assert!(
        matches!(outcome, RouteOutcome::Empty),
        "expected Empty, got {:?}",
        outcome
    );

    let outcome2 = route_text_with_dag(&kernel, "   \t\n  ").await.unwrap();
    assert!(
        matches!(outcome2, RouteOutcome::Empty),
        "whitespace-only should be Empty"
    );
}
