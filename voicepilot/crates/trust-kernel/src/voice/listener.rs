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
        let mut buffer: Vec<i16> = Vec::new();
        let mut elapsed = Duration::ZERO;

        while elapsed < self.max_duration {
            let chunk = self.recorder.record_chunk(self.chunk_duration)?;

            // recorder 耗尽(如 mock 用完预设块)—— 退出循环。
            if chunk.is_empty() {
                break;
            }

            buffer.extend_from_slice(&chunk);
            elapsed += self.chunk_duration;

            // 检测静音超时 —— 仅在语音段已结束时返回 Some。
            if let Some(segment) = self.vad.detect_end_of_speech(&buffer) {
                buffer.truncate(segment.speech_end_sample);
                return Ok(ListenOutcome::SpeechEnded { samples: buffer });
            }
        }

        // 退出循环后:用 detect() 判断 buffer 中是否有任何语音。
        // detect() 在 "音频末尾仍有语音" 时也返回 Speech,这正是 Timeout 场景
        // (用户持续说话未停顿,达到 max_duration)。
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
