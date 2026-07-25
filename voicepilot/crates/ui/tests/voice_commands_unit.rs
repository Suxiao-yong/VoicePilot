#![cfg(feature = "voice")]

//! voice_listen 函数单元测试 —— 使用 MockVoiceListen,不实际录音/转写。
//! 验证 VoiceListenOutcome → VoiceListenResult 转换逻辑。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use trust_kernel::voice::error::{VoiceError, VoiceResult};
use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::voice_commands::{
    voice_listen, VoiceListen, VoiceListenOutcome, VoiceListenResult,
};

/// MockVoiceListen —— 返回预设的 outcome 或 error,用于测试 voice_listen 函数。
struct MockVoiceListen {
    outcome: Mutex<Option<VoiceResult<VoiceListenOutcome>>>,
    call_count: AtomicUsize,
}

impl MockVoiceListen {
    fn with_outcome(outcome: VoiceResult<VoiceListenOutcome>) -> Self {
        Self {
            outcome: Mutex::new(Some(outcome)),
            call_count: AtomicUsize::new(0),
        }
    }
}

impl VoiceListen for MockVoiceListen {
    fn listen(&self, _cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut guard = self.outcome.lock().unwrap();
        guard.take().unwrap_or_else(|| {
            Err(VoiceError::InferenceFailed(
                "mock exhausted — no more outcomes".to_string(),
            ))
        })
    }
}

#[test]
fn voice_listen_returns_success_when_transcription_present() {
    let mock = MockVoiceListen::with_outcome(Ok(VoiceListenOutcome::Success {
        transcription: "整理下载目录".to_string(),
        route_outcome: RouteTextResult::Routed {
            skill_id: "files.organize".to_string(),
            // W7: Routed 加 slots 字段(voice 路径不调 LLM,此处置空 Vec)。
            slots: vec![],
        },
        stopped_by_vad: true,
    }));

    let result = voice_listen(&mock, &AtomicBool::new(false));

    match result {
        VoiceListenResult::Success {
            transcription,
            route_outcome,
            stopped_by_vad,
        } => {
            assert_eq!(transcription, "整理下载目录");
            assert!(matches!(
                route_outcome,
                RouteTextResult::Routed { ref skill_id, .. } if skill_id == "files.organize"
            ));
            assert!(stopped_by_vad);
        }
        other => panic!("expected Success, got {:?}", other),
    }
}

#[test]
fn voice_listen_returns_no_speech_when_transcription_none_and_vad_stopped() {
    let mock = MockVoiceListen::with_outcome(Ok(VoiceListenOutcome::NoSpeech));

    let result = voice_listen(&mock, &AtomicBool::new(false));
    assert!(
        matches!(result, VoiceListenResult::NoSpeech),
        "expected NoSpeech, got {:?}",
        result
    );
}

#[test]
fn voice_listen_returns_timeout_when_transcription_none_and_not_vad_stopped() {
    let mock = MockVoiceListen::with_outcome(Ok(VoiceListenOutcome::Timeout {
        transcription: None,
        route_outcome: RouteTextResult::Empty,
    }));

    let result = voice_listen(&mock, &AtomicBool::new(false));
    match result {
        VoiceListenResult::Timeout {
            transcription,
            route_outcome,
        } => {
            assert!(transcription.is_none());
            assert!(matches!(route_outcome, RouteTextResult::Empty));
        }
        other => panic!("expected Timeout, got {:?}", other),
    }
}

#[test]
fn voice_listen_returns_timeout_with_transcription_when_available() {
    let mock = MockVoiceListen::with_outcome(Ok(VoiceListenOutcome::Timeout {
        transcription: Some("部分转录文本".to_string()),
        route_outcome: RouteTextResult::Unmatched {
            text: "部分转录文本".to_string(),
        },
    }));

    let result = voice_listen(&mock, &AtomicBool::new(false));
    match result {
        VoiceListenResult::Timeout {
            transcription: Some(t),
            route_outcome,
        } => {
            assert_eq!(t, "部分转录文本");
            assert!(matches!(route_outcome, RouteTextResult::Unmatched { .. }));
        }
        other => panic!("expected Timeout with transcription, got {:?}", other),
    }
}

#[test]
fn voice_listen_returns_error_when_listener_fails() {
    let mock = MockVoiceListen::with_outcome(Err(VoiceError::ModelMissing(
        "ggml-tiny.bin".to_string(),
    )));

    let result = voice_listen(&mock, &AtomicBool::new(false));
    match result {
        VoiceListenResult::Error { message } => {
            assert!(message.contains("ggml-tiny.bin"));
            assert!(message.contains("model missing"));
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn voice_listen_calls_listener_exactly_once() {
    let mock = MockVoiceListen::with_outcome(Ok(VoiceListenOutcome::NoSpeech));

    let _ = voice_listen(&mock, &AtomicBool::new(false));

    assert_eq!(mock.call_count.load(Ordering::SeqCst), 1);
}

// ===== 任务 3: build_transcription_final_payload + TranscriptionFinalPayload 测试 =====

use voicepilot_ui::voice_commands::{
    build_transcription_final_payload, TranscriptionFinalPayload,
};

#[test]
fn build_payload_returns_some_for_success_result() {
    let result = VoiceListenResult::Success {
        transcription: "整理下载目录".to_string(),
        route_outcome: RouteTextResult::Routed {
            skill_id: "files.organize".to_string(),
            // W7: Routed 加 slots 字段(voice 路径不调 LLM,此处置空 Vec)。
            slots: vec![],
        },
        stopped_by_vad: true,
    };

    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_some());
    let p = payload.unwrap();
    assert_eq!(p.transcription, "整理下载目录");
    assert!(matches!(
        p.route_outcome,
        RouteTextResult::Routed { ref skill_id, .. } if skill_id == "files.organize"
    ));
    assert!(p.stopped_by_vad);
}

#[test]
fn build_payload_returns_some_for_timeout_with_transcription() {
    let result = VoiceListenResult::Timeout {
        transcription: Some("部分文本".to_string()),
        route_outcome: RouteTextResult::Empty,
    };

    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_some());
    let p = payload.unwrap();
    assert_eq!(p.transcription, "部分文本");
    assert!(!p.stopped_by_vad);
}

#[test]
fn build_payload_returns_none_for_no_speech() {
    let result = VoiceListenResult::NoSpeech;
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_none());
}

#[test]
fn build_payload_returns_none_for_error() {
    let result = VoiceListenResult::Error {
        message: "model missing".to_string(),
    };
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_none());
}

#[test]
fn build_payload_returns_none_for_timeout_without_transcription() {
    let result = VoiceListenResult::Timeout {
        transcription: None,
        route_outcome: RouteTextResult::Empty,
    };
    let payload = build_transcription_final_payload(&result);
    assert!(payload.is_none());
}

#[test]
fn transcription_final_payload_is_serializable() {
    let payload = TranscriptionFinalPayload {
        transcription: "test".to_string(),
        route_outcome: RouteTextResult::Routed {
            skill_id: "files.organize".to_string(),
            // W7: Routed 加 slots 字段(voice 路径不调 LLM,此处置空 Vec)。
            slots: vec![],
        },
        stopped_by_vad: true,
        slots: vec![],
    };
    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains("\"transcription\":\"test\""));
    assert!(json.contains("\"stopped_by_vad\":true"));
}
