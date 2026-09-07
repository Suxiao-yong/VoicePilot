//! W7 Plan 6 Task 2 (Wave 3 修订): Settings LLM UI 冒烟测试。
//!
//! Wave 3 Task 3.1:LLM API key 不再通过 KV 往返 —— 它只存在于
//! SecretStore(keyring)。读取 DTO 为 `SettingsView`(`llm_api_key_present`,
//! 无 key 值);写入 DTO 为 `SettingsUpdate`(`llm_api_key: Option<String>` +
//! `clear_llm_api_key`)。
//!
//! 覆盖场景:非 secret LLM 字段 KV 往返 / update_settings 持久化 + key 入
//! SecretStore + rebuild_llm_client / privacy_mode=true 强制 disabled /
//! llm_enabled=false 或无 key 强制 disabled。
//!
//! 注意:测试用 `AppState::new_in_memory_with_secret_store` 注入内存 store,
//! 避免把测试 key 写入真实 Windows Credential Manager。

#![cfg(all(feature = "tauri", feature = "llm"))]

use std::sync::Arc;

use trust_kernel::secrets::{InMemorySecretStore, SecretStore};
use voicepilot_ui::settings_commands::{
    flatten_to_kv, get_settings, merge_from_kv, update_settings, SettingsUpdate, SettingsView,
};
use voicepilot_ui::state::AppState;

fn new_test_state() -> anyhow::Result<AppState> {
    let store: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
    AppState::new_in_memory_with_secret_store(store)
}

/// 镜像 `update_settings_command` 函数体:纯 `update_settings` 持久化 KV +
/// SecretStore 操作,command 包装额外刷新 serving-applied LlmClient。
fn update_settings_and_rebuild_llm(state: &AppState, update: &SettingsUpdate) {
    update_settings(state, update).expect("update_settings");
    let view = SettingsView::from(update);
    state.rebuild_llm_client(&view);
}

/// 一个默认 SettingsUpdate(secret 字段显式)。
fn default_update() -> SettingsUpdate {
    let view = SettingsView::default();
    SettingsUpdate {
        voice_model_path: view.voice_model_path,
        voice_language: view.voice_language,
        voice_threads: view.voice_threads,
        vad_energy_threshold: view.vad_energy_threshold,
        vad_max_silence_ms: view.vad_max_silence_ms,
        vad_min_speech_ms: view.vad_min_speech_ms,
        voice_max_duration_ms: view.voice_max_duration_ms,
        voice_chunk_duration_ms: view.voice_chunk_duration_ms,
        privacy_mode: view.privacy_mode,
        compensation_ttl_hours: view.compensation_ttl_hours,
        tts_enabled: view.tts_enabled,
        tts_model_path: view.tts_model_path,
        llm_enabled: view.llm_enabled,
        llm_base_url: view.llm_base_url,
        llm_model: view.llm_model,
        llm_provider_url: view.llm_provider_url,
        llm_api_key: None,
        clear_llm_api_key: false,
        uia_allowed_apps: view.uia_allowed_apps,
    }
}

/// Test 1: 非 secret LLM 字段通过 flatten_to_kv → merge_from_kv 往返一致;
/// `llm.api_key` 绝不出现在 KV 中。
#[test]
fn settings_llm_fields_flatten_merge_roundtrip_no_key_in_kv() {
    let dto = SettingsView {
        llm_enabled: true,
        llm_api_key_present: true,
        llm_base_url: "https://api.deepseek.com/v1".to_string(),
        llm_model: "deepseek-chat".to_string(),
        llm_provider_url: "https://platform.deepseek.com/api_keys".to_string(),
        privacy_mode: false,
        ..Default::default()
    };

    let kv = flatten_to_kv(&dto);
    let got = merge_from_kv(&kv).expect("merge_from_kv should succeed for valid LLM KV");

    assert_eq!(got.llm_enabled, dto.llm_enabled);
    assert_eq!(got.llm_base_url, dto.llm_base_url);
    assert_eq!(got.llm_model, dto.llm_model);
    assert_eq!(got.llm_provider_url, dto.llm_provider_url);
    assert_eq!(got.privacy_mode, dto.privacy_mode);
    assert!(
        !kv.iter().any(|(k, _)| k == "llm.api_key"),
        "llm.api_key must never be persisted in KV"
    );
}

/// Test 2: update_settings 持久化 KV + 把 key 写入 SecretStore + rebuild_llm_client
/// 使 state.llm_client() 反映新配置;get_settings 只报告 key 存在,不回读值。
#[test]
fn update_settings_persists_and_rebuilds_llm_client() {
    let state = new_test_state().expect("new test state");

    assert!(
        !state.llm_client().is_enabled(),
        "fresh AppState should have disabled LlmClient"
    );

    let mut update = default_update();
    update.llm_enabled = true;
    update.llm_api_key = Some("sk-persisted".to_string());

    update_settings_and_rebuild_llm(&state, &update);

    let kernel_llm = state
        .kernel
        .llm_client()
        .expect("kernel should own the rebuilt LlmClient");
    assert!(
        kernel_llm.is_enabled(),
        "kernel llm_client should be enabled after update_settings with a stored key"
    );
    assert!(
        state.llm_client().is_enabled(),
        "AppState::llm_client() should delegate to kernel"
    );

    // get_settings 报告 key 存在,但绝不返回 key 值。
    let retrieved = get_settings(&state).expect("get_settings");
    assert!(
        retrieved.llm_api_key_present,
        "key present flag must be true"
    );
    assert!(retrieved.llm_enabled);

    // key 在 SecretStore,不在 SQLite。
    assert_eq!(
        state.kernel.llm_api_key().expect("read store").as_deref(),
        Some("sk-persisted")
    );
    {
        let conn = state.kernel.conn();
        assert!(
            state
                .kernel
                .config_repo()
                .get(&conn, "llm.api_key")
                .expect("query legacy key")
                .is_none(),
            "SQLite must not hold the API key plaintext"
        );
        assert_eq!(
            state
                .kernel
                .config_repo()
                .get(&conn, "llm.api_key_present")
                .expect("query presence flag")
                .as_deref(),
            Some("true")
        );
    }
}

/// Test 3: privacy_mode=true 强制 rebuild_llm_client 返回 disabled(覆盖 llm_enabled+key)。
#[test]
fn privacy_mode_true_returns_disabled_llm_client() {
    let state = new_test_state().expect("new test state");

    let mut update = default_update();
    update.llm_enabled = true;
    update.llm_api_key = Some("sk-should-be-ignored".to_string());
    update.privacy_mode = true;

    let view = SettingsView::from(&update);
    let new_client = state.rebuild_llm_client(&view);
    assert!(
        !new_client.is_enabled(),
        "privacy_mode=true must force disabled LlmClient even with a stored key"
    );

    update_settings_and_rebuild_llm(&state, &update);
    assert!(
        !state.llm_client().is_enabled(),
        "kernel llm_client must be disabled under privacy_mode=true"
    );
}

/// Test 4: llm_enabled=false 或无 key → rebuild_llm_client 返回 disabled。
#[test]
fn disabled_llm_when_api_key_missing_or_llm_enabled_false() {
    let state = new_test_state().expect("new test state");

    // Case A: llm_enabled=false → disabled(即使 store 中有 key)。
    {
        let mut update = default_update();
        update.llm_enabled = false;
        update.llm_api_key = Some("sk-test".to_string());
        update_settings_and_rebuild_llm(&state, &update);
        assert!(
            !state.llm_client().is_enabled(),
            "llm_enabled=false must yield disabled LlmClient even with a stored key"
        );
    }

    // Case B: store 无 key → disabled(即使 llm_enabled=true)。
    {
        let mut update = default_update();
        update.llm_enabled = true;
        update.clear_llm_api_key = true;
        update_settings_and_rebuild_llm(&state, &update);
        assert!(
            !state.llm_client().is_enabled(),
            "no stored key must yield disabled LlmClient even with llm_enabled=true"
        );
        assert!(
            state.kernel.llm_api_key().expect("read store").is_none(),
            "clear_llm_api_key must remove the stored key"
        );
    }
}

/// 启动路径等价物：只持久化不 rebuild（模拟“配置过但重启后”），
/// 启动重建入口必须让路由重新可用。
#[test]
fn startup_rebuild_from_persisted_settings_enables_llm() {
    use voicepilot_ui::settings_commands::startup_rebuild_llm;
    let state = new_test_state().expect("state");
    let mut update = default_update();
    update.llm_enabled = true;
    update.llm_api_key = Some("sk-test-startup".to_string());
    update_settings(&state, &update).expect("persist");
    assert!(state.kernel.llm_client().is_none());
    startup_rebuild_llm(&state);
    let client = state
        .kernel
        .llm_client()
        .expect("client after startup rebuild");
    assert!(client.is_enabled());
}
