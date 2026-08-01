//! W8 Plan 3 Task 7: LlmClient::explain_failure wiremock 测试.
//!
//! 5 场景:LLM 成功 / HTTP 失败 / JSON 解析失败 / 非法 category / LLM 未配置。
//! 全部 `#[cfg(feature = "llm")] #[tokio::test]`。
//!
//! Spec §2.5: `LlmClient::explain_failure(step, audit_logs) -> LlmResult<LlmAnalysis>`
//! - 中文 system prompt 约束(仅基于事实 / category=Unknown 时不强行解释 / ≤ 200 字)
//! - function calling schema `{ root_cause_zh, category, suggested_fix, confidence }`
//! - 失败时返回 LlmError,调用方(Task 8 `execute_task_explain_with_llm`)回退 structured_only

#![cfg(feature = "llm")]

use chrono::Utc;
use trust_kernel::audit::AuditEvent;
use trust_kernel::llm::client::LlmClient;
use trust_kernel::llm::types::LlmError;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::explanation_repo::FailureCategory;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_step() -> StepRecord {
    let mut step = StepRecord::new("step-001", "task-001", 1);
    step.status = StepStatus::Failed;
    step
}

fn make_audit_logs() -> Vec<AuditEvent> {
    vec![AuditEvent {
        log_id: "log-001".to_string(),
        task_id: "task-001".to_string(),
        step_id: Some("step-001".to_string()),
        event_type: "mcp_call_failed".to_string(),
        details: serde_json::json!({"error": "command not found", "server": "playwright"}),
        timestamp: Utc::now(),
        prev_hash: None,
        hash: String::new(),
    }]
}

fn llm_client(base_url: &str) -> LlmClient {
    LlmClient::new(base_url, "test-api-key", "gpt-4o-mini")
}

#[tokio::test]
async fn explain_failure_success_returns_llm_analysis() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "explain_failure",
                            "arguments": "{\"root_cause_zh\": \"Playwright MCP 服务器命令未找到,spawn 失败\", \"category\": \"mcp_unavailable\", \"suggested_fix\": \"请在 Settings 中检查 Playwright MCP 配置\", \"confidence\": 0.9}"
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let analysis = result.unwrap();
    assert_eq!(analysis.category, FailureCategory::McpUnavailable);
    assert!(
        analysis.root_cause_zh.contains("Playwright"),
        "got: {}",
        analysis.root_cause_zh
    );
    assert!(analysis.confidence > 0.8);
    assert!(analysis.suggested_fix.is_some());
}

#[tokio::test]
async fn explain_failure_http_500_returns_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, LlmError::Http(_)),
        "expected Http error, got {:?}",
        err
    );
}

#[tokio::test]
async fn explain_failure_missing_tool_calls_returns_parse_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "no tool call"}}]
        })))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, LlmError::Parse(_)),
        "expected Parse error, got {:?}",
        err
    );
}

#[tokio::test]
async fn explain_failure_invalid_category_returns_parse_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "name": "explain_failure",
                            "arguments": "{\"root_cause_zh\": \"x\", \"category\": \"invalid_category\", \"confidence\": 0.5}"
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let client = llm_client(&server.uri());
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        LlmError::Parse(msg) => {
            assert!(msg.contains("invalid category"), "got: {}", msg);
        }
        other => panic!("expected Parse error, got {:?}", other),
    }
}

#[tokio::test]
async fn explain_failure_not_configured_returns_err() {
    // disabled client → Err(NotConfigured)
    let client = LlmClient::disabled();
    let step = make_step();
    let logs = make_audit_logs();
    let result = client.explain_failure(&step, &logs).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, LlmError::NotConfigured),
        "expected NotConfigured, got {:?}",
        err
    );
}
