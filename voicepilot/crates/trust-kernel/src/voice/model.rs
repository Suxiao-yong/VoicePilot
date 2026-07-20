//! ModelRegistry — resolves Whisper model file paths from ~/.voicepilot/models/.
//!
//! V1.1 §2.1: W5 supports ggml-tiny.bin (default), ggml-base.bin, ggml-small.bin.
//! Users download manually (CLI prints URLs); W6+ may add auto-download.

use crate::voice::error::{VoiceError, VoiceResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub name: &'static str,
    pub path: PathBuf,
    pub size_hint_mb: u32,
    pub download_url: &'static str,
}

pub struct ModelRegistry {
    home_dir: PathBuf,
}

impl ModelRegistry {
    /// Use the user's home directory from $HOME (Unix) or %USERPROFILE% (Windows).
    pub fn new() -> Self {
        let home_dir = dirs_or_fallback();
        Self { home_dir }
    }

    /// Test-only constructor with explicit home directory.
    pub fn with_home_dir(home_dir: PathBuf) -> Self {
        Self { home_dir }
    }

    fn models_dir(&self) -> PathBuf {
        self.home_dir.join(".voicepilot").join("models")
    }

    fn spec_for(&self, name: &'static str, size_mb: u32, url: &'static str) -> ModelSpec {
        ModelSpec {
            name,
            path: self.models_dir().join(name),
            size_hint_mb: size_mb,
            download_url: url,
        }
    }

    /// Default model: ggml-tiny.bin (fastest, ~75MB).
    pub fn default_model(&self) -> ModelSpec {
        self.spec_for(
            "ggml-tiny.bin",
            75,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        )
    }

    /// All known models, sorted by size ascending.
    pub fn all_known_models(&self) -> Vec<ModelSpec> {
        let mut all = vec![
            self.spec_for(
                "ggml-tiny.bin",
                75,
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
            ),
            self.spec_for(
                "ggml-tiny.en.bin",
                75,
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
            ),
            self.spec_for(
                "ggml-base.bin",
                142,
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
            ),
            self.spec_for(
                "ggml-base.en.bin",
                142,
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
            ),
            self.spec_for(
                "ggml-small.bin",
                466,
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
            ),
        ];
        all.sort_by_key(|m| m.size_hint_mb);
        all
    }

    pub fn is_model_present(&self, name: &str) -> bool {
        self.models_dir().join(name).is_file()
    }

    pub fn resolve(&self, name: &str) -> VoiceResult<PathBuf> {
        let path = self.models_dir().join(name);
        if path.is_file() {
            Ok(path)
        } else {
            Err(VoiceError::ModelMissing(name.to_string()))
        }
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn dirs_or_fallback() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home);
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(profile);
    }
    // Fallback for tests / unusual environments.
    PathBuf::from(".")
}
