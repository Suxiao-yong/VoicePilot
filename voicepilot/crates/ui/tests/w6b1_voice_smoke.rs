#![cfg(feature = "voice")]

//! W6b-1 端到端冒烟测试 —— V1.1 §8.2 Main Chat 语音输入管道。
//!
//! 用 mock VoiceListen 验证 voice_listen 函数的编排逻辑:
//! - Success → VoiceListenResult::Success
//! - NoSpeech → VoiceListenResult::NoSpeech
//! - Timeout with transcription → VoiceListenResult::Timeout
//! - Error → VoiceListenResult::Error
//! - build_transcription_final_payload 在 Success/Timeout-with-transcription 时返回 Some
//!
//! 不实际录音(需麦克风 + 模型),实际录音测试标 #[ignore] 在 voice_integration.rs 中。

use std::sync::Mutex;

use trust_kernel::voice::error::{VoiceError, VoiceResult};
use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::voice_commands::{
    build_transcription_final_payload, voice_listen, VoiceListen, VoiceListenOutcome,
    VoiceListenResult,
};

struct StubVoiceListen {
    outcome: Mutex<Option<VoiceResult<VoiceListenOutcome>>>,
}

impl StubVoiceListen {
    fn new(outcome: VoiceResult<VoiceListenOutcome>) -> Self {
        Self {
            outcome: Mutex::new(Some(outcome)),
        }
    }
}

impl VoiceListen for StubVoiceListen {
    fn listen(&self) -> VoiceResult<VoiceListenOutcome> {
        self.outcome.lock().unwrap().take().unwrap_or_else(|| {
            Err(VoiceError::InferenceFailed("stub exhausted".to_string()))
        })
    }
}

/// §11.1 W6b-1 gate:voice_listen 完整管道(Success 路径)。
#[test]
fn w6b1_smoke_voice_listen_success_returns_result_with_transcription() {
    let stub = StubVoiceListen::new(Ok(VoiceListenOutcome::Success {
        transcription: "整理下载目录里的 PDF".to_string(),
        route_outcome: RouteTextResult::Routed {
            skill_id: "files.organize".to_string(),
        },
        stopped_by_vad: true,
    }));

    let result = voice_listen(&stub);

    // 验证:VoiceListenResult::Success
    let transcription = match &result {
        VoiceListenResult::Success {
            transcription,
            stopped_by_vad: true,
            ..
        } => transcription.clone(),
        other => panic!("expected Success with stopped_by_vad=true, got {:?}", other),
    };
    assert_eq!(transcription, "整理下载目录里的 PDF");

    // 验证:transcription-final payload 构造正确
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_some(), "payload should be Some for Success");
    let payload = payload.unwrap();
    assert_eq!(payload.transcription, "整理下载目录里的 PDF");
    assert!(payload.stopped_by_vad);
}

/// §11.1 W6b-1 gate:voice_listen NoSpeech 路径。
#[test]
fn w6b1_smoke_voice_listen_no_speech_returns_no_speech_result() {
    let stub = StubVoiceListen::new(Ok(VoiceListenOutcome::NoSpeech));

    let result = voice_listen(&stub);

    assert!(
        matches!(result, VoiceListenResult::NoSpeech),
        "expected NoSpeech, got {:?}",
        result
    );

    // NoSpeech 不应发射 transcription-final 事件
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_none(), "payload should be None for NoSpeech");
}

/// §11.1 W6b-1 gate:voice_listen Timeout(含 transcription)路径。
#[test]
fn w6b1_smoke_voice_listen_timeout_with_transcription_returns_timeout_result() {
    let stub = StubVoiceListen::new(Ok(VoiceListenOutcome::Timeout {
        transcription: Some("用户持续说话".to_string()),
        route_outcome: RouteTextResult::Unmatched {
            text: "用户持续说话".to_string(),
        },
    }));

    let result = voice_listen(&stub);

    match &result {
        VoiceListenResult::Timeout {
            transcription: Some(t),
            route_outcome,
        } => {
            assert_eq!(t, "用户持续说话");
            assert!(matches!(route_outcome, RouteTextResult::Unmatched { .. }));
        }
        other => panic!("expected Timeout with transcription, got {:?}", other),
    }

    // Timeout with transcription 应发射事件
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_some());
    let payload = payload.unwrap();
    assert_eq!(payload.transcription, "用户持续说话");
    assert!(!payload.stopped_by_vad);
}

/// §11.1 W6b-1 gate:voice_listen Timeout(无 transcription)路径。
#[test]
fn w6b1_smoke_voice_listen_timeout_without_transcription_returns_timeout_result() {
    let stub = StubVoiceListen::new(Ok(VoiceListenOutcome::Timeout {
        transcription: None,
        route_outcome: RouteTextResult::Empty,
    }));

    let result = voice_listen(&stub);

    assert!(
        matches!(
            result,
            VoiceListenResult::Timeout {
                transcription: None,
                ..
            }
        ),
        "expected Timeout without transcription, got {:?}",
        result
    );

    // Timeout without transcription 不应发射事件
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_none());
}

/// §11.1 W6b-1 gate:voice_listen Error 路径(模型缺失)。
#[test]
fn w6b1_smoke_voice_listen_model_missing_returns_error_result() {
    let stub = StubVoiceListen::new(Err(VoiceError::ModelMissing("ggml-tiny.bin".to_string())));

    let result = voice_listen(&stub);

    match &result {
        VoiceListenResult::Error { message } => {
            assert!(message.contains("ggml-tiny.bin"));
            assert!(message.contains("model missing"));
        }
        other => panic!("expected Error, got {:?}", other),
    }

    // Error 不应发射事件
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_none());
}

/// §11.1 W6b-1 gate:voice_listen Error 路径(麦克风拒绝)。
#[test]
fn w6b1_smoke_voice_listen_mic_denied_returns_error_result() {
    let stub = StubVoiceListen::new(Err(VoiceError::MicDenied));

    let result = voice_listen(&stub);

    match &result {
        VoiceListenResult::Error { message } => {
            assert!(message.contains("microphone"));
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

/// §11.1 W6b-1 gate:VoiceListenResult 所有变体可序列化(供 Tauri IPC 传输)。
#[test]
fn w6b1_smoke_all_voice_listen_result_variants_serialize_to_json() {
    let cases = vec![
        serde_json::to_string(&VoiceListenResult::Success {
            transcription: "test".to_string(),
            route_outcome: RouteTextResult::Routed {
                skill_id: "files.organize".to_string(),
            },
            stopped_by_vad: true,
        })
        .unwrap(),
        serde_json::to_string(&VoiceListenResult::NoSpeech).unwrap(),
        serde_json::to_string(&VoiceListenResult::Timeout {
            transcription: None,
            route_outcome: RouteTextResult::Empty,
        })
        .unwrap(),
        serde_json::to_string(&VoiceListenResult::Error {
            message: "test error".to_string(),
        })
        .unwrap(),
    ];

    // 所有序列化结果都应包含 "kind" 标签
    for json in &cases {
        assert!(json.contains("\"kind\""), "missing kind tag in: {}", json);
    }

    // 验证各 kind 标签正确
    assert!(cases[0].contains("\"kind\":\"success\""));
    assert!(cases[1].contains("\"kind\":\"no_speech\""));
    assert!(cases[2].contains("\"kind\":\"timeout\""));
    assert!(cases[3].contains("\"kind\":\"error\""));
}
