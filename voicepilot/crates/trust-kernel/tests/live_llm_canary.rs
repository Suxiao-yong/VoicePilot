//! Live LLM canary — `#[ignore]`d by default, never runs in CI.
//!
//! Purpose: pin the load-bearing assumption behind removing forced
//! `tool_choice`, namely "in auto mode the model still emits `tool_calls`
//! for routing instructions". If deepseek (or a future default model) ever
//! answers routing prompts with plain content instead, this test goes red
//! while the whole wiremock suite stays green — exactly the blind spot the
//! R1 review flagged.
//!
//! Run manually with real credentials (never committed):
//! ```powershell
//! $env:VOICEPILOT_LIVE_LLM_URL = "https://api.deepseek.com/v1"
//! $env:VOICEPILOT_LIVE_LLM_KEY = "<key>"
//! $env:VOICEPILOT_LIVE_LLM_MODEL = "deepseek-v4-flash"
//! cargo test -p trust-kernel --features llm --test live_llm_canary -- --ignored --nocapture
//! ```
#![cfg(feature = "llm")]

use trust_kernel::llm::client::LlmClient;
use trust_kernel::skills::manifest::files_organize_manifest;

fn live_client() -> Option<LlmClient> {
    let url = std::env::var("VOICEPILOT_LIVE_LLM_URL").ok()?;
    let key = std::env::var("VOICEPILOT_LIVE_LLM_KEY").ok()?;
    let model = std::env::var("VOICEPILOT_LIVE_LLM_MODEL").ok()?;
    if url.trim().is_empty() || key.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    Some(LlmClient::new(&url, &key, &model))
}

/// Canary: unambiguous instruction ("整理下载目录" → files.organize) must
/// come back `Ok` with a `Some` skill id — i.e. the model really took the
/// `tool_calls` path, not a content-only reply (which would `Err(Parse)`).
#[ignore = "needs real LLM credentials via VOICEPILOT_LIVE_LLM_* env"]
#[tokio::test]
async fn live_canary_classify_emits_tool_calls() {
    let Some(client) = live_client() else {
        eprintln!("SKIP: set VOICEPILOT_LIVE_LLM_URL/KEY/MODEL to run the live canary");
        return;
    };
    let skills = vec![files_organize_manifest()];
    let resp = client
        .classify_and_extract("整理下载目录", &skills)
        .await
        .expect("live classify must succeed");
    assert_eq!(resp.matched_skill_id.as_deref(), Some("files.organize"));
}
