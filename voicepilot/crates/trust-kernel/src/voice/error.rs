//! Voice subsystem error types — V1.1 §2.1 (voice input extension).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error(
        "voice model missing: {0} (run `voicepilot voice list-models` for download instructions)"
    )]
    ModelMissing(String),

    #[error("microphone access denied")]
    MicDenied,

    // W6c P2 #1:迁移到 sherpa-rs 后,文案去除 whisper 字眼(语义保持中性)
    #[error("inference failed: {0}")]
    InferenceFailed(String),

    #[error("invalid WAV file: {0}")]
    InvalidWav(String),

    #[error("no speech detected in audio")]
    NoSpeechDetected,

    #[error("audio capture failed: {0}")]
    CaptureFailed(String),

    #[error("model load failed: {0}")]
    ModelLoadFailed(String),

    #[error("model download failed: {0}")]
    DownloadFailed(String),
}

pub type VoiceResult<T> = Result<T, VoiceError>;
