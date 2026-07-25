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
    /// W7: LLM 客户端缓存(配置变更时重建)。
    ///
    /// `LlmClient` 非 `Clone`(持有 `reqwest::Client`),用 `Arc<LlmClient>` 共享。
    /// `Mutex<Option<Arc<...>>>` 表达"未初始化 / 已初始化"两态:
    ///   - `None`:首次调用前(或 Settings 未启用 LLM),`llm_client()` 返回 `LlmClient::disabled()`
    ///   - `Some(arc)`:已通过 `rebuild_llm_client` 构建并缓存
    ///
    /// Settings 变更(privacy_mode / llm_enabled / llm_api_key)→ `update_settings_command`
    /// 调 `rebuild_llm_client` + 写入此字段,下次 `route_text` 即用新配置。
    #[cfg(feature = "llm")]
    pub llm_client: Arc<std::sync::Mutex<Option<Arc<trust_kernel::llm::client::LlmClient>>>>,
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
            #[cfg(feature = "llm")]
            llm_client: Arc::new(std::sync::Mutex::new(None)),
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
            #[cfg(feature = "llm")]
            llm_client: Arc::new(std::sync::Mutex::new(None)),
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

    /// W7: 根据 Settings 构建 LlmClient(不写入缓存,仅返回新实例)。
    ///
    /// 隐私契约(对应 `configuration-and-automation-safety`):
    ///   - `privacy_mode = true` → 强制 `LlmClient::disabled()`(不发送任何网络请求)
    ///   - `llm_enabled = false` → 同上
    ///   - `llm_api_key` 空 → 同上(避免发送无凭证请求触发 401)
    ///
    /// 调用方:`update_settings_command` 在持久化 Settings 后调此方法 +
    /// 写入 `self.llm_client` 缓存,下次 `route_text` 即用新配置。
    #[cfg(all(feature = "tauri", feature = "llm"))]
    pub fn rebuild_llm_client(
        &self,
        settings: &crate::settings_commands::SettingsDto,
    ) -> Arc<trust_kernel::llm::client::LlmClient> {
        use trust_kernel::llm::client::LlmClient;
        if settings.privacy_mode || !settings.llm_enabled || settings.llm_api_key.is_empty() {
            return Arc::new(LlmClient::disabled());
        }
        Arc::new(LlmClient::new(
            &settings.llm_base_url,
            &settings.llm_api_key,
            &settings.llm_model,
        ))
    }

    /// W7: 读取缓存的 LlmClient;未初始化时返回 `LlmClient::disabled()`(纯 keyword 路由)。
    ///
    /// `route_text` 调此方法获取 LlmClient 传给 `SkillRouter::with_llm`。
    /// `LlmClient::disabled()` 的 `is_enabled() = false`,`SkillRouter::route_with_llm`
    /// 检测到 disabled 会跳过 LLM 分支,纯走 keyword 匹配。
    #[cfg(feature = "llm")]
    pub fn llm_client(&self) -> Arc<trust_kernel::llm::client::LlmClient> {
        use trust_kernel::llm::client::LlmClient;
        let guard = self.llm_client.lock().unwrap();
        guard
            .clone()
            .unwrap_or_else(|| Arc::new(LlmClient::disabled()))
    }
}
