//! W8 Plan 3 Task 8: execute_task_explain_with_llm 端到端测试.
//!
//! 6 场景:
//! 1. LLM=None → structured_only(llm_analysis=None,不持久化)
//! 2. LLM 启用 + step Succeeded → structured_only(不调 LLM)
//! 3. LLM 启用 + step Failed + LLM 成功 → 持久化 + llm_explain_called 审计
//! 4. LLM 启用 + step Failed + HTTP 失败 → 回退 structured_only(不持久化)
//! 5. LLM 启用 + step Failed + 非法 category → 回退 structured_only(不持久化)
//! 6. step 不存在 → Err
//!
//! 全部 `#[cfg(feature = "llm")]` 因为 `execute_task_explain_with_llm` 引用 `LlmClient`。
//! `llm` 在 default feature 中,所以 `cargo test` 默认运行全部测试。

#![cfg(feature = "llm")]

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::explanation_repo::{TaskExplanationRecord, TaskExplanationRepo};
use trust_kernel::skills::task_explain::{TaskExplainInput, execute_task_explain_with_llm};

fn seed_failed_step(kernel: &TrustKernel, step_id: &str, task_id: &str) {
    kernel.create_task(task_id, "seed failed task").unwrap();
    let mut step = StepRecord::new(step_id, task_id, 1);
    step.status = StepStatus::Failed;
    kernel.create_step(&step).unwrap();
    // Emit a mcp_call_failed audit event so extract_failed_tool_calls has data.
    kernel
        .audit_append_external(
            task_id,
            Some(step_id),
            "mcp_call_failed",
            serde_json::json!({
                "tool_name": "playwright.navigate",
                "args": {"url": "https://example.com"},
                "error": "command not found",
            }),
        )
        .unwrap();
}

fn seed_succeeded_step(kernel: &TrustKernel, step_id: &str, task_id: &str) {
    kernel.create_task(task_id, "seed succeeded task").unwrap();
    let mut step = StepRecord::new(step_id, task_id, 1);
    step.status = StepStatus::Succeeded;
    kernel.create_step(&step).unwrap();
}

#[tokio::test]
async fn explain_with_llm_none_returns_structured_only() {
    // LLM=None → structured_only,不调 LLM,不持久化。
    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_failed_step(&kernel, "step-001", "task-001");

    let input = TaskExplainInput {
        task_id: "task-001".to_string(),
        step_id: "step-001".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = execute_task_explain_with_llm(&kernel, &input, &approver, None).await;

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_none());
    assert_eq!(explanation.step_id, "step-001");
    assert_eq!(explanation.status, StepStatus::Failed);
    assert_eq!(explanation.failed_tool_calls.len(), 1);
    assert_eq!(
        explanation.failed_tool_calls[0].tool_name,
        "playwright.navigate"
    );

    // No persistence.
    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();
    let persisted = repo.get_by_step_id(&conn, "step-001").unwrap();
    assert!(persisted.is_none());
}

#[tokio::test]
async fn explain_with_llm_succeeded_step_returns_structured_only() {
    // step Succeeded → 不调 LLM,即使 LLM 启用。
    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_succeeded_step(&kernel, "step-002", "task-002");

    let input = TaskExplainInput {
        task_id: "task-002".to_string(),
        step_id: "step-002".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    // 即使传 disabled LlmClient,step Succeeded 也不应触发 LLM 调用。
    let llm = trust_kernel::llm::client::LlmClient::disabled();
    let result = execute_task_explain_with_llm(&kernel, &input, &approver, Some(&llm)).await;

    assert!(result.is_ok());
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_none());
    assert_eq!(explanation.status, StepStatus::Succeeded);
}

#[tokio::test]
async fn explain_with_step_not_found_returns_err() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let input = TaskExplainInput {
        task_id: "no-such-task".to_string(),
        step_id: "no-such-step".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = execute_task_explain_with_llm(&kernel, &input, &approver, None).await;

    assert!(result.is_err());
}

// ===== LLM 启用场景(wiremock)=====

use trust_kernel::llm::client::LlmClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn llm_client(base_url: &str) -> LlmClient {
    LlmClient::new(base_url, "test-api-key", "gpt-4o-mini")
}

fn ok_response(category: &str, root_cause: &str, confidence: f32) -> serde_json::Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "explain_failure",
                        "arguments": format!(
                            "{{\"root_cause_zh\": \"{}\", \"category\": \"{}\", \"confidence\": {}}}",
                            root_cause, category, confidence
                        )
                    }
                }]
            }
        }]
    })
}

#[tokio::test]
async fn explain_with_llm_failed_step_persists_and_audits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response(
            "mcp_unavailable",
            "Playwright MCP 命令未找到",
            0.9,
        )))
        .mount(&server)
        .await;

    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_failed_step(&kernel, "step-010", "task-010");
    let client = llm_client(&server.uri());

    let input = TaskExplainInput {
        task_id: "task-010".to_string(),
        step_id: "step-010".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = execute_task_explain_with_llm(&kernel, &input, &approver, Some(&client)).await;

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_some());
    let analysis = explanation.llm_analysis.unwrap();
    assert_eq!(
        analysis.category,
        trust_kernel::skills::explanation_repo::FailureCategory::McpUnavailable
    );
    assert!(analysis.confidence > 0.5);

    // 持久化到 task_explanations 表。
    // 用 block scope 限制 conn guard 生命周期,避免后面 list_audit_for_task
    // 再次 lock 同一 mutex 导致死锁(参考 project_memory reentrancy deadlock)。
    {
        let conn = kernel.conn();
        let repo = TaskExplanationRepo::new();
        let persisted = repo.get_by_step_id(&conn, "step-010").unwrap();
        assert!(persisted.is_some(), "expected persistence");
        let rec: TaskExplanationRecord = persisted.unwrap();
        assert_eq!(rec.category, "mcp_unavailable");
        assert!(rec.root_cause_zh.contains("Playwright"));
    }

    // llm_explain_called 审计事件已发。
    let audit = kernel.list_audit_for_task("task-010").unwrap();
    assert!(
        audit.iter().any(|e| e.event_type == "llm_explain_called"),
        "expected llm_explain_called audit event"
    );
}

#[tokio::test]
async fn explain_with_llm_http_failure_falls_back_to_structured_only() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_failed_step(&kernel, "step-020", "task-020");
    let client = llm_client(&server.uri());

    let input = TaskExplainInput {
        task_id: "task-020".to_string(),
        step_id: "step-020".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = execute_task_explain_with_llm(&kernel, &input, &approver, Some(&client)).await;

    assert!(result.is_ok(), "HTTP failure must not propagate");
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_none(), "expected fallback");

    // 不持久化。用 block scope 限制 conn guard 生命周期,避免后面
    // list_audit_for_task 再次 lock 同一 mutex 导致死锁。
    {
        let conn = kernel.conn();
        let repo = TaskExplanationRepo::new();
        assert!(repo.get_by_step_id(&conn, "step-020").unwrap().is_none());
    }

    // 不发 llm_explain_called 审计事件。
    let audit = kernel.list_audit_for_task("task-020").unwrap();
    assert!(
        !audit.iter().any(|e| e.event_type == "llm_explain_called"),
        "expected no llm_explain_called audit on HTTP failure"
    );
}

#[tokio::test]
async fn explain_with_llm_invalid_category_falls_back_to_structured_only() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response(
            "invalid_category",
            "x",
            0.5,
        )))
        .mount(&server)
        .await;

    let kernel = TrustKernel::open_in_memory().unwrap();
    seed_failed_step(&kernel, "step-030", "task-030");
    let client = llm_client(&server.uri());

    let input = TaskExplainInput {
        task_id: "task-030".to_string(),
        step_id: "step-030".to_string(),
        limit: 10,
    };
    let approver = AutoApprover;
    let result = execute_task_explain_with_llm(&kernel, &input, &approver, Some(&client)).await;

    assert!(result.is_ok());
    let explanation = result.unwrap();
    assert!(explanation.llm_analysis.is_none(), "expected fallback");

    let conn = kernel.conn();
    let repo = TaskExplanationRepo::new();
    assert!(repo.get_by_step_id(&conn, "step-030").unwrap().is_none());
}
