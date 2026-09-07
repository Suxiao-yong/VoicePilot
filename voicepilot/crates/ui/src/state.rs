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
    /// TTS 自激防护(Phase 1,与 buzz TTS_COOLDOWN 同值):tts_command 合成成功后
    /// 按“音频时长 + cooldown”估算播放结束时刻;下次 listen 落在此窗口内的
    /// chunk 直接丢弃(防音箱尾音被当成用户说话)。None = 无防护。
    #[cfg(feature = "voice")]
    pub tts_cooldown_until: Arc<std::sync::Mutex<Option<std::time::SystemTime>>>,
    /// 录音并发防护(桌宠化改造):同一时刻只允许一路录音(主界面麦克风 /
    /// 桌宠单击录音),防止两路同时打开 cpal 输入流。voice_listen_command 开头
    /// `compare_exchange(false → true)` 抢占,结束时(含错误路径)复位。
    #[cfg(all(feature = "tauri", feature = "voice"))]
    pub recording: std::sync::atomic::AtomicBool,
    /// 桌宠确认气泡是否可见(桌宠化改造):鼠标穿透看门狗据此把气泡区域
    /// 纳入可交互热区。PetWindow 经 pet_set_bubble_visible 命令同步。
    #[cfg(feature = "tauri")]
    pub bubble_visible: std::sync::atomic::AtomicBool,
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
            #[cfg(feature = "voice")]
            tts_cooldown_until: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(all(feature = "tauri", feature = "voice"))]
            recording: std::sync::atomic::AtomicBool::new(false),
            #[cfg(feature = "tauri")]
            bubble_visible: std::sync::atomic::AtomicBool::new(false),
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
            #[cfg(feature = "voice")]
            tts_cooldown_until: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    /// Wave 3 Task 3.1: 注入 SecretStore 的内存版测试构造器 —— 测试用内存
    /// 实现,避免把测试 key 写入真实 Windows Credential Manager。
    pub fn new_in_memory_with_secret_store(
        store: std::sync::Arc<dyn trust_kernel::secrets::SecretStore>,
    ) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory_with_secret_store(store)?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }

    /// W1 Task 1.3: 根据 Settings 构建 LlmClient 并写入 kernel 的单一运行时配置。
    ///
    /// 隐私契约(对应 `configuration-and-automation-safety`):
    ///   - `privacy_mode = true` → 强制 `LlmClient::disabled()`(不发送任何网络请求)
    ///   - `llm_enabled = false` → 同上
    ///   - SecretStore 无 key / key 为空 → 同上(避免发送无凭证请求触发 401)
    ///
    /// Wave 3 Task 3.1:LLM key 只从 kernel 的 SecretStore(keyring)读取,
    /// 绝不来自 Settings DTO / SQLite 明文。
    ///
    /// AppState 不再持有 LLM 缓存(避免双份运行时配置):本方法通过
    /// `kernel.set_llm_client(...)` 写入,路由统一从 `kernel.llm_client()` 读取。
    #[cfg(all(feature = "tauri", feature = "llm"))]
    pub fn rebuild_llm_client(
        &self,
        settings: &crate::settings_commands::SettingsView,
    ) -> Arc<trust_kernel::llm::client::LlmClient> {
        use trust_kernel::llm::client::LlmClient;
        let stored_key = self.kernel.llm_api_key().unwrap_or_default();
        let has_key = stored_key.as_deref().is_some_and(|k| !k.is_empty());
        let client = if settings.privacy_mode || !settings.llm_enabled || !has_key {
            Arc::new(LlmClient::disabled())
        } else {
            Arc::new(LlmClient::new(
                &settings.llm_base_url,
                stored_key.as_deref().unwrap_or_default(),
                &settings.llm_model,
            ))
        };
        self.kernel.set_llm_client(Some(client.clone()));
        client
    }

    /// W1 Task 1.3: 读取 kernel 的 LLM 客户端(单一运行时配置);未设置时返回
    /// `LlmClient::disabled()`(纯 keyword 路由)。
    #[cfg(feature = "llm")]
    pub fn llm_client(&self) -> Arc<trust_kernel::llm::client::LlmClient> {
        use trust_kernel::llm::client::LlmClient;
        self.kernel
            .llm_client()
            .unwrap_or_else(|| Arc::new(LlmClient::disabled()))
    }
}
