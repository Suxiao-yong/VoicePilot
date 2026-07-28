//! W8 Plan 2 Task 8: LlmClient::decompose_to_dag wiremock 测试.
//!
//! 6 个 wiremock 场景(spec §2.2 + §5 错误处理):
//! 1. 成功 — LLM 返回合法 DAG,parse + 4 层校验通过
//! 2. HTTP 失败 — 500 错误,期望 LlmError::Http
//! 3. 非法 skill_id — LLM 返回 DAG 含未在 candidate_skills 中的 skill_id,期望 LlmError::Parse
//! 4. 模板错误 — LLM 返回 DAG 含 ${prev.output.x} 但 prev 不存在,期望 LlmError::Parse(validate_dag 失败)
//! 5. 超时 — wiremock delay > client timeout,期望 LlmError::Timeout
//! 6. max_total_steps 超限 — LLM 返回 max_total_steps=100,期望 LlmError::Parse
//!
//! 第 7 个测试:验证 `record_llm_decompose_called` 审计事件包含
//! 硬约束 4 字段(plan_id / llm_model / latency_ms / token_count)。

#![cfg(feature = "llm")]

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;
use trust_kernel::llm::client::{record_llm_decompose_called, LlmClient};
use trust_kernel::llm::types::{DecomposeStats, LlmError};
use trust_kernel::policy::types::ELevel;
use trust_kernel::skills::dag_types::{
    DagNode, DagPlan, MAX_TOTAL_STEPS_HARD_LIMIT,
};
use trust_kernel::skills::manifest::{files_organize_manifest, task_explain_manifest, SkillManifest};
use trust_kernel::skills::template::{SlotKind, SlotTemplate, TemplateExpr, VarRef, VarScope};
use trust_kernel::kernel::TrustKernel;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_client(base_url: &str) -> LlmClient {
    LlmClient::new(base_url, "sk-test", "deepseek-chat")
}

fn build_client_with_timeout(base_url: &str, timeout: Duration) -> LlmClient {
    LlmClient::with_timeout(base_url, "sk-test", "deepseek-chat", timeout)
}

fn candidate_skills() -> Vec<SkillManifest> {
    // 用 W7 既有 built-in manifest,避免重新构造 SkillManifest 全字段
    vec![files_organize_manifest(), task_explain_manifest()]
}

fn ok_response_with_dag(plan: &DagPlan) -> serde_json::Value {
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

fn single_node_plan(skill_id: &str, max_steps: u32) -> DagPlan {
    DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: skill_id.into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Literal("test".into()),
            },
            risk_ceiling: ELevel::E1,
        }],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: max_steps,
    }
}

#[tokio::test]
async fn decompose_to_dag_succeeds_with_valid_dag() {
    let server = MockServer::start().await;
    let plan = single_node_plan("task.explain", 5);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri());
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain recent", &skills, &[]).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    let dag = result.unwrap();
    assert_eq!(dag.nodes.len(), 1);
    assert_eq!(dag.nodes[0].skill_id, "task.explain");
}

#[tokio::test]
async fn decompose_to_dag_returns_http_error_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .mount(&server)
        .await;

    let client = build_client(&server.uri());
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, LlmError::Http(_)), "expected Http, got {:?}", err);
}

#[tokio::test]
async fn decompose_to_dag_rejects_illegal_skill_id() {
    let server = MockServer::start().await;
    // LLM 返回 skill_id = "nonexistent.skill"(不在 candidate_skills)
    let plan = single_node_plan("nonexistent.skill", 5);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri());
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = format!("{:?}", err);
    assert!(matches!(err, LlmError::Parse(_)), "expected Parse, got {}", msg);
    assert!(
        msg.contains("illegal skill_id") || msg.contains("not in candidate_skills"),
        "got: {}",
        msg
    );
}

#[tokio::test]
async fn decompose_to_dag_rejects_max_total_steps_over_20() {
    let server = MockServer::start().await;
    // LLM 返回 max_total_steps = 100(硬上限 20)
    // 注:OpenAI function calling schema 写了 maximum: 20,但 LLM 可能不遵守 → 必须运行时校验
    let plan = single_node_plan("task.explain", MAX_TOTAL_STEPS_HARD_LIMIT + 80);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri());
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = format!("{:?}", err);
    assert!(matches!(err, LlmError::Parse(_)), "expected Parse, got {}", msg);
    assert!(
        msg.contains("max_total_steps") || msg.contains("hard limit"),
        "got: {}",
        msg
    );
}

#[tokio::test]
async fn decompose_to_dag_returns_timeout_on_slow_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(ok_response_with_dag(&single_node_plan("task.explain", 5)))
                .set_delay(Duration::from_secs(2)),
        )
        .mount(&server)
        .await;

    // client timeout = 100ms,server delay = 2s → 超时
    let client = build_client_with_timeout(&server.uri(), Duration::from_millis(100));
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, LlmError::Timeout(_)),
        "expected Timeout, got {:?}",
        err
    );
}

#[tokio::test]
async fn decompose_to_dag_returns_not_configured_when_api_key_empty() {
    let server = MockServer::start().await;
    // 不 mount 任何 mock — is_enabled() 应在发送前返回 false
    let client = LlmClient::new(&server.uri(), "", "deepseek-chat");
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, LlmError::NotConfigured),
        "expected NotConfigured, got {:?}",
        err
    );
}

#[tokio::test]
async fn decompose_to_dag_rejects_template_with_dangling_prev_ref() {
    // 模板错误:节点引用 ${prev.output.path} 但 prev_node_id=None(首节点)
    // SlotTemplateEngine::validate_dag 应失败 → LlmError::Parse
    let server = MockServer::start().await;
    let plan = DagPlan {
        plan_id: format!("plan-{}", uuid::Uuid::new_v4()),
        user_goal: "test".into(),
        nodes: vec![DagNode {
            node_id: "n1".into(),
            skill_id: "task.explain".into(),
            input_template: SlotTemplate {
                kind: SlotKind::Text,
                template: TemplateExpr::Var(VarRef {
                    scope: VarScope::Prev,
                    path: "output.path".into(),
                }),
            },
            risk_ceiling: ELevel::E0,
        }],
        edges: vec![],
        loop_specs: HashMap::new(),
        max_total_steps: 5,
    };
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri());
    let skills = candidate_skills();
    let result = client.decompose_to_dag("explain", &skills, &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = format!("{:?}", err);
    assert!(
        matches!(err, LlmError::Parse(_)),
        "expected Parse, got {}",
        msg
    );
    assert!(
        msg.contains("template validation failed") || msg.contains("prev"),
        "got: {}",
        msg
    );
}

#[tokio::test]
async fn decompose_to_dag_traced_returns_stats_with_token_count() {
    // 验证 decompose_to_dag_traced 返回的 DecomposeStats 携带
    // llm_model / latency_ms / token_count(供 llm_decompose_called 审计)
    let server = MockServer::start().await;
    let plan = single_node_plan("task.explain", 5);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response_with_dag(&plan)))
        .mount(&server)
        .await;

    let client = build_client(&server.uri());
    let skills = candidate_skills();
    let (dag, stats) = client
        .decompose_to_dag_traced("explain recent", &skills, &[])
        .await
        .expect("traced call should succeed");
    assert_eq!(dag.nodes.len(), 1);
    assert_eq!(stats.llm_model, "deepseek-chat");
    assert_eq!(stats.token_count, 42);
    // latency_ms 应非负(网络极快时可能为 0,不强制 > 0)
    let _ = stats.latency_ms;
}

#[test]
fn record_llm_decompose_called_emits_audit_with_required_fields() {
    // 硬约束(project_memory):llm_decompose_called 审计事件必须携带
    // plan_id / llm_model / latency_ms / token_count 4 个字段。
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 必须先 create_task 才能写 audit_logs(FK 约束)
    let task_id = format!("task-{}", uuid::Uuid::new_v4());
    kernel.create_task(&task_id, "test goal").unwrap();

    let stats = DecomposeStats {
        llm_model: "deepseek-chat".into(),
        latency_ms: 1234,
        token_count: 5678,
    };
    record_llm_decompose_called(&kernel, &task_id, "plan-abc", &stats).unwrap();

    let events: Vec<serde_json::Value> = kernel
        .list_audit_recent(50)
        .unwrap()
        .into_iter()
        .filter(|e| e.event_type == "llm_decompose_called")
        .map(|e| e.details)
        .collect();
    assert_eq!(events.len(), 1, "expected 1 llm_decompose_called event");
    let e = &events[0];
    assert_eq!(e["plan_id"], "plan-abc");
    assert_eq!(e["llm_model"], "deepseek-chat");
    assert_eq!(e["latency_ms"], 1234);
    assert_eq!(e["token_count"], 5678);
}
