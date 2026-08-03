//! Energy-threshold VAD (Voice Activity Detection).
//!
//! W5 PoC: simple RMS energy per frame, no neural network.
//! W6+ may swap in Silero VAD or WebRTC VAD via feature flag.
//!
//! Algorithm:
//!   1. Split samples into frames of `frame_ms` duration.
//!   2. Compute RMS energy per frame.
//!   3. A frame is "speech" if energy >= `energy_threshold`.
//!   4. Track contiguous speech frames; once `min_speech_ms` reached, mark
//!      speech_start. Continue until `max_silence_ms` of contiguous silent
//!      frames ends the speech segment.
//!   5. Return Speech { speech_end_sample } or NoSpeech.

#[derive(Debug, Clone)]
pub struct VadConfig {
    pub frame_ms: u32,
    pub sample_rate: u32,
    pub energy_threshold: f32,
    pub min_speech_ms: u32,
    pub max_silence_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            sample_rate: 16000,
            energy_threshold: 100.0,
            min_speech_ms: 200,
            max_silence_ms: 700,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VadOutcome {
    /// Speech detected, speech_end_sample is the sample index where speech
    /// ended (after silence timeout). speech_start_sample is where it began.
    Speech {
        speech_start_sample: usize,
        speech_end_sample: usize,
    },
    NoSpeech,
}

/// 静音超时结束的语音段(V1.1 §2.1 + W6b-1 issue #45)。
///
/// 仅在 VAD 检测到 "语音段 + 静音超时" 时返回,不包含 "音频末尾仍在说话"
/// 的情况。VoiceListener 用此方法判断是否应停止录音。
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechSegment {
    pub speech_start_sample: usize,
    pub speech_end_sample: usize,
}

pub struct VadDetector {
    config: VadConfig,
    frame_size: usize,
    min_speech_frames: usize,
    max_silence_frames: usize,
}

impl VadDetector {
    pub fn new(config: VadConfig) -> Self {
        let frame_size = ((config.frame_ms as u64 * config.sample_rate as u64) / 1000) as usize;
        let min_speech_frames =
            ((config.min_speech_ms as u64 * config.sample_rate as u64) / 1000) as usize / frame_size;
        let max_silence_frames =
            ((config.max_silence_ms as u64 * config.sample_rate as u64) / 1000) as usize / frame_size;
        Self {
            config,
            frame_size,
            min_speech_frames,
            max_silence_frames,
        }
    }

    pub fn detect(&self, samples: &[i16]) -> VadOutcome {
        let n_frames = samples.len() / self.frame_size;
        if n_frames == 0 {
            return VadOutcome::NoSpeech;
        }

        let mut frame_energy: Vec<f32> = Vec::with_capacity(n_frames);
        for i in 0..n_frames {
            let start = i * self.frame_size;
            let end = start + self.frame_size;
            let energy = rms_energy(&samples[start..end]);
            frame_energy.push(energy);
        }

        let mut in_speech = false;
        let mut speech_start_frame = 0usize;
        let mut speech_frame_count = 0usize;
        let mut silence_frame_count = 0usize;
        let mut last_speech_frame = 0usize;

        for (i, &energy) in frame_energy.iter().enumerate() {
            let is_speech = energy >= self.config.energy_threshold;
            if is_speech {
                if !in_speech {
                    speech_start_frame = i;
                    in_speech = true;
                    speech_frame_count = 1;
                } else {
                    speech_frame_count += 1;
                }
                silence_frame_count = 0;
                last_speech_frame = i;
            } else if in_speech {
                silence_frame_count += 1;
                if silence_frame_count >= self.max_silence_frames {
                    // End of speech segment.
                    if speech_frame_count >= self.min_speech_frames {
                        let speech_start_sample = speech_start_frame * self.frame_size;
                        // speech_end_sample is the sample index where speech ended
                        // (after silence timeout), per doc contract.
                        let speech_end_sample = (i + 1) * self.frame_size;
                        return VadOutcome::Speech {
                            speech_start_sample,
                            speech_end_sample,
                        };
                    }
                    in_speech = false;
                }
            }
        }

        // Handle speech that ends at end of audio (no trailing silence).
        if in_speech && speech_frame_count >= self.min_speech_frames {
            let speech_start_sample = speech_start_frame * self.frame_size;
            let speech_end_sample = (last_speech_frame + 1) * self.frame_size;
            return VadOutcome::Speech {
                speech_start_sample,
                speech_end_sample,
            };
        }

        VadOutcome::NoSpeech
    }

    /// 检测语音是否已通过静音超时结束(W6b-1 issue #45)。
    ///
    /// 返回 `Some(SpeechSegment)` 仅当:
    ///   - 检测到 >= `min_speech_ms` 的连续语音段
    ///   - 之后有 >= `max_silence_ms` 的连续静音(触发静音超时)
    ///
    /// 返回 `None` 如果:
    ///   - 没有语音
    ///   - 语音仍在进行中(未达到静音超时,音频末尾仍有语音)
    ///   - 语音段长度不足 `min_speech_ms`
    ///
    /// 与 `detect()` 的区别:`detect()` 在 "音频末尾仍有语音" 时也返回 `Speech`,
    /// 而 `detect_end_of_speech` 不返回这种情况(因为语音尚未结束)。
    /// VoiceListener 在循环中调用本方法,仅在 `Some` 时停止录音。
    pub fn detect_end_of_speech(&self, samples: &[i16]) -> Option<SpeechSegment> {
        let n_frames = samples.len() / self.frame_size;
        if n_frames == 0 {
            return None;
        }

        let mut in_speech = false;
        let mut speech_start_frame = 0usize;
        let mut speech_frame_count = 0usize;
        let mut silence_frame_count = 0usize;

        for i in 0..n_frames {
            let start = i * self.frame_size;
            let end = start + self.frame_size;
            let energy = rms_energy(&samples[start..end]);
            let is_speech = energy >= self.config.energy_threshold;
            if is_speech {
                if !in_speech {
                    speech_start_frame = i;
                    in_speech = true;
                    speech_frame_count = 1;
                } else {
                    speech_frame_count += 1;
                }
                silence_frame_count = 0;
            } else if in_speech {
                silence_frame_count += 1;
                if silence_frame_count >= self.max_silence_frames {
                    // 静音超时 —— 仅当语音段足够长才返回。
                    if speech_frame_count >= self.min_speech_frames {
                        let speech_start_sample = speech_start_frame * self.frame_size;
                        let speech_end_sample = (i + 1) * self.frame_size;
                        return Some(SpeechSegment {
                            speech_start_sample,
                            speech_end_sample,
                        });
                    }
                    // 语音段太短,重置状态继续寻找。
                    in_speech = false;
                }
            }
        }

        // 注:不处理 "音频末尾仍有语音" 情况 —— 这正是本方法与 detect() 的区别。
        None
    }

    /// W10 Plan 3: 快速判断 chunk 是否含语音(spec §5.2 v2 修订 #6)。
    ///
    /// 计算整个 chunk 的 RMS 能量,与 `config.energy_threshold` 比较。
    /// 用于 listener 在每个 chunk 录制后快速检测首个 voiced chunk(t0)。
    ///
    /// 与 `detect()` / `detect_end_of_speech()` 的区别:
    /// - `detect()` 按 frame 切分,返回 Speech/NoSpeech + sample 索引
    /// - `detect_end_of_speech()` 检测静音超时结束的语音段
    /// - `chunk_has_speech()` 是粗粒度快速判断(整 chunk 一个 RMS 值),
    ///   用于 t0 触发,不返回 sample 索引
    ///
    /// 空 chunk 返回 false(RMS = 0 < threshold)。
    pub fn chunk_has_speech(&self, samples: &[i16]) -> bool {
        if samples.is_empty() {
            return false;
        }
        rms_energy(samples) >= self.config.energy_threshold
    }
}

fn rms_energy(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f64 = samples
        .iter()
        .map(|&s| {
            let f = s as f64;
            f * f
        })
        .sum();
    (sum_squares / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod w10_plan3_tests {
    use super::*;

    #[test]
    fn chunk_has_speech_returns_true_for_high_energy_chunk() {
        // 构造 16000 samples(1 秒 @ 16kHz),全为 10000 → 高能量
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![10_000; 16000];
        assert!(
            vad.chunk_has_speech(&samples),
            "high energy chunk should have speech"
        );
    }

    #[test]
    fn chunk_has_speech_returns_false_for_silent_chunk() {
        let vad = VadDetector::new(VadConfig::default());
        // 全 0 样本 → 能量 0 < threshold(100.0)
        let samples: Vec<i16> = vec![0; 16000];
        assert!(
            !vad.chunk_has_speech(&samples),
            "silent chunk should not have speech"
        );
    }

    #[test]
    fn chunk_has_speech_returns_false_for_empty_chunk() {
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![];
        assert!(
            !vad.chunk_has_speech(&samples),
            "empty chunk should not have speech"
        );
    }

    #[test]
    fn chunk_has_speech_threshold_boundary() {
        // energy_threshold = 100.0,default config
        // RMS = sqrt(mean(x^2))。构造 samples 使 RMS = 100.0
        // x = 100 → RMS = 100.0(刚好等于 threshold,>= 判定为 speech)
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![100; 16000];
        assert!(
            vad.chunk_has_speech(&samples),
            "RMS=100.0 should be >= threshold=100.0"
        );
    }
}
