#![cfg(feature = "llm")]

//! RED contract tests for the future side-effect-free planner pipeline.

use std::sync::Arc;

use trust_kernel::extensions::registry::ExtensionCatalog;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::planner::{PlanResult, PlannerInput, PlannerPipeline, PlannerSource};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn reject_any_llm_call(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(server)
        .await;
}

fn persistence_counts(kernel: &TrustKernel) -> (i64, i64, i64) {
    let conn = kernel.conn();
    let tasks = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .expect("count tasks");
    let audit_logs = conn
        .query_row("SELECT COUNT(*) FROM audit_logs", [], |row| row.get(0))
        .expect("count audit logs");
    let taints = conn
        .query_row("SELECT COUNT(*) FROM taints", [], |row| row.get(0))
        .expect("count taints");
    (tasks, audit_logs, taints)
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

#[tokio::test]
async fn keyword_fast_path_returns_skill_without_llm_call_or_persistence() {
    let server = MockServer::start().await;
    reject_any_llm_call(&server).await;
    let (kernel, pipeline) = pipeline_with_mock_llm(&server).await;
    let before = persistence_counts(&kernel);

    let (plan, trace) = pipeline
        .plan(PlannerInput {
            text: "整理下载目录".to_string(),
            source: PlannerSource::Text,
            snapshot: None,
        })
        .await
        .expect("keyword plan");

    match plan {
        PlanResult::Skill {
            extension_id,
            slots,
        } => {
            assert_eq!(extension_id, "files.organize");
            assert!(slots.is_empty());
        }
        other => panic!("expected keyword Skill plan, got {other:?}"),
    }
    assert!(!trace.snapshot_id.is_empty());
    assert!(!trace.used_llm);
    assert!(trace.llm_model.is_none());
    assert!(trace.token_count.is_none());
    assert_eq!(before, persistence_counts(&kernel));
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "keyword fast path must not call the LLM"
    );
}

#[tokio::test]
async fn text_and_voice_sources_produce_the_same_skill_plan() {
    let server = MockServer::start().await;
    reject_any_llm_call(&server).await;
    let (kernel, pipeline) = pipeline_with_mock_llm(&server).await;

    let (text_plan, text_trace) = pipeline
        .plan(PlannerInput {
            text: "整理下载目录".to_string(),
            source: PlannerSource::Text,
            snapshot: None,
        })
        .await
        .expect("text plan");
    let (voice_plan, voice_trace) = pipeline
        .plan(PlannerInput {
            text: "整理下载目录".to_string(),
            source: PlannerSource::Voice,
            snapshot: None,
        })
        .await
        .expect("voice plan");

    match (text_plan, voice_plan) {
        (
            PlanResult::Skill {
                extension_id: text_extension_id,
                ..
            },
            PlanResult::Skill {
                extension_id: voice_extension_id,
                ..
            },
        ) => assert_eq!(text_extension_id, voice_extension_id),
        (text, voice) => panic!("expected Skill plans, got text={text:?}, voice={voice:?}"),
    }
    assert_eq!(text_trace.snapshot_id, voice_trace.snapshot_id);
    assert!(!text_trace.used_llm);
    assert!(!voice_trace.used_llm);
    assert_eq!(persistence_counts(&kernel), (0, 0, 0));
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "text and voice keyword plans must not call the LLM"
    );
}
