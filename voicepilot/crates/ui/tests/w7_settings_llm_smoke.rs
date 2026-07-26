//! W7 Plan 6 Task 2: Settings LLM UI E2E 冒烟测试。
//!
//! 覆盖 4 个场景:DTO LLM 字段 flatten/merge 往返 / update_settings 持久化 + rebuild_llm_client /
//! privacy_mode=true 强制 disabled / api_key 空 or llm_enabled=false 强制 disabled。
//!
//! 与 settings_commands_unit.rs 的区别:本测试覆盖 LLM 5 字段(roundtrip)+ AppState.rebuild_llm_client
//! 真实调用(验证 LlmClient::is_enabled() 布尔结果,而非仅 KV 序列化)。
//!
//! 注意:纯逻辑函数 `update_settings` 只持久化 KV,不重建 LlmClient 缓存
//! (见 settings_commands.rs:164-171)。Tauri command 包装 `update_settings_command`
//! 才调 `rebuild_llm_client` + 写缓存。本测试镜像 command 函数体
//! (与 settings_commands_unit.rs::update_settings_syncs_allowed_apps_to_kernel 同模式),
//! 以便在 unit test 中验证端到端契约(无法构造 `tauri::State<'_, AppState>`)。

#![cfg(all(feature = "tauri", feature = "llm"))]

use voicepilot_ui::settings_commands::{
    flatten_to_kv, get_settings, merge_from_kv, update_settings, SettingsDto,
};
use voicepilot_ui::state::AppState;

/// 镜像 `update_settings_command` 函数体:纯 `update_settings` 只持久化 KV,
/// command 包装额外刷新 serving-applied LlmClient 缓存。unit test 无法构造
/// `tauri::State`,故手动调 `rebuild_llm_client` + 写缓存。
fn update_settings_and_rebuild_llm(state: &AppState, settings: &SettingsDto) {
    update_settings(state, settings).expect("update_settings");
    let new_llm = state.rebuild_llm_client(settings);
    *state.llm_client.lock().unwrap() = Some(new_llm);
}

/// Test 1: SettingsDto LLM 5 字段 + privacy_mode 通过 flatten_to_kv → merge_from_kv 往返一致。
#[test]
fn settings_dto_llm_fields_flatten_merge_roundtrip() {
    let dto = SettingsDto {
        llm_enabled: true,
        llm_api_key: "sk-test123".to_string(),
        llm_base_url: "https://api.deepseek.com/v1".to_string(),
        llm_model: "deepseek-chat".to_string(),
        llm_provider_url: "https://platform.deepseek.com/api_keys".to_string(),
        privacy_mode: false,
        ..Default::default()
    };

    let kv = flatten_to_kv(&dto);
    let got = merge_from_kv(&kv).expect("merge_from_kv should succeed for valid LLM KV");

    assert_eq!(got.llm_enabled, dto.llm_enabled);
    assert_eq!(got.llm_api_key, dto.llm_api_key);
    assert_eq!(got.llm_base_url, dto.llm_base_url);
    assert_eq!(got.llm_model, dto.llm_model);
    assert_eq!(got.llm_provider_url, dto.llm_provider_url);
    assert_eq!(got.privacy_mode, dto.privacy_mode);
}

/// Test 2: update_settings 持久化 KV + rebuild_llm_client 使 state.llm_client() 反映新配置。
#[test]
fn update_settings_persists_and_rebuilds_llm_client() {
    let state = AppState::new_in_memory().expect("AppState::new_in_memory");

    // 初始状态:llm_client 缓存为 None → llm_client() 返回 disabled()。
    assert!(
        !state.llm_client().is_enabled(),
        "fresh AppState should have disabled LlmClient"
    );

    let dto = SettingsDto {
        llm_enabled: true,
        llm_api_key: "sk-persisted".to_string(),
        privacy_mode: false,
        ..Default::default()
    };

    // 镜像 update_settings_command 函数体:持久化 KV + 重建并缓存 LlmClient。
    update_settings_and_rebuild_llm(&state, &dto);

    // serving-applied LlmClient 应为 enabled(api_key + base_url 均非空)。
    let llm_client = state.llm_client();
    assert!(
        llm_client.is_enabled(),
        "llm_client should be enabled after update_settings with valid config"
    );

    // accepted/persisted KV 应能通过 get_settings 读回相同值。
    let retrieved = get_settings(&state).expect("get_settings");
    assert_eq!(retrieved.llm_api_key, "sk-persisted");
    assert!(retrieved.llm_enabled);

    // 验证 KV 确实落盘(而非 get_settings 返回 Default)。
    let kv = flatten_to_kv(&retrieved);
    let api_key_kv = kv
        .iter()
        .find(|(k, _)| k == "llm.api_key")
        .map(|(_, v)| v.clone())
        .expect("llm.api_key should be in KV");
    assert_eq!(api_key_kv, "sk-persisted");
}

/// Test 3: privacy_mode=true 强制 rebuild_llm_client 返回 disabled(覆盖 llm_enabled+api_key)。
#[test]
fn privacy_mode_true_returns_disabled_llm_client() {
    let state = AppState::new_in_memory().expect("AppState::new_in_memory");

    let dto = SettingsDto {
        llm_enabled: true,
        llm_api_key: "sk-should-be-ignored".to_string(),
        privacy_mode: true,
        ..Default::default()
    };

    // 直接调 rebuild_llm_client:privacy_mode 胜过 llm_enabled + api_key。
    let new_client = state.rebuild_llm_client(&dto);
    assert!(
        !new_client.is_enabled(),
        "privacy_mode=true must force disabled LlmClient even with valid api_key"
    );

    // 通过 update_settings + cache 刷新镜像 command body,验证缓存态一致。
    update_settings_and_rebuild_llm(&state, &dto);
    assert!(
        !state.llm_client().is_enabled(),
        "cached llm_client must be disabled under privacy_mode=true"
    );
}

/// Test 4: llm_enabled=false OR llm_api_key 空 → rebuild_llm_client 返回 disabled。
#[test]
fn disabled_llm_when_api_key_empty_or_llm_enabled_false() {
    let state = AppState::new_in_memory().expect("AppState::new_in_memory");

    // Case A: llm_enabled=false → disabled(即使 api_key 非空)。
    let dto_a = SettingsDto {
        llm_enabled: false,
        llm_api_key: "sk-test".to_string(),
        privacy_mode: false,
        ..Default::default()
    };
    let client_a = state.rebuild_llm_client(&dto_a);
    assert!(
        !client_a.is_enabled(),
        "llm_enabled=false must yield disabled LlmClient even with non-empty api_key"
    );

    // Case B: llm_api_key 空 → disabled(即使 llm_enabled=true)。
    let dto_b = SettingsDto {
        llm_enabled: true,
        llm_api_key: String::new(),
        privacy_mode: false,
        ..Default::default()
    };
    let client_b = state.rebuild_llm_client(&dto_b);
    assert!(
        !client_b.is_enabled(),
        "empty llm_api_key must yield disabled LlmClient even with llm_enabled=true"
    );
}
