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
