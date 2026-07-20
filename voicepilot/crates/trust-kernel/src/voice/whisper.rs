//! WhisperEngine — Whisper.cpp FFI wrapper via whisper-rs.
//!
//! V1.1 §2.1: load ggml model, transcribe mono 16kHz i16 PCM samples → text.
//!
//! Notes:
//!   - Whisper.cpp expects 16kHz mono f32 samples internally; we convert i16 → f32.
//!   - Language hint improves accuracy; None = auto-detect (slower).
//!   - Threads default to 4 (sensible for modern CPUs).

use crate::voice::error::{VoiceError, VoiceResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WhisperConfig {
    pub model_path: PathBuf,
    pub language: Option<String>,
    pub threads: u32,
    pub translate: bool,
    pub print_progress: bool,
    pub print_special: bool,
    pub print_realtime: bool,
    pub print_timestamps: bool,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::new(),
            language: None,
            threads: 4,
            translate: false,
            print_progress: false,
            print_special: false,
            print_realtime: false,
            print_timestamps: false,
        }
    }
}

pub struct WhisperEngine {
    ctx: whisper_rs::WhisperContext,
    config: WhisperConfig,
}

impl WhisperEngine {
    pub fn new(config: WhisperConfig) -> VoiceResult<Self> {
        if !config.model_path.is_file() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "model file not found: {}",
                config.model_path.display()
            )));
        }
        let ctx_params = whisper_rs::WhisperContextParameters::default();
        let ctx = whisper_rs::WhisperContext::new_with_params(
            config.model_path.to_str().ok_or_else(|| {
                VoiceError::ModelLoadFailed("model path is not valid UTF-8".to_string())
            })?,
            ctx_params,
        )
        .map_err(|e| VoiceError::ModelLoadFailed(format!("whisper context load failed: {}", e)))?;
        Ok(Self { ctx, config })
    }

    /// Transcribe mono 16kHz i16 samples → text. Empty samples yield NoSpeechDetected.
    pub fn transcribe(&self, samples: &[i16]) -> VoiceResult<String> {
        if samples.is_empty() {
            return Err(VoiceError::NoSpeechDetected);
        }
        // i16 → f32 in [-1.0, 1.0]
        let samples_f32: Vec<f32> = samples
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect();

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| VoiceError::InferenceFailed(format!("create_state failed: {}", e)))?;

        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
            best_of: 1,
        });
        params.set_n_threads(self.config.threads as i32);
        params.set_translate(self.config.translate);
        params.set_print_progress(self.config.print_progress);
        params.set_print_special(self.config.print_special);
        params.set_print_realtime(self.config.print_realtime);
        params.set_print_timestamps(self.config.print_timestamps);
        if let Some(lang) = &self.config.language {
            params.set_language(Some(lang.as_str()));
        } else {
            params.set_language(None);
        }

        state
            .full(params, &samples_f32)
            .map_err(|e| VoiceError::InferenceFailed(format!("full inference failed: {}", e)))?;

        let segment_count = state
            .full_n_segments()
            .map_err(|e| VoiceError::InferenceFailed(format!("full_n_segments failed: {}", e)))?;

        let mut text = String::new();
        for i in 0..segment_count {
            let segment = state
                .full_get_segment_text(i)
                .map_err(|e| VoiceError::InferenceFailed(format!("get_segment_text failed: {}", e)))?;
            text.push_str(&segment);
            text.push(' ');
        }

        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            Err(VoiceError::NoSpeechDetected)
        } else {
            Ok(trimmed)
        }
    }

    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }
}
