#![cfg(feature = "voice")]

//! VoiceListener 单元测试 —— 使用 MockVoiceRecorder,不实际录音。
//! 验证 VAD-based 自动停止逻辑(issue #45)。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use trust_kernel::voice::error::VoiceResult;
use trust_kernel::voice::listener::{ListenOutcome, VoiceListener, VoiceRecorder};
use trust_kernel::voice::vad::{VadConfig, VadDetector};

/// 生成正弦波样本(模拟语音,能量高于 VAD 阈值)。
/// amplitude=5000 时 RMS ≈ 3535,远高于默认阈值 100。
fn generate_sine_wave(duration_ms: u32, sample_rate: u32, freq: f32) -> Vec<i16> {
    let n_samples = (duration_ms as f32 / 1000.0 * sample_rate as f32) as usize;
    (0..n_samples)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (5000.0 * (2.0 * std::f32::consts::PI * freq * t).sin()) as i16
        })
        .collect()
}

/// 生成静音样本(全零,RMS=0,低于 VAD 阈值)。
fn generate_silence(duration_ms: u32, sample_rate: u32) -> Vec<i16> {
    let n_samples = (duration_ms as f32 / 1000.0 * sample_rate as f32) as usize;
    vec![0i16; n_samples]
}

/// MockVoiceRecorder —— 按顺序返回预设的音频块,用于测试 VoiceListener 编排逻辑。
struct MockVoiceRecorder {
    chunks: Vec<Vec<i16>>,
    call_count: AtomicUsize,
}

impl MockVoiceRecorder {
    fn new(chunks: Vec<Vec<i16>>) -> Self {
        Self {
            chunks,
            call_count: AtomicUsize::new(0),
        }
    }
}

impl VoiceRecorder for MockVoiceRecorder {
    fn record_chunk(&self, _duration: Duration) -> VoiceResult<Vec<i16>> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        if idx >= self.chunks.len() {
            // 所有预设块已耗尽 —— 返回空,触发 VoiceListener 退出循环。
            return Ok(Vec::new());
        }
        Ok(self.chunks[idx].clone())
    }
}

#[test]
fn voice_listener_stops_on_silence_after_speech() {
    // 场景:500ms 语音 + 1000ms 静音(分 2 块,每块 750ms)。
    // VAD 默认 max_silence_ms=700,所以在第 2 块结束时,silence_frame_count
    // 会达到 35,触发 detect_end_of_speech 返回 Some。
    let chunk1 = {
        let mut v = generate_sine_wave(500, 16000, 200.0);
        v.extend(generate_silence(250, 16000));
        v
    };
    let chunk2 = generate_silence(750, 16000);

    let recorder = Arc::new(MockVoiceRecorder::new(vec![chunk1, chunk2]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(750),
    );

    let outcome = listener.listen().expect("listen should succeed");

    match outcome {
        ListenOutcome::SpeechEnded { samples } => {
            // speech_end_sample 应在静音超时触发点(第 60 帧 = 19200 样本)。
            // 允许 ±1 帧容差(因为 chunk 边界可能不在帧边界上)。
            assert!(
                samples.len() >= 19000 && samples.len() <= 19520,
                "expected ~19200 samples, got {}",
                samples.len()
            );
        }
        other => panic!("expected SpeechEnded, got {:?}", other),
    }
}

#[test]
fn voice_listener_returns_no_speech_when_only_silence() {
    // 场景:只有静音,VAD 永远不会触发,recorder 耗尽后退出。
    let chunk1 = generate_silence(750, 16000);
    let chunk2 = generate_silence(750, 16000);

    let recorder = Arc::new(MockVoiceRecorder::new(vec![chunk1, chunk2]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(750),
    );

    let outcome = listener.listen().expect("listen should succeed");
    assert!(
        matches!(outcome, ListenOutcome::NoSpeech),
        "expected NoSpeech, got {:?}",
        outcome
    );
}

#[test]
fn voice_listener_returns_timeout_when_continuous_speech_exceeds_max_duration() {
    // 场景:持续语音(无静音),max_duration=1.5s,chunk_duration=750ms。
    // 第 2 块后 elapsed=1.5s >= max_duration,退出循环。
    // detect() 在 end-of-audio 分支返回 Speech,所以是 Timeout(不是 NoSpeech)。
    let chunk1 = generate_sine_wave(750, 16000, 200.0);
    let chunk2 = generate_sine_wave(750, 16000, 200.0);

    let recorder = Arc::new(MockVoiceRecorder::new(vec![chunk1, chunk2]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_millis(1500),
        Duration::from_millis(750),
    );

    let outcome = listener.listen().expect("listen should succeed");
    match outcome {
        ListenOutcome::Timeout { samples } => {
            // max_duration=1500ms,所以最多录制 1500ms = 24000 样本。
            assert!(
                !samples.is_empty(),
                "timeout should have captured some samples"
            );
            assert!(
                samples.len() <= 24000,
                "expected at most 24000 samples (1.5s), got {}",
                samples.len()
            );
        }
        other => panic!("expected Timeout, got {:?}", other),
    }
}

#[test]
fn voice_listener_returns_no_speech_when_recorder_immediately_exhausted() {
    // 场景:recorder 立即返回空(mock chunks 为空),VoiceListener 应返回 NoSpeech。
    let recorder = Arc::new(MockVoiceRecorder::new(vec![]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(500),
    );

    let outcome = listener.listen().expect("listen should succeed");
    assert!(
        matches!(outcome, ListenOutcome::NoSpeech),
        "expected NoSpeech, got {:?}",
        outcome
    );
}

#[test]
fn voice_listener_stops_immediately_when_cancel_flag_set_before_chunk() {
    // cancel flag 在 listen 开始前就为 true,应立即返回 NoSpeech(无音频采集)。
    let recorder = Arc::new(MockVoiceRecorder::new(vec![
        generate_sine_wave(750, 16000, 200.0),
    ]));
    let vad = VadDetector::new(VadConfig::default());
    let listener = VoiceListener::new(
        recorder,
        vad,
        Duration::from_secs(30),
        Duration::from_millis(750),
    );
    let cancel = AtomicBool::new(true);
    let outcome = listener
        .listen_with_cancel(&cancel)
        .expect("listen should succeed");
    assert!(
        matches!(outcome, ListenOutcome::NoSpeech),
        "expected NoSpeech when cancel flag set, got {:?}",
        outcome
    );
}

#[test]
fn voice_listener_stops_midway_when_cancel_flag_set_after_first_chunk() {
    // 第 1 块正常录制,之后 cancel flag 置 true,第 2 块前退出循环。
    let cancel_arc = Arc::new(AtomicBool::new(false));

    struct CancelAfterFirst {
        inner: MockVoiceRecorder,
        cancel: Arc<AtomicBool>,
    }
    impl VoiceRecorder for CancelAfterFirst {
        fn record_chunk(&self, d: Duration) -> VoiceResult<Vec<i16>> {
            let r = self.inner.record_chunk(d)?;
            if !r.is_empty() {
                self.cancel.store(true, Ordering::SeqCst);
            }
            Ok(r)
        }
    }

    let wrapper = Arc::new(CancelAfterFirst {
        inner: MockVoiceRecorder::new(vec![
            generate_sine_wave(750, 16000, 200.0),
            generate_sine_wave(750, 16000, 200.0),
        ]),
        cancel: cancel_arc.clone(),
    });
    let listener = VoiceListener::new(
        wrapper,
        VadDetector::new(VadConfig::default()),
        Duration::from_secs(30),
        Duration::from_millis(750),
    );
    let outcome = listener
        .listen_with_cancel(&cancel_arc)
        .expect("listen");
    match outcome {
        ListenOutcome::Timeout { samples } => {
            assert!(!samples.is_empty(), "should have 1 chunk of samples");
        }
        ListenOutcome::SpeechEnded { .. } => {
            // 也可能 VAD 在 1 块内就触发(边界),可接受
        }
        other => panic!("expected Timeout or SpeechEnded on cancel, got {:?}", other),
    }
}
