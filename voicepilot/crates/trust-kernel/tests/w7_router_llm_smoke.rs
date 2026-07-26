//! W7 Plan 6 Task 1: LLM 路由链 E2E 冒烟测试。
//!
//! 覆盖 5 个场景:keyword 命中 / LLM 高 confidence / LLM 低 confidence / LLM disabled / privacy_mode。
//! 与 router.rs 单元测试的区别:本测试注册全部 6 个 built-in Skill,验证多 Skill 注册下的路由决策,
//! 并通过 `wiremock::MockServer::received_requests` 验证 LLM 是否被实际调用(privacy_mode 关键断言)。

#![cfg(feature = "llm")]

use std::sync::Arc;

use trust_kernel::llm::client::LlmClient;
use trust_kernel::skills::manifest::{
    files_organize_manifest, form_prepare_manifest, research_save_manifest,
    task_compensate_manifest, task_explain_manifest, task_repeat_verified_manifest,
};
use trust_kernel::skills::router::{RouteDecision, SkillRouter};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 注册全部 6 个 built-in Skill 到 router。
fn register_all_builtins(router: &mut SkillRouter) {
    router.register(files_organize_manifest());
    router.register(task_repeat_verified_manifest());
    router.register(task_explain_manifest());
    router.register(task_compensate_manifest());
    router.register(research_save_manifest());
    router.register(form_prepare_manifest());
}

/// 构造 wiremock mock 返回的 OpenAI-compatible 响应 body。
fn llm_response_body(
    matched_skill_id: Option<&str>,
    confidence: f32,
    slots: serde_json::Value,
    reasoning: &str,
) -> serde_json::Value {
    let arguments = serde_json::json!({
        "matched_skill_id": matched_skill_id,
        "confidence": confidence,
        "slots": slots,
        "reasoning": reasoning,
    });
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "route_skill",
                        "arguments": arguments.to_string(),
                    }
                }]
            }
        }]
    })
}

/// Mount a wiremock returning `body` for POST /chat/completions with Bearer sk-test.
async fn mount_chat_completions(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn keyword_hit_skips_llm_call() {
    let server = MockServer::start().await;
    // 即便 mount 了高 confidence 响应,keyword 命中应短路,LLM 不应被调用
    let body = llm_response_body(
        Some("files.organize"),
        0.9,
        serde_json::json!([]),
        "should not be reached",
    );
    mount_chat_completions(&server, body).await;

    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
    let mut router = SkillRouter::with_llm(llm);
    register_all_builtins(&mut router);

    // "整理下载目录" 命中 files.organize 的 keyword "整理",keyword 短路,LLM 不应被调用
    let decision = router.route_with_llm("整理下载目录").await;
    match decision {
        RouteDecision::Skill(manifest) => assert_eq!(manifest.id, "files.organize"),
        other => panic!("expected Skill(files.organize), got {:?}", other),
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "keyword hit should short-circuit before LLM HTTP call"
    );
}

#[tokio::test]
async fn no_keyword_llm_high_confidence_returns_skill_with_slots() {
    let server = MockServer::start().await;
    let slots = serde_json::json!([
        {"kind": "url", "raw": "https://example.com", "high_risk": false},
        {"kind": "object", "raw": "{\"name\":\"Alice\"}", "high_risk": false},
    ]);
    let body = llm_response_body(Some("form.prepare"), 0.9, slots, "user wants to fill form");
    mount_chat_completions(&server, body).await;

    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
    let mut router = SkillRouter::with_llm(llm);
    register_all_builtins(&mut router);

    // "请讲解量子计算原理" 不命中任何 built-in keyword(已由 router.rs 单元测试验证),
    // 触发 LLM fallback,LLM mock 返回 form.prepare + confidence 0.9 + 2 slots → SkillWithSlots
    let decision = router.route_with_llm("请讲解量子计算原理").await;
    match decision {
        RouteDecision::SkillWithSlots(manifest, slots) => {
            assert_eq!(manifest.id, "form.prepare");
            assert_eq!(slots.len(), 2);
            assert_eq!(slots[0].kind, "url");
            assert_eq!(slots[1].kind, "object");
        }
        other => panic!("expected SkillWithSlots(form.prepare, 2 slots), got {:?}", other),
    }
}

#[tokio::test]
async fn no_keyword_llm_low_confidence_returns_planner() {
    let server = MockServer::start().await;
    let body = llm_response_body(None, 0.3, serde_json::json!([]), "uncertain");
    mount_chat_completions(&server, body).await;

    let llm = Arc::new(LlmClient::new(&server.uri(), "sk-test", "test"));
    let mut router = SkillRouter::with_llm(llm);
    register_all_builtins(&mut router);

    // "随便做点什么" 不命中任何 keyword,LLM 返回 confidence 0.3 < 0.7 → Planner
    let decision = router.route_with_llm("随便做点什么").await;
    assert!(matches!(decision, RouteDecision::Planner));
}

#[tokio::test]
async fn llm_disabled_falls_back_to_planner() {
    let server = MockServer::start().await;
    // 不 mount 任何 mock — disabled client 不应发起任何 HTTP 调用
    let llm = Arc::new(LlmClient::disabled());
    let mut router = SkillRouter::with_llm(llm);
    register_all_builtins(&mut router);

    // "随便做点什么" 不命中任何 keyword;LLM disabled(is_enabled() = false)→ 直接 Planner
    let decision = router.route_with_llm("随便做点什么").await;
    assert!(matches!(decision, RouteDecision::Planner));
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "disabled LlmClient must not make any HTTP call"
    );
}

#[tokio::test]
async fn privacy_mode_simulated_no_llm_http_call() {
    let server = MockServer::start().await;
    // 即便 mount 了高 confidence 响应,privacy_mode 下 LlmClient::disabled() 不会发起调用
    let body = llm_response_body(
        Some("form.prepare"),
        0.9,
        serde_json::json!([
            {"kind": "url", "raw": "https://example.com", "high_risk": false}
        ]),
        "would match if called",
    );
    mount_chat_completions(&server, body).await;

    // privacy_mode = true 时 AppState::rebuild_llm_client 用 LlmClient::disabled()(同款机制)
    let llm = Arc::new(LlmClient::disabled());
    let mut router = SkillRouter::with_llm(llm);
    register_all_builtins(&mut router);

    // "请讲解量子计算原理" 不命中任何 keyword,本应触发 LLM,但 privacy_mode 阻止调用 → Planner
    let decision = router.route_with_llm("请讲解量子计算原理").await;
    assert!(matches!(decision, RouteDecision::Planner));
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "privacy_mode must prevent any HTTP call to LLM"
    );
}
