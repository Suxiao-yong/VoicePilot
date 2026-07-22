//! Voice input subsystem — V1.1 §2.1 (voice input extension, W5).
//!
//! Pipeline: audio capture (cpal) → VAD (energy threshold) →
//! sherpa-rs OfflineRecognizer 转写 → SkillRouter::route →
//! Skill execution.
//!
//! W6b-3b:whisper-rs → sherpa-rs 迁移(修复 issue #49),新增 tts 子模块(VP-FR-002)。
//!
//! All modules feature-gated under `voice` feature (default on).

pub mod error;
pub mod model;
pub mod wav;
pub mod vad;
pub mod asr;
pub mod tts;
pub mod audio;
pub mod router_bridge;
pub mod listener;
pub mod model_download;
