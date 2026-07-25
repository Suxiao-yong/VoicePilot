use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;

#[cfg(feature = "tauri")]
use crate::approver::ApprovalRegistry;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
    #[cfg(feature = "tauri")]
    pub approval_registry: ApprovalRegistry,
    /// Voice 取消标志(issue #57)。cancel_voice_command 设为 true;
    /// voice_listen_command 开始时重置为 false,循环中检查。
    #[cfg(feature = "voice")]
    pub kill_switch: Arc<std::sync::atomic::AtomicBool>,
    /// SherpaAsrEngine 缓存(issue #61)。miss 时加载并缓存(Arc 共享);
    /// Settings 更新 voice_model_path 时应 invalidate(设为 None)。
    /// `SherpaAsrEngine` 内部持有 `Mutex<SenseVoiceRecognizer>`(`Send + Sync` 但不 `Clone`),
    /// 必须用 `Arc<SherpaAsrEngine>` 共享。
    #[cfg(feature = "voice")]
    pub asr_cache: Arc<std::sync::Mutex<Option<Arc<trust_kernel::voice::asr::SherpaAsrEngine>>>>,
    /// SherpaTtsEngine 缓存(VP-FR-002)。miss 时加载并缓存(Arc 共享);
    /// Settings 更新 tts_model_path 时应 invalidate(设为 None)。
    /// `SherpaTtsEngine` 内部持有 `Mutex<VitsTts>`(`Send + Sync` 但不 `Clone`),
    /// 必须用 `Arc<SherpaTtsEngine>` 共享。
    #[cfg(feature = "voice")]
    pub tts_cache: Arc<std::sync::Mutex<Option<Arc<trust_kernel::voice::tts::SherpaTtsEngine>>>>,
    /// TTS 播放取消标志(VP-FR-002 可中断)。cancel_tts_command 设为 true;
    /// tts_command 开始时重置为 false,合成前后检查。
    #[cfg(feature = "voice")]
    pub tts_cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    #[cfg(feature = "tauri")]
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
            approval_registry: ApprovalRegistry::new(),
            #[cfg(feature = "voice")]
            kill_switch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(feature = "voice")]
            asr_cache: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "voice")]
            tts_cache: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "voice")]
            tts_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    #[cfg(not(feature = "tauri"))]
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
            #[cfg(feature = "voice")]
            kill_switch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(feature = "voice")]
            asr_cache: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "voice")]
            tts_cache: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(feature = "voice")]
            tts_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }
}
