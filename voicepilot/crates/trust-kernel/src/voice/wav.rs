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
