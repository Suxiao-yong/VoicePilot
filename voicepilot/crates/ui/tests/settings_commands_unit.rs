#![cfg(feature = "tauri")]

//! settings_commands 单元测试 —— get_settings/update_settings Tauri command 逻辑。

use voicepilot_ui::settings_commands::{SettingsDto, flatten_to_kv, merge_from_kv};

#[test]
fn settings_dto_default_has_sensible_values() {
    let dto = SettingsDto::default();
    assert_eq!(dto.voice_model_path, "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17");
    assert_eq!(dto.voice_threads, 4);
    assert!(!dto.privacy_mode); // 默认隐私模式关闭
    assert_eq!(dto.compensation_ttl_hours, 24);
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
    };
    let kv = flatten_to_kv(&dto);
    assert!(kv.iter().any(|(k, _)| k == "voice.model_path"));
    assert!(kv.iter().any(|(k, _)| k == "voice.threads"));
    assert!(kv.iter().any(|(k, _)| k == "privacy.mode"));
    let restored = merge_from_kv(&kv).expect("merge");
    assert_eq!(restored.voice_model_path, "/models/base.bin");
    assert_eq!(restored.voice_threads, 8);
    assert!(restored.privacy_mode);
    assert_eq!(restored.compensation_ttl_hours, 48);
}

#[test]
fn settings_merge_from_partial_kv_uses_defaults_for_missing() {
    let kv = vec![("voice.threads".to_string(), "16".to_string())];
    let dto = merge_from_kv(&kv).expect("merge");
    assert_eq!(dto.voice_threads, 16);
    assert_eq!(dto.voice_model_path, "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17");
    assert_eq!(dto.compensation_ttl_hours, 24);
}
