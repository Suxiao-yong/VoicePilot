//! Voice input subsystem — V1.1 §2.1 (voice input extension, W5).
//!
//! Pipeline: audio capture (cpal) → VAD (energy threshold) →
//! Whisper.cpp transcription (whisper-rs) → SkillRouter::route →
//! Skill execution.
//!
//! All modules feature-gated under `voice` feature (default on).

pub mod error;
pub mod model;
pub mod wav;
pub mod vad;
pub mod whisper;
pub mod audio;
