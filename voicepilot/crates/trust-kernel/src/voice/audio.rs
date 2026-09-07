//! AudioRecorder — cpal microphone capture, continuous-stream based.
//!
//! V1.1 §2.1: records mono 16kHz i16 PCM samples from default input device.
//!
//! 2026-09 voice 修复(录音效果差根因):
//!   旧实现每次 `record_chunk` 都 build_input_stream + play + sleep + drop,
//!   WASAPI 流启停之间有样本丢失 + 每次启动延迟 → 语音断续,ASR 识别差。
//!   现在**一条常驻流**:callback 把样本推入环形队列,`record_chunk` 只消费,
//!   chunk 之间零丢失、无启停开销。
//!
//! 重采样:旧线性插值无抗混叠(48k→16k 高频镜像折叠进语音频带),换 rubato
//! SincFixedIn 抗混叠重采样。首选直接请求 16kHz mono(WASAPI 共享模式多数
//! 设备不支持自定义 mix format → 自动回退设备默认配置 + downmix + resample)。

use crate::voice::error::{VoiceError, VoiceResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

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

/// 环形队列容量(原始设备帧)。2s @16k 足够覆盖 listen 间隙,满则丢最旧。
const BUF_CAP_FRAMES: usize = 32_000;

pub struct AudioRecorder {
    config: AudioRecorderConfig,
    device: cpal::Device,
    supported_config: cpal::SupportedStreamConfig,
    /// 常驻流回调写入的原始交错样本(设备原生声道),满则丢最旧。
    samples: Arc<(Mutex<VecDeque<i16>>, Condvar)>,
    /// lazy 启动的常驻流 + 实际打开的 (sample_rate, channels)。
    stream: Mutex<Option<(cpal::Stream, u32, u16)>>,
}

// SAFETY: cpal 把 Stream 标记为 !Send/!Sync 是跨平台保守策略(cpal#588),
// Windows WASAPI 下 stream 只持有 IAudioClient COM 句柄,本机使用模式为
// 创建后仅 stay-alive + drop(不跨线程移动),callback 数据经 Arc<Mutex+Condvar>
// 传递,不经过 Stream 本身。项目仅面向 Windows 桌面。
unsafe impl Send for AudioRecorder {}
unsafe impl Sync for AudioRecorder {}

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

        let supported_config = device.default_input_config().map_err(|e| {
            VoiceError::CaptureFailed(format!("default_input_config failed: {}", e))
        })?;

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
            samples: Arc::new((Mutex::new(VecDeque::new()), Condvar::new())),
            stream: Mutex::new(None),
        })
    }

    /// 打开常驻流(幂等)。首选请求目标 16k mono;设备不支持时回退默认配置。
    fn ensure_stream(&self) -> VoiceResult<()> {
        let mut guard = self.stream.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        let sample_format = self.supported_config.sample_format();

        // 首选:直接请求目标采样率/声道(避免重采样;WASAPI 共享模式常失败,失败即回退)。
        for (rate, channels) in [
            (self.config.sample_rate, self.config.channels),
            (
                self.supported_config.sample_rate().0,
                self.supported_config.channels(),
            ),
        ] {
            if let Ok(stream) = self.try_open(
                sample_format,
                StreamConfig {
                    sample_rate: cpal::SampleRate(rate),
                    channels,
                    buffer_size: cpal::BufferSize::Default,
                },
            ) {
                *guard = Some((stream, rate, channels));
                tracing::debug!("audio stream opened at {} Hz / {} ch", rate, channels);
                return Ok(());
            }
        }
        Err(VoiceError::CaptureFailed(
            "build_input_stream failed for configured and default formats".to_string(),
        ))
    }

    fn try_open(
        &self,
        sample_format: SampleFormat,
        stream_config: StreamConfig,
    ) -> VoiceResult<cpal::Stream> {
        let samples = Arc::clone(&self.samples);
        let err_fn = |err| tracing::warn!("audio stream error: {}", err);
        let stream = match sample_format {
            SampleFormat::I16 => self
                .device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &_| {
                        push_samples(&samples, data);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| {
                    VoiceError::CaptureFailed(format!("build_input_stream failed: {}", e))
                })?,
            SampleFormat::F32 => self
                .device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &_| {
                        // f32 [-1.0, 1.0] → i16
                        let converted: Vec<i16> = data
                            .iter()
                            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                            .collect();
                        push_samples(&samples, &converted);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| {
                    VoiceError::CaptureFailed(format!("build_input_stream failed: {}", e))
                })?,
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
        Ok(stream)
    }

    /// Record for fixed duration, return mono i16 samples at `config.sample_rate`.
    ///
    /// 常驻流模式下从环形队列消费,chunk 之间无样本丢失。流未启动时 lazy 启动。
    pub fn record_with_timeout(&self, duration: Duration) -> VoiceResult<Vec<i16>> {
        self.ensure_stream()?;
        let (rate, channels) = {
            let guard = self.stream.lock().unwrap();
            let (_, rate, channels) = guard.as_ref().expect("stream opened above");
            (*rate, *channels)
        };
        let target_rate = self.config.sample_rate;

        // 需要 duration×rate 个原始帧(每帧 channels 个交错样本)。
        let frames_needed = (duration.as_secs_f64() * rate as f64).ceil() as usize;
        let samples_needed = frames_needed * channels as usize;
        let raw = self.read_samples(samples_needed, duration)?;
        let mono = if channels > 1 {
            downmix_to_mono(&raw, channels)
        } else {
            raw
        };
        if rate != target_rate {
            Ok(resample_anti_alias(&mono, rate, target_rate))
        } else {
            Ok(mono)
        }
    }

    /// 从环形队列阻塞读 `need` 个原始样本。流挂死(无数据)超过 3×duration 返回空。
    ///
    /// 增量搬运:环形队列上限 BUF_CAP_FRAMES,长录音的 `need` 可能超过上限
    /// (如 5s @48k 立体声需 480k 样本),等“队列一次攒够 need”永远等不满。
    /// 每次醒来就把已有数据搬进 `out`,攒够即返;超时则返回已攒的部分
    /// (全程无数据才 warn + 返回空)。
    fn read_samples(&self, need: usize, duration: Duration) -> VoiceResult<Vec<i16>> {
        let (lock, cv) = &*self.samples;
        let deadline = Instant::now() + duration * 3 + Duration::from_millis(500);
        let mut out = Vec::with_capacity(need);
        let mut buf = lock.lock().unwrap();
        loop {
            if !buf.is_empty() {
                out.extend(buf.drain(..));
                if out.len() >= need {
                    out.truncate(need);
                    return Ok(out);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                if out.is_empty() {
                    tracing::warn!("audio stream produced no data for {:?}", duration);
                }
                return Ok(out);
            }
            let (guard, _) = cv
                .wait_timeout(buf, deadline.saturating_duration_since(now))
                .unwrap();
            buf = guard;
        }
    }
}

/// 音频线程回调:推入环形队列,满则丢最旧(无人消费时说明不在录音,旧数据无价值)。
fn push_samples(samples: &Arc<(Mutex<VecDeque<i16>>, Condvar)>, data: &[i16]) {
    let (lock, cv) = &**samples;
    let mut buf = lock.lock().unwrap();
    if buf.len() + data.len() > BUF_CAP_FRAMES {
        let overflow = buf.len() + data.len() - BUF_CAP_FRAMES;
        let drop_n = overflow.min(buf.len());
        buf.drain(..drop_n);
    }
    buf.extend(data);
    cv.notify_all();
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

/// Sinc 抗混叠重采样(rubato)。替代旧线性插值:48k→16k 时高频镜像不再折叠进语音频带。
/// SincFixedIn 是流式 resampler,输入分块喂入,输出累积;末段固有滤波延迟(<10ms)可忽略。
fn resample_anti_alias(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let max_chunk = 4096usize.min(samples.len().max(1));
    let mut resampler = match SincFixedIn::<f32>::new(ratio, 1.0, params, max_chunk, 1) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("rubato init failed ({}), falling back to linear", e);
            return resample_linear_legacy(samples, from, to);
        }
    };
    let mut out: Vec<f32> = Vec::new();
    let mut chunks = samples.chunks(max_chunk).peekable();
    while let Some(chunk) = chunks.next() {
        let mut input: Vec<f32> = chunk.iter().map(|&s| s as f32 / 32768.0).collect();
        // SincFixedIn 要求每块恰好 max_chunk 帧,尾块补零对齐(引入 <85ms 静音尾,可忽略),
        // 否则 process 报错导致整体回退线性插值、抗混叠名存实亡。
        if chunks.peek().is_none() && input.len() < max_chunk {
            input.resize(max_chunk, 0.0);
        }
        match resampler.process(&[input], None) {
            Ok(waves) => out.extend_from_slice(&waves[0]),
            Err(e) => {
                tracing::warn!("rubato process failed ({}), falling back to linear", e);
                return resample_linear_legacy(samples, from, to);
            }
        }
    }
    // 补零会多产出几帧,截断到理论长度(与线性兜底公式一致)。
    out.truncate((samples.len() as f64 * ratio) as usize);
    out.iter()
        .map(|&f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// 线性插值 —— 仅 rubato 不可用时的兜底(旧 PoC 行为)。
fn resample_linear_legacy(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_keeps_length_ratio() {
        // 48k → 16k:输出 ≈ 输入/3
        let input: Vec<i16> = (0..48000).map(|i| ((i % 1000) as i16 - 500) * 20).collect();
        let out = resample_anti_alias(&input, 48000, 16000);
        assert!(
            out.len() >= 15000 && out.len() <= 16200,
            "expected ~16000 samples, got {}",
            out.len()
        );
    }

    #[test]
    fn resample_same_rate_passthrough() {
        let input = vec![1i16, -2, 3, -4];
        let out = resample_anti_alias(&input, 16000, 16000);
        assert_eq!(out, input);
    }

    #[test]
    fn resample_tail_chunk_does_not_fall_back_to_linear() {
        // 80000 帧 @48k(真实 5s 录音经 downmix 后的长度):4096 无法整除,
        // 尾块 2176 帧。12kHz 正弦在 16k 目标下必须被低通滤掉(>7.6k 截止);
        // 若回退线性插值,12k 会混叠成 4k 原幅 tone → RMS ≈ 7071。
        let input: Vec<i16> = (0..80000)
            .map(|i| {
                ((i as f64 * 12000.0 * 2.0 * std::f64::consts::PI / 48000.0).sin() * 10000.0) as i16
            })
            .collect();
        let out = resample_anti_alias(&input, 48000, 16000);
        assert_eq!(
            out.len(),
            80000 / 3,
            "output length must be truncated to theory"
        );
        let rms =
            (out.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / out.len() as f64).sqrt();
        assert!(
            rms < 3000.0,
            "12kHz should be filtered, not aliased; rms={}",
            rms
        );
    }

    #[test]
    fn resample_empty_passthrough() {
        let out = resample_anti_alias(&[], 48000, 16000);
        assert!(out.is_empty());
    }

    #[test]
    fn downmix_stereo_averages() {
        let stereo: Vec<i16> = vec![100, 300, -100, -300, 1000, 2000]; // 3 frames
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono, vec![200, -200, 1500]);
    }

    #[test]
    fn push_samples_evicts_oldest_when_full() {
        let samples: Arc<(Mutex<VecDeque<i16>>, Condvar)> =
            Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        push_samples(&samples, &vec![1i16; BUF_CAP_FRAMES]);
        push_samples(&samples, &vec![2i16; 100]);
        let lock = samples.0.lock().unwrap();
        assert_eq!(lock.len(), BUF_CAP_FRAMES);
        assert_eq!(lock.back(), Some(&2i16));
    }
}
