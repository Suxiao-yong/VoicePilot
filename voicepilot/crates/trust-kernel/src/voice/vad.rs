//! VAD — Silero (sherpa-onnx) + energy fallback.
//!
//! W12 补齐：优先使用 `sherpa-libs` / `~/.voicepilot/models/silero_vad.onnx` 的 Silero VAD，
//! 缺模型或加载失败时自动回退到 RMS 能量阈值（保持原有测试通过）。
//!
//! Phase 1(对标 block/buzz 端点策略):max_silence 700→300ms(迟钝感来源);
//! 能量路径加 hysteresis(进入/退出双阈值)+onset 确认(防爆破音误触);
//! 所有返回的 speech_start 统一回溯 pre_roll(防丢首字,下限 0)。

use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct VadConfig {
    pub frame_ms: u32,
    pub sample_rate: u32,
    /// 能量进入阈值:未说话时,帧 RMS >= 此值才计 voiced。
    pub energy_threshold: f32,
    pub min_speech_ms: u32,
    /// 静音超时(Phase 1:700→300,与 buzz SILENCE_FLUSH_FRAMES=19 同值)。
    pub max_silence_ms: u32,
    /// 能量退出阈值(hysteresis):说话中,帧 RMS < 此值才计静音。
    /// 低于进入阈值,防止尾音/弱音节处抖动切碎。
    pub energy_exit_threshold: f32,
    /// onset 确认:连续多少帧 voiced 才算进入语音(防爆破音/敲击误触)。
    pub onset_frames: u32,
    /// pre-roll:返回的 speech_start 往前回溯(防丢首字,下限钳到 0)。
    pub pre_roll_ms: u32,
    /// TTS 停播后 cooldown(与 buzz TTS_COOLDOWN=150ms 同值,防尾音自激)。
    pub tts_cooldown_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            sample_rate: 16000,
            energy_threshold: 100.0,
            min_speech_ms: 200,
            max_silence_ms: 300,
            energy_exit_threshold: 60.0,
            onset_frames: 3,
            pre_roll_ms: 300,
            tts_cooldown_ms: 150,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VadOutcome {
    Speech {
        speech_start_sample: usize,
        speech_end_sample: usize,
    },
    NoSpeech,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechSegment {
    pub speech_start_sample: usize,
    pub speech_end_sample: usize,
}

#[cfg(feature = "voice")]
fn silero_model_path() -> PathBuf {
    // ponytail: 优先 sherpa-libs（随包分发），其次 ~/.voicepilot/models（可下载）
    let candidates = [
        PathBuf::from("sherpa-libs/sherpa-onnx-v1.12.9-win-x64-shared/silero_vad.onnx"),
        PathBuf::from("sherpa-libs/silero_vad.onnx"),
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".voicepilot")
            .join("models")
            .join("silero_vad.onnx"),
    ];
    for p in candidates {
        if p.is_file() {
            return p;
        }
    }
    // 默认返回 ~/.voicepilot/models/silero_vad.onnx（调用方决定是否下载）
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".voicepilot")
        .join("models")
        .join("silero_vad.onnx")
}

#[cfg(feature = "voice")]
fn try_create_silero(config: &VadConfig) -> Option<Mutex<sherpa_rs::silero_vad::SileroVad>> {
    let model_path = silero_model_path();
    if !model_path.is_file() {
        return None;
    }
    let silero_cfg = sherpa_rs::silero_vad::SileroVadConfig {
        model: model_path.to_string_lossy().into_owned(),
        min_silence_duration: config.max_silence_ms as f32 / 1000.0,
        min_speech_duration: config.min_speech_ms as f32 / 1000.0,
        max_speech_duration: 10.0,
        threshold: 0.5,
        sample_rate: config.sample_rate,
        window_size: 512,
        provider: None,
        num_threads: Some(1),
        debug: false,
    };
    match sherpa_rs::silero_vad::SileroVad::new(silero_cfg, 30.0) {
        Ok(vad) => Some(Mutex::new(vad)),
        Err(_) => None,
    }
}

pub struct VadDetector {
    config: VadConfig,
    frame_size: usize,
    min_speech_frames: usize,
    max_silence_frames: usize,
    onset_frames: usize,
    pre_roll_samples: usize,
    #[cfg(feature = "voice")]
    silero: Option<Mutex<sherpa_rs::silero_vad::SileroVad>>,
    /// 已喂给 Silero 的样本数(增量检测)。Silero 是流式状态机,重复喂全量
    /// buffer 会让窗口状态推进两倍、分段错乱 —— 所有喂入点只喂 `samples[fed..]`。
    fed_samples: Mutex<usize>,
}

impl VadDetector {
    pub fn new(config: VadConfig) -> Self {
        let frame_size = ((config.frame_ms as u64 * config.sample_rate as u64) / 1000) as usize;
        let min_speech_frames = ((config.min_speech_ms as u64 * config.sample_rate as u64) / 1000)
            as usize
            / frame_size.max(1);
        let max_silence_frames = ((config.max_silence_ms as u64 * config.sample_rate as u64) / 1000)
            as usize
            / frame_size.max(1);
        #[cfg(feature = "voice")]
        let silero = try_create_silero(&config);
        let pre_roll_samples =
            (config.pre_roll_ms as u64 * config.sample_rate as u64 / 1000) as usize;
        Self {
            onset_frames: (config.onset_frames.max(1)) as usize,
            pre_roll_samples,
            config,
            frame_size,
            min_speech_frames: min_speech_frames.max(1),
            max_silence_frames: max_silence_frames.max(1),
            #[cfg(feature = "voice")]
            silero,
            fed_samples: Mutex::new(0),
        }
    }

    /// 重置增量喂入状态(新 listen 开始前调用)。
    pub fn reset(&self) {
        *self.fed_samples.lock().unwrap() = 0;
    }

    /// 把 `samples[fed..]` 喂给 Silero 并返回该段(供各检测函数复用)。
    /// `buffer_shrank=true` 表示调用方 buffer 被截断(新 listen),强制重喂。
    ///
    /// 不变量(review P1-F1):所有 Silero 喂入必须经此函数、传入**全量累积 buffer**、
    /// 按 fed 顺序推进。禁止传入单 chunk(会触发 fed 清零 + 历史重喂,破坏流式状态)。
    /// `chunk_has_speech` 因此 intentionally 纯能量实现,不碰 Silero。
    #[cfg(feature = "voice")]
    fn feed_new(&self, vad: &mut sherpa_rs::silero_vad::SileroVad, samples: &[i16]) {
        let mut fed = self.fed_samples.lock().unwrap();
        if samples.len() < *fed {
            *fed = 0; // buffer 被截断,重新从头喂
        }
        if *fed < samples.len() {
            let f32s: Vec<f32> = samples[*fed..]
                .iter()
                .map(|&s| s as f32 / 32768.0)
                .collect();
            vad.accept_waveform(f32s);
            *fed = samples.len();
        }
    }

    /// 是否可用 Silero（模型已加载）
    pub fn is_silero(&self) -> bool {
        #[cfg(feature = "voice")]
        {
            self.silero.is_some()
        }
        #[cfg(not(feature = "voice"))]
        {
            false
        }
    }

    /// speech_start 回溯 pre_roll(防丢首字,下限钳到 0)。
    fn backtrack(&self, start: usize) -> usize {
        start.saturating_sub(self.pre_roll_samples)
    }

    pub fn detect(&self, samples: &[i16]) -> VadOutcome {
        #[cfg(feature = "voice")]
        if let Some(mtx) = &self.silero {
            if let Ok(mut vad) = mtx.lock() {
                self.feed_new(&mut vad, samples);
                if !vad.is_empty() {
                    let seg = vad.front();
                    let start = seg.start as usize;
                    let len = seg.samples.len();
                    let speech_start = self.backtrack(start);
                    let speech_end = start + len;
                    vad.pop();
                    vad.clear();
                    if len >= self.min_speech_frames * self.frame_size {
                        return VadOutcome::Speech {
                            speech_start_sample: speech_start.min(samples.len()),
                            speech_end_sample: speech_end.min(samples.len()),
                        };
                    }
                } else {
                    vad.clear();
                }
                // Silero 未检出或段太短,回退能量(兼容合成数据与真实静音)
                let energy_out = self.detect_energy(samples);
                if energy_out != VadOutcome::NoSpeech {
                    return energy_out;
                }
                return VadOutcome::NoSpeech;
            }
        }
        self.detect_energy(samples)
    }

    pub fn detect_end_of_speech(&self, samples: &[i16]) -> Option<SpeechSegment> {
        #[cfg(feature = "voice")]
        if let Some(mtx) = &self.silero {
            if let Ok(mut vad) = mtx.lock() {
                self.feed_new(&mut vad, samples);
                if !vad.is_empty() {
                    let seg = vad.front();
                    let start = seg.start as usize;
                    let len = seg.samples.len();
                    vad.pop();
                    vad.clear();
                    if len >= self.min_speech_frames * self.frame_size {
                        return Some(SpeechSegment {
                            speech_start_sample: self.backtrack(start).min(samples.len()),
                            speech_end_sample: (start + len).min(samples.len()),
                        });
                    }
                } else {
                    vad.clear();
                    // 回退能量:若 Silero 未检出但能量检出,返回能量结果(兼容测试)
                    if let Some(seg) = self.detect_end_of_speech_energy(samples) {
                        return Some(seg);
                    }
                }
                return None;
            }
        }
        self.detect_end_of_speech_energy(samples)
    }

    /// 单 chunk 是否含语音(仅 W10 timings 路径的 t0 打点用)。
    ///
    /// intentionally 纯能量实现,不碰 Silero:调用方传入的是单 chunk 而非全量
    /// buffer,经 `feed_new` 会触发 fed 清零 + 历史重喂,破坏 Silero 流式状态
    /// (review P1-F1)。t0 只是延迟遥测,能量阈值足够;真正的端点判定走
    /// `detect_end_of_speech`(Silero + 能量回退),不受影响。
    pub fn chunk_has_speech(&self, samples: &[i16]) -> bool {
        if samples.is_empty() {
            return false;
        }
        rms_energy(samples) >= self.config.energy_threshold
    }

    // ---- energy fallback impls ----
    //
    // Phase 1(对标 buzz 端点策略):统一扫描器,两个调用方共享。
    // - hysteresis:未说话时用进入阈值,说话中用更低的退出阈值(防尾音抖动切碎)
    // - onset 确认:连续 onset_frames 个 voiced 帧才算进入(防爆破音/敲击误触)
    fn scan_energy(&self, samples: &[i16]) -> EnergyScan {
        let n_frames = samples.len() / self.frame_size;
        let mut scan = EnergyScan {
            start_frame: None,
            end_frame: None,
            trailing_speech: false,
            last_voiced_frame: 0,
        };
        if n_frames == 0 {
            return scan;
        }
        let mut in_speech = false;
        let mut onset_count = 0usize;
        let mut candidate_start = 0usize;
        let mut speech_frame_count = 0usize;
        let mut silence_frame_count = 0usize;
        for i in 0..n_frames {
            let start = i * self.frame_size;
            let end = start + self.frame_size;
            let energy = rms_energy(&samples[start..end]);
            let voiced = if in_speech {
                energy >= self.config.energy_exit_threshold
            } else {
                energy >= self.config.energy_threshold
            };
            if voiced {
                if !in_speech {
                    onset_count += 1;
                    if onset_count == 1 {
                        candidate_start = i;
                    }
                    if onset_count >= self.onset_frames {
                        in_speech = true;
                        speech_frame_count = onset_count;
                        silence_frame_count = 0;
                        scan.last_voiced_frame = i;
                        if speech_frame_count >= self.min_speech_frames
                            && scan.start_frame.is_none()
                        {
                            scan.start_frame = Some(candidate_start);
                        }
                    }
                } else {
                    speech_frame_count += 1;
                    silence_frame_count = 0;
                    scan.last_voiced_frame = i;
                    if speech_frame_count >= self.min_speech_frames && scan.start_frame.is_none() {
                        scan.start_frame = Some(candidate_start);
                    }
                }
            } else if in_speech {
                onset_count = 0;
                silence_frame_count += 1;
                if silence_frame_count >= self.max_silence_frames {
                    if scan.start_frame.is_some() {
                        scan.end_frame = Some(i + 1);
                        return scan;
                    }
                    in_speech = false;
                }
            } else {
                onset_count = 0;
            }
        }
        scan.trailing_speech = in_speech && scan.start_frame.is_some();
        scan
    }

    fn detect_energy(&self, samples: &[i16]) -> VadOutcome {
        let scan = self.scan_energy(samples);
        match (scan.start_frame, scan.end_frame) {
            (Some(s), Some(e)) => VadOutcome::Speech {
                speech_start_sample: self.backtrack(s * self.frame_size).min(samples.len()),
                speech_end_sample: (e * self.frame_size).min(samples.len()),
            },
            (Some(s), None) if scan.trailing_speech => VadOutcome::Speech {
                speech_start_sample: self.backtrack(s * self.frame_size).min(samples.len()),
                speech_end_sample: ((scan.last_voiced_frame + 1) * self.frame_size)
                    .min(samples.len()),
            },
            _ => VadOutcome::NoSpeech,
        }
    }

    fn detect_end_of_speech_energy(&self, samples: &[i16]) -> Option<SpeechSegment> {
        let scan = self.scan_energy(samples);
        match (scan.start_frame, scan.end_frame) {
            (Some(s), Some(e)) => Some(SpeechSegment {
                speech_start_sample: self.backtrack(s * self.frame_size).min(samples.len()),
                speech_end_sample: (e * self.frame_size).min(samples.len()),
            }),
            _ => None,
        }
    }
}

/// 能量扫描结果(帧下标,转样本时 × frame_size)。
/// `end_frame` 仅静音超时截断时为 Some(末尾仍在说话则为 None + trailing=true)。
struct EnergyScan {
    start_frame: Option<usize>,
    end_frame: Option<usize>,
    trailing_speech: bool,
    last_voiced_frame: usize,
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
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![10_000; 16000];
        assert!(vad.chunk_has_speech(&samples));
    }

    #[test]
    fn chunk_has_speech_returns_false_for_silent_chunk() {
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![0; 16000];
        assert!(!vad.chunk_has_speech(&samples));
    }

    #[test]
    fn chunk_has_speech_returns_false_for_empty_chunk() {
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![];
        assert!(!vad.chunk_has_speech(&samples));
    }

    #[test]
    fn chunk_has_speech_threshold_boundary() {
        let vad = VadDetector::new(VadConfig::default());
        let samples: Vec<i16> = vec![100; 16000];
        assert!(vad.chunk_has_speech(&samples));
    }

    // ===== Phase 1(buzz 端点策略)测试 =====

    #[test]
    fn hysteresis_bridges_mid_energy_dip() {
        // 进入100 / 退出60:200ms强语音 + 400ms中等能量(RMS 70)。
        // 无 hysteresis 会被判 400ms 静音(≥300ms)而截断;有则全程 voiced → None。
        let vad = VadDetector::new(VadConfig::default());
        let mut samples = vec![5000i16; 3200];
        samples.extend(vec![70i16; 6400]);
        assert!(
            vad.detect_end_of_speech(&samples).is_none(),
            "mid-energy dip above exit threshold must not end speech"
        );
        // 对照:同样位置纯静音 400ms 应触发截断。
        let mut control = vec![5000i16; 3200];
        control.extend(vec![0i16; 6400]);
        assert!(
            vad.detect_end_of_speech(&control).is_some(),
            "pure silence after speech must end speech"
        );
    }

    #[test]
    fn onset_ignores_short_blip() {
        // min 40ms + onset 3帧:2帧(40ms)爆破音不满足 onset → NoSpeech。
        let config = VadConfig {
            min_speech_ms: 40,
            ..VadConfig::default()
        };
        let vad = VadDetector::new(config);
        let mut samples = vec![5000i16; 640];
        samples.extend(vec![0i16; 9600]);
        assert!(
            matches!(vad.detect(&samples), VadOutcome::NoSpeech),
            "2-frame blip must not enter speech with onset=3"
        );
        // 对照:4帧(80ms)满足 onset + min → 静音超时后截断为 Speech。
        let mut voiced = vec![5000i16; 1280];
        voiced.extend(vec![0i16; 9600]);
        assert!(
            matches!(vad.detect(&voiced), VadOutcome::Speech { .. }),
            "4-frame onset must enter speech"
        );
    }

    #[test]
    fn pre_roll_backtracks_speech_start() {
        // 500ms静音 + 500ms语音 + 500ms静音:能量起点 frame25(8000样本),
        // 回溯 300ms(15帧) → frame10 = 3200样本。
        let vad = VadDetector::new(VadConfig::default());
        let mut samples = vec![0i16; 8000];
        samples.extend(vec![5000i16; 8000]);
        samples.extend(vec![0i16; 8000]);
        match vad.detect(&samples) {
            VadOutcome::Speech {
                speech_start_sample,
                ..
            } => assert_eq!(
                speech_start_sample, 3200,
                "pre-roll must backtrack start to 3200"
            ),
            other => panic!("expected Speech, got {:?}", other),
        }
    }
}
