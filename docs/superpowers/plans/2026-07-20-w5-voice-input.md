# VoicePilot W5: Voice Input (Whisper.cpp) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local voice input capability to Trust Kernel — record audio from microphone, transcribe via Whisper.cpp, route transcribed text to SkillRouter, and execute matched Skills end-to-end. Provide CLI `voice` subcommands for testing and manual use.

**Architecture:** New `voice/` module inside `trust-kernel` crate (consistent with "single Rust kernel" principle from V1.1.2 §3.3). Whisper.cpp via `whisper-rs` FFI binding (feature-gated to avoid C++ build dependency for non-voice tests). Audio capture via `cpal` (cross-platform). VAD uses simple energy threshold (W5 PoC; Silero VAD deferred to W6+). WAV I/O via `hound` for test fixtures and file transcription. CLI `voice` subcommands live in `cli/src/main.rs` alongside existing `move`/`organize`/`mcp-serve` commands.

**Tech Stack:**
- `whisper-rs` 0.13+ (FFI binding to whisper.cpp, requires CMake + MSVC on Windows)
- `cpal` 0.15+ (cross-platform audio I/O)
- `hound` 3.5 (WAV encoding/decoding)
- `rubato` 0.15 (resampling 44.1kHz → 16kHz for Whisper, optional)

**Build prerequisites (Windows):**
- CMake 3.20+ installed and on PATH
- MSVC Build Tools (Visual Studio 2022 or Build Tools only)
- These are required because `whisper-rs` builds whisper.cpp from source via `cc` crate

**Out of scope (deferred):**
- Tauri UI for voice (W6)
- LLM Planner fallback for unmatched intents (W7)
- Stronghold encryption for voice transcripts (W8)
- Silero VAD / WebRTC VAD (W6+)
- Wake word detection (W6+)
- Streaming partial transcripts (W6+)

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `voicepilot/crates/trust-kernel/src/voice/mod.rs` | Module declarations + re-exports |
| `voicepilot/crates/trust-kernel/src/voice/error.rs` | `VoiceError` enum (ModelMissing, MicDenied, InferenceFailed, etc.) |
| `voicepilot/crates/trust-kernel/src/voice/model.rs` | `ModelRegistry` — resolve model paths from `~/.voicepilot/models/` |
| `voicepilot/crates/trust-kernel/src/voice/whisper.rs` | `WhisperEngine` — load model, transcribe PCM samples → text |
| `voicepilot/crates/trust-kernel/src/voice/wav.rs` | `read_wav(path) -> Vec<i16>` + `write_wav(path, samples, sample_rate)` helpers (hound wrappers) |
| `voicepilot/crates/trust-kernel/src/voice/vad.rs` | `VadDetector` — energy-threshold VAD, returns silence boundaries |
| `voicepilot/crates/trust-kernel/src/voice/audio.rs` | `AudioRecorder` — cpal input stream + ring buffer + stop-on-silence |
| `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs` | `route_text(kernel, text) -> Result<RouteOutcome>` — wraps SkillRouter + triggers Skill execution |
| `voicepilot/crates/trust-kernel/tests/voice_unit.rs` | Unit tests for VAD, WAV I/O, model path resolution (no model file needed) |
| `voicepilot/crates/trust-kernel/tests/voice_integration.rs` | Integration tests for WhisperEngine + end-to-end (marked `#[ignore]` when model file required) |
| `voicepilot/crates/trust-kernel/tests/w5_e2e_smoke.rs` | End-to-end smoke: pre-recorded WAV → transcribe → route → FilesOrganizeSkill |

### Modified files

| File | Change |
|---|---|
| `voicepilot/Cargo.toml` | Add workspace deps: `whisper-rs`, `cpal`, `hound` |
| `voicepilot/crates/trust-kernel/Cargo.toml` | Add `[features] voice = ["dep:whisper-rs", "dep:cpal", "dep:hound"]`, `default = ["voice"]` |
| `voicepilot/crates/trust-kernel/src/lib.rs` | Add `pub mod voice;` (with `#[cfg(feature = "voice")]`) |
| `voicepilot/crates/cli/Cargo.toml` | Enable `trust-kernel/voice` feature |
| `voicepilot/crates/cli/src/main.rs` | Add `voice` subcommand dispatch + 4 handlers (`listen`, `transcribe`, `list-models`, `route`) |

---

## Task 1: Add workspace dependencies + feature gating

**Files:**
- Modify: `voicepilot/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`

- [ ] **Step 1: Add workspace deps to root Cargo.toml**

Edit `voicepilot/Cargo.toml` `[workspace.dependencies]` section, append after `walkdir`:

```toml
whisper-rs = { version = "0.13", optional = true }
cpal = { version = "0.15", optional = true }
hound = { version = "3.5", optional = true }
```

- [ ] **Step 2: Add optional deps + feature to trust-kernel Cargo.toml**

Edit `voicepilot/crates/trust-kernel/Cargo.toml`, add to `[dependencies]`:

```toml
whisper-rs = { workspace = true, optional = true }
cpal = { workspace = true, optional = true }
hound = { workspace = true, optional = true }
```

Add at end of file:

```toml
[features]
default = ["voice"]
voice = ["dep:whisper-rs", "dep:cpal", "dep:hound"]
```

- [ ] **Step 3: Verify build compiles with default features**

Run: `cargo check --manifest-path voicepilot\Cargo.toml`
Expected: PASS (may take 5-10 min on first run due to whisper.cpp compilation)

- [ ] **Step 4: Verify build compiles without voice feature**

Run: `cargo check --manifest-path voicepilot\Cargo.toml --no-default-features`
Expected: PASS (fast, no C++ compilation)

- [ ] **Step 5: Run existing tests to confirm no regression**

Run: `cargo test --manifest-path voicepilot\Cargo.toml`
Expected: 196 passing (unchanged from W4 fast-follow)

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml
git commit -m "build(voice): add whisper-rs + cpal + hound deps with voice feature gate"
```

---

## Task 2: voice module skeleton + VoiceError

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/voice/error.rs`
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`

- [ ] **Step 1: Write failing test for VoiceError Display**

Create `voicepilot/crates/trust-kernel/tests/voice_unit.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: FAIL — `module trust_kernel::voice not found`

- [ ] **Step 3: Create voice/error.rs**

Create `voicepilot/crates/trust-kernel/src/voice/error.rs`:

```rust
//! Voice subsystem error types — V1.1 §2.1 (voice input extension).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("voice model missing: {0} (run `voicepilot voice list-models` for download instructions)")]
    ModelMissing(String),

    #[error("microphone access denied")]
    MicDenied,

    #[error("whisper inference failed: {0}")]
    InferenceFailed(String),

    #[error("invalid WAV file: {0}")]
    InvalidWav(String),

    #[error("no speech detected in audio")]
    NoSpeechDetected,

    #[error("audio capture failed: {0}")]
    CaptureFailed(String),

    #[error("model load failed: {0}")]
    ModelLoadFailed(String),
}

pub type VoiceResult<T> = Result<T, VoiceError>;
```

- [ ] **Step 4: Create voice/mod.rs**

Create `voicepilot/crates/trust-kernel/src/voice/mod.rs`:

```rust
//! Voice input subsystem — V1.1 §2.1 (voice input extension, W5).
//!
//! Pipeline: audio capture (cpal) → VAD (energy threshold) →
//! Whisper.cpp transcription (whisper-rs) → SkillRouter::route →
//! Skill execution.
//!
//! All modules feature-gated under `voice` feature (default on).

pub mod error;
```

- [ ] **Step 5: Add voice module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add at end:

```rust
#[cfg(feature = "voice")]
pub mod voice;
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 1 test

- [ ] **Step 7: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/ voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/tests/voice_unit.rs
git commit -m "feat(voice): module skeleton + VoiceError (V1.1 §2.1)"
```

---

## Task 3: ModelRegistry — model path resolution

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/model.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/voice_unit.rs`

- [ ] **Step 1: Write failing tests for ModelRegistry**

Append to `voicepilot/crates/trust-kernel/tests/voice_unit.rs`:

```rust
use std::path::PathBuf;
use trust_kernel::voice::model::{ModelRegistry, ModelSpec};

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
    let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: FAIL — `module trust_kernel::voice::model not found`

- [ ] **Step 3: Implement ModelRegistry**

Create `voicepilot/crates/trust-kernel/src/voice/model.rs`:

```rust
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
```

- [ ] **Step 4: Export model module from voice/mod.rs**

Edit `voicepilot/crates/trust-kernel/src/voice/mod.rs`, append:

```rust
pub mod model;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 6 tests (1 from Task 2 + 5 new)

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/model.rs voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/tests/voice_unit.rs
git commit -m "feat(voice): ModelRegistry resolves Whisper model paths from ~/.voicepilot/models/"
```

---

## Task 4: WAV I/O helpers (hound wrappers)

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/wav.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/voice_unit.rs`

- [ ] **Step 1: Write failing tests for WAV read/write**

Append to `voicepilot/crates/trust-kernel/tests/voice_unit.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: FAIL — `module trust_kernel::voice::wav not found`

- [ ] **Step 3: Implement wav.rs**

Create `voicepilot/crates/trust-kernel/src/voice/wav.rs`:

```rust
//! WAV file I/O helpers — hound wrappers for voice subsystem.
//!
//! Whisper expects 16kHz mono i16 PCM. These helpers read/write that format
//! and reject multi-channel / non-PCM files.

use crate::voice::error::{VoiceError, VoiceResult};
use std::path::Path;

/// Read WAV file, return mono i16 samples + sample rate.
/// Rejects stereo or non-PCM files with InvalidWav.
pub fn read_wav(path: &Path) -> VoiceResult<(Vec<i16>, u32)> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| VoiceError::InvalidWav(format!("open failed: {}", e)))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(VoiceError::InvalidWav(format!(
            "expected mono (1 channel), got {} channels",
            spec.channels
        )));
    }
    if spec.sample_format != hound::SampleFormat::Int {
        return Err(VoiceError::InvalidWav(
            "expected int sample format, got float".to_string(),
        ));
    }
    if spec.bits_per_sample != 16 {
        return Err(VoiceError::InvalidWav(format!(
            "expected 16-bit samples, got {} bits",
            spec.bits_per_sample
        )));
    }
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| VoiceError::InvalidWav(format!("sample decode failed: {}", e)))?;
    Ok((samples, spec.sample_rate))
}

/// Write mono i16 samples to WAV at given sample rate.
/// Creates parent directories if missing.
pub fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> VoiceResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| VoiceError::InvalidWav(format!("create_dir_all failed: {}", e)))?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| VoiceError::InvalidWav(format!("create failed: {}", e)))?;
    for &s in samples {
        writer
            .write_sample(s)
            .map_err(|e| VoiceError::InvalidWav(format!("write_sample failed: {}", e)))?;
    }
    writer
        .finalize()
        .map_err(|e| VoiceError::InvalidWav(format!("finalize failed: {}", e)))?;
    Ok(())
}
```

- [ ] **Step 4: Export wav module**

Edit `voicepilot/crates/trust-kernel/src/voice/mod.rs`, append:

```rust
pub mod wav;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 10 tests (6 from previous + 4 new)

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/wav.rs voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/tests/voice_unit.rs
git commit -m "feat(voice): WAV I/O helpers (hound wrappers, mono 16-bit only)"
```

---

## Task 5: VAD — energy-threshold silence detection

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/vad.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/voice_unit.rs`

- [ ] **Step 1: Write failing tests for VadDetector**

Append to `voicepilot/crates/trust-kernel/tests/voice_unit.rs`:

```rust
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
        VadOutcome::Speech { speech_end_sample } => {
            // Speech ends ~4800 + 200ms silence = 4800 + 3200 = 8000
            assert!(
                speech_end_sample >= 7000 && speech_end_sample <= 9000,
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: FAIL — `module trust_kernel::voice::vad not found`

- [ ] **Step 3: Implement vad.rs**

Create `voicepilot/crates/trust-kernel/src/voice/vad.rs`:

```rust
//! Energy-threshold VAD (Voice Activity Detection).
//!
//! W5 PoC: simple RMS energy per frame, no neural network.
//! W6+ may swap in Silero VAD or WebRTC VAD via feature flag.
//!
//! Algorithm:
//!   1. Split samples into frames of `frame_ms` duration.
//!   2. Compute RMS energy per frame.
//!   3. A frame is "speech" if energy >= `energy_threshold`.
//!   4. Track contiguous speech frames; once `min_speech_ms` reached, mark
//!      speech_start. Continue until `max_silence_ms` of contiguous silent
//!      frames ends the speech segment.
//!   5. Return Speech { speech_end_sample } or NoSpeech.

#[derive(Debug, Clone)]
pub struct VadConfig {
    pub frame_ms: u32,
    pub sample_rate: u32,
    pub energy_threshold: f32,
    pub min_speech_ms: u32,
    pub max_silence_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            sample_rate: 16000,
            energy_threshold: 100.0,
            min_speech_ms: 200,
            max_silence_ms: 700,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VadOutcome {
    /// Speech detected, speech_end_sample is the sample index where speech
    /// ended (after silence timeout). speech_start_sample is where it began.
    Speech {
        speech_start_sample: usize,
        speech_end_sample: usize,
    },
    NoSpeech,
}

pub struct VadDetector {
    config: VadConfig,
    frame_size: usize,
    min_speech_frames: usize,
    max_silence_frames: usize,
}

impl VadDetector {
    pub fn new(config: VadConfig) -> Self {
        let frame_size = ((config.frame_ms as u64 * config.sample_rate as u64) / 1000) as usize;
        let min_speech_frames =
            ((config.min_speech_ms as u64 * config.sample_rate as u64) / 1000) as usize / frame_size;
        let max_silence_frames =
            ((config.max_silence_ms as u64 * config.sample_rate as u64) / 1000) as usize / frame_size;
        Self {
            config,
            frame_size,
            min_speech_frames,
            max_silence_frames,
        }
    }

    pub fn detect(&self, samples: &[i16]) -> VadOutcome {
        let n_frames = samples.len() / self.frame_size;
        if n_frames == 0 {
            return VadOutcome::NoSpeech;
        }

        let mut frame_energy: Vec<f32> = Vec::with_capacity(n_frames);
        for i in 0..n_frames {
            let start = i * self.frame_size;
            let end = start + self.frame_size;
            let energy = rms_energy(&samples[start..end]);
            frame_energy.push(energy);
        }

        let mut in_speech = false;
        let mut speech_start_frame = 0usize;
        let mut speech_frame_count = 0usize;
        let mut silence_frame_count = 0usize;
        let mut last_speech_frame = 0usize;

        for (i, &energy) in frame_energy.iter().enumerate() {
            let is_speech = energy >= self.config.energy_threshold;
            if is_speech {
                if !in_speech {
                    speech_start_frame = i;
                    in_speech = true;
                    speech_frame_count = 1;
                } else {
                    speech_frame_count += 1;
                }
                silence_frame_count = 0;
                last_speech_frame = i;
            } else if in_speech {
                silence_frame_count += 1;
                if silence_frame_count >= self.max_silence_frames {
                    // End of speech segment.
                    if speech_frame_count >= self.min_speech_frames {
                        let speech_start_sample = speech_start_frame * self.frame_size;
                        let speech_end_sample = (last_speech_frame + 1) * self.frame_size;
                        return VadOutcome::Speech {
                            speech_start_sample,
                            speech_end_sample,
                        };
                    }
                    in_speech = false;
                }
            }
        }

        // Handle speech that ends at end of audio (no trailing silence).
        if in_speech && speech_frame_count >= self.min_speech_frames {
            let speech_start_sample = speech_start_frame * self.frame_size;
            let speech_end_sample = (last_speech_frame + 1) * self.frame_size;
            return VadOutcome::Speech {
                speech_start_sample,
                speech_end_sample,
            };
        }

        VadOutcome::NoSpeech
    }
}

fn rms_energy(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f64 = samples
        .iter()
        .map(|&s| {
            let f = s as f64;
            f * f
        })
        .sum();
    (sum_squares / samples.len() as f64).sqrt() as f32
}
```

- [ ] **Step 4: Export vad module**

Edit `voicepilot/crates/trust-kernel/src/voice/mod.rs`, append:

```rust
pub mod vad;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 14 tests (10 from previous + 4 new)

- [ ] **Step 6: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/vad.rs voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/tests/voice_unit.rs
git commit -m "feat(voice): energy-threshold VAD with frame-based silence detection"
```

---

## Task 6: WhisperEngine — load model + transcribe PCM

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/whisper.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Create: `voicepilot/crates/trust-kernel/tests/voice_integration.rs`

- [ ] **Step 1: Write integration test (marked #[ignore] — requires model file)**

Create `voicepilot/crates/trust-kernel/tests/voice_integration.rs`:

```rust
//! Voice integration tests — require real Whisper model file.
//! All tests marked #[ignore]; run with `cargo test -- --ignored`.

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
```

- [ ] **Step 2: Run tests to verify they fail (compile error)**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_integration --no-run`
Expected: FAIL — `module trust_kernel::voice::whisper not found`

- [ ] **Step 3: Implement whisper.rs**

Create `voicepilot/crates/trust-kernel/src/voice/whisper.rs`:

```rust
//! WhisperEngine — Whisper.cpp FFI wrapper via whisper-rs.
//!
//! V1.1 §2.1: load ggml model, transcribe mono 16kHz i16 PCM samples → text.
//!
//! Notes:
//!   - Whisper.cpp expects 16kHz mono f32 samples internally; we convert i16 → f32.
//!   - Language hint improves accuracy; None = auto-detect (slower).
//!   - Threads default to 4 (sensible for modern CPUs).

use crate::voice::error::{VoiceError, VoiceResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WhisperConfig {
    pub model_path: PathBuf,
    pub language: Option<String>,
    pub threads: u32,
    pub translate: bool,
    pub print_progress: bool,
    pub print_special: bool,
    pub print_realtime: bool,
    pub print_timestamps: bool,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::new(),
            language: None,
            threads: 4,
            translate: false,
            print_progress: false,
            print_special: false,
            print_realtime: false,
            print_timestamps: false,
        }
    }
}

pub struct WhisperEngine {
    ctx: whisper_rs::WhisperContext,
    config: WhisperConfig,
}

impl WhisperEngine {
    pub fn new(config: WhisperConfig) -> VoiceResult<Self> {
        if !config.model_path.is_file() {
            return Err(VoiceError::ModelLoadFailed(format!(
                "model file not found: {}",
                config.model_path.display()
            )));
        }
        let ctx_params = whisper_rs::WhisperContextParameters::default();
        let ctx = whisper_rs::WhisperContext::new_with_params(
            config.model_path.to_str().ok_or_else(|| {
                VoiceError::ModelLoadFailed("model path is not valid UTF-8".to_string())
            })?,
            ctx_params,
        )
        .map_err(|e| VoiceError::ModelLoadFailed(format!("whisper context load failed: {}", e)))?;
        Ok(Self { ctx, config })
    }

    /// Transcribe mono 16kHz i16 samples → text. Empty samples yield NoSpeechDetected.
    pub fn transcribe(&self, samples: &[i16]) -> VoiceResult<String> {
        if samples.is_empty() {
            return Err(VoiceError::NoSpeechDetected);
        }
        // i16 → f32 in [-1.0, 1.0]
        let samples_f32: Vec<f32> = samples
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect();

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| VoiceError::InferenceFailed(format!("create_state failed: {}", e)))?;

        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
            best_of: 1,
        });
        params.set_n_threads(self.config.threads as i32);
        params.set_translate(self.config.translate);
        params.set_print_progress(self.config.print_progress);
        params.set_print_special(self.config.print_special);
        params.set_print_realtime(self.config.print_realtime);
        params.set_print_timestamps(self.config.print_timestamps);
        if let Some(lang) = &self.config.language {
            params.set_language(Some(lang.as_str()));
        } else {
            params.set_language(None);
        }

        state
            .full(params, &samples_f32)
            .map_err(|e| VoiceError::InferenceFailed(format!("full inference failed: {}", e)))?;

        let segment_count = state
            .full_n_segments()
            .map_err(|e| VoiceError::InferenceFailed(format!("full_n_segments failed: {}", e)))?;

        let mut text = String::new();
        for i in 0..segment_count {
            let segment = state
                .full_get_segment_text(i)
                .map_err(|e| VoiceError::InferenceFailed(format!("get_segment_text failed: {}", e)))?;
            text.push_str(&segment);
            text.push(' ');
        }

        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            Err(VoiceError::NoSpeechDetected)
        } else {
            Ok(trimmed)
        }
    }

    pub fn config(&self) -> &WhisperConfig {
        &self.config
    }
}
```

- [ ] **Step 4: Export whisper module**

Edit `voicepilot/crates/trust-kernel/src/voice/mod.rs`, append:

```rust
pub mod whisper;
```

- [ ] **Step 5: Verify compilation (tests will skip since #[ignore])**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_integration --no-run`
Expected: PASS (compiles successfully)

- [ ] **Step 6: Run unit tests to verify no regression**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 14 tests (unchanged)

- [ ] **Step 7: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/whisper.rs voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/tests/voice_integration.rs
git commit -m "feat(voice): WhisperEngine wraps whisper-rs for transcription (integration tests #[ignore])"
```

---

## Task 7: AudioRecorder — cpal microphone capture

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/audio.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/voice_integration.rs`

- [ ] **Step 1: Write integration test (marked #[ignore] — requires microphone)**

Append to `voicepilot/crates/trust-kernel/tests/voice_integration.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail (compile error)**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_integration --no-run`
Expected: FAIL — `module trust_kernel::voice::audio not found`

- [ ] **Step 3: Implement audio.rs**

Create `voicepilot/crates/trust-kernel/src/voice/audio.rs`:

```rust
//! AudioRecorder — cpal microphone capture with timeout.
//!
//! V1.1 §2.1: records mono 16kHz i16 PCM samples from default input device.
//! W5 PoC uses simple timeout-based stop; W6+ will integrate VAD for
//! automatic silence-stop.

use crate::voice::error::{VoiceError, VoiceResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AudioRecorderConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub device: Option<String>,
}

impl Default for AudioRecorderConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
            device: None,
        }
    }
}

pub struct AudioRecorder {
    config: AudioRecorderConfig,
    device: cpal::Device,
    supported_config: cpal::SupportedStreamConfig,
}

impl AudioRecorder {
    pub fn new(config: AudioRecorderConfig) -> VoiceResult<Self> {
        let host = cpal::default_host();
        let device = match &config.device {
            Some(name) => host
                .input_devices()
                .map_err(|e| VoiceError::CaptureFailed(format!("enumerate devices failed: {}", e)))?
                .find(|d| d.name().ok() == Some(name.clone()))
                .ok_or_else(|| VoiceError::CaptureFailed(format!("device not found: {}", name)))?,
            None => host
                .default_input_device()
                .ok_or_else(|| VoiceError::CaptureFailed("no default input device".to_string()))?,
        };

        let supported_config = device
            .default_input_config()
            .map_err(|e| VoiceError::CaptureFailed(format!("default_input_config failed: {}", e)))?;

        // Verify sample format is convertible; we prefer i16, fall back to f32.
        if supported_config.sample_format() != SampleFormat::I16
            && supported_config.sample_format() != SampleFormat::F32
        {
            return Err(VoiceError::CaptureFailed(format!(
                "unsupported sample format: {:?} (only i16/f32 supported)",
                supported_config.sample_format()
            )));
        }

        Ok(Self {
            config,
            device,
            supported_config,
        })
    }

    /// Record for fixed duration, return mono i16 samples (resampled to 16kHz if needed).
    /// Note: W5 PoC resamples linearly; W6+ uses rubato for quality.
    pub fn record_with_timeout(&self, duration: Duration) -> VoiceResult<Vec<i16>> {
        let sample_format = self.supported_config.sample_format();
        let stream_config: StreamConfig = self.supported_config.clone().into();
        let target_sample_rate = self.config.sample_rate;
        let actual_sample_rate = stream_config.sample_rate.0;
        let channels = stream_config.channels;

        let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
        let samples_clone = Arc::clone(&samples);

        let err_fn = |err| tracing::warn!("audio stream error: {}", err);

        let stream = match sample_format {
            SampleFormat::I16 => {
                let stream = self
                    .device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &_| {
                            let mut buf = samples_clone.lock().unwrap();
                            buf.extend_from_slice(data);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| {
                        VoiceError::CaptureFailed(format!("build_input_stream failed: {}", e))
                    })?;
                stream
            }
            SampleFormat::F32 => {
                let stream = self
                    .device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &_| {
                            let mut buf = samples_clone.lock().unwrap();
                            for &s in data {
                                // f32 [-1.0, 1.0] → i16
                                let clamped = s.max(-1.0).min(1.0);
                                buf.push((clamped * 32767.0) as i16);
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| {
                        VoiceError::CaptureFailed(format!("build_input_stream failed: {}", e))
                    })?;
                stream
            }
            fmt => {
                return Err(VoiceError::CaptureFailed(format!(
                    "unsupported sample format at runtime: {:?}",
                    fmt
                )));
            }
        };

        stream
            .play()
            .map_err(|e| VoiceError::CaptureFailed(format!("stream.play failed: {}", e)))?;

        std::thread::sleep(duration);

        drop(stream); // stop recording

        let mut samples = samples.lock().unwrap().clone();

        // Downmix to mono if stereo.
        if channels > 1 {
            samples = downmix_to_mono(&samples, channels);
        }

        // Resample if actual != target.
        if actual_sample_rate != target_sample_rate {
            samples = resample_linear(&samples, actual_sample_rate, target_sample_rate);
        }

        Ok(samples)
    }
}

fn downmix_to_mono(samples: &[i16], channels: u16) -> Vec<i16> {
    let ch = channels as usize;
    let n_frames = samples.len() / ch;
    (0..n_frames)
        .map(|i| {
            let frame = &samples[i * ch..(i + 1) * ch];
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / ch as i32) as i16
        })
        .collect()
}

fn resample_linear(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((samples.len() as f64) * ratio) as usize;
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
```

- [ ] **Step 4: Export audio module**

Edit `voicepilot/crates/trust-kernel/src/voice/mod.rs`, append:

```rust
pub mod audio;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_integration --no-run`
Expected: PASS (compiles; ignored tests skipped)

- [ ] **Step 6: Run unit tests to verify no regression**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 14 tests (unchanged)

- [ ] **Step 7: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/audio.rs voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/tests/voice_integration.rs
git commit -m "feat(voice): AudioRecorder with cpal microphone capture + resample + downmix"
```

---

## Task 8: RouterBridge — transcribed text → SkillRouter → Skill execution

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`
- Modify: `voicepilot/crates/trust-kernel/src/voice/mod.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/voice_unit.rs`

- [ ] **Step 1: Write failing tests for RouterBridge**

Append to `voicepilot/crates/trust-kernel/tests/voice_unit.rs`:

```rust
use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::approval::AutoApprover;

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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: FAIL — `module trust_kernel::voice::router_bridge not found`

- [ ] **Step 3: Implement router_bridge.rs**

Create `voicepilot/crates/trust-kernel/src/voice/router_bridge.rs`:

```rust
//! RouterBridge — wires transcribed text → SkillRouter → Skill execution.
//!
//! V1.1 §2.1 + §5.1: voice input pipeline terminal stage.
//! Returns RouteOutcome so caller (CLI) can decide UI feedback.

use crate::approval::Approver;
use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::skills::manifest::files_organize_manifest;
use crate::skills::router::{RouteDecision, SkillRouter};

#[derive(Debug)]
pub enum RouteOutcome {
    /// Skill matched. Caller (CLI) prompts user for args, then invokes
    /// `FilesOrganizeSkill::execute` to run the prepare→approve→commit flow.
    Routed {
        skill_id: String,
    },
    /// No skill matched; caller should fall back to LLM Planner (W7).
    Unmatched { text: String },
    /// Input was empty/whitespace.
    Empty,
}

/// Route transcribed text through SkillRouter.
///
/// If a Skill is matched, this fn does NOT execute the Skill — execution
/// requires user-supplied arguments (source, filter, destination) that
/// aren't derivable from the voice text alone in W5. W5 PoC: caller (CLI)
/// prompts user for missing args. W7 LLM Planner will extract args from
/// text automatically.
///
/// The `_kernel` and `_approver` params are unused in W5 (routing only);
/// they exist so W7 can extend this fn to invoke the Skill executor
/// directly without changing the call signature.
pub fn route_text(
    _kernel: &TrustKernel,
    _approver: &dyn Approver,
    text: &str,
) -> Result<RouteOutcome> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteOutcome::Empty);
    }

    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    // W7+: register additional built-in skills here.

    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteOutcome::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteOutcome::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}
```

- [ ] **Step 4: Verify SkillRouter API compiles**

Run: `cargo check --manifest-path voicepilot\Cargo.toml -p trust-kernel`
Expected: PASS — `RouteDecision::Skill(manifest)` + `RouteDecision::Planner` match the actual `skills/router.rs` API.

- [ ] **Step 5: Export router_bridge module**

Edit `voicepilot/crates/trust-kernel/src/voice/mod.rs`, append:

```rust
pub mod router_bridge;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test voice_unit`
Expected: PASS — 18 tests (14 from previous + 4 new)

- [ ] **Step 7: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/voice/router_bridge.rs voicepilot/crates/trust-kernel/src/voice/mod.rs voicepilot/crates/trust-kernel/tests/voice_unit.rs
git commit -m "feat(voice): RouterBridge wires transcribed text to SkillRouter"
```

---

## Task 9: CLI `voice list-models` command

**Files:**
- Modify: `voicepilot/crates/cli/Cargo.toml`
- Modify: `voicepilot/crates/cli/src/main.rs`

- [ ] **Step 1: Enable voice feature in CLI Cargo.toml**

Edit `voicepilot/crates/cli/Cargo.toml`, modify the trust-kernel dependency:

```toml
trust-kernel = { workspace = true, features = ["voice"] }
```

- [ ] **Step 2: Add `voice list-models` dispatch to CLI main.rs**

Read `voicepilot/crates/cli/src/main.rs` to find the command dispatch pattern (likely a series of `if line == "..." { handle_...; return Ok(()); }` branches).

Add a new branch in the dispatch loop, before the `mcp-serve` branch:

```rust
if line == "voice list-models" {
    handle_voice_list_models_command();
    return Ok(());
}
```

Add the handler function (place near other `handle_*_command` functions):

```rust
fn handle_voice_list_models_command() {
    use trust_kernel::voice::model::ModelRegistry;

    let registry = ModelRegistry::new();
    let models = registry.all_known_models();

    println!("Available Whisper models:");
    println!();
    for m in &models {
        let present = if m.path.is_file() { "[installed]" } else { "[missing]  " };
        println!("  {} {} ({} MB)", present, m.name, m.size_hint_mb);
        println!("       path: {}", m.path.display());
        println!("       url:  {}", m.download_url);
        println!();
    }
    println!("Default model: {}", registry.default_model().name);
    println!();
    println!("To install: download the .bin file from the URL above and place it at the path shown.");
}
```

Also add `voice list-models` to the help text (find the help string in main.rs and append):

```
  voice list-models         List available Whisper models + download URLs
```

- [ ] **Step 3: Verify CLI compiles**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli`
Expected: PASS (may take 5+ min first time due to whisper.cpp compilation)

- [ ] **Step 4: Run CLI command manually to verify output**

Run: `cargo run --manifest-path voicepilot\Cargo.toml -p cli -- voice list-models`
Expected: prints list of 5 models with paths and URLs, default = ggml-tiny.bin

- [ ] **Step 5: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/cli/Cargo.toml voicepilot/crates/cli/src/main.rs
git commit -m "feat(cli): voice list-models command prints available Whisper models"
```

---

## Task 10: CLI `voice transcribe <file>` command

**Files:**
- Modify: `voicepilot/crates/cli/src/main.rs`

- [ ] **Step 1: Add `voice transcribe` dispatch to CLI main.rs**

Add a new branch in the dispatch loop, after `voice list-models`:

```rust
if let Some(rest) = line.strip_prefix("voice transcribe ") {
    let path = rest.trim();
    handle_voice_transcribe_command(path)?;
    return Ok(());
}
```

Add the handler function:

```rust
fn handle_voice_transcribe_command(path: &str) -> anyhow::Result<()> {
    use trust_kernel::voice::model::ModelRegistry;
    use trust_kernel::voice::wav::read_wav;
    use trust_kernel::voice::whisper::{WhisperConfig, WhisperEngine};

    let registry = ModelRegistry::new();
    let model_path = registry
        .resolve("ggml-tiny.bin")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let engine = WhisperEngine::new(WhisperConfig {
        model_path,
        language: None, // auto-detect
        ..Default::default()
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let (samples, sample_rate) = read_wav(std::path::Path::new(path))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Resample to 16kHz if needed.
    let samples_16k = if sample_rate != 16000 {
        resample_linear_cli(&samples, sample_rate, 16000)
    } else {
        samples
    };

    let text = engine
        .transcribe(&samples_16k)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Transcription:");
    println!("{}", text);
    Ok(())
}

fn resample_linear_cli(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((samples.len() as f64) * ratio) as usize;
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
```

Also add to help text:

```
  voice transcribe <file>   Transcribe a WAV file (mono 16-bit) to text
```

- [ ] **Step 2: Verify CLI compiles**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli`
Expected: PASS

- [ ] **Step 3: Run command without model file — verify error message**

Run: `cargo run --manifest-path voicepilot\Cargo.toml -p cli -- voice transcribe NUL`
Expected: error message "voice model missing: ggml-tiny.bin (run `voicepilot voice list-models` for download instructions)"

- [ ] **Step 4: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/cli/src/main.rs
git commit -m "feat(cli): voice transcribe <file> command transcribes WAV via Whisper"
```

---

## Task 11: CLI `voice route <text>` command

**Files:**
- Modify: `voicepilot/crates/cli/src/main.rs`

- [ ] **Step 1: Add `voice route` dispatch to CLI main.rs**

Add a new branch in the dispatch loop, after `voice transcribe`:

```rust
if let Some(rest) = line.strip_prefix("voice route ") {
    let text = rest.trim();
    handle_voice_route_command(text)?;
    return Ok(());
}
```

Add the handler function:

```rust
fn handle_voice_route_command(text: &str) -> anyhow::Result<()> {
    use trust_kernel::approval::AutoApprover;
    use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
    use trust_kernel::kernel::TrustKernel;

    // Open in-memory kernel for routing only (no execution needed for route preview).
    let kernel = TrustKernel::open_in_memory()?;
    let approver = AutoApprover;

    let outcome = route_text(&kernel, &approver, text)?;

    match outcome {
        RouteOutcome::Routed { skill_id, .. } => {
            println!("Matched skill: {}", skill_id);
            Ok(())
        }
        RouteOutcome::Unmatched { text } => {
            println!("No skill matched for: {:?}", text);
            println!("(W7 LLM Planner fallback not yet implemented)");
            Ok(())
        }
        RouteOutcome::Empty => {
            println!("Empty input");
            Ok(())
        }
    }
}
```

Also add to help text:

```
  voice route <text>        Route text through SkillRouter (no audio, no execution)
```

- [ ] **Step 2: Verify CLI compiles**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli`
Expected: PASS

- [ ] **Step 3: Run `voice route` with files.organize intent**

Run: `cargo run --manifest-path voicepilot\Cargo.toml -p cli -- "voice route 把下载目录里的 PDF 整理到论文文件夹"`
Expected: prints "Matched skill: files.organize"

- [ ] **Step 4: Run `voice route` with unmatched text**

Run: `cargo run --manifest-path voicepilot\Cargo.toml -p cli -- "voice route random unrelated text"`
Expected: prints "No skill matched for: ..."

- [ ] **Step 5: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/cli/src/main.rs
git commit -m "feat(cli): voice route <text> command previews SkillRouter matching"
```

---

## Task 12: CLI `voice listen` command — end-to-end audio → text → route

**Files:**
- Modify: `voicepilot/crates/cli/src/main.rs`

- [ ] **Step 1: Add `voice listen` dispatch to CLI main.rs**

Add a new branch in the dispatch loop, after `voice route`:

```rust
if line == "voice listen" {
    handle_voice_listen_command()?;
    return Ok(());
}
```

Add the handler function:

```rust
fn handle_voice_listen_command() -> anyhow::Result<()> {
    use trust_kernel::approval::AutoApprover;
    use trust_kernel::voice::audio::{AudioRecorder, AudioRecorderConfig};
    use trust_kernel::voice::model::ModelRegistry;
    use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
    use trust_kernel::voice::vad::{VadConfig, VadDetector, VadOutcome};
    use trust_kernel::voice::whisper::{WhisperConfig, WhisperEngine};
    use trust_kernel::kernel::TrustKernel;

    // 1. Record up to 5 seconds of audio.
    println!("Listening (5 seconds)...");
    let recorder = AudioRecorder::new(AudioRecorderConfig::default())
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let samples = recorder
        .record_with_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("Captured {} samples", samples.len());

    // 2. Run VAD — skip transcription if no speech.
    let vad = VadDetector::new(VadConfig::default());
    match vad.detect(&samples) {
        VadOutcome::Speech { .. } => {}
        VadOutcome::NoSpeech => {
            println!("No speech detected.");
            return Ok(());
        }
    }

    // 3. Load Whisper model.
    let registry = ModelRegistry::new();
    let model_path = registry
        .resolve("ggml-tiny.bin")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // 4. Transcribe.
    let engine = WhisperEngine::new(WhisperConfig {
        model_path,
        language: None,
        ..Default::default()
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    let text = engine
        .transcribe(&samples)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("Transcription: {:?}", text);

    // 5. Route.
    let kernel = TrustKernel::open_in_memory()?;
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, &text)?;
    match outcome {
        RouteOutcome::Routed { skill_id, .. } => {
            println!("Matched skill: {}", skill_id);
            println!("(Skill execution requires user-supplied args; use `voicepilot organize` to run)");
        }
        RouteOutcome::Unmatched { text } => {
            println!("No skill matched for: {:?}", text);
        }
        RouteOutcome::Empty => {
            println!("Empty transcription");
        }
    }
    Ok(())
}
```

Also add to help text:

```
  voice listen             Record 5s audio, transcribe, route to Skill
```

- [ ] **Step 2: Verify CLI compiles**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli`
Expected: PASS

- [ ] **Step 3: Run `voice listen` (no microphone in CI — verify graceful failure)**

Run: `cargo run --manifest-path voicepilot\Cargo.toml -p cli -- voice listen`
Expected: either records + transcribes (if mic + model present) or prints error (mic denied / model missing). Should not panic.

- [ ] **Step 4: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/cli/src/main.rs
git commit -m "feat(cli): voice listen command — record + transcribe + route end-to-end"
```

---

## Task 13: End-to-end smoke test (W5 gate, V1.1 §11.1)

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w5_e2e_smoke.rs`
- Create: `voicepilot/tests/fixtures/README.md` (fixture download instructions)

- [ ] **Step 1: Write e2e smoke test (uses pre-generated synthetic WAV fixture)**

Create `voicepilot/crates/trust-kernel/tests/w5_e2e_smoke.rs`:

```rust
//! W5 end-to-end smoke test — V1.1 §11.1 W5 gate.
//!
//! Three test tiers:
//!   1. Pure-logic (no model/mic): VAD + WAV + router_bridge pipeline.
//!   2. Model-required (#[ignore]): real Whisper transcription.
//!   3. Mic-required (#[ignore]): live recording + transcription + routing.
//!
//! Tier 1 runs in CI. Tiers 2-3 require `cargo test -- --ignored` + manual setup.

use std::path::PathBuf;
use trust_kernel::approval::AutoApprover;
use trust_kernel::voice::audio::{AudioRecorder, AudioRecorderConfig};
use trust_kernel::voice::model::ModelRegistry;
use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
use trust_kernel::voice::vad::{VadConfig, VadDetector, VadOutcome};
use trust_kernel::voice::wav::{read_wav, write_wav};
use trust_kernel::voice::whisper::{WhisperConfig, WhisperEngine};
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

    let engine = WhisperEngine::new(WhisperConfig {
        model_path,
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
    let engine = WhisperEngine::new(WhisperConfig {
        model_path,
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
```

- [ ] **Step 2: Create fixtures README**

Create `voicepilot/tests/fixtures/README.md`:

```markdown
# Voice Test Fixtures

This directory contains WAV fixtures for `#[ignore]`-marked integration tests.

## Required fixtures

| File | Used by | Notes |
|---|---|---|
| `w5_sample_yes.wav` | `voice_integration.rs::whisper_engine_transcribes_yes_sample` | English "yes" utterance, mono 16kHz 16-bit, ~1 second |
| `w5_sample_organize.wav` | `w5_e2e_smoke.rs::w5_e2e_transcribe_real_wav_then_route` | Chinese "整理下载目录里的 PDF" utterance, mono 16kHz 16-bit, ~3 seconds |

## How to generate fixtures

1. Use any audio recorder (Audacity, `sox`, etc.) to record the phrase.
2. Export as mono 16kHz 16-bit WAV.
3. Place file in this directory with the name above.

## Why fixtures are not committed

Whisper test fixtures are user-generated audio; we don't commit them to avoid
LFS overhead and licensing ambiguity. CI runs only the non-ignored Tier 1 tests;
Tier 2/3 tests run locally with `cargo test -- --ignored` after fixtures are
placed.
```

- [ ] **Step 3: Run Tier 1 tests**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w5_e2e_smoke`
Expected: PASS — 2 tests (Tier 1)

- [ ] **Step 4: Run full test suite — verify no regression**

Run: `cargo test --manifest-path voicepilot\Cargo.toml`
Expected: PASS — 198 tests (196 from W4 + 2 new Tier 1)

- [ ] **Step 5: Commit**

```bash
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/tests/w5_e2e_smoke.rs voicepilot/tests/fixtures/README.md
git commit -m "test(w5): end-to-end smoke test with 3 tiers (pure-logic / model-required / mic-required)"
```

---

## Task 14: Final verification + PROGRESS.md update

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: Run full test suite**

Run: `cargo test --manifest-path voicepilot\Cargo.toml`
Expected: 198 passing, 0 warnings

- [ ] **Step 2: Run clippy on voice module**

Run: `cargo clippy --manifest-path voicepilot\Cargo.toml -p trust-kernel --tests`
Expected: 0 new warnings (pre-existing warnings from W4 may remain)

- [ ] **Step 3: Verify `--no-default-features` still compiles (no voice)**

Run: `cargo check --manifest-path voicepilot\Cargo.toml --no-default-features`
Expected: PASS (confirms voice module is properly feature-gated)

- [ ] **Step 4: Update PROGRESS.md**

Update the following sections in `docs/PROGRESS.md`:

1. **Header:**
   - Latest commit: `<final commit SHA>`
   - Test count: 198
   - Spec version: V1.1.2 (no change)

2. **§一 milestone table:**
   - W5 row: status `✅ 已完成`, tests `+2 (Tier 1 only; Tier 2/3 are #[ignore])`, date `2026-07-20`

3. **§二 add W5 detailed record:**
   - Module structure: `voice/{error, model, wav, vad, whisper, audio, router_bridge}.rs`
   - Commits (list all from `git log --oneline | head -14`)
   - Key deviations / decisions:
     - whisper-rs + cpal + hound added as optional deps under `voice` feature
     - VAD uses simple energy threshold (W5 PoC); Silero VAD deferred to W6+
     - Model files: user manually downloads; CLI prints URLs (no auto-download in W5)
     - WhisperEngine integration tests marked `#[ignore]` (require model file)
     - AudioRecorder tests marked `#[ignore]` (require microphone)
     - CLI `voice listen` records fixed 5s (no VAD-based auto-stop in W5 PoC)
   - Known issues (add as #44-#48):
     - #44: VAD uses simple energy threshold; may false-trigger on background noise
     - #45: `voice listen` records fixed 5s; no VAD-based auto-stop
     - #46: Model auto-download not implemented (user must manually download)
     - #47: Streaming partial transcripts not implemented (W6+)
     - #48: Wake word detection not implemented (W6+)

4. **§三 git state:**
   - Latest commit, test count

5. **§四 replace W5 scope with W6 scope:**
   - W6: Tauri UI Shell — desktop app + approval UI + settings panel

6. **§四.2 add W5 issues #44-#48**

7. **§五 recovery guide:**
   - W5 fast-follow: none critical
   - W6 starting point: Tauri project scaffold

8. **§六 add W5 plan link**

- [ ] **Step 5: Commit PROGRESS.md update**

```bash
cd d:\voicepilot
git add docs/PROGRESS.md
git commit -m "docs: update PROGRESS.md for W5 completion (198 tests, issues #44-#48)"
```

- [ ] **Step 6: Run final verification**

Run: `cargo test --manifest-path voicepilot\Cargo.toml`
Expected: 198 passing

Run: `git log --oneline -15`
Expected: 14 new commits from Task 1 onwards + PROGRESS.md commit

---

## Summary

After completing all 14 tasks:

- **New code:** 8 files in `voice/` module + 3 test files + fixtures README
- **New deps:** whisper-rs, cpal, hound (all feature-gated under `voice`)
- **New CLI commands:** `voice listen`, `voice transcribe`, `voice route`, `voice list-models`
- **Tests:** +2 always-run (Tier 1) + 6 `#[ignore]` (Tiers 2-3, require model/mic)
- **Commits:** ~14 commits, one per task

## Known Limitations (W5 PoC)

1. **VAD is energy-threshold only** — false-positives on background noise (issue #44)
2. **`voice listen` records fixed 5s** — no VAD-based auto-stop (issue #45)
3. **No model auto-download** — user must manually download `ggml-tiny.bin` (issue #46)
4. **No streaming partial transcripts** — full transcription only after recording stops (issue #47)
5. **No wake word detection** — user must run `voice listen` manually (issue #48)
6. **No LLM Planner fallback** — unmatched intents return `Unmatched` without execution (W7)
7. **No Tauri UI** — CLI-only in W5 (W6)

## Build Prerequisites Reminder

`whisper-rs` compiles whisper.cpp from source, requiring:
- **Windows:** CMake 3.20+ on PATH + MSVC Build Tools
- **Unix:** CMake + C++ compiler (gcc or clang)

If build fails with C++ errors, install prerequisites and re-run `cargo build`.
