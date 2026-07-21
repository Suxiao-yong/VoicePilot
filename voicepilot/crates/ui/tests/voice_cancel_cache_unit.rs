#![cfg(feature = "voice")]

use std::sync::atomic::{AtomicBool, Ordering};
use trust_kernel::voice::error::VoiceResult;
use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::voice_commands::{voice_listen, VoiceListen, VoiceListenOutcome, VoiceListenResult};

struct CancelAwareMock {
    cancel_check_count: std::sync::atomic::AtomicUsize,
}

impl VoiceListen for CancelAwareMock {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome> {
        self.cancel_check_count.fetch_add(1, Ordering::SeqCst);
        if cancel.load(Ordering::SeqCst) {
            return Ok(VoiceListenOutcome::NoSpeech);
        }
        Ok(VoiceListenOutcome::Success {
            transcription: "hello".to_string(),
            route_outcome: RouteTextResult::Empty,
            stopped_by_vad: true,
        })
    }
}

#[test]
fn voice_listen_returns_no_speech_when_cancel_set() {
    let mock = CancelAwareMock {
        cancel_check_count: Default::default(),
    };
    let cancel = AtomicBool::new(true);
    let result = voice_listen(&mock, &cancel);
    match result {
        VoiceListenResult::NoSpeech => {}
        other => panic!("expected NoSpeech, got {:?}", other),
    }
}

#[test]
fn voice_listen_returns_success_when_cancel_not_set() {
    let mock = CancelAwareMock {
        cancel_check_count: Default::default(),
    };
    let cancel = AtomicBool::new(false);
    let result = voice_listen(&mock, &cancel);
    match result {
        VoiceListenResult::Success { .. } => {}
        other => panic!("expected Success, got {:?}", other),
    }
}
