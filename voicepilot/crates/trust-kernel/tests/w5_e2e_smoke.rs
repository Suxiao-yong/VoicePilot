#![cfg(feature = "voice")]

//! W5 end-to-end smoke test — V1.1 §11.1 W5 gate.
//!
//! Three test tiers:
//!   1. Pure-logic (no model/mic): VAD + WAV + router_bridge pipeline.
//!   2. Model-required (#[ignore]): real Whisper transcription.
//!   3. Mic-required (#[ignore]): live recording + transcription + routing.
//!
//! Tier 1 runs in CI. Tiers 2-3 require `cargo test -- --ignored` + manual setup.

use std::path::PathBuf;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::voice::audio::{AudioRecorder, AudioRecorderConfig};
use trust_kernel::voice::model::ModelRegistry;
use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
use trust_kernel::voice::vad::{VadConfig, VadDetector, VadOutcome};
use trust_kernel::voice::wav::{read_wav, write_wav};
use trust_kernel::voice::asr::{SherpaAsrConfig, SherpaAsrEngine};
use trust_kernel::kernel::TrustKernel;

// ============================================================================
// Tier 1: Pure-logic pipeline (runs in CI, no model/mic required)
// ============================================================================

#[test]
fn w5_e2e_synthetic_audio_pipeline_runs_without_panic() {
    // Generate 1 second of synthetic "speech" (loud samples) + 0.5s silence.
    let mut samples: Vec<i16> = (0..16000).map(|i| (i % 100) as i16 * 100).collect();
    samples.extend(vec![0i16; 8000]);

    // VAD should detect speech.
    let vad = VadDetector::new(VadConfig::default());
    let outcome = vad.detect(&samples);
    assert!(matches!(outcome, VadOutcome::Speech { .. }));

    // Write to WAV, read back, verify round-trip.
    let dir = std::env::temp_dir().join("vp-w5-e2e-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let wav_path = dir.join("synthetic.wav");
    write_wav(&wav_path, &samples, 16000).unwrap();

    let (read_samples, sample_rate) = read_wav(&wav_path).unwrap();
    assert_eq!(sample_rate, 16000);
    assert_eq!(read_samples.len(), samples.len());

    // Router bridge — use a synthetic transcription text.
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, "整理下载目录里的 PDF").unwrap();
    assert!(matches!(outcome, RouteOutcome::Routed { .. }));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn w5_e2e_vad_rejects_silence_before_transcription() {
    // 2 seconds of pure silence — VAD should return NoSpeech, simulating
    // the listen command's early-exit path.
    let samples: Vec<i16> = vec![0; 32000];
    let vad = VadDetector::new(VadConfig::default());
    let outcome = vad.detect(&samples);
    assert!(matches!(outcome, VadOutcome::NoSpeech));
}

// ============================================================================
// Tier 2: Model-required (#[ignore])
// ============================================================================

#[test]
#[ignore]
fn w5_e2e_transcribe_real_wav_then_route() {
    let registry = ModelRegistry::new();
    let model_path = registry
        .resolve("ggml-tiny.bin")
        .expect("model missing; run `voicepilot voice list-models`");

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("w5_sample_organize.wav");
    let (samples, _sr) = read_wav(&fixture).expect("fixture missing");

    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir: model_path,
        language: Some("zh".to_string()),
        ..Default::default()
    })
    .expect("engine init");

    let text = engine.transcribe(&samples).expect("transcribe");
    eprintln!("Transcribed: {:?}", text);

    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, &text).expect("route");
    // We don't assert Matched — the fixture may not contain files.organize keywords.
    // The test just verifies the pipeline runs end-to-end.
    eprintln!("Route outcome: {:?}", outcome);
}

// ============================================================================
// Tier 3: Mic-required (#[ignore])
// ============================================================================

#[test]
#[ignore]
fn w5_e2e_listen_live_microphone_end_to_end() {
    let recorder = AudioRecorder::new(AudioRecorderConfig::default())
        .expect("recorder init (mic?)");
    println!("Speak now (5s)...");
    let samples = recorder
        .record_with_timeout(std::time::Duration::from_secs(5))
        .expect("record");

    let vad = VadDetector::new(VadConfig::default());
    if matches!(vad.detect(&samples), VadOutcome::NoSpeech) {
        eprintln!("No speech detected; skipping transcription");
        return;
    }

    let registry = ModelRegistry::new();
    let model_path = registry.resolve("ggml-tiny.bin").expect("model missing");
    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir: model_path,
        language: None,
        ..Default::default()
    })
    .expect("engine");

    let text = engine.transcribe(&samples).expect("transcribe");
    eprintln!("Heard: {:?}", text);

    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, &text).expect("route");
    eprintln!("Outcome: {:?}", outcome);
}
