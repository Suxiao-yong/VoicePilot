//! ModelRegistry — resolves sherpa-onnx SenseVoice model directory under ~/.voicepilot/models/.
//!
//! W6b-3b:whisper.cpp 单文件 ggml 模型已被 sherpa-onnx SenseVoice 目录模型取代。
//! 默认模型为 `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17`(目录形式,
//! 内含 `model.onnx` + `tokens.txt`,~234MB 压缩 tar.bz2)。
//! 用户可手动下载(CLI 打印 URL)或调用 `model_download::download_model` 自动下载解压。

use crate::voice::error::{VoiceError, VoiceResult};
use std::path::PathBuf;

/// 默认 SenseVoice 模型目录名(github releases 上的 tar.bz2 解压后顶层目录名)。
pub const SENSE_VOICE_DIR_NAME: &str = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17";

/// 默认 SenseVoice 模型 tar.bz2 下载 URL(github releases)。
pub const SENSE_VOICE_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17.tar.bz2";

/// SenseVoice 压缩包大小(用于进度提示,单位 MB)。
pub const SENSE_VOICE_SIZE_MB: u32 = 234;

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

    /// Default model: sherpa-onnx SenseVoice (zh-en-ja-ko-yue, ~234MB tar.bz2).
    pub fn default_model(&self) -> ModelSpec {
        self.spec_for(SENSE_VOICE_DIR_NAME, SENSE_VOICE_SIZE_MB, SENSE_VOICE_URL)
    }

    /// All known models, sorted by size ascending.
    ///
    /// W6b-3b:目前仅支持 SenseVoice 一个模型(whisper.cpp ggml 模型已弃用)。
    pub fn all_known_models(&self) -> Vec<ModelSpec> {
        vec![self.spec_for(
            SENSE_VOICE_DIR_NAME,
            SENSE_VOICE_SIZE_MB,
            SENSE_VOICE_URL,
        )]
    }

    /// 检查指定模型(目录形式)是否已存在。
    ///
    /// sherpa-onnx 模型是目录而非单文件,故用 `is_dir()`。
    pub fn is_model_present(&self, name: &str) -> bool {
        self.models_dir().join(name).is_dir()
    }

    /// 解析模型目录路径。
    ///
    /// 返回 `~/.voicepilot/models/<model_id>` 目录 PathBuf(由调用方或
    /// `SherpaAsrEngine::new` 进一步校验 `model.onnx` + `tokens.txt` 存在)。
    pub fn resolve(&self, model_id: &str) -> VoiceResult<PathBuf> {
        let model_root = self.models_dir();
        let model_dir = model_root.join(model_id);
        if model_dir.is_dir() {
            Ok(model_dir)
        } else {
            Err(VoiceError::ModelMissing(format!(
                "model dir not found: {} (expected at {})",
                model_id,
                model_dir.display()
            )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_sense_voice() {
        let reg = ModelRegistry::with_home_dir(PathBuf::from("/tmp/fake-home"));
        let spec = reg.default_model();
        assert_eq!(spec.name, SENSE_VOICE_DIR_NAME);
        assert_eq!(spec.download_url, SENSE_VOICE_URL);
        assert!(spec.size_hint_mb > 0);
        assert!(spec.path.ends_with(SENSE_VOICE_DIR_NAME));
    }

    #[test]
    fn all_known_models_contains_only_sense_voice() {
        let reg = ModelRegistry::with_home_dir(PathBuf::from("/tmp/fake-home"));
        let all = reg.all_known_models();
        assert_eq!(all.len(), 1, "expected only SenseVoice model after W6b-3b");
        assert_eq!(all[0].name, SENSE_VOICE_DIR_NAME);
    }

    #[test]
    fn is_model_present_returns_false_for_nonexistent_dir() {
        let reg = ModelRegistry::with_home_dir(PathBuf::from("/tmp/fake-home"));
        assert!(!reg.is_model_present("definitely_nonexistent_model_dir_xxx"));
    }

    #[test]
    fn resolve_returns_error_for_nonexistent_dir() {
        let reg = ModelRegistry::with_home_dir(PathBuf::from("/tmp/fake-home"));
        let result = reg.resolve("definitely_nonexistent_model_dir_xxx");
        assert!(matches!(result, Err(VoiceError::ModelMissing(_))));
    }

    #[test]
    fn resolve_returns_dir_path_when_present() {
        let home = std::env::temp_dir().join("vp-w6b3b-model-resolve-test");
        std::fs::remove_dir_all(&home).ok();
        let models_dir = home.join(".voicepilot").join("models");
        std::fs::create_dir_all(models_dir.join(SENSE_VOICE_DIR_NAME)).unwrap();

        let reg = ModelRegistry::with_home_dir(home.clone());
        let path = reg.resolve(SENSE_VOICE_DIR_NAME).unwrap();
        assert!(path.is_dir());
        assert!(path.ends_with(SENSE_VOICE_DIR_NAME));

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn is_model_present_detects_existing_dir() {
        let home = std::env::temp_dir().join("vp-w6b3b-model-present-test");
        std::fs::remove_dir_all(&home).ok();
        let models_dir = home.join(".voicepilot").join("models");
        std::fs::create_dir_all(models_dir.join(SENSE_VOICE_DIR_NAME)).unwrap();

        let reg = ModelRegistry::with_home_dir(home.clone());
        assert!(reg.is_model_present(SENSE_VOICE_DIR_NAME));
        assert!(!reg.is_model_present("some-other-nonexistent-model"));

        std::fs::remove_dir_all(&home).ok();
    }
}
