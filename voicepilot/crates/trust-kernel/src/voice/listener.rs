//! VoiceListener —— VAD-based 录音编排器(W6b-1 issue #45)。
//!
//! 替换 W5 PoC 的 `AudioRecorder::record_with_timeout` 固定超时,
//! 用 VAD 检测静音后自动停止录音。
//!
//! 设计(选项 A:新增编排器,不破坏 W5 API):
//!   1. 循环调用 `VoiceRecorder::record_chunk` 录制短块(默认 500ms)
//!   2. 累积到 buffer,调用 `VadDetector::detect_end_of_speech` 检测静音超时
//!   3. 如果返回 `Some(SpeechSegment)`,截断 buffer 到 `speech_end_sample`,
//!      返回 `SpeechEnded`
//!   4. 如果 `elapsed >= max_duration`,退出循环,返回 `Timeout`(可能含语音)
//!   5. 如果 recorder 返回空(耗尽),退出循环,根据 `detect()` 判断
//!      是 `NoSpeech` 还是 `Timeout`
//!
//! `VoiceRecorder` trait 抽象录音,便于单元测试注入 mock。

use std::sync::Arc;
use std::time::Duration;

use crate::voice::audio::{AudioRecorder, AudioRecorderConfig};
#[allow(unused_imports)]
use crate::voice::error::{VoiceError, VoiceResult};
use crate::voice::vad::{VadDetector, VadOutcome};

/// 抽象录音接口 —— 生产用 `AudioRecorderAdapter`,测试用 mock。
pub trait VoiceRecorder: Send + Sync {
    /// 录制指定时长的音频块,返回 mono 16kHz i16 PCM 样本。
    fn record_chunk(&self, duration: Duration) -> VoiceResult<Vec<i16>>;
}

/// 生产用 `VoiceRecorder` 实现,封装 `AudioRecorder`。
pub struct AudioRecorderAdapter {
    inner: AudioRecorder,
}

impl std::fmt::Debug for AudioRecorderAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioRecorderAdapter").finish()
    }
}

impl AudioRecorderAdapter {
    /// 用给定的录音配置创建 adapter。
    pub fn new(config: AudioRecorderConfig) -> VoiceResult<Self> {
        Ok(Self {
            inner: AudioRecorder::new(config)?,
        })
    }
}

impl VoiceRecorder for AudioRecorderAdapter {
    fn record_chunk(&self, duration: Duration) -> VoiceResult<Vec<i16>> {
        self.inner.record_with_timeout(duration)
    }
}

/// VoiceListener 的监听结果。
#[derive(Debug, Clone)]
pub enum ListenOutcome {
    /// VAD 检测到语音段 + 静音超时,自动停止。
    /// `samples` 已截断到 `speech_end_sample`。
    SpeechEnded { samples: Vec<i16> },
    /// 没有检测到任何语音。
    NoSpeech,
    /// 达到 `max_duration` 或 recorder 耗尽,但 VAD 未触发。
    /// `samples` 可能包含语音(用户持续说话未停顿),调用方决定是否转写。
    Timeout { samples: Vec<i16> },
}

/// VAD-based 录音编排器。
pub struct VoiceListener {
    recorder: Arc<dyn VoiceRecorder>,
    vad: VadDetector,
    max_duration: Duration,
    chunk_duration: Duration,
}

/// Partial transcript callback 类型(W6b-2 issue #47)。
/// listener 在 listen 期间每 2s 调一次,传入当前累积的 PCM 样本。
pub type PartialCallback<'a> = Option<&'a dyn Fn(&[i16])>;

/// W10 Plan 3: voice listen 的时间戳对(spec §5.3 voice_started_at + first_partial_received_at)。
///
/// 由 `listen_with_cancel_partial_and_timings` 返回,caller 传给
/// `LatencyRecorder::record()` 写入 voice_latency_samples 表。
///
/// 两者都为 Some 时才计算 latency;若任一为 None,caller 跳过记录(no-op)。
///
/// **为何用 SystemTime 而非 Instant:** 与 `VoiceLatencyTiming` 保持一致,
/// 支持 `duration_since(UNIX_EPOCH)` 转 epoch ms(用于 started_at_ms 存储)。
#[derive(Debug, Clone, Default)]
pub struct ListenTimings {
    /// VAD 检测首个 voiced chunk 的 SystemTime(t0)。
    /// None = 整个 listen 期间未检测到语音。
    pub voice_started_at: Option<std::time::SystemTime>,
    /// 首个 partial transcript 回调的 SystemTime(t1)。
    /// None = 未传 partial_callback 或 2s 内未触发回调。
    pub first_partial_at: Option<std::time::SystemTime>,
}

impl VoiceListener {
    /// 创建 VoiceListener。
    ///
    /// 参数:
    /// - `recorder`: 录音器(生产用 `AudioRecorderAdapter`,测试用 mock)
    /// - `vad`: VAD 检测器(已配置好 `VadConfig`)
    /// - `max_duration`: 最大录音时长(兜底,防止 VAD 不触发时无限录音)
    /// - `chunk_duration`: 每次录制的块时长(影响 VAD 检测延迟 + 录音开销)
    pub fn new(
        recorder: Arc<dyn VoiceRecorder>,
        vad: VadDetector,
        max_duration: Duration,
        chunk_duration: Duration,
    ) -> Self {
        Self {
            recorder,
            vad,
            max_duration,
            chunk_duration,
        }
    }

    /// 开始监听,返回 `ListenOutcome`。
    ///
    /// 流程:
    /// 1. 循环 `record_chunk` 累积到 buffer
    /// 2. 每次累积后调用 `detect_end_of_speech`
    /// 3. `Some` → 截断 + 返回 `SpeechEnded`
    /// 4. `elapsed >= max_duration` → 退出循环
    /// 5. 退出循环后用 `detect()` 判断是 `Timeout` 还是 `NoSpeech`
    pub fn listen(&self) -> VoiceResult<ListenOutcome> {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        self.listen_with_cancel(&cancel)
    }

    /// 带 cancel flag 的 listen —— V1.1.2 issue #57 voice 取消机制。
    ///
    /// 循环开始前 + 每 chunk 录制前检查 cancel flag。若为 true,立即退出循环。
    /// 退出后用 `vad.detect()` 判断 Timeout(有语音)/ NoSpeech(无语音)。
    /// post-loop 逻辑与 `listen` 保持一致(`VadOutcome::Speech → Timeout` /
    /// `VadOutcome::NoSpeech → NoSpeech`)。
    ///
    /// W6b-2 Task 6:薄包装,委托给 `listen_with_cancel_and_partial(cancel, None)`。
    pub fn listen_with_cancel(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> VoiceResult<ListenOutcome> {
        self.listen_with_cancel_and_partial(cancel, None)
    }

    /// 带 cancel flag + partial transcript callback 的 listen(W6b-2 issue #47)。
    ///
    /// 与 `listen_with_cancel` 相同的循环逻辑,额外:
    /// - 维护 `last_partial_elapsed: Duration`(基于 chunk_duration 累积的"模拟时间")
    /// - VAD 检测后,若 `elapsed - last_partial_elapsed >= 2s` 且 `partial_callback` 为 `Some(cb)`,
    ///   调 `cb(&buffer)` 然后重置 `last_partial_elapsed = elapsed`
    ///
    /// 用累积 `elapsed` 而非 `Instant::now()`,便于单元测试注入 mock recorder
    /// (mock 瞬间返回,wall clock 不增加)。
    ///
    /// `partial_callback: Option<&dyn Fn(&[i16])>` —— listener 同步调用,无需 `Send + Sync`。
    pub fn listen_with_cancel_and_partial(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
        partial_callback: PartialCallback<'_>,
    ) -> VoiceResult<ListenOutcome> {
        use std::sync::atomic::Ordering;
        let mut buffer: Vec<i16> = Vec::new();
        let mut elapsed = Duration::ZERO;
        let mut last_partial_elapsed = Duration::ZERO;

        while elapsed < self.max_duration {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            let chunk = self.recorder.record_chunk(self.chunk_duration)?;
            if chunk.is_empty() {
                break;
            }
            buffer.extend_from_slice(&chunk);
            elapsed += self.chunk_duration;
            if let Some(segment) = self.vad.detect_end_of_speech(&buffer) {
                buffer.truncate(segment.speech_end_sample);
                return Ok(ListenOutcome::SpeechEnded { samples: buffer });
            }
            // W6b-2 issue #47:每 2s 发射一次 partial transcript callback
            if let Some(cb) = partial_callback {
                if elapsed - last_partial_elapsed >= Duration::from_secs(2) {
                    cb(&buffer);
                    last_partial_elapsed = elapsed;
                }
            }
        }

        match self.vad.detect(&buffer) {
            VadOutcome::Speech { speech_end_sample, .. } => {
                buffer.truncate(speech_end_sample);
                if buffer.is_empty() {
                    Ok(ListenOutcome::NoSpeech)
                } else {
                    Ok(ListenOutcome::Timeout { samples: buffer })
                }
            }
            VadOutcome::NoSpeech => Ok(ListenOutcome::NoSpeech),
        }
    }

    /// W10 Plan 3: 带 cancel + partial + 时间戳追踪的 listen(spec §5.3)。
    ///
    /// 与 `listen_with_cancel_and_partial` 相同的循环逻辑,额外:
    /// - 每个 chunk 录制后调 `vad.chunk_has_speech(&chunk)`,首次 true 时设置
    ///   `voice_started_at = Some(SystemTime::now())`(t0)
    /// - 首次 `partial_callback` 调用时设置 `first_partial_at = Some(SystemTime::now())`(t1)
    ///
    /// 返回 `(ListenOutcome, ListenTimings)`。caller 用 `LatencyRecorder::record(&timing)`
    /// 写入 voice_latency_samples 表(仅当 timings 两字段都为 Some 时才记录)。
    ///
    /// **为何用 SystemTime 而非 Instant:** 与 `VoiceLatencyTiming` 保持一致,
    /// 支持 `duration_since(UNIX_EPOCH)` 转 epoch ms(用于 started_at_ms 存储)。
    ///
    /// **向后兼容:** 现有 `listen` / `listen_with_cancel` / `listen_with_cancel_and_partial`
    /// 方法签名不变。新方法仅由需要延迟追踪的 caller(CLI voice listen / UI voice loop)使用。
    pub fn listen_with_cancel_partial_and_timings(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
        partial_callback: PartialCallback<'_>,
    ) -> VoiceResult<(ListenOutcome, ListenTimings)> {
        use std::sync::atomic::Ordering;
        use std::time::SystemTime;
        let mut buffer: Vec<i16> = Vec::new();
        let mut elapsed = Duration::ZERO;
        let mut last_partial_elapsed = Duration::ZERO;
        let mut timings = ListenTimings::default();

        while elapsed < self.max_duration {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            let chunk = self.recorder.record_chunk(self.chunk_duration)?;
            if chunk.is_empty() {
                break;
            }
            // W10 Plan 3: 检测首个 voiced chunk(t0)
            if timings.voice_started_at.is_none() && self.vad.chunk_has_speech(&chunk) {
                timings.voice_started_at = Some(SystemTime::now());
            }
            buffer.extend_from_slice(&chunk);
            elapsed += self.chunk_duration;
            if let Some(segment) = self.vad.detect_end_of_speech(&buffer) {
                buffer.truncate(segment.speech_end_sample);
                return Ok((ListenOutcome::SpeechEnded { samples: buffer }, timings));
            }
            // W6b-2 issue #47:每 2s 发射一次 partial transcript callback
            // W10 Plan 3: 首次 callback 时记录 t1
            if let Some(cb) = partial_callback {
                if elapsed - last_partial_elapsed >= Duration::from_secs(2) {
                    if timings.first_partial_at.is_none() {
                        timings.first_partial_at = Some(SystemTime::now());
                    }
                    cb(&buffer);
                    last_partial_elapsed = elapsed;
                }
            }
        }

        let outcome = match self.vad.detect(&buffer) {
            VadOutcome::Speech { speech_end_sample, .. } => {
                buffer.truncate(speech_end_sample);
                if buffer.is_empty() {
                    ListenOutcome::NoSpeech
                } else {
                    ListenOutcome::Timeout { samples: buffer }
                }
            }
            VadOutcome::NoSpeech => ListenOutcome::NoSpeech,
        };
        Ok((outcome, timings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::vad::VadConfig;

    #[test]
    fn listen_outcome_debug_format_works() {
        let outcome = ListenOutcome::SpeechEnded {
            samples: vec![1, 2, 3],
        };
        assert!(format!("{:?}", outcome).contains("SpeechEnded"));
    }

    #[test]
    fn audio_recorder_adapter_returns_error_for_nonexistent_device() {
        // 显式指定不存在的设备名,应返回 CaptureFailed 错误。
        let config = AudioRecorderConfig {
            device: Some("__definitely_nonexistent_device__".to_string()),
            ..Default::default()
        };
        let result = AudioRecorderAdapter::new(config);
        assert!(result.is_err(), "expected error for nonexistent device");
        match result {
            Err(VoiceError::CaptureFailed(_)) => {}
            other => panic!("expected CaptureFailed, got {:?}", other),
        }
    }

    // ===== W10 Plan 3: listen_with_cancel_partial_and_timings tests =====

    /// Mock VoiceRecorder:返回预设的 chunks,耗尽后返回空。
    struct MockRecorder {
        chunks: Vec<Vec<i16>>,
        call_count: std::sync::Mutex<usize>,
    }

    impl MockRecorder {
        fn new(chunks: Vec<Vec<i16>>) -> Self {
            Self {
                chunks,
                call_count: std::sync::Mutex::new(0),
            }
        }
    }

    impl VoiceRecorder for MockRecorder {
        fn record_chunk(&self, _duration: Duration) -> VoiceResult<Vec<i16>> {
            let mut count = self.call_count.lock().unwrap();
            if *count < self.chunks.len() {
                let chunk = self.chunks[*count].clone();
                *count += 1;
                Ok(chunk)
            } else {
                Ok(vec![])
            }
        }
    }

    #[test]
    fn listen_with_timings_captures_voice_started_at_on_first_voiced_chunk() {
        // 构造 4 chunks:2 静音 + 1 语音(高能量)+ 1 静音
        // voice_started_at 应在 voiced chunk(第 3 个 chunk)时设置
        let silence_chunk = vec![0i16; 8000]; // 500ms @ 16kHz,静音
        let voiced_chunk = vec![10_000i16; 8000]; // 500ms,高能量
        let recorder = std::sync::Arc::new(MockRecorder::new(vec![
            silence_chunk.clone(),
            silence_chunk.clone(),
            voiced_chunk,
            silence_chunk,
        ]));
        let vad = VadDetector::new(VadConfig::default());
        let listener = VoiceListener::new(
            recorder,
            vad,
            Duration::from_secs(10),
            Duration::from_millis(500),
        );
        let cancel = std::sync::atomic::AtomicBool::new(false);

        let (outcome, timings) = listener
            .listen_with_cancel_partial_and_timings(&cancel, None)
            .unwrap();

        // voice_started_at 应在 voiced chunk(第 3 个 chunk)时设置
        assert!(
            timings.voice_started_at.is_some(),
            "voice_started_at must be Some after voiced chunk detected"
        );
        // first_partial_at 应为 None(未传 partial_callback)
        assert!(
            timings.first_partial_at.is_none(),
            "first_partial_at must be None without partial_callback"
        );
        // outcome 不强制断言类型,VAD 行为已由 W6b-1 测试覆盖
        let _ = outcome;
    }

    #[test]
    fn listen_with_timings_returns_none_when_all_silent() {
        let silence_chunk = vec![0i16; 8000];
        let recorder = std::sync::Arc::new(MockRecorder::new(vec![
            silence_chunk.clone(),
            silence_chunk.clone(),
        ]));
        let vad = VadDetector::new(VadConfig::default());
        let listener = VoiceListener::new(
            recorder,
            vad,
            Duration::from_secs(1),
            Duration::from_millis(500),
        );
        let cancel = std::sync::atomic::AtomicBool::new(false);

        let (outcome, timings) = listener
            .listen_with_cancel_partial_and_timings(&cancel, None)
            .unwrap();

        assert!(
            timings.voice_started_at.is_none(),
            "voice_started_at must be None for all-silent input"
        );
        assert!(timings.first_partial_at.is_none());
        // outcome 应为 NoSpeech
        assert!(matches!(outcome, ListenOutcome::NoSpeech));
    }

    #[test]
    fn listen_with_timings_captures_first_partial_at_on_first_callback() {
        // 构造足够长的 voiced 序列触发 partial callback(2s = 4 chunks @ 500ms)。
        // 全部为 voiced chunks —— 避免 detect_end_of_speech 在 2s 前提前触发
        // (detect_end_of_speech 需 voiced + 静音超时,全 voiced 不会返回 Some)。
        // loop 在 max_duration(5s)或 recorder 耗尽(5 chunks = 2.5s)后退出。
        let voiced_chunk = vec![10_000i16; 8000];
        let recorder = std::sync::Arc::new(MockRecorder::new(vec![
            voiced_chunk.clone(),
            voiced_chunk.clone(),
            voiced_chunk.clone(),
            voiced_chunk.clone(),
            voiced_chunk,
        ]));
        let vad = VadDetector::new(VadConfig::default());
        let listener = VoiceListener::new(
            recorder,
            vad,
            Duration::from_secs(5),
            Duration::from_millis(500),
        );
        let cancel = std::sync::atomic::AtomicBool::new(false);

        let partial_call_count = std::sync::Mutex::new(0);
        let partial_cb: &dyn Fn(&[i16]) = &|_samples| {
            *partial_call_count.lock().unwrap() += 1;
        };

        let (outcome, timings) = listener
            .listen_with_cancel_partial_and_timings(&cancel, Some(partial_cb))
            .unwrap();

        // 首个 partial callback 应设置 first_partial_at
        assert!(
            timings.first_partial_at.is_some(),
            "first_partial_at must be Some after partial_callback fires"
        );
        assert!(
            timings.voice_started_at.is_some(),
            "voice_started_at must be Some(voiced chunk present)"
        );
        // 验证 partial callback 至少调用一次
        assert!(
            *partial_call_count.lock().unwrap() >= 1,
            "partial_callback should fire at least once at 2s elapsed"
        );
        let _ = outcome;
    }
}
