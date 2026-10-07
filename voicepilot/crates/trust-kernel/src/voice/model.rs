//! ModelRegistry — resolves sherpa-onnx SenseVoice model directory under ~/.voicepilot/models/.
//!
//! W6b-3b:whisper.cpp 单文件 ggml 模型已被 sherpa-onnx SenseVoice 目录模型取代。
//! 默认模型为 `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17`(目录形式,
//! 内含 `model.onnx` + `tokens.txt`,约 1 GB 压缩 tar.bz2)。
//! 用户可手动下载(CLI 打印 URL)或调用 `model_download::download_model` 自动下载解压。

use crate::voice::error::{VoiceError, VoiceResult};
use std::path::PathBuf;

/// 默认 SenseVoice 模型目录名(github releases 上的 tar.bz2 解压后顶层目录名)。
pub const SENSE_VOICE_DIR_NAME: &str = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17";

/// 默认 SenseVoice 模型 tar.bz2 下载 URL(github releases)。
pub const SENSE_VOICE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17.tar.bz2";

/// SenseVoice 压缩包大小(用于进度提示,单位 MB,四舍五入)。
pub const SENSE_VOICE_SIZE_MB: u32 = 1048;

/// SenseVoice pinned archive size in exact bytes.
pub const SENSE_VOICE_ARCHIVE_SIZE_BYTES: u64 = 1_047_870_769;

/// SenseVoice pinned archive SHA-256 digest.
pub const SENSE_VOICE_ARCHIVE_SHA256: &str =
    "f6b2a72ebcb1ac7a764d4cfccd886e6bcb2a95c4657c2199d0ba95ed4b9ea71a";

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub name: &'static str,
    pub path: PathBuf,
    pub size_hint_mb: u32,
    pub archive_size_bytes: u64,
    pub archive_sha256: &'static str,
    pub download_url: &'static str,
}

pub struct ModelRegistry {
    home_dir: PathBuf,
    /// 旧版模型目录根(测试可注入):`%LOCALAPPDATA%`。生产用
    /// `dirs::data_local_dir()`;None 表示不探测旧路径。
    local_data_root: Option<PathBuf>,
}

impl ModelRegistry {
    /// Use the user's home directory from %USERPROFILE% (Windows-only).
    pub fn new() -> Self {
        let home_dir = dirs_or_fallback();
        Self {
            home_dir,
            local_data_root: None,
        }
    }

    /// Test-only constructor with explicit home directory.
    pub fn with_home_dir(home_dir: PathBuf) -> Self {
        Self {
            home_dir,
            local_data_root: None,
        }
    }

    /// Test-only constructor with explicit home + `%LOCALAPPDATA%` root
    /// (legacy model dir probe).
    pub fn with_dirs(home_dir: PathBuf, local_data_root: Option<PathBuf>) -> Self {
        Self {
            home_dir,
            local_data_root,
        }
    }

    /// Test-only constructor that injects a legacy `%LOCALAPPDATA%` root.
    pub fn with_legacy_probe(local_data_root: PathBuf) -> Self {
        Self {
            home_dir: dirs_or_fallback(),
            local_data_root: Some(local_data_root),
        }
    }

    fn models_dir(&self) -> PathBuf {
        self.home_dir.join(".voicepilot").join("models")
    }

    /// Wave 3 Task 3.2:旧版模型目录 `%LOCALAPPDATA%\voicepilot\models`。
    /// 兼容探测:若 canonical 无该模型而旧路径有,直接使用旧路径(不复制大文件)。
    fn legacy_models_dir(&self) -> PathBuf {
        let root = self
            .local_data_root
            .clone()
            .or_else(dirs::data_local_dir)
            .unwrap_or_default();
        root.join("voicepilot").join("models")
    }

    fn spec_for(&self, name: &'static str, size_mb: u32, url: &'static str) -> ModelSpec {
        ModelSpec {
            name,
            path: self.models_dir().join(name),
            size_hint_mb: size_mb,
            archive_size_bytes: SENSE_VOICE_ARCHIVE_SIZE_BYTES,
            archive_sha256: SENSE_VOICE_ARCHIVE_SHA256,
            download_url: url,
        }
    }

    /// Default model: sherpa-onnx SenseVoice (zh-en-ja-ko-yue, ~1 GB tar.bz2).
    pub fn default_model(&self) -> ModelSpec {
        self.spec_for(SENSE_VOICE_DIR_NAME, SENSE_VOICE_SIZE_MB, SENSE_VOICE_URL)
    }

    /// All known models, sorted by size ascending.
    ///
    /// W6b-3b:目前仅支持 SenseVoice 一个模型(whisper.cpp ggml 模型已弃用)。
    pub fn all_known_models(&self) -> Vec<ModelSpec> {
        vec![self.spec_for(SENSE_VOICE_DIR_NAME, SENSE_VOICE_SIZE_MB, SENSE_VOICE_URL)]
    }

    /// 检查指定模型(目录形式)是否已存在(canonical 或旧版兼容路径)。
    ///
    /// sherpa-onnx 模型是目录而非单文件,故用 `is_dir()`。
    pub fn is_model_present(&self, name: &str) -> bool {
        self.models_dir().join(name).is_dir() || self.legacy_models_dir().join(name).is_dir()
    }

    /// 解析模型目录路径(canonical 优先,其次旧版 `%LOCALAPPDATA%` 兼容路径)。
    ///
    /// 返回模型目录 PathBuf(由调用方或 `SherpaAsrEngine::new` 进一步校验
    /// `model.onnx` + `tokens.txt` 存在)。
    pub fn resolve(&self, model_id: &str) -> VoiceResult<PathBuf> {
        let model_root = self.models_dir();
        let model_dir = model_root.join(model_id);
        if model_dir.is_dir() {
            return Ok(model_dir);
        }
        let legacy_dir = self.legacy_models_dir().join(model_id);
        if legacy_dir.is_dir() {
            return Ok(legacy_dir);
        }
        Err(VoiceError::ModelMissing(format!(
            "model dir not found: {} (expected at {} or legacy {})",
            model_id,
            model_dir.display(),
            legacy_dir.display()
        )))
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
