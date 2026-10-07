#![cfg(feature = "llm")]

//! 快照门禁 + 上下文注入的集成测试（wiremock 挡掉真实云端）。

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;
use trust_kernel::extensions::registry::ExtensionCatalog;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::planner::{
    PlanResult, PlannerInput, PlannerPipeline, PlannerSource, RealtimeSnapshot, SnapshotMemory,
    SnapshotVoice,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fresh_snapshot() -> RealtimeSnapshot {
    RealtimeSnapshot {
        taken_at: SystemTime::now(),
        transcript_chars: 6,
        voice: SnapshotVoice {
            outcome_kind: "speech_ended",
            stopped_by_vad: true,
            sample_count: 80000,
            vad_backend: "energy",
            voice_started_ago_ms: Some(900),
        },
        memory: SnapshotMemory {
            prev_turns: vec!["用户：打开记事本 → routed:quick.app_control".to_string()],
        },
        privacy_mode: false,
    }
}

async fn pipeline_with_mock_llm(server: &MockServer) -> (Arc<TrustKernel>, PlannerPipeline) {
    let kernel = Arc::new(TrustKernel::open_in_memory().expect("open in-memory kernel"));
    kernel.set_llm_client(Some(Arc::new(LlmClient::new(
        &server.uri(),
        "sk-test",
        "test-model",
    ))));
    let catalog = ExtensionCatalog::load(&kernel).expect("load extension catalog");
    let pipeline = PlannerPipeline::new(kernel.clone(), catalog.snapshot());
    (kernel, pipeline)
}

fn mock_classify_hit() -> Mock {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "route_skill",
                            "arguments": "{\"matched_skill_id\":\"files.organize\",\"confidence\":0.9,\"slots\":[],\"reasoning\":\"test\"}"
                        }
                    }]
                }
            }]
        })))
}

#[tokio::test]
async fn snapshot_context_is_sent_to_llm() {
    let server = MockServer::start().await;
    mock_classify_hit().mount(&server).await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;

    let (plan, _trace) = pipeline
        .plan(PlannerInput {
            text: "月球上的紫色大象跳了几支舞".to_string(),
            source: PlannerSource::Voice,
            snapshot: Some(fresh_snapshot()),
        })
        .await
        .expect("snapshot plan");
    assert!(matches!(plan, PlanResult::Skill { .. }), "got {plan:?}");

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1, "classify hit must stop before decompose");
    let body = String::from_utf8_lossy(requests[0].body.as_ref()).into_owned();
    assert!(body.contains("上文"), "context block missing: {body}");
    assert!(body.contains("打开记事本"), "prev turn missing: {body}");
    assert!(body.contains("紫色大象"), "user text missing: {body}");
}

#[tokio::test]
async fn stale_snapshot_is_rejected_without_llm_call() {
    let server = MockServer::start().await;
    mock_classify_hit().mount(&server).await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;

    let mut snap = fresh_snapshot();
    snap.taken_at = SystemTime::now() - Duration::from_secs(60);
    let err = pipeline
        .plan(PlannerInput {
            text: "整理下载目录".to_string(),
            source: PlannerSource::Voice,
            snapshot: Some(snap),
        })
        .await
        .expect_err("stale snapshot must fail");
    assert!(
        err.to_string().contains("stale realtime snapshot"),
        "got {err}"
    );
    assert!(
        server
            .received_requests()
            .await
            .expect("requests")
            .is_empty(),
        "stale snapshot must not call the LLM"
    );
}

#[tokio::test]
async fn repeated_query_hits_classify_cache() {
    let server = MockServer::start().await;
    mock_classify_hit().mount(&server).await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;
    for _ in 0..2 {
        let (plan, _) = pipeline
            .plan(PlannerInput {
                text: "月球上的紫色大象跳了几支舞".to_string(),
                source: PlannerSource::Voice,
                snapshot: None,
            })
            .await
            .expect("cached plan");
        assert!(matches!(plan, PlanResult::Skill { .. }));
    }
    let requests = server.received_requests().await.expect("requests");
    assert_eq!(
        requests.len(),
        1,
        "second identical query must hit cache, got {}",
        requests.len()
    );
}

#[tokio::test]
async fn chat_fallback_answers_when_no_skill_matches() {
    // mock 对所有请求返回纯文本（无 tool_calls）：classify 与 decompose
    // 均解析失败，只能走聊天兜底。
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"role": "assistant", "content": "我是 VoicePilot，可以帮你整理文件、打开应用。"}}]
        })))
        .mount(&server)
        .await;
    let (_kernel, pipeline) = pipeline_with_mock_llm(&server).await;
    let (plan, trace) = pipeline
        .plan(PlannerInput {
            text: "月球上的紫色大象跳了几支舞".to_string(),
            source: PlannerSource::Voice,
            snapshot: None,
        })
        .await
        .expect("chat plan");
    match plan {
        PlanResult::Chat { text } => assert!(text.contains("VoicePilot"), "got {text}"),
        other => panic!("expected Chat fallback, got {other:?}"),
    }
    assert!(trace.used_llm);
}
