#![cfg(feature = "voice")]

use trust_kernel::voice::error::VoiceError;

#[test]
fn voice_error_displays_human_readable_messages() {
    assert_eq!(
        VoiceError::ModelMissing("ggml-tiny.bin".into()).to_string(),
        "voice model missing: ggml-tiny.bin (run `voicepilot voice list-models` for download instructions)"
    );
    assert_eq!(
        VoiceError::MicDenied.to_string(),
        "microphone access denied"
    );
    assert_eq!(
        VoiceError::InferenceFailed("out of memory".into()).to_string(),
        "whisper inference failed: out of memory"
    );
    assert_eq!(
        VoiceError::InvalidWav("truncated header".into()).to_string(),
        "invalid WAV file: truncated header"
    );
    assert_eq!(
        VoiceError::NoSpeechDetected.to_string(),
        "no speech detected in audio"
    );
}

use trust_kernel::voice::model::ModelRegistry;

#[test]
fn model_registry_resolves_default_model_path() {
    let home = std::env::temp_dir().join("vp-w5-model-test-home");
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(&home).unwrap();

    let registry = ModelRegistry::with_home_dir(home.clone());
    let spec = registry.default_model();
    assert_eq!(spec.name, "ggml-tiny.bin");
    assert_eq!(spec.path, home.join(".voicepilot").join("models").join("ggml-tiny.bin"));
    assert!(spec.size_hint_mb >= 70 && spec.size_hint_mb <= 80);
}

#[test]
fn model_registry_detects_existing_model_file() {
    let home = std::env::temp_dir().join("vp-w5-model-test-home-2");
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(home.join(".voicepilot").join("models")).unwrap();
    std::fs::write(
        home.join(".voicepilot").join("models").join("ggml-tiny.bin"),
        b"fake model bytes"
    ).unwrap();

    let registry = ModelRegistry::with_home_dir(home.clone());
    assert!(registry.is_model_present("ggml-tiny.bin"));
    assert!(!registry.is_model_present("ggml-base.bin"));
}

#[test]
fn model_registry_lists_all_known_models() {
    let home = std::env::temp_dir().join("vp-w5-model-test-home-3");
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(&home).unwrap();

    let registry = ModelRegistry::with_home_dir(home);
    let all = registry.all_known_models();
    let names: Vec<&str> = all.iter().map(|m| m.name).collect();
    assert!(names.contains(&"ggml-tiny.bin"));
    assert!(names.contains(&"ggml-base.bin"));
    assert!(names.contains(&"ggml-small.bin"));
    // Sorted by size_hint_mb ascending (tiny first).
    assert_eq!(all[0].name, "ggml-tiny.bin");
}

#[test]
fn model_registry_resolve_returns_missing_error_for_absent_file() {
    let home = std::env::temp_dir().join("vp-w5-model-test-home-4");
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(&home).unwrap();

    let registry = ModelRegistry::with_home_dir(home);
    let result = registry.resolve("ggml-tiny.bin");
    assert!(matches!(result, Err(VoiceError::ModelMissing(_))));
}

#[test]
fn model_registry_resolve_returns_path_for_present_file() {
    let home = std::env::temp_dir().join("vp-w5-model-test-home-5");
    std::fs::remove_dir_all(&home).ok();
    let models_dir = home.join(".voicepilot").join("models");
    std::fs::create_dir_all(&models_dir).unwrap();
    std::fs::write(models_dir.join("ggml-tiny.bin"), b"fake").unwrap();

    let registry = ModelRegistry::with_home_dir(home);
    let path = registry.resolve("ggml-tiny.bin").unwrap();
    assert!(path.ends_with("ggml-tiny.bin"));
}

use trust_kernel::voice::wav::{read_wav, write_wav};

#[test]
fn wav_write_then_read_round_trip_preserves_samples() {
    let dir = std::env::temp_dir().join("vp-w5-wav-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("round_trip.wav");

    let samples: Vec<i16> = vec![0, 1000, -1000, 32767, -32768, 500];
    write_wav(&path, &samples, 16000).unwrap();

    let (read_samples, sample_rate) = read_wav(&path).unwrap();
    assert_eq!(sample_rate, 16000);
    assert_eq!(read_samples, samples);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn wav_read_returns_invalid_wav_error_for_non_wav_file() {
    let dir = std::env::temp_dir().join("vp-w5-wav-test-2");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("not_a_wav.wav");
    std::fs::write(&path, b"this is not a wav file").unwrap();

    let result = read_wav(&path);
    assert!(matches!(result, Err(VoiceError::InvalidWav(_))));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn wav_read_returns_invalid_wav_error_for_missing_file() {
    let path = std::path::Path::new("/definitely/nonexistent/w5-test.wav");
    let result = read_wav(path);
    assert!(matches!(result, Err(VoiceError::InvalidWav(_))));
}

#[test]
fn wav_write_creates_parent_dirs_if_missing() {
    let dir = std::env::temp_dir().join("vp-w5-wav-test-3").join("nested").join("deeper");
    let path = dir.join("out.wav");
    write_wav(&path, &[100, 200, 300], 16000).unwrap();
    assert!(path.is_file());
    std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap()).ok();
}

use trust_kernel::voice::vad::{VadDetector, VadConfig, VadOutcome};

#[test]
fn vad_returns_speech_when_samples_above_threshold() {
    let config = VadConfig {
        frame_ms: 20,
        sample_rate: 16000,
        energy_threshold: 100.0,
        min_speech_ms: 100,
        max_silence_ms: 700,
    };
    let vad = VadDetector::new(config);
    // 500ms of loud samples (sine-like), all above threshold.
    let samples: Vec<i16> = (0..8000).map(|i| (i % 100) as i16 * 100).collect();
    let outcome = vad.detect(&samples);
    assert!(matches!(outcome, VadOutcome::Speech { .. }));
}

#[test]
fn vad_returns_no_speech_when_all_samples_silent() {
    let config = VadConfig {
        frame_ms: 20,
        sample_rate: 16000,
        energy_threshold: 100.0,
        min_speech_ms: 100,
        max_silence_ms: 700,
    };
    let vad = VadDetector::new(config);
    let samples: Vec<i16> = vec![0; 16000]; // 1 second of silence
    let outcome = vad.detect(&samples);
    assert!(matches!(outcome, VadOutcome::NoSpeech));
}

#[test]
fn vad_detects_silence_after_speech_with_correct_boundary() {
    let config = VadConfig {
        frame_ms: 20,
        sample_rate: 16000,
        energy_threshold: 100.0,
        min_speech_ms: 100,
        max_silence_ms: 200, // 200ms of silence ends speech
    };
    let vad = VadDetector::new(config);
    // 300ms loud (4800 samples) + 400ms silent (6400 samples) = 11200 total
    let mut samples: Vec<i16> = (0..4800).map(|i| (i % 100) as i16 * 100).collect();
    samples.extend(vec![0i16; 6400]);
    let outcome = vad.detect(&samples);
    match outcome {
        VadOutcome::Speech { speech_end_sample, .. } => {
            // Speech ends ~4800 + 200ms silence = 4800 + 3200 = 8000
            assert!(
                (7000..=9000).contains(&speech_end_sample),
                "speech_end_sample {} should be near 8000",
                speech_end_sample
            );
        }
        other => panic!("expected Speech, got {:?}", other),
    }
}

#[test]
fn vad_ignores_speech_shorter_than_min_speech_ms() {
    let config = VadConfig {
        frame_ms: 20,
        sample_rate: 16000,
        energy_threshold: 100.0,
        min_speech_ms: 500, // require 500ms of speech
        max_silence_ms: 700,
    };
    let vad = VadDetector::new(config);
    // Only 100ms of loud samples (below min_speech_ms).
    let mut samples: Vec<i16> = (0..1600).map(|i| (i % 100) as i16 * 100).collect();
    samples.extend(vec![0i16; 6400]);
    let outcome = vad.detect(&samples);
    assert!(matches!(outcome, VadOutcome::NoSpeech));
}

use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::approval::approver::AutoApprover;

#[test]
fn router_bridge_routes_files_organize_intent() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let outcome = route_text(
        &kernel,
        &approver,
        "把下载目录里的 PDF 整理到论文文件夹",
    )
    .unwrap();
    match outcome {
        RouteOutcome::Routed { skill_id, .. } => {
            assert_eq!(skill_id, "files.organize");
        }
        other => panic!("expected Routed, got {:?}", other),
    }
}

#[test]
fn router_bridge_returns_unmatched_for_unknown_intent() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, "random unrelated text without keywords").unwrap();
    assert!(matches!(outcome, RouteOutcome::Unmatched { .. }));
}

#[test]
fn router_bridge_returns_empty_for_blank_input() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, "   ").unwrap();
    assert!(matches!(outcome, RouteOutcome::Empty));
}

#[test]
fn router_bridge_routes_with_skill_keyword_match() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let approver = AutoApprover;
    // "整理" + "下载" — should match files.organize via keyword fallback.
    let outcome = route_text(&kernel, &approver, "整理下载文件夹").unwrap();
    match outcome {
        RouteOutcome::Routed { skill_id, .. } => assert_eq!(skill_id, "files.organize"),
        other => panic!("expected Routed, got {:?}", other),
    }
}
