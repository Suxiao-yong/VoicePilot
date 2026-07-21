//! AudioRecorder — cpal microphone capture with timeout.
//!
//! V1.1 §2.1: records mono 16kHz i16 PCM samples from default input device.
//! W5 PoC uses simple timeout-based stop; W6+ will integrate VAD for
//! automatic silence-stop.

use crate::voice::error::{VoiceError, VoiceResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
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
                                let clamped = s.clamp(-1.0, 1.0);
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
