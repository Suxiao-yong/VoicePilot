//! SherpaTtsEngine — sherpa-rs VitsTts 包装(V1.1 VP-FR-002)。
//!
//! 提供短文本 → PCM samples 合成,UI 播放。可被 cancel_tts_command 中断。
//!
//! 实现说明:sherpa-rs 0.6.8 的 `VitsTts::create` 需要 `&mut self`,
//! 我们用 `Mutex<VitsTts>` 包装以暴露 `&self` 接口(与 SherpaAsrEngine 一致;
//! 单线程使用时 lock 不会阻塞,多线程时串行化合成,语义安全)。
//!
//! API 注记:`VitsTts::new` 不可失败(返回 `Self`),无效 ONNX 文件会让
//! sherpa-onnx C 库 native abort(同 ASR 情况,Rust 无法捕获 foreign exception)。
//! 因此 `new` 中预校验 model.int8.onnx / lexicon.txt / tokens.txt 存在,避免触达 C 库。
//!
//! W6c P2 #2(方案 A):sherpa-rs `TtsAudio` struct 暴露 `sample_rate: u32`(由模型决定,
//! 中文 VITS 通常 22050 Hz),`synth` 返回 `(Vec<i16>, u32)` 让调用方拿到实际采样率,
//! `SherpaTtsEngine` 还用 `AtomicU32` 缓存最近一次合成时的实际 sample_rate。

use crate::voice::error::{VoiceError, VoiceResult};
use sherpa_rs::tts::{VitsTts, VitsTtsConfig};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct SherpaTtsConfig {
    /// sherpa-onnx TTS 模型目录(包含 model.int8.onnx + lexicon.txt + tokens.txt)。
    pub model_dir: PathBuf,
    /// 配置声明的采样率(W6c P2 #2 方案 A 后仅作初始默认值,
    /// 实际播放用 `SherpaTtsEngine::actual_sample_rate()`,由模型决定)。
    pub sample_rate: u32,
    pub num_threads: u32,
    pub speed: f32,
}

impl Default for SherpaTtsConfig {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::new(),
            sample_rate: 16000,
            num_threads: 1,
            speed: 1.0,
        }
    }
}

// W6c P2 #3:手动 PartialEq 排除 sample_rate(模型决定值,不参与缓存失效判断)。
// model_dir / num_threads / speed 是用户配置,改变这些应触发缓存失效。
impl PartialEq for SherpaTtsConfig {
    fn eq(&self, other: &Self) -> bool {
        self.model_dir == other.model_dir
            && self.num_threads == other.num_threads
            && self.speed == other.speed
    }
}

impl Eq for SherpaTtsConfig {}

pub struct SherpaTtsEngine {
    config: SherpaTtsConfig,
    /// `VitsTts::create` 取 `&mut self`,用 Mutex 包装以暴露 `&self`。
    tts: Mutex<VitsTts>,
    /// W6c P2 #2:实际 sample_rate 由模型决定(中文 VITS 通常 22050 Hz),
    /// 首次 synth 后缓存。voice_commands.rs 用此值写 WAV 头避免音调失真。
    /// 初始值为 config.sample_rate(16000),首次 synth 后被覆盖。
    actual_sample_rate: AtomicU32,
}

impl SherpaTtsEngine {
    /// 加载模型。校验 model_dir 存在且包含 model.int8.onnx + lexicon.txt + tokens.txt。
    pub fn new(config: SherpaTtsConfig) -> VoiceResult<Self> {
        if !config.model_dir.is_dir() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "tts model_dir not found: {}",
                config.model_dir.display()
            )));
        }
        let model_onnx = config.model_dir.join("model.int8.onnx");
        if !model_onnx.is_file() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "model.int8.onnx not found in: {}",
                config.model_dir.display()
            )));
        }
        let lexicon_txt = config.model_dir.join("lexicon.txt");
        if !lexicon_txt.is_file() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "lexicon.txt not found in: {}",
                config.model_dir.display()
            )));
        }
        let tokens_txt = config.model_dir.join("tokens.txt");
        if !tokens_txt.is_file() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "tokens.txt not found in: {}",
                config.model_dir.display()
            )));
        }

        // API 验证:docs.rs/sherpa-rs/0.6.8。VitsTtsConfig 字段名以 docs.rs 为准。
        let mut tts_config = VitsTtsConfig {
            model: model_onnx.to_string_lossy().into_owned(),
            lexicon: lexicon_txt.to_string_lossy().into_owned(),
            tokens: tokens_txt.to_string_lossy().into_owned(),
            ..Default::default()
        };
        tts_config.onnx_config.num_threads = config.num_threads as i32;

        // VitsTts::new 不可失败(返回 Self)。预校验已确保文件存在;
        // 若 ONNX 文件内容无效,sherpa-onnx C 库会 native abort,Rust 无法捕获。
        let tts = VitsTts::new(tts_config);

        let initial_sr = config.sample_rate;
        Ok(Self {
            config,
            tts: Mutex::new(tts),
            actual_sample_rate: AtomicU32::new(initial_sr),
        })
    }

    /// 合成文本 → i16 PCM samples + 实际 sample_rate(mono,采样率由模型决定,
    /// 中文 VITS 通常 22050 Hz)。空文本返回 Err(NoSpeechDetected)。
    ///
    /// W6c P2 #2:返回 `(Vec<i16>, u32)` 让调用方拿到实际 sample_rate;
    /// 同时缓存到 `actual_sample_rate` 字段供后续查询。
    pub fn synth(&self, text: &str) -> VoiceResult<(Vec<i16>, u32)> {
        if text.trim().is_empty() {
            return Err(VoiceError::NoSpeechDetected);
        }
        let mut tts = self
            .tts
            .lock()
            .map_err(|e| VoiceError::InferenceFailed(format!("tts lock failed: {}", e)))?;
        // sid=0:默认说话人(VITS 单说话人模型)。sample_rate 由模型决定,不由调用方传入。
        let audio = tts
            .create(text, 0, self.config.speed)
            .map_err(|e| VoiceError::InferenceFailed(format!("tts generate failed: {}", e)))?;
        // sherpa-rs `TtsAudio` 暴露 `sample_rate: u32`(W6c P2 #2 调研确认)。
        let actual_sr = audio.sample_rate;
        self.actual_sample_rate.store(actual_sr, Ordering::SeqCst);
        // sherpa-rs 返回 f32 samples;转 i16。
        let samples: Vec<i16> = audio
            .samples
            .iter()
            .map(|&f| (f * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect();
        if samples.is_empty() {
            return Err(VoiceError::InferenceFailed(
                "tts returned empty samples".to_string(),
            ));
        }
        Ok((samples, actual_sr))
    }

    pub fn config(&self) -> &SherpaTtsConfig {
        &self.config
    }

    /// W6c P2 #2:返回最近一次 synth 的实际 sample_rate(由模型决定)。
    /// 若尚未 synth 过,返回 config.sample_rate(初始默认 16000)。
    pub fn actual_sample_rate(&self) -> u32 {
        self.actual_sample_rate.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_missing_model_dir() {
        let config = SherpaTtsConfig {
            model_dir: std::path::PathBuf::from("E:/definitely_nonexistent_tts_dir"),
            ..Default::default()
        };
        let err = SherpaTtsEngine::new(config)
            .err()
            .expect("expected Err for missing tts model_dir");
        let err_msg = format!("{}", err);
        assert!(
            err_msg.contains("tts model_dir not found"),
            "err should mention tts model_dir not found, got: {}",
            err_msg
        );
    }

    #[test]
    fn new_rejects_dir_without_valid_model() {
        let tmp = tempfile::tempdir().unwrap();
        // 创建空目录(无 model.int8.onnx / lexicon.txt / tokens.txt)。
        let config = SherpaTtsConfig {
            model_dir: tmp.path().to_path_buf(),
            ..Default::default()
        };
        let err = SherpaTtsEngine::new(config)
            .err()
            .expect("expected Err for empty tts model_dir");
        let err_msg = format!("{}", err);
        assert!(
            err_msg.contains("model.int8.onnx not found"),
            "err should mention model.int8.onnx not found, got: {}",
            err_msg
        );
    }

    // ===== W6c P2 #3:PartialEq 测试 =====

    #[test]
    fn tts_config_eq_ignores_sample_rate() {
        // sample_rate 是模型决定值,不参与比较
        let a = SherpaTtsConfig {
            model_dir: PathBuf::from("/models/tts"),
            sample_rate: 16000,
            num_threads: 1,
            speed: 1.0,
        };
        let b = SherpaTtsConfig {
            model_dir: PathBuf::from("/models/tts"),
            sample_rate: 22050, // 不同
            num_threads: 1,
            speed: 1.0,
        };
        assert_eq!(a, b, "configs differing only in sample_rate should be equal");
    }

    #[test]
    fn tts_config_ne_differs_on_model_dir() {
        let a = SherpaTtsConfig {
            model_dir: PathBuf::from("/models/tts-a"),
            ..Default::default()
        };
        let b = SherpaTtsConfig {
            model_dir: PathBuf::from("/models/tts-b"),
            ..Default::default()
        };
        assert_ne!(a, b);
    }

    #[test]
    fn tts_config_ne_differs_on_num_threads() {
        let a = SherpaTtsConfig {
            num_threads: 1,
            ..Default::default()
        };
        let b = SherpaTtsConfig {
            num_threads: 2,
            ..Default::default()
        };
        assert_ne!(a, b);
    }

    #[test]
    fn tts_config_ne_differs_on_speed() {
        let a = SherpaTtsConfig {
            speed: 1.0,
            ..Default::default()
        };
        let b = SherpaTtsConfig {
            speed: 1.5,
            ..Default::default()
        };
        assert_ne!(a, b);
    }

    /// 集成测试:真实模型 synth。
    /// 需要 VOICEPILOT_TTS_MODEL_DIR 环境变量指向 sherpa-onnx TTS 模型目录。
    /// 没有模型时 SKIP,不 FAIL。
    #[test]
    fn synth_real_model_returns_samples() {
        let model_dir = match std::env::var("VOICEPILOT_TTS_MODEL_DIR") {
            Ok(v) => std::path::PathBuf::from(v),
            Err(_) => {
                eprintln!("SKIP: VOICEPILOT_TTS_MODEL_DIR not set");
                return;
            }
        };
        if !model_dir.is_dir() {
            eprintln!("SKIP: tts model_dir not a dir: {}", model_dir.display());
            return;
        }
        let engine = match SherpaTtsEngine::new(SherpaTtsConfig {
            model_dir,
            ..Default::default()
        }) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("SKIP: tts engine load failed: {}", e);
                return;
            }
        };
        let (samples, sample_rate) = engine.synth("已为您整理下载目录").unwrap();
        assert!(!samples.is_empty(), "synth should return non-empty samples");
        // 真实模型 sample_rate 通常 22050(中文 VITS),不会是 0
        assert!(sample_rate > 0, "sample_rate should be > 0");
        // actual_sample_rate 应被缓存为 synth 返回值
        assert_eq!(engine.actual_sample_rate(), sample_rate);
    }
}
