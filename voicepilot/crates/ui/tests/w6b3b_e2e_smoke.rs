#![cfg(feature = "voice")]

//! W6b-3b 端到端冒烟测试 —— V1.1 §8.4 SlotParser + §8.3 TTS Settings + Fix 1 wav_path。
//!
//! 验证 W6b-3b 关键集成路径:
//! - SlotParser 从混合转写文本提取 path/app/number + high_risk 标记
//! - build_transcription_final_payload 注入 slots 到 transcription-final 事件
//! - TTS settings(tts_enabled + tts_model_path)KV 往返持久化
//! - 默认 tts_enabled = true(VP-FR-002 默认开启语音反馈)
//! - TtsResult 包含 wav_path 字段(Fix 1:前端 <audio> 播放所需)
//!
//! 不实际录音/合成(需麦克风 + 模型),仅验证数据流与序列化契约。

use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::settings_commands::{SettingsView, flatten_to_kv, merge_from_kv};
use voicepilot_ui::slot_parser::{SlotKind, SlotParser};
use voicepilot_ui::voice_commands::{VoiceListenResult, build_transcription_final_payload};

/// §8.4 SlotParser 从混合文本提取 path / app / number 三类 Slot。
/// 输入:"打开 notepad 整理 C:\\temp 5 个文件"
/// 期望:至少 1 个 App slot(notepad)+ 1 个 Path slot(C:\\temp)+ 1 个 Number slot(5)。
#[test]
fn slot_parser_extracts_path_app_number_mixed() {
    let text = "打开 notepad 整理 C:\\temp 5 个文件";
    let slots = SlotParser::parse(text);
    let has_app = slots
        .iter()
        .any(|s| s.kind == SlotKind::App && s.raw == "notepad");
    let has_path = slots
        .iter()
        .any(|s| s.kind == SlotKind::Path && s.raw == "C:\\temp");
    let has_number = slots
        .iter()
        .any(|s| s.kind == SlotKind::Number && s.raw == "5");
    assert!(has_app, "expected App slot 'notepad', got {:?}", slots);
    assert!(has_path, "expected Path slot 'C:\\temp', got {:?}", slots);
    assert!(has_number, "expected Number slot '5', got {:?}", slots);
}

/// §8.4 SlotParser 把 path / recipient / delete_target 标记为 high_risk,
/// app / number 标记为非 high_risk(用于 UI 强制视觉确认)。
#[test]
fn slot_parser_marks_high_risk_flags() {
    // path slot 应为 high_risk
    let slots = SlotParser::parse("整理 C:\\Users\\test 的图片");
    let path_slot = slots
        .iter()
        .find(|s| s.kind == SlotKind::Path)
        .expect("expected a Path slot");
    assert!(path_slot.high_risk, "Path slot should be high_risk");

    // recipient slot 应为 high_risk
    let slots = SlotParser::parse("发送给 alice@example.com 报告");
    let rcp_slot = slots
        .iter()
        .find(|s| s.kind == SlotKind::Recipient)
        .expect("expected a Recipient slot");
    assert!(rcp_slot.high_risk, "Recipient slot should be high_risk");

    // delete_target slot 应为 high_risk
    let slots = SlotParser::parse("删除 test.txt");
    let dt_slot = slots
        .iter()
        .find(|s| s.kind == SlotKind::DeleteTarget)
        .expect("expected a DeleteTarget slot");
    assert!(dt_slot.high_risk, "DeleteTarget slot should be high_risk");

    // app slot 应为非 high_risk
    let slots = SlotParser::parse("打开 notepad");
    let app_slot = slots
        .iter()
        .find(|s| s.kind == SlotKind::App)
        .expect("expected an App slot");
    assert!(!app_slot.high_risk, "App slot should NOT be high_risk");

    // number slot 应为非 high_risk
    let slots = SlotParser::parse("整理 5 个文件");
    let num_slot = slots
        .iter()
        .find(|s| s.kind == SlotKind::Number)
        .expect("expected a Number slot");
    assert!(!num_slot.high_risk, "Number slot should NOT be high_risk");
}

/// build_transcription_final_payload 在 Success 路径注入 slots
/// (transcription-final 事件携带 slots 供前端 Chip 修改 UI)。
#[test]
fn build_final_payload_includes_slots() {
    let result = VoiceListenResult::Success {
        transcription: "打开 notepad 整理 C:\\temp".to_string(),
        route_outcome: RouteTextResult::Routed {
            skill_id: "files.organize".to_string(),
            // W7: Routed 加 slots 字段(voice 路径不调 LLM,此处置空 Vec)。
            slots: vec![],
        },
        stopped_by_vad: true,
    };
    let payload = build_transcription_final_payload(&result)
        .expect("payload should be Some for Success with transcription");
    assert!(!payload.slots.is_empty(), "slots should not be empty");
    assert!(
        payload.slots.iter().any(|s| s.kind == SlotKind::Path),
        "expected Path slot in payload, got {:?}",
        payload.slots
    );
    assert!(
        payload.slots.iter().any(|s| s.kind == SlotKind::App),
        "expected App slot in payload, got {:?}",
        payload.slots
    );
}

/// §8.3 TTS settings(tts_enabled + tts_model_path)KV 往返持久化。
/// 修改后 flatten → merge 应保留自定义值。
#[test]
fn tts_settings_roundtrip() {
    let dto = SettingsView {
        tts_enabled: false,
        tts_model_path: "/models/custom-tts".to_string(),
        ..Default::default()
    };
    let kv = flatten_to_kv(&dto);
    let restored = merge_from_kv(&kv).expect("merge should succeed");
    assert!(!restored.tts_enabled);
    assert_eq!(restored.tts_model_path, "/models/custom-tts");
}

/// VP-FR-002 默认 tts_enabled = true(语音反馈默认开启)。
#[test]
fn default_tts_enabled_is_true() {
    let dto = SettingsView::default();
    assert!(
        dto.tts_enabled,
        "tts_enabled should default to true (VP-FR-002)"
    );
}

/// Fix 1: TtsResult 包含 wav_path 字段(前端 <audio> 播放所需)。
/// 序列化后的 JSON 应包含 "wav_path" 键,且 Some(path) 时值非 null。
#[test]
fn tts_result_includes_wav_path() {
    use voicepilot_ui::voice_commands::TtsResult;

    // Some(path) 路径:JSON 应包含 wav_path 且值非 null
    let with_path = TtsResult {
        played: true,
        interrupted: false,
        sample_count: 1024,
        wav_path: Some("/tmp/voicepilot-tts/tts-123.wav".to_string()),
        error: None,
    };
    let json = serde_json::to_string(&with_path).expect("serialize TtsResult");
    assert!(
        json.contains("\"wav_path\""),
        "json should contain wav_path field, got: {}",
        json
    );
    assert!(
        json.contains("\"/tmp/voicepilot-tts/tts-123.wav\""),
        "wav_path value should be the path string, got: {}",
        json
    );

    // None 路径:JSON 中 wav_path 应为 null
    let without_path = TtsResult {
        played: false,
        interrupted: false,
        sample_count: 0,
        wav_path: None,
        error: Some("tts disabled".to_string()),
    };
    let json_none = serde_json::to_string(&without_path).expect("serialize TtsResult");
    assert!(
        json_none.contains("\"wav_path\":null"),
        "wav_path should serialize to null when None, got: {}",
        json_none
    );
}
