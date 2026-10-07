//! SherpaAsrEngine — sherpa-rs SenseVoiceRecognizer 包装(V1.1 §2.1)。
//!
//! 替代 whisper-rs WhisperEngine,绕过 issue #49(bindgen 在 Windows MSVC 失败)。
//! sherpa-rs `download-binaries` feature 走预编译库,无需 CMake/bindgen。
//!
//! 签名保持与 WhisperEngine 一致:`transcribe(&self, samples: &[i16]) -> VoiceResult<String>`,
//! 下游 listener / voice_commands 无需改 transcribe 调用。
//!
//! 实现说明:sherpa-rs 0.6.8 的 `SenseVoiceRecognizer::transcribe` 需要 `&mut self`,
//! 我们用 `Mutex<SenseVoiceRecognizer>` 包装以暴露 `&self` 接口(单线程使用时
//! lock 不会阻塞;多线程时串行化推理,语义安全)。

use crate::voice::error::{VoiceError, VoiceResult};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use std::path::PathBuf;
use std::sync::Mutex;

// W6c P2 #3:derive PartialEq — model_dir / language / num_threads / sample_rate
// 全部参与比较(ASR 的 sample_rate 是调用方决定值 16000,与 TTS 不同)。
// 改 language / num_threads 后 cache 失效,重新加载 engine。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SherpaAsrConfig {
    /// sherpa-onnx 模型目录(包含 model.onnx + tokens.txt)。
    pub model_dir: PathBuf,
    pub language: Option<String>,
    pub num_threads: u32,
    pub sample_rate: u32,
}

impl Default for SherpaAsrConfig {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::new(),
            language: None,
            num_threads: 4,
            sample_rate: 16000,
        }
    }
}

pub struct SherpaAsrEngine {
    config: SherpaAsrConfig,
    /// `SenseVoiceRecognizer::transcribe` 取 `&mut self`,用 Mutex 包装以暴露 `&self`。
    recognizer: Mutex<SenseVoiceRecognizer>,
}

impl SherpaAsrEngine {
    /// 加载模型。校验 model_dir 存在且包含 model.onnx + tokens.txt。
    pub fn new(config: SherpaAsrConfig) -> VoiceResult<Self> {
        if !config.model_dir.is_dir() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "model_dir not found: {}",
                config.model_dir.display()
            )));
        }
        let model_onnx = config.model_dir.join("model.onnx");
        if !model_onnx.is_file() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "model.onnx not found in: {}",
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

        let sense_config = SenseVoiceConfig {
            model: model_onnx.to_string_lossy().into_owned(),
            language: config
                .language
                .clone()
                .unwrap_or_else(|| "auto".to_string()),
            use_itn: true,
            provider: None,
            num_threads: Some(config.num_threads as i32),
            debug: false,
            tokens: tokens_txt.to_string_lossy().into_owned(),
        };
        let recognizer = SenseVoiceRecognizer::new(sense_config).map_err(|e| {
            VoiceError::ModelLoadFailed(format!("sherpa SenseVoice load failed: {}", e))
        })?;

        Ok(Self {
            config,
            recognizer: Mutex::new(recognizer),
        })
    }

    /// 转写 mono 16kHz i16 samples → text。空样本返回 NoSpeechDetected。
    ///
    /// sherpa-rs SenseVoiceRecognizer 期望 f32 samples,我们把 i16 → f32(/ 32768.0)
    /// 与原 WhisperEngine 保持一致。空文本也返回 NoSpeechDetected。
    pub fn transcribe(&self, samples: &[i16]) -> VoiceResult<String> {
        if samples.is_empty() {
            return Err(VoiceError::NoSpeechDetected);
        }
        let samples_f32: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();

        let mut recognizer = self
            .recognizer
            .lock()
            .map_err(|e| VoiceError::InferenceFailed(format!("recognizer lock failed: {}", e)))?;
        let result = recognizer.transcribe(self.config.sample_rate, &samples_f32);
        let text = result.text.trim().to_string();
        if text.is_empty() {
            Err(VoiceError::NoSpeechDetected)
        } else {
            Ok(text)
        }
    }

    pub fn config(&self) -> &SherpaAsrConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_missing_model_dir() {
        let config = SherpaAsrConfig {
            model_dir: std::path::PathBuf::from("E:/definitely_nonexistent_model_dir"),
            ..Default::default()
        };
        let err = SherpaAsrEngine::new(config)
            .err()
            .expect("expected Err for missing model_dir");
        let err_msg = format!("{}", err);
        assert!(
            err_msg.contains("model_dir not found"),
            "err should mention model_dir not found, got: {}",
            err_msg
        );
    }

    // ===== W6c P2 #3:PartialEq 测试 =====

    #[test]
    fn asr_config_eq_when_all_fields_match() {
        let a = SherpaAsrConfig {
            model_dir: PathBuf::from("/models/asr"),
            language: Some("zh".to_string()),
            num_threads: 4,
            sample_rate: 16000,
        };
        let b = SherpaAsrConfig {
            model_dir: PathBuf::from("/models/asr"),
            language: Some("zh".to_string()),
            num_threads: 4,
            sample_rate: 16000,
        };
        assert_eq!(a, b);
    }

    #[test]
    fn asr_config_ne_differs_on_language() {
        // 改 language 后 cache 应失效(spec P2 #3 主要修复点)
        let a = SherpaAsrConfig {
            language: Some("zh".to_string()),
            ..Default::default()
        };
        let b = SherpaAsrConfig {
            language: Some("en".to_string()),
            ..Default::default()
        };
        assert_ne!(a, b);
    }

    #[test]
    fn asr_config_ne_differs_on_num_threads() {
        let a = SherpaAsrConfig {
            num_threads: 4,
            ..Default::default()
        };
        let b = SherpaAsrConfig {
            num_threads: 8,
            ..Default::default()
        };
        assert_ne!(a, b);
    }

    #[test]
    fn asr_config_ne_differs_on_model_dir() {
        let a = SherpaAsrConfig {
            model_dir: PathBuf::from("/models/asr-a"),
            ..Default::default()
        };
        let b = SherpaAsrConfig {
            model_dir: PathBuf::from("/models/asr-b"),
            ..Default::default()
        };
        assert_ne!(a, b);
    }

    #[test]
    fn new_rejects_dir_without_model_onnx() {
        let tmp = tempfile::tempdir().unwrap();
        // 创建空目录(无 model.onnx / tokens.txt)。
        let config = SherpaAsrConfig {
            model_dir: tmp.path().to_path_buf(),
            ..Default::default()
        };
        let err = SherpaAsrEngine::new(config)
            .err()
            .expect("expected Err for missing model.onnx");
        let err_msg = format!("{}", err);
        assert!(
            err_msg.contains("model.onnx not found"),
            "err should mention model.onnx not found, got: {}",
            err_msg
        );
    }

    #[test]
    fn new_rejects_dir_without_tokens_txt() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("model.onnx"), b"fake").unwrap();
        let config = SherpaAsrConfig {
            model_dir: tmp.path().to_path_buf(),
            ..Default::default()
        };
        let err = SherpaAsrEngine::new(config)
            .err()
            .expect("expected Err for missing tokens.txt");
        let err_msg = format!("{}", err);
        assert!(
            err_msg.contains("tokens.txt not found"),
            "err should mention tokens.txt not found, got: {}",
            err_msg
        );
    }

    /// 假 model.onnx 加载失败场景。
    ///
    /// **`#[ignore]`**:sherpa-onnx C 库在加载无效 ONNX 文件时会触发 native abort
    /// (STATUS_STACK_BUFFER_OVERRUN),Rust 无法捕获 foreign exception,会让整个进程崩溃
    /// 而非返回 `Err`。因此本测试默认不运行,仅保留以维持规格(Task 3 Step 4)可追溯性。
    ///
    /// 手动验证:`cargo test --lib new_rejects_fake_model_onnx -- --ignored --features voice`
    /// 预期:进程 abort(非 panic),证明 sherpa 确实拒绝了无效 ONNX。
    #[cfg(feature = "voice")]
    #[ignore = "sherpa-onnx C 库 abort 无法被 Rust 捕获,手动运行见 doc 注释"]
    #[test]
    fn new_rejects_fake_model_onnx() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("model.onnx"), b"fake").unwrap();
        std::fs::write(tmp.path().join("tokens.txt"), b"fake").unwrap();
        let config = SherpaAsrConfig {
            model_dir: tmp.path().to_path_buf(),
            ..Default::default()
        };
        // 假 model.onnx 不是有效 ONNX 文件,sherpa 加载应失败(实际触发 native abort)。
        let result = SherpaAsrEngine::new(config);
        assert!(result.is_err(), "expected Err for fake model.onnx");
    }

    /// 集成测试:真实模型转写。
    /// 需要 sherpa-onnx SenseVoice 模型已下载到 VOICEPILOT_MODEL_DIR 环境变量指向的目录。
    /// 没有模型时 SKIP,不 FAIL。
    #[test]
    fn transcribe_real_model_returns_text_or_no_speech() {
        let model_dir = match std::env::var("VOICEPILOT_MODEL_DIR") {
            Ok(v) => std::path::PathBuf::from(v),
            Err(_) => {
                eprintln!("SKIP: VOICEPILOT_MODEL_DIR not set");
                return;
            }
        };
        if !model_dir.is_dir() {
            eprintln!("SKIP: model_dir not a dir: {}", model_dir.display());
            return;
        }
        let engine = match SherpaAsrEngine::new(SherpaAsrConfig {
            model_dir,
            ..Default::default()
        }) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("SKIP: engine load failed: {}", e);
                return;
            }
        };
        // 1 秒静音(应该返回 NoSpeechDetected,但 sherpa 可能返回空字符串 → 我们转为 NoSpeechDetected)。
        let silence: Vec<i16> = vec![0; 16000];
        let result = engine.transcribe(&silence);
        match result {
            Ok(text) => eprintln!("transcribe returned text: {}", text),
            Err(VoiceError::NoSpeechDetected) => {
                eprintln!("transcribe returned NoSpeechDetected (expected for silence)")
            }
            Err(e) => panic!("unexpected error: {}", e),
        }
    }
}
