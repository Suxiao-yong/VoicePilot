//! Voice listen Tauri command —— V1.1 §8.2 Main Chat 语音输入桥接。
//!
//! 桥接 W5 voice 模块(record + vad + transcribe + route)到 Tauri webview。
//! 整个模块用 `#[cfg(feature = "voice")]` 门控(在 lib.rs 中)。
//!
//! 设计:
//! - `VoiceListen` trait 抽象 listen→transcribe→route 管道,便于注入 mock
//! - `voice_listen` 纯函数把 `VoiceListenOutcome` 转为 `VoiceListenResult`
//! - `VoiceListenImpl` 生产实现,用 `VoiceListener` + `WhisperEngine` + `route_text`
//! - `voice_listen_command` Tauri command,构造 `VoiceListenImpl` + 发射事件
//!   (在任务 3 中实现)
//! - `build_transcription_final_payload` 纯函数构造事件 payload(任务 3)

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::voice::error::VoiceResult;
use trust_kernel::voice::listener::{ListenOutcome, VoiceListener, VoiceRecorder};
use trust_kernel::voice::model::ModelRegistry;
use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
use trust_kernel::voice::vad::{VadConfig, VadDetector};
use trust_kernel::voice::whisper::{WhisperConfig, WhisperEngine};

use crate::commands::RouteTextResult;

/// `voice_listen` 返回给 webview 的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceListenResult {
    /// 成功:VAD 触发停止 + 转写成功 + 路由完成。
    Success {
        transcription: String,
        route_outcome: RouteTextResult,
        stopped_by_vad: bool,
    },
    /// 没有检测到语音。
    NoSpeech,
    /// 达到 max_duration 但 VAD 未触发。可能含 transcription(用户持续说话)。
    Timeout {
        transcription: Option<String>,
        route_outcome: RouteTextResult,
    },
    /// 发生错误(如模型缺失、麦克风拒绝)。
    Error {
        message: String,
    },
}

/// `VoiceListen` trait 的内部 outcome(不含 Error,Error 通过 `Result` 传递)。
#[derive(Debug, Clone)]
pub enum VoiceListenOutcome {
    /// 成功:转写 + 路由完成。
    Success {
        transcription: String,
        route_outcome: RouteTextResult,
        stopped_by_vad: bool,
    },
    /// 没有检测到语音。
    NoSpeech,
    /// 达到 max_duration 但 VAD 未触发。
    Timeout {
        transcription: Option<String>,
        route_outcome: RouteTextResult,
    },
}

/// 抽象 voice listen 管道(listen → transcribe → route)。
///
/// 生产用 `VoiceListenImpl`,测试用 mock(实现此 trait 返回预设 outcome)。
pub trait VoiceListen: Send + Sync {
    fn listen(&self) -> VoiceResult<VoiceListenOutcome>;
}

/// 把 `VoiceListenOutcome` 转为 `VoiceListenResult`。
///
/// 这是纯函数,不涉及 Tauri —— 便于单元测试。
pub fn voice_listen(listener: &dyn VoiceListen) -> VoiceListenResult {
    match listener.listen() {
        Ok(outcome) => match outcome {
            VoiceListenOutcome::Success {
                transcription,
                route_outcome,
                stopped_by_vad,
            } => VoiceListenResult::Success {
                transcription,
                route_outcome,
                stopped_by_vad,
            },
            VoiceListenOutcome::NoSpeech => VoiceListenResult::NoSpeech,
            VoiceListenOutcome::Timeout {
                transcription,
                route_outcome,
            } => VoiceListenResult::Timeout {
                transcription,
                route_outcome,
            },
        },
        Err(e) => VoiceListenResult::Error {
            message: e.to_string(),
        },
    }
}

// ===== VoiceListenImpl: 生产实现(任务 3 中由 Tauri command 使用) =====

/// 生产用 `VoiceListen` 实现,编排 `VoiceListener` + `WhisperEngine` + `route_text`。
pub struct VoiceListenImpl {
    recorder: Arc<dyn VoiceRecorder>,
    whisper_config: WhisperConfig,
    kernel: Arc<TrustKernel>,
    max_duration: Duration,
    chunk_duration: Duration,
}

impl VoiceListenImpl {
    /// 用默认 VAD 配置 + 默认录音配置创建。
    pub fn new(
        recorder: Arc<dyn VoiceRecorder>,
        whisper_config: WhisperConfig,
        kernel: Arc<TrustKernel>,
    ) -> Self {
        Self {
            recorder,
            whisper_config,
            kernel,
            max_duration: Duration::from_secs(30),
            chunk_duration: Duration::from_millis(500),
        }
    }

    /// 用默认模型(ggml-tiny.bin)创建,便于 Tauri command 构造。
    pub fn with_default_model(
        recorder: Arc<dyn VoiceRecorder>,
        kernel: Arc<TrustKernel>,
    ) -> VoiceResult<Self> {
        let registry = ModelRegistry::new();
        let model_path = registry.resolve("ggml-tiny.bin")?;
        let whisper_config = WhisperConfig {
            model_path,
            language: None, // 自动检测
            ..Default::default()
        };
        Ok(Self::new(recorder, whisper_config, kernel))
    }

    /// 转写样本,返回文本。空样本或无语音时返回 `NoSpeechDetected` 错误。
    fn transcribe(&self, samples: &[i16]) -> VoiceResult<String> {
        let engine = WhisperEngine::new(self.whisper_config.clone())?;
        engine.transcribe(samples)
    }

    /// 路由文本到 Skill。错误时降级为 `Empty`(避免阻塞 voice listen 流程)。
    fn route(&self, text: &str) -> RouteTextResult {
        let outcome = route_text(&self.kernel, &AutoApprover, text);
        match outcome {
            Ok(RouteOutcome::Routed { skill_id }) => RouteTextResult::Routed { skill_id },
            Ok(RouteOutcome::Unmatched { text }) => RouteTextResult::Unmatched { text },
            Ok(RouteOutcome::Empty) => RouteTextResult::Empty,
            Err(_) => RouteTextResult::Empty,
        }
    }
}

impl VoiceListen for VoiceListenImpl {
    fn listen(&self) -> VoiceResult<VoiceListenOutcome> {
        let vad = VadDetector::new(VadConfig::default());
        let listener = VoiceListener::new(
            self.recorder.clone(),
            vad,
            self.max_duration,
            self.chunk_duration,
        );

        let outcome = listener.listen()?;

        match outcome {
            ListenOutcome::SpeechEnded { samples } => {
                let transcription = self.transcribe(&samples)?;
                let route_outcome = self.route(&transcription);
                Ok(VoiceListenOutcome::Success {
                    transcription,
                    route_outcome,
                    stopped_by_vad: true,
                })
            }
            ListenOutcome::NoSpeech => Ok(VoiceListenOutcome::NoSpeech),
            ListenOutcome::Timeout { samples } => {
                // 尝试转写已有的样本(可能是用户持续说话)
                let transcription = if samples.is_empty() {
                    None
                } else {
                    self.transcribe(&samples).ok()
                };
                let route_outcome = match &transcription {
                    Some(t) => self.route(t),
                    None => RouteTextResult::Empty,
                };
                Ok(VoiceListenOutcome::Timeout {
                    transcription,
                    route_outcome,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_listen_result_success_serializes_correctly() {
        let result = VoiceListenResult::Success {
            transcription: "hello".to_string(),
            route_outcome: RouteTextResult::Routed {
                skill_id: "files.organize".to_string(),
            },
            stopped_by_vad: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"kind\":\"success\""));
        assert!(json.contains("\"transcription\":\"hello\""));
        assert!(json.contains("\"stopped_by_vad\":true"));
    }

    #[test]
    fn voice_listen_result_no_speech_serializes_correctly() {
        let result = VoiceListenResult::NoSpeech;
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"kind\":\"no_speech\""));
    }

    #[test]
    fn voice_listen_result_error_serializes_correctly() {
        let result = VoiceListenResult::Error {
            message: "model missing".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"kind\":\"error\""));
        assert!(json.contains("\"message\":\"model missing\""));
    }

    #[test]
    fn voice_listen_result_deserializes_roundtrip() {
        let original = VoiceListenResult::Timeout {
            transcription: Some("test".to_string()),
            route_outcome: RouteTextResult::Empty,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: VoiceListenResult = serde_json::from_str(&json).unwrap();
        match parsed {
            VoiceListenResult::Timeout {
                transcription,
                route_outcome,
            } => {
                assert_eq!(transcription, Some("test".to_string()));
                assert!(matches!(route_outcome, RouteTextResult::Empty));
            }
            other => panic!("expected Timeout, got {:?}", other),
        }
    }
}
