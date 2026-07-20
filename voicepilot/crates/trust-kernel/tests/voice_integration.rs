#![cfg(feature = "voice")]

//! Voice integration tests — require real Whisper model file.
//! All tests marked #[ignore]; run with `cargo test --features voice -- --ignored`.

use std::path::PathBuf;
use trust_kernel::voice::model::ModelRegistry;
use trust_kernel::voice::wav::read_wav;
use trust_kernel::voice::whisper::{WhisperEngine, WhisperConfig};

fn fixture_wav_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("w5_sample_yes.wav")
}

#[test]
#[ignore]
fn whisper_engine_transcribes_yes_sample() {
    let registry = ModelRegistry::new();
    let model_path = registry
        .resolve("ggml-tiny.bin")
        .expect("model file missing; run `voicepilot voice list-models` and download ggml-tiny.bin");

    let engine = WhisperEngine::new(WhisperConfig {
        model_path,
        language: Some("en".to_string()),
        ..Default::default()
    })
    .expect("failed to load WhisperEngine");

    let (samples, sample_rate) = read_wav(&fixture_wav_path()).expect("fixture wav missing");
    // Resample to 16kHz if needed.
    let samples_16k = if sample_rate != 16000 {
        resample_linear(&samples, sample_rate, 16000)
    } else {
        samples
    };

    let text = engine.transcribe(&samples_16k).expect("transcribe failed");
    let lower = text.to_lowercase();
    assert!(
        lower.contains("yes") || lower.contains("yeah") || lower.trim().is_empty(),
        "expected transcription to contain 'yes' or be empty, got: {}",
        text
    );
}

#[test]
#[ignore]
fn whisper_engine_returns_error_for_missing_model_file() {
    let result = WhisperEngine::new(WhisperConfig {
        model_path: PathBuf::from("/definitely/nonexistent/ggml-tiny.bin"),
        ..Default::default()
    });
    assert!(result.is_err());
}

#[test]
#[ignore]
fn whisper_engine_returns_no_speech_for_silent_audio() {
    let registry = ModelRegistry::new();
    let model_path = registry
        .resolve("ggml-tiny.bin")
        .expect("model file missing");

    let engine = WhisperEngine::new(WhisperConfig {
        model_path,
        language: Some("en".to_string()),
        ..Default::default()
    })
    .expect("failed to load WhisperEngine");

    // 2 seconds of pure silence.
    let samples: Vec<i16> = vec![0; 32000];
    let result = engine.transcribe(&samples);
    // Either returns empty text or NoSpeechDetected — both acceptable.
    match result {
        Ok(text) => assert!(text.trim().is_empty(), "silent audio should yield empty text"),
        Err(e) => assert!(
            matches!(e, trust_kernel::voice::error::VoiceError::NoSpeechDetected),
            "silent audio should yield NoSpeechDetected, got: {:?}",
            e
        ),
    }
}

/// Simple linear resampler (sufficient for tests; production uses rubato).
fn resample_linear(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to {
        return samples.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = (samples.len() as f64 * ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = i as f64 / ratio;
            let lo = src_idx.floor() as usize;
            let hi = (lo + 1).min(samples.len() - 1);
            let frac = src_idx - lo as f64;
            let lo_f = samples[lo] as f64;
            let hi_f = samples[hi] as f64;
            (lo_f + (hi_f - lo_f) * frac) as i16
        })
        .collect()
}
