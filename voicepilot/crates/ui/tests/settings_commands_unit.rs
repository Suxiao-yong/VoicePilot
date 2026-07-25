#![cfg(feature = "tauri")]

//! settings_commands 单元测试 —— get_settings/update_settings Tauri command 逻辑。

use voicepilot_ui::settings_commands::{SettingsDto, flatten_to_kv, merge_from_kv};

#[test]
fn settings_dto_default_has_sensible_values() {
    let dto = SettingsDto::default();
    // W6b-3b Fix 3:voice_model_path 默认空字符串(用 ModelRegistry 解析默认模型),
    // tts_model_path 默认空字符串(未配置时 tts_command 返回友好错误)。
    assert_eq!(dto.voice_model_path, "");
    assert_eq!(dto.tts_model_path, "");
    assert_eq!(dto.voice_threads, 4);
    assert!(!dto.privacy_mode); // 默认隐私模式关闭
    assert_eq!(dto.compensation_ttl_hours, 24);
    assert!(dto.tts_enabled); // VP-FR-002 默认开启
}

#[test]
fn settings_dto_roundtrip_through_kv() {
    let dto = SettingsDto {
        voice_model_path: "/models/base.bin".to_string(),
        voice_language: Some("zh".to_string()),
        voice_threads: 8,
        vad_energy_threshold: 150.0,
        vad_max_silence_ms: 800,
        vad_min_speech_ms: 300,
        voice_max_duration_ms: 60000,
        voice_chunk_duration_ms: 750,
        privacy_mode: true,
        compensation_ttl_hours: 48,
        tts_enabled: false,
        tts_model_path: "/models/tts-test".to_string(),
        // W7: 新增 5 字段用 Default 填充,本测试只验证原有字段往返。
        ..SettingsDto::default()
    };
    let kv = flatten_to_kv(&dto);
    assert!(kv.iter().any(|(k, _)| k == "voice.model_path"));
    assert!(kv.iter().any(|(k, _)| k == "voice.threads"));
    assert!(kv.iter().any(|(k, _)| k == "privacy.mode"));
    assert!(kv.iter().any(|(k, _)| k == "tts.enabled"));
    assert!(kv.iter().any(|(k, _)| k == "tts.model_path"));
    // W7: 验证 LLM KV 也被展平
    assert!(kv.iter().any(|(k, _)| k == "llm.enabled"));
    assert!(kv.iter().any(|(k, _)| k == "llm.api_key"));
    let restored = merge_from_kv(&kv).expect("merge");
    assert_eq!(restored.voice_model_path, "/models/base.bin");
    assert_eq!(restored.voice_threads, 8);
    assert!(restored.privacy_mode);
    assert_eq!(restored.compensation_ttl_hours, 48);
    assert_eq!(restored.tts_enabled, false);
    assert_eq!(restored.tts_model_path, "/models/tts-test");
}

#[test]
fn settings_merge_from_partial_kv_uses_defaults_for_missing() {
    let kv = vec![("voice.threads".to_string(), "16".to_string())];
    let dto = merge_from_kv(&kv).expect("merge");
    assert_eq!(dto.voice_threads, 16);
    // W6b-3b Fix 3:缺失 voice.model_path 时默认空字符串(而非模型名)。
    assert_eq!(dto.voice_model_path, "");
    assert_eq!(dto.compensation_ttl_hours, 24);
}
