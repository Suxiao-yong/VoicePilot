//! Voice listen Tauri command —— V1.1 §8.2 Main Chat 语音输入桥接。
//!
//! 桥接 W5 voice 模块(record + vad + transcribe + route)到 Tauri webview。
//! 整个模块用 `#[cfg(feature = "voice")]` 门控(在 lib.rs 中)。
//!
//! W6b-2 Task 5:
//! - `VoiceListen::listen` 接收 `cancel: &AtomicBool`(issue #57)
//! - `VoiceListenImpl::with_engine` 接收 `Arc<SherpaAsrEngine>`(issue #61 缓存)
//! - `voice_listen_command` 从 ConfigRepo 读 settings + 用 asr_cache
//! - `cancel_voice_command` 设 kill_switch 为 true

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::voice::error::VoiceResult;
use trust_kernel::voice::listener::{AudioRecorderAdapter, ListenOutcome, VoiceListener, VoiceRecorder};
use trust_kernel::voice::model::ModelRegistry;
use trust_kernel::voice::router_bridge::{block_on_planner, RouteOutcome};
use trust_kernel::voice::vad::{VadConfig, VadDetector};
use trust_kernel::voice::listener::ListenTimings;
use trust_kernel::planner::{RealtimeSnapshot, SnapshotMemory, SnapshotVoice};
use trust_kernel::turns::TurnRecord;
use trust_kernel::voice::asr::{SherpaAsrConfig, SherpaAsrEngine};

use crate::commands::RouteTextResult;
use crate::slot_parser::{Slot, SlotParser};

/// `voice_listen` 返回给 webview 的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceListenResult {
    Success {
        transcription: String,
        route_outcome: RouteTextResult,
        stopped_by_vad: bool,
    },
    NoSpeech,
    Timeout {
        transcription: Option<String>,
        route_outcome: RouteTextResult,
    },
    Error {
        message: String,
    },
}

/// `VoiceListen` trait 的内部 outcome(不含 Error,Error 通过 `Result` 传递)。
#[derive(Debug, Clone)]
pub enum VoiceListenOutcome {
    Success {
        transcription: String,
        route_outcome: RouteTextResult,
        stopped_by_vad: bool,
    },
    NoSpeech,
    Timeout {
        transcription: Option<String>,
        route_outcome: RouteTextResult,
    },
}

/// 抽象 voice listen 管道(listen → transcribe → route)。
///
/// W6b-2 Task 5:`listen` 接收 `cancel: &AtomicBool`(issue #57)。
pub trait VoiceListen: Send + Sync {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome>;
}

/// 把 `VoiceListenOutcome` 转为 `VoiceListenResult`(纯函数,便于单元测试)。
///
/// W6b-2 Task 5:接收 `cancel: &AtomicBool` 透传给 listener。
pub fn voice_listen(
    listener: &dyn VoiceListen,
    cancel: &AtomicBool,
) -> VoiceListenResult {
    match listener.listen(cancel) {
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

// ===== 批2:Siri 波形 —— RmsEmitterRecorder 装饰器 =====

/// `audio-level` 事件 payload:归一化 RMS 电平(0..=1),webview 波形柱消费。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AudioLevelPayload {
    pub level: f32,
}

/// 计算 i16 PCM 块的 RMS 并归一化到 0..=1(除以 i16::MAX)。
/// 静音(全 0)= 0;满幅方波 ≈ 1.0;正常语音约 0.02~0.25。
pub fn compute_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    ((sum_sq / samples.len() as f64).sqrt() / i16::MAX as f64) as f32
}

/// 装饰器(方案定稿 §4):包装任意 `VoiceRecorder`,每 chunk 录完后计算 RMS
/// emit `audio-level` 给 webview 驱动悬浮球波形柱。trust-kernel / cpal 层零改动。
struct RmsEmitterRecorder {
    inner: Arc<dyn VoiceRecorder>,
    app: AppHandle,
}

impl VoiceRecorder for RmsEmitterRecorder {
    fn record_chunk(&self, duration: Duration) -> VoiceResult<Vec<i16>> {
        let chunk = self.inner.record_chunk(duration)?;
        // 语音典型 RMS 0.02~0.25,×4 增益映射到 UI 可用区间,截断到 1.0
        let level = (compute_rms(&chunk) * 4.0).min(1.0);
        let _ = self.app.emit(
            "audio-level",
            AudioLevelPayload { level },
        );
        Ok(chunk)
    }
}

// ===== VoiceListenImpl: 生产实现 =====

/// 生产用 `VoiceListen` 实现,编排 `VoiceListener` + `SherpaAsrEngine` + `route_text`。
///
/// W6b-2 Task 5:`cached_engine: Option<Arc<SherpaAsrEngine>>` 用于模型缓存(issue #61)。
/// W6b-2 Task 6:`partial_app: Option<AppHandle>` 用于发射 `transcription-partial` 事件(issue #47)。
pub struct VoiceListenImpl {
    recorder: Arc<dyn VoiceRecorder>,
    asr_config: SherpaAsrConfig,
    /// 缓存的 SherpaAsrEngine(issue #61)。Some 时 transcribe 直接用;
    /// None 时 fallback 到每次 `SherpaAsrEngine::new(asr_config.clone())`。
    cached_engine: Option<Arc<SherpaAsrEngine>>,
    /// AppHandle 用于发射 `transcription-partial` 事件(issue #47)。
    /// Some 时 listen 期间每 2s 调 engine.transcribe 并 emit partial。
    partial_app: Option<AppHandle>,
    kernel: Arc<TrustKernel>,
    max_duration: Duration,
    chunk_duration: Duration,
    /// 桌宠手动模式(桌宠化改造):true 时禁用 VAD 自动停(max_silence 拉到 30s)
    /// 并把硬顶放宽到 60s,由再次单击触发 kill_switch 结束录音 —— cancel 路径
    /// 返回 `Timeout{samples}`,buffer 里有语音即正常转写(不丢样本)。
    manual_stop: bool,
    /// TTS 自激防护(Phase 1):透传给 `VoiceListener::with_tts_cooldown_until`。
    /// 由 `voice_listen_steps` 从 `AppState::tts_cooldown_until` 快照填入。
    tts_cooldown_until: Option<std::time::SystemTime>,
}

/// VAD 后端标签（silero/energy）：探测需加载 ONNX 模型，进程内只做一次，
/// 避免每轮 listen 重复加载（M1 修复）。
static VAD_BACKEND: OnceLock<&'static str> = OnceLock::new();

fn vad_backend_label() -> &'static str {
    VAD_BACKEND.get_or_init(|| {
        if VadDetector::new(VadConfig::default()).is_silero() {
            "silero"
        } else {
            "energy"
        }
    })
}

impl VoiceListenImpl {
    /// 用默认 VAD + 默认录音配置创建(cached_engine = None, partial_app = None)。
    pub fn new(
        recorder: Arc<dyn VoiceRecorder>,
        asr_config: SherpaAsrConfig,
        kernel: Arc<TrustKernel>,
    ) -> Self {
        Self {
            recorder,
            asr_config,
            cached_engine: None,
            partial_app: None,
            kernel,
            max_duration: Duration::from_secs(30),
            chunk_duration: Duration::from_millis(500),
            manual_stop: false,
            tts_cooldown_until: None,
        }
    }

    /// 桌宠手动模式开关(桌宠化改造):见 `manual_stop` 字段文档。
    pub fn with_manual_stop(mut self, manual_stop: bool) -> Self {
        self.manual_stop = manual_stop;
        self
    }

    /// TTS cooldown 快照注入(Phase 1 自激防护)。`None` = 无防护。
    pub fn with_tts_cooldown_until(mut self, until: Option<std::time::SystemTime>) -> Self {
        self.tts_cooldown_until = until;
        self
    }

    /// 用默认模型(sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17)创建,便于 Tauri command 构造。
    pub fn with_default_model(
        recorder: Arc<dyn VoiceRecorder>,
        kernel: Arc<TrustKernel>,
    ) -> VoiceResult<Self> {
        let registry = ModelRegistry::new();
        let model_dir = registry.resolve("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17")?;
        let asr_config = SherpaAsrConfig {
            model_dir,
            language: None,
            ..Default::default()
        };
        Ok(Self::new(recorder, asr_config, kernel))
    }

    /// 用缓存的 SherpaAsrEngine + AppHandle 创建(issue #61 + #47)。
    /// 调用方负责从 `state.asr_cache` 取出 `Arc<SherpaAsrEngine>` 传入。
    /// `app` 用于 listen 期间发射 `transcription-partial` 事件。
    pub fn with_engine(
        recorder: Arc<dyn VoiceRecorder>,
        engine: Arc<SherpaAsrEngine>,
        app: AppHandle,
        kernel: Arc<TrustKernel>,
    ) -> Self {
        let asr_config = engine.config().clone();
        Self {
            recorder,
            asr_config,
            cached_engine: Some(engine),
            partial_app: Some(app),
            kernel,
            max_duration: Duration::from_secs(30),
            chunk_duration: Duration::from_millis(500),
            manual_stop: false,
            tts_cooldown_until: None,
        }
    }

    /// 转写样本。优先用 cached_engine,否则每次 new SherpaAsrEngine。
    fn transcribe(&self, samples: &[i16]) -> VoiceResult<String> {
        if let Some(engine) = &self.cached_engine {
            return engine.transcribe(samples);
        }
        let engine = SherpaAsrEngine::new(self.asr_config.clone())?;
        engine.transcribe(samples)
    }

    /// 带快照的路由（voice 路径用）。快照为 None 时等价 `route()`。
    fn route_with_snapshot(
        &self,
        text: &str,
        snapshot: Option<&RealtimeSnapshot>,
    ) -> RouteTextResult {
        use trust_kernel::voice::router_bridge::route_text_with_snapshot;
        let kernel = self.kernel.clone();
        let text_for_closure = text.to_string();
        let snapshot = snapshot.cloned();
        let outcome = block_on_planner(async move {
            route_text_with_snapshot(&kernel, &text_for_closure, snapshot).await
        });
        match outcome {
            Ok(RouteOutcome::Routed { skill_id }) => RouteTextResult::Routed {
                skill_id,
                slots: vec![],
            },
            Ok(RouteOutcome::Unmatched { text }) => RouteTextResult::Unmatched { text },
            Ok(RouteOutcome::Chat { text }) => RouteTextResult::Chat { text },
            Ok(RouteOutcome::Empty) => RouteTextResult::Empty,
            #[cfg(feature = "llm")]
            Ok(RouteOutcome::DagPlan(_)) => RouteTextResult::Unmatched {
                text: text.to_string(),
            },
            Err(_) => RouteTextResult::Empty,
        }
    }

    /// 现采快照：final 转写 + timings + VAD 后端探测 + 最近 3 轮回忆。
    /// privacy_mode 下返回 None（无快照、无记忆、无注入）。
    fn build_snapshot(
        &self,
        transcription: &str,
        timings: &ListenTimings,
        sample_count: usize,
        outcome_kind: &'static str,
        stopped_by_vad: bool,
    ) -> Option<RealtimeSnapshot> {
        if self.kernel.privacy_mode() {
            return None;
        }
        let prev_turns = self
            .kernel
            .recent_turns(3)
            .unwrap_or_default()
            .into_iter()
            .rev()
            .map(|t| {
                let short: String = t.transcript.chars().take(200).collect();
                format!("用户：{} → {}", short, t.outcome)
            })
            .collect();
        let vad_backend = vad_backend_label();
        let now = std::time::SystemTime::now();
        Some(RealtimeSnapshot {
            taken_at: now,
            transcript_chars: transcription.chars().count(),
            voice: SnapshotVoice {
                outcome_kind,
                stopped_by_vad,
                sample_count,
                vad_backend,
                voice_started_ago_ms: timings.voice_started_at.and_then(|t0| {
                    now.duration_since(t0).ok().map(|d| d.as_millis() as u64)
                }),
            },
            memory: SnapshotMemory { prev_turns },
            privacy_mode: false,
        })
    }

    /// 封轮：摘要落 turns 表并 piggyback 30 天 TTL。privacy 下跳过；失败只记日志不阻断返回。
    fn seal_turn(
        &self,
        transcription: &str,
        route_outcome: &RouteTextResult,
        timings: &ListenTimings,
        source: &str,
    ) {
        if self.kernel.privacy_mode() {
            return;
        }
        let outcome = match route_outcome {
            RouteTextResult::Routed { skill_id, .. } => format!("routed:{skill_id}"),
            RouteTextResult::Unmatched { .. } => "unmatched".to_string(),
            RouteTextResult::Chat { .. } => "chat".to_string(),
            RouteTextResult::Empty => "empty".to_string(),
        };
        let latency_ms = match (timings.voice_started_at, timings.first_partial_at) {
            (Some(a), Some(b)) => b
                .duration_since(a)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            _ => 0,
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        let transcript: String = transcription.chars().take(500).collect();
        let rec = TurnRecord {
            turn_id: format!(
                "turn-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(now_ms * 1_000_000)
            ),
            started_at_ms: now_ms,
            source: source.to_string(),
            transcript,
            outcome,
            plan_id: String::new(),
            latency_ms,
            sensitive: false,
        };
        if let Err(e) = self.kernel.record_turn(&rec) {
            eprintln!("[voice] seal_turn record failed: {e}");
        }
        let _ = self.kernel.prune_turns_older_than(30, now_ms);
        // Phase B：封轮钩子触发长期记忆蒸馏（mem0 简化版）。
        // 后台线程 fire-and-forget：LLM 调用慢，绝不阻塞本轮返回；
        // maybe_distill 内部自检 privacy_mode / llm 可用 / 批量阈值，
        // 任何失败静默收敛（记忆是增益不是依赖）。
        let kernel = self.kernel.clone_arc();
        std::thread::spawn(move || {
            if let Err(e) = trust_kernel::memory::maybe_distill(&kernel) {
                tracing::warn!(error = ?e, "memory distill failed (non-fatal)");
            }
        });
    }

    /// 收尾：转写 → 快照 → 路由 → 封轮。NoSpeech 不封轮（无信息量）。
    fn finish_listen(
        &self,
        outcome: ListenOutcome,
        timings: ListenTimings,
    ) -> VoiceResult<VoiceListenOutcome> {
        match outcome {
            ListenOutcome::SpeechEnded { samples } => {
                let transcription = self.transcribe(&samples)?;
                let snapshot = self.build_snapshot(
                    &transcription,
                    &timings,
                    samples.len(),
                    "speech_ended",
                    true,
                );
                let route_outcome = self.route_with_snapshot(&transcription, snapshot.as_ref());
                self.seal_turn(&transcription, &route_outcome, &timings, "voice");
                Ok(VoiceListenOutcome::Success {
                    transcription,
                    route_outcome,
                    stopped_by_vad: true,
                })
            }
            ListenOutcome::NoSpeech => Ok(VoiceListenOutcome::NoSpeech),
            ListenOutcome::Timeout { samples } => {
                let transcription = if samples.is_empty() {
                    None
                } else {
                    self.transcribe(&samples).ok()
                };
                let (route_outcome, sealed) = match &transcription {
                    Some(t) => {
                        let snapshot =
                            self.build_snapshot(t, &timings, samples.len(), "timeout", false);
                        let r = self.route_with_snapshot(t, snapshot.as_ref());
                        self.seal_turn(t, &r, &timings, "voice");
                        (r, true)
                    }
                    None => (RouteTextResult::Empty, false),
                };
                let _ = sealed;
                Ok(VoiceListenOutcome::Timeout {
                    transcription,
                    route_outcome,
                })
            }
        }
    }
}

impl VoiceListen for VoiceListenImpl {
    fn listen(&self, cancel: &AtomicBool) -> VoiceResult<VoiceListenOutcome> {
        // 桌宠手动模式:max_silence 拉到 30s 等效禁用 VAD 自动停,硬顶放宽到
        // 60s;结束完全依赖再次单击的 kill_switch(cancel → Timeout{samples} 转写)。
        let (vad_config, max_duration) = if self.manual_stop {
            (
                VadConfig {
                    max_silence_ms: 30_000,
                    ..VadConfig::default()
                },
                Duration::from_secs(60),
            )
        } else {
            (VadConfig::default(), self.max_duration)
        };
        let vad = VadDetector::new(vad_config);
        let listener = VoiceListener::new(
            self.recorder.clone(),
            vad,
            max_duration,
            self.chunk_duration,
        )
        .with_tts_cooldown_until(self.tts_cooldown_until);

        // W6b-2 issue #47:若 cached_engine 和 partial_app 都有,构造 partial callback
        // 闭包,每 2s 调 engine.transcribe 并 emit `transcription-partial` 事件。
        let partial_cb: PartialCbOpt = match (&self.cached_engine, &self.partial_app) {
            (Some(engine), Some(app)) => {
                let engine_clone = Arc::clone(engine);
                let app_clone = app.clone();
                Some(Box::new(move |samples: &[i16]| {
                    if let Ok(text) = engine_clone.transcribe(samples) {
                        let slots = SlotParser::parse(&text);
                        let payload = TranscriptionPartialPayload {
                            partial: text,
                            timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            slots,
                        };
                        let _ = app_clone.emit("transcription-partial", payload);
                    }
                }))
            }
            _ => None,
        };

        let (outcome, timings) = match partial_cb.as_ref() {
            Some(cb) => listener.listen_with_cancel_partial_and_timings(cancel, Some(cb))?,
            None => {
                let outcome = listener.listen_with_cancel(cancel)?;
                (outcome, ListenTimings::default())
            }
        };

        self.finish_listen(outcome, timings)
    }
}

/// Partial transcript callback box 类型(W6b-2 issue #47)。
/// 用于 `VoiceListenImpl::listen` 内构造闭包传给 `listen_with_cancel_and_partial`。
type PartialCbOpt = Option<Box<dyn Fn(&[i16]) + Send + Sync>>;

// ===== transcription-final 事件 + voice_listen_command + cancel_voice_command =====

/// `transcription-partial` 事件 payload(W6b-2 issue #47)。
/// listen 期间每 2s 发射一次,webview 实时显示 partial 转写。
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionPartialPayload {
    pub partial: String,
    pub timestamp_ms: i64,
    pub slots: Vec<Slot>,
}

/// `transcription-final` 事件 payload,发射给 webview。
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionFinalPayload {
    pub transcription: String,
    pub route_outcome: RouteTextResult,
    pub stopped_by_vad: bool,
    pub slots: Vec<Slot>,
}

/// 从 `VoiceListenResult` 构造 `transcription-final` 事件 payload。
pub fn build_transcription_final_payload(
    result: &VoiceListenResult,
) -> Option<TranscriptionFinalPayload> {
    match result {
        VoiceListenResult::Success {
            transcription,
            route_outcome,
            stopped_by_vad,
        } => Some(TranscriptionFinalPayload {
            transcription: transcription.clone(),
            route_outcome: route_outcome.clone(),
            stopped_by_vad: *stopped_by_vad,
            slots: SlotParser::parse(transcription),
        }),
        VoiceListenResult::Timeout {
            transcription: Some(t),
            route_outcome,
        } => Some(TranscriptionFinalPayload {
            transcription: t.clone(),
            route_outcome: route_outcome.clone(),
            stopped_by_vad: false,
            slots: SlotParser::parse(t),
        }),
        VoiceListenResult::NoSpeech
        | VoiceListenResult::Error { .. }
        | VoiceListenResult::Timeout {
            transcription: None,
            ..
        } => None,
    }
}

/// Tauri command:开始 voice listen,返回 `VoiceListenResult`。
///
/// W6b-2 Task 5 流程:
/// 1. 重置 kill_switch 为 false
/// 2. 从 ConfigRepo 加载 voice settings(§8.3 Settings 持久化)
/// 3. 检查 asr_cache,miss 时加载(issue #61)
/// 4. 用 `VoiceListenImpl::with_engine` 构造 listener
/// 5. 调 `voice_listen(&listener, &state.kill_switch)`(透传 cancel)
/// 6. 发射 `transcription-final` 事件(若有 transcription)
///
/// 桌宠化改造:
/// - `manualStop` 参数:桌宠单击录音传 true —— 禁用 VAD 自动停(max_silence=30s)
///   + 硬顶放宽到 60s,由再次单击的 cancel 结束;缺省 false 保持主界面 VAD 行为不变
/// - 录音并发防护:`state.recording` compare_exchange 抢占,主界面麦克风与桌宠
///   共用输入设备,同一时刻只允许一路 listen;结束(含任何错误早退)时复位
#[tauri::command]
pub async fn voice_listen_command(
    state: tauri::State<'_, crate::state::AppState>,
    app: AppHandle,
    manual_stop: Option<bool>,
) -> Result<VoiceListenResult, String> {
    // 并发防护:抢占失败说明另一路录音正在进行(main 或 pet),拒绝第二路
    if state
        .recording
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err("另一路录音正在进行中(主界面或桌宠),请先停止".to_string());
    }

    let result = voice_listen_steps(&state, &app, manual_stop.unwrap_or(false));
    eprintln!(
        "[voice] listen finished manual_stop={} kind={}",
        manual_stop.unwrap_or(false),
        match &result {
            Ok(r) => match r {
                VoiceListenResult::Success { .. } => "success",
                VoiceListenResult::NoSpeech => "no_speech",
                VoiceListenResult::Timeout { .. } => "timeout",
                VoiceListenResult::Error { .. } => "error",
            },
            Err(_) => "invoke-err",
        }
    );

    // 无论成功失败都释放录音位(桌宠/主界面据此可再次发起录音)
    state
        .recording
        .store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

/// 原 voice_listen_command 主体(步骤 1-5),拆出便于并发防护统一复位。
fn voice_listen_steps(
    state: &crate::state::AppState,
    app: &AppHandle,
    manual_stop: bool,
) -> Result<VoiceListenResult, String> {
    use trust_kernel::voice::audio::AudioRecorderConfig;

    // 1. 重置 cancel flag
    state
        .kill_switch
        .store(false, std::sync::atomic::Ordering::SeqCst);

    // 1b. Phase 1 TTS 自激防护:尽早快照 cooldown 窗口。review P2-S1:asr_cache
    // miss 时模型加载耗时秒级,若在步骤 4 才快照,窗口会被加载时间吃掉大半。
    let tts_cooldown_until = state
        .tts_cooldown_until
        .lock()
        .map(|guard| *guard)
        .unwrap_or(None);

    // 2. 从 Settings 加载 voice 配置(V1.1.2 §8.3 Settings 持久化)
    let settings = load_voice_settings(&state.kernel).map_err(|e| e.to_string())?;
    // W6b-3b Fix 3:voice_model_path 为空时用 ModelRegistry 解析默认模型路径
    // (~/.voicepilot/models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17)。
    // 此前默认值是模型名(相对路径),SherpaAsrEngine::new 校验 model_dir.is_dir()
    // 时从 CWD 查找必失败。
    let model_dir = if settings.voice_model_path.is_empty() {
        let registry = ModelRegistry::new();
        registry.default_model().path
    } else {
        std::path::PathBuf::from(&settings.voice_model_path)
    };
    let asr_config = SherpaAsrConfig {
        model_dir: model_dir.clone(),
        language: settings.voice_language.clone(),
        num_threads: settings.voice_threads,
        ..Default::default()
    };

    // 3. 检查 asr_cache,miss 时加载(issue #61)
    //    W6c P2 #3:用全 config 比较替换仅 model_dir 比较,
    //    改 language / num_threads 也触发缓存失效。
    let engine: Arc<SherpaAsrEngine> = {
        let mut cache = state.asr_cache.lock().map_err(|e| e.to_string())?;
        let needs_reload =
            cache_needs_reload(cache.as_ref().map(|e| e.config()), &asr_config);
        if needs_reload {
            let new_engine = SherpaAsrEngine::new(asr_config.clone())
                .map_err(|e| e.to_string())?;
            *cache = Some(Arc::new(new_engine));
        }
        Arc::clone(cache.as_ref().expect("cache should be populated"))
    };

    // 4. 构造 VoiceListenImpl(用缓存的 engine + AppHandle 用于 partial 事件)
    //    批2:真实 recorder 外包一层 RmsEmitterRecorder,listen 循环每 chunk
    //    (500ms)emit audio-level 驱动 Siri 波形柱。
    let recorder: Arc<dyn VoiceRecorder> = Arc::new(RmsEmitterRecorder {
        inner: Arc::new(
            AudioRecorderAdapter::new(AudioRecorderConfig::default())
                .map_err(|e| e.to_string())?,
        ),
        app: app.clone(),
    });
    let listener = VoiceListenImpl::with_engine(recorder, engine, app.clone(), state.kernel.clone())
        .with_manual_stop(manual_stop)
        .with_tts_cooldown_until(tts_cooldown_until);

    // 5. 执行 listen + 发射 transcription-final
    eprintln!("[voice] listen begin manual_stop={}", manual_stop);
    let result = voice_listen(&listener, &state.kill_switch);
    if let Some(payload) = build_transcription_final_payload(&result) {
        let _ = app.emit("transcription-final", payload);
    }
    Ok(result)
}

/// Tauri command:取消正在进行的 voice listen(issue #57)。
///
/// 设 kill_switch 为 true,循环中下一 chunk 前检查后退出。
#[tauri::command]
pub async fn cancel_voice_command(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state
        .kill_switch
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// 从 ConfigRepo 加载 voice 相关 settings(V1.1.2 §8.3 Settings 持久化)。
fn load_voice_settings(
    kernel: &TrustKernel,
) -> Result<crate::settings_commands::SettingsView, crate::error::UiError> {
    let conn = kernel.conn();
    let kv = kernel.config_repo().list(&conn)?;
    crate::settings_commands::merge_from_kv(&kv)
}

/// W6c P2 #3:检查缓存是否需要重新加载(用全 config 比较替换仅 model_dir)。
///
/// `cached` 为 None 时返回 true(miss);`cached` config 与 `new_config` 不同时返回 true
/// (改 language / num_threads / speed 等任意字段都触发失效)。
/// 对于 SherpaTtsConfig,PartialEq 已排除 sample_rate(模型决定值,不参与失效判断)。
pub fn cache_needs_reload<C: PartialEq>(cached: Option<&C>, new_config: &C) -> bool {
    cached.is_none_or(|c| c != new_config)
}

// ===== tts_command + cancel_tts_command (VP-FR-002 voice feedback) =====

use trust_kernel::voice::tts::{SherpaTtsConfig, SherpaTtsEngine};
use trust_kernel::voice::wav;

/// TTS 播放结果(返回给 webview)。
///
/// W6b-3b Fix 1:`wav_path` 字段返回合成的 WAV 文件绝对路径,
/// 由前端 `<audio>` 元素通过 `convertFileSrc` 播放(后端不再尝试用 cpal 播放)。
/// `played: true` 语义改为"已合成可供播放"。
#[derive(Debug, Clone, Serialize)]
pub struct TtsResult {
    pub played: bool,
    pub interrupted: bool,
    pub sample_count: usize,
    pub wav_path: Option<String>,
    pub error: Option<String>,
}

/// Tauri command:合成文本并通过 cpal 播放(VP-FR-002)。
/// 若 `tts_cancel` 在播放期间被设为 true,立即停止并返回 `interrupted: true`。
#[tauri::command]
pub async fn tts_command(
    state: tauri::State<'_, crate::state::AppState>,
    text: String,
) -> Result<TtsResult, String> {
    // 1. 检查 tts_enabled
    let settings = load_voice_settings(&state.kernel).map_err(|e| e.to_string())?;
    if !settings.tts_enabled {
        return Ok(TtsResult {
            played: false,
            interrupted: false,
            sample_count: 0,
            wav_path: None,
            error: Some("tts disabled in settings".to_string()),
        });
    }

    // 2. 重置 cancel flag
    state
        .tts_cancel
        .store(false, std::sync::atomic::Ordering::SeqCst);

    // 3. 加载 / 缓存 TTS engine
    // W6b-3b Fix 3:tts_model_path 为空时返回友好错误(未配置)。
    // TTS 模型(vits-icefall-zh-aishell3)与 ASR 模型不同,ModelRegistry 当前
    // 只有 ASR 模型,故空路径时提示用户配置 tts_model_path。
    if settings.tts_model_path.is_empty() {
        return Ok(TtsResult {
            played: false,
            interrupted: false,
            sample_count: 0,
            wav_path: None,
            error: Some(
                "TTS model path not configured. Please set tts_model_path in Settings.".to_string(),
            ),
        });
    }
    let model_dir = std::path::PathBuf::from(&settings.tts_model_path);
    // W6c P2 #3:用全 config 比较替换仅 model_dir 比较,
    // 改 num_threads / speed 也触发缓存失效。
    // 注意:SherpaTtsConfig 的 PartialEq 排除 sample_rate(模型决定值)。
    let tts_config = SherpaTtsConfig {
        model_dir: model_dir.clone(),
        ..Default::default()
    };
    let engine: Arc<SherpaTtsEngine> = {
        let mut cache = state.tts_cache.lock().map_err(|e| e.to_string())?;
        let needs_reload =
            cache_needs_reload(cache.as_ref().map(|e| e.config()), &tts_config);
        if needs_reload {
            let new_engine = SherpaTtsEngine::new(tts_config.clone())
                .map_err(|e| e.to_string())?;
            *cache = Some(Arc::new(new_engine));
        }
        Arc::clone(cache.as_ref().expect("tts cache should be populated"))
    };

    // 4. 合成(W6c P2 #2:synth 返回 (samples, sample_rate),actual_sample_rate 也被缓存)
    let samples = match engine.synth(&text) {
        Ok(s) => s.0,
        Err(e) => {
            return Ok(TtsResult {
                played: false,
                interrupted: false,
                sample_count: 0,
                wav_path: None,
                error: Some(e.to_string()),
            });
        }
    };

    // 5. 通过 cpal 播放(用 voice/audio 模块现有 AudioPlayer;若没有,写临时 WAV 到 tempdir 再用 cpal 播放)
    // 简化实现:写 WAV 到 tempdir,然后用系统默认播放器播放(VP-FR-002 简化路径)。
    let wav_dir = std::env::temp_dir().join("voicepilot-tts");
    std::fs::create_dir_all(&wav_dir).map_err(|e| e.to_string())?;
    let wav_path = wav_dir.join(format!("tts-{}.wav", chrono::Utc::now().timestamp_millis()));
    // W6c P2 #2:用 engine.actual_sample_rate()(由模型决定,中文 VITS 通常 22050 Hz)
    // 而非 engine.config().sample_rate(16000 默认值),避免播放速度/音调失真。
    wav::write_wav(&wav_path, &samples, engine.actual_sample_rate())
        .map_err(|e| e.to_string())?;

    // 6. 检查 cancel(简化实现:播放前检查一次,完整实现需在播放线程中循环检查)
    let interrupted = state.tts_cancel.load(std::sync::atomic::Ordering::SeqCst);
    if interrupted {
        return Ok(TtsResult {
            played: false,
            interrupted: true,
            sample_count: samples.len(),
            wav_path: None,
            error: None,
        });
    }

    // 7. W6b-3b Fix 1:后端不再用 cpal 播放(此前桩实现返回 played: true 但用户听不到声音)。
    // 改为返回 WAV 路径,由前端 `<audio>` 元素通过 `convertFileSrc` 播放。
    // `played: true` 语义改为“已合成可供播放”。前端 `MainView.tsx` 负责实际播放与中断。
    // Phase 1 TTS 自激防护:按合成音频时长估算播放结束时刻 + cooldown,供下次
    // listen 丢弃窗口内 chunk(防音箱尾音被当成用户说话)。前端实际播放时刻与
    // 此估算有偏差是安全的:窗口过期偏早只退化为旧行为(无防护),不引入新风险。
    {
        let play_secs = samples.len() as f64 / engine.actual_sample_rate() as f64;
        let cooldown_ms =
            trust_kernel::voice::vad::VadConfig::default().tts_cooldown_ms as f64;
        let until = std::time::SystemTime::now()
            + std::time::Duration::from_millis((play_secs * 1000.0) as u64 + cooldown_ms as u64);
        if let Ok(mut guard) = state.tts_cooldown_until.lock() {
            *guard = Some(until);
        }
    }
    Ok(TtsResult {
        played: true,
        interrupted: false,
        sample_count: samples.len(),
        wav_path: Some(wav_path.to_string_lossy().into_owned()),
        error: None,
    })
}

/// Tauri command:取消正在进行的 TTS 播放(VP-FR-002 可中断)。
///
/// review P1-F2:取消即停播,不会再有尾音 → 必须同步清除 `tts_cooldown_until`,
/// 否则下次 listen 开头 N 秒用户说话会被当尾音丢弃(防护反伤)。
#[tauri::command]
pub async fn cancel_tts_command(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state
        .tts_cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut guard) = state.tts_cooldown_until.lock() {
        *guard = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 批2:compute_rms 测试 =====

    #[test]
    fn compute_rms_is_zero_for_silence_and_empty() {
        assert_eq!(compute_rms(&[]), 0.0);
        assert_eq!(compute_rms(&vec![0i16; 8000]), 0.0);
    }

    #[test]
    fn compute_rms_full_scale_square_wave_is_one() {
        // 满幅方波:|x| = i16::MAX → rms = i16::MAX → 归一化 = 1.0
        let chunk = vec![i16::MAX; 8000];
        let rms = compute_rms(&chunk);
        assert!((rms - 1.0).abs() < 1e-6, "expected ~1.0, got {}", rms);
    }

    #[test]
    fn compute_rms_half_amplitude_square_wave_is_half() {
        // 半幅方波:rms = 16384/32767 ≈ 0.5
        let chunk = vec![16384i16; 8000];
        let rms = compute_rms(&chunk);
        assert!(
            (rms - 16384.0 / i16::MAX as f32).abs() < 1e-6,
            "expected ~0.5, got {}",
            rms
        );
    }

    #[test]
    fn compute_rms_negative_samples_use_magnitude() {
        // 负样本取平方后与正样本等价
        let neg = vec![-16384i16; 4000];
        let pos = vec![16384i16; 4000];
        let mut mixed = neg;
        mixed.extend_from_slice(&pos);
        let rms = compute_rms(&mixed);
        assert!(
            (rms - 16384.0 / i16::MAX as f32).abs() < 1e-6,
            "expected ~0.5, got {}",
            rms
        );
    }

    #[test]
    fn voice_listen_result_success_serializes_correctly() {
        let result = VoiceListenResult::Success {
            transcription: "hello".to_string(),
            route_outcome: RouteTextResult::Routed {
                skill_id: "files.organize".to_string(),
                // W7: Routed 加 slots 字段(voice 路径不调 LLM,此处置空 Vec)。
                slots: vec![],
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

    #[test]
    fn build_final_payload_includes_slots_for_success() {
        let result = VoiceListenResult::Success {
            transcription: "打开 notepad 整理 C:\\temp".to_string(),
            route_outcome: RouteTextResult::Routed {
                skill_id: "files.organize".to_string(),
                // W7: Routed 加 slots 字段(voice 路径不调 LLM,此处置空 Vec)。
                slots: vec![],
            },
            stopped_by_vad: true,
        };
        let payload = build_transcription_final_payload(&result).unwrap();
        assert!(!payload.slots.is_empty(), "slots should not be empty for path/app text");
        assert!(payload.slots.iter().any(|s| matches!(s.kind, crate::slot_parser::SlotKind::Path)));
        assert!(payload.slots.iter().any(|s| matches!(s.kind, crate::slot_parser::SlotKind::App)));
    }

    // ===== W6c P2 #3:cache_needs_reload helper 测试 =====

    use trust_kernel::voice::asr::SherpaAsrConfig;
    use trust_kernel::voice::tts::SherpaTtsConfig;

    #[test]
    fn cache_needs_reload_returns_true_when_no_cached_config() {
        let new = SherpaAsrConfig::default();
        assert!(cache_needs_reload::<SherpaAsrConfig>(None, &new));
    }

    #[test]
    fn cache_needs_reload_returns_false_when_asr_config_equal() {
        let cached = SherpaAsrConfig {
            model_dir: std::path::PathBuf::from("/models/asr"),
            language: Some("zh".to_string()),
            num_threads: 4,
            sample_rate: 16000,
        };
        let new = cached.clone();
        assert!(!cache_needs_reload(Some(&cached), &new));
    }

    #[test]
    fn cache_needs_reload_returns_true_when_language_changes() {
        // W6c P2 #3 核心修复:改 language 后缓存应失效
        let cached = SherpaAsrConfig {
            language: Some("zh".to_string()),
            ..Default::default()
        };
        let new = SherpaAsrConfig {
            language: Some("en".to_string()),
            ..Default::default()
        };
        assert!(cache_needs_reload(Some(&cached), &new));
    }

    #[test]
    fn cache_needs_reload_returns_true_when_num_threads_changes() {
        let cached = SherpaAsrConfig {
            num_threads: 4,
            ..Default::default()
        };
        let new = SherpaAsrConfig {
            num_threads: 8,
            ..Default::default()
        };
        assert!(cache_needs_reload(Some(&cached), &new));
    }

    #[test]
    fn cache_needs_reload_returns_false_when_tts_config_equal_ignoring_sample_rate() {
        // SherpaTtsConfig 的 PartialEq 排除 sample_rate(模型决定值)
        let cached = SherpaTtsConfig {
            model_dir: std::path::PathBuf::from("/models/tts"),
            sample_rate: 16000,
            num_threads: 1,
            speed: 1.0,
        };
        let new = SherpaTtsConfig {
            model_dir: std::path::PathBuf::from("/models/tts"),
            sample_rate: 22050, // 不同,但 PartialEq 不比较此字段
            num_threads: 1,
            speed: 1.0,
        };
        assert!(!cache_needs_reload(Some(&cached), &new));
    }

    #[test]
    fn cache_needs_reload_returns_true_when_tts_speed_changes() {
        let cached = SherpaTtsConfig {
            speed: 1.0,
            ..Default::default()
        };
        let new = SherpaTtsConfig {
            speed: 1.5,
            ..Default::default()
        };
        assert!(cache_needs_reload(Some(&cached), &new));
    }
}
