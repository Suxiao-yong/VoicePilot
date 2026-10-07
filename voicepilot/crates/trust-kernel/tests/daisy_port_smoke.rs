//! Daisy 移植冒烟：43 个新 Skill 的接线锁定。
//!
//! - 每个 builtin id 都能通过快照门解析出执行目标（dispatch_skill_executor
//!   第一步，等价于“不是 unknown skill_id”）。2026 免审批后不再用
//!   AutoDenier+空输入 dispatch（无槽位 skill 会真实执行，锁屏/杀进程都会
//!   发生），改走零副作用的快照门解析。
//! - 站内搜快路由端到端：mock LLM 全拒答，PlannerPipeline 仍直接给出
//!   sys.open_url + url 槽位（零 LLM 调用）。

use trust_kernel::extensions::registry::ExtensionCatalog;
use trust_kernel::kernel::TrustKernel;

fn new_skill_ids() -> Vec<String> {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let catalog = ExtensionCatalog::load(&kernel).expect("catalog load");
    catalog
        .candidates()
        .iter()
        .map(|d| d.manifest.id.clone())
        .filter(|id| {
            id.starts_with("sys.")
                || id.starts_with("clip.")
                || id.starts_with("fs.")
                || id.starts_with("web.")
                || *id == "shell.run"
                || id.starts_with("media.")
                || *id == "doc.convert"
                || *id == "doc.office"
                || *id == "mail.compose"
                || id.starts_with("pim.")
        })
        .collect()
}

#[test]
fn ported_skill_count_is_forty_three() {
    let ids = new_skill_ids();
    assert_eq!(ids.len(), 43, "移植 Skill 应为 43 个，实际 {ids:?}");
}

#[test]
fn all_ported_skills_pass_snapshot_gate() {
    // 接线验证：每个 builtin id 能通过快照门解析出执行目标
    // （dispatch_skill_executor 的第一步，等价于旧的“不是 unknown skill_id”）。
    // 2026 免审批后不再 dispatch 空输入（无槽位 skill 会真实执行，如
    // sys.lock_screen，测试会锁机器）——改走快照解析，零副作用。
    let ids = new_skill_ids();
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let snapshot = kernel.extension_snapshot();
    for id in ids {
        let r = snapshot.resolve_execution_target(&id);
        assert!(r.is_ok(), "{id} 接线缺失（快照门解析失败）: {:?}", r.err());
    }
}

#[test]
fn old_builtin_ids_still_dispatch() {
    // 与 all_ported_skills_pass_snapshot_gate 同理：走快照门解析，零副作用。
    // dispatch_skill_executor 内部第一步即此解析，解析成功即证明接线在。
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let snapshot = kernel.extension_snapshot();
    for id in [
        "files.organize",
        "task.repeat_verified",
        "task.explain",
        "task.compensate",
        "research.save_markdown",
        "form.prepare",
        "form.submit",
    ] {
        let r = snapshot.resolve_execution_target(id);
        assert!(r.is_ok(), "{id} 接线缺失（快照门解析失败）: {:?}", r.err());
    }
}

#[cfg(feature = "llm")]
#[tokio::test]
async fn site_search_routes_to_open_url_without_llm() {
    use std::sync::Arc;
    use trust_kernel::llm::client::LlmClient;
    use trust_kernel::planner::{PlanResult, PlannerInput, PlannerPipeline, PlannerSource};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // LLM 全拒答：快路由若调 LLM，此处必 500。
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let kernel = Arc::new(TrustKernel::open_in_memory().expect("open in-memory kernel"));
    kernel.set_llm_client(Some(Arc::new(LlmClient::new(
        &server.uri(),
        "sk-test",
        "test-model",
    ))));
    let catalog = ExtensionCatalog::load(&kernel).expect("load extension catalog");
    let pipeline = PlannerPipeline::new(kernel.clone(), catalog.snapshot());
    let (plan, trace) = pipeline
        .plan(PlannerInput {
            text: "打开抖音搜索世界杯".to_string(),
            source: PlannerSource::Text,
            snapshot: None,
        })
        .await
        .expect("plan");
    assert!(!trace.used_llm, "站内搜不应调用 LLM");
    match plan {
        PlanResult::Skill {
            extension_id,
            slots,
        } => {
            assert_eq!(extension_id, "sys.open_url");
            assert_eq!(slots.len(), 1);
            assert_eq!(slots[0].kind, "url");
            assert!(
                slots[0].raw.starts_with("https://www.douyin.com/search/"),
                "{}",
                slots[0].raw
            );
        }
        other => panic!("expected sys.open_url Skill, got {other:?}"),
    }
}
