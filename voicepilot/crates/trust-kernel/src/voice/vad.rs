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
