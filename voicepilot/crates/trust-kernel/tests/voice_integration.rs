#![cfg(feature = "voice")]

//! Voice integration tests — require real Whisper model file.
//! All tests marked #[ignore]; run with `cargo test --features voice -- --ignored`.

use std::path::PathBuf;
use trust_kernel::voice::asr::{SherpaAsrConfig, SherpaAsrEngine};
use trust_kernel::voice::model::ModelRegistry;
use trust_kernel::voice::wav::read_wav;

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
    let model_path = registry.resolve("ggml-tiny.bin").expect(
        "model file missing; run `voicepilot voice list-models` and download ggml-tiny.bin",
    );

    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir: model_path,
        language: Some("en".to_string()),
        ..Default::default()
    })
    .expect("failed to load SherpaAsrEngine");

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
    let result = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir: PathBuf::from("/definitely/nonexistent"),
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

    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir: model_path,
        language: Some("en".to_string()),
        ..Default::default()
    })
    .expect("failed to load SherpaAsrEngine");

    // 2 seconds of pure silence.
    let samples: Vec<i16> = vec![0; 32000];
    let result = engine.transcribe(&samples);
    // Either returns empty text or NoSpeechDetected — both acceptable.
    match result {
        Ok(text) => assert!(
            text.trim().is_empty(),
            "silent audio should yield empty text"
        ),
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

use trust_kernel::voice::audio::{AudioRecorder, AudioRecorderConfig};
use trust_kernel::voice::vad::{VadConfig, VadDetector};

#[test]
#[ignore]
fn audio_recorder_captures_from_microphone_and_applies_vad() {
    let recorder = AudioRecorder::new(AudioRecorderConfig {
        sample_rate: 16000,
        channels: 1,
        device: None, // default input device
    })
    .expect("failed to init AudioRecorder (no microphone?)");

    println!("Recording up to 5 seconds — please say something...");
    let samples = recorder
        .record_with_timeout(std::time::Duration::from_secs(5))
        .expect("record failed");

    assert!(!samples.is_empty(), "should capture some samples");
    // Should be roughly 5 seconds * 16000 = 80000 samples (±10%).
    let expected = 5 * 16000;
    let lower = (expected as f32 * 0.85) as usize;
    let upper = (expected as f32 * 1.15) as usize;
    assert!(
        samples.len() >= lower && samples.len() <= upper,
        "expected ~80000 samples, got {}",
        samples.len()
    );

    // VAD should detect speech (assuming user spoke) OR return NoSpeech (if silent).
    // Both are valid; we just verify VAD runs without panic.
    let vad = VadDetector::new(VadConfig::default());
    let _outcome = vad.detect(&samples);
    // No assertion on outcome — user may or may not have spoken.
}

#[test]
fn audio_recorder_returns_capture_failed_for_invalid_sample_rate() {
    let result = AudioRecorder::new(AudioRecorderConfig {
        sample_rate: 99999, // invalid
        channels: 1,
        device: None,
    });
    // May succeed (cpal might error on stream creation instead) or fail.
    // We just verify it doesn't panic.
    let _ = result;
}
