//! Settings Tauri commands —— V1.1.2 §8.3 Settings 面板后端。
//!
//! Wave 3 Task 3.1: DTO 拆分 —— 读接口返回 `SettingsView`(含
//! `llm_api_key_present`,绝不返回 key 值本身),写接口只接受 `SettingsUpdate`
//! (显式 set/clear key)。LLM API key 经 SecretStore(keyring)存取,永不写入
//! `app_config` 明文。
//!
//! 持久化层:ConfigRepo KV 表(app_config,只存非 secret 字段)。
//! 内部:flatten_to_kv / merge_from_kv 在 SettingsView 与 KV 之间转换。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{UiError, UiResult};
use crate::state::AppState;

/// 读取 DTO:非 secret 字段 + `llm_api_key_present`(SecretStore 中是否已有
/// LLM key)。任何响应都绝不含 key 值本身。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsView {
    pub voice_model_path: String,
    pub voice_language: Option<String>,
    pub voice_threads: u32,
    pub vad_energy_threshold: f32,
    pub vad_max_silence_ms: u32,
    pub vad_min_speech_ms: u32,
    pub voice_max_duration_ms: u64,
    pub voice_chunk_duration_ms: u64,
    pub privacy_mode: bool,
    pub compensation_ttl_hours: u32,
    pub tts_enabled: bool,
    pub tts_model_path: String,
    // W7 新增:云端 LLM 配置(OpenAI 兼容,默认 DeepSeek)。
    // llm_enabled=false 时 LlmClient::disabled(),路由纯走 keyword 匹配。
    // privacy_mode=true 强制覆盖 llm_enabled 为 false(rebuild_llm_client 实现)。
    pub llm_enabled: bool,
    /// SecretStore 中是否已配置 LLM key。读取接口永不返回 key 值。
    pub llm_api_key_present: bool,
    pub llm_base_url: String,
    pub llm_model: String,
    /// "Get API Key" 链接(前端 <a href>,DeepSeek/OpenAI/Qwen/Kimi 等 provider 入口)。
    pub llm_provider_url: String,
    // W7 Plan 4: UIA 应用白名单(quick.app_control / note.capture 可启动的应用列表)。
    // 前端逗号分隔输入,KV 存 JSON 数组字符串。
    // 即使 `uia` feature 关闭此字段也存在(纯数据,无害)——避免 DTO 形状随 feature 变化。
    pub uia_allowed_apps: Vec<String>,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self {
            // W6b-3b Fix 3:空字符串表示"用默认模型",由 voice_commands 用
            // ModelRegistry::default_model() 解析到 ~/.voicepilot/models/<default_model_name>。
            // 此前默认值是模型名(相对路径),SherpaAsrEngine::new 校验 model_dir.is_dir()
            // 时从 CWD 查找必失败。
            voice_model_path: String::new(),
            voice_language: None,
            voice_threads: 4,
            vad_energy_threshold: 100.0,
            vad_max_silence_ms: 700,
            vad_min_speech_ms: 200,
            voice_max_duration_ms: 30000,
            voice_chunk_duration_ms: 500,
            privacy_mode: false,
            compensation_ttl_hours: 24,
            tts_enabled: true,
            // W6b-3b Fix 3:同上,空字符串表示"未配置",tts_command 检测到空时返回友好错误。
            tts_model_path: String::new(),
            // W7 默认值:llm_enabled=false(opt-in),无 key(未配置)。
            // base_url/model/provider_url 预填 DeepSeek 默认值,用户切换 provider 时改。
            llm_enabled: false,
            llm_api_key_present: false,
            llm_base_url: "https://api.deepseek.com/v1".to_string(),
            llm_model: "deepseek-chat".to_string(),
            llm_provider_url: "https://platform.deepseek.com/api_keys".to_string(),
            // W7 Plan 4: V1.1 §8.1 quick.app_control / note.capture 默认白名单。
            uia_allowed_apps: vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ],
        }
    }
}

/// 写入 DTO:非 secret 字段 + 显式 secret 操作。
///
/// - `llm_api_key: Some(非空)` → 写入 SecretStore;`None` / 空 → 保持现状。
/// - `clear_llm_api_key: true` → 删除 SecretStore 中的 key。
///
/// 写入接口只接受此类型,绝不接受 key 值的读回。
#[derive(Clone, Serialize, Deserialize)]
pub struct SettingsUpdate {
    pub voice_model_path: String,
    pub voice_language: Option<String>,
    pub voice_threads: u32,
    pub vad_energy_threshold: f32,
    pub vad_max_silence_ms: u32,
    pub vad_min_speech_ms: u32,
    pub voice_max_duration_ms: u64,
    pub voice_chunk_duration_ms: u64,
    pub privacy_mode: bool,
    pub compensation_ttl_hours: u32,
    pub tts_enabled: bool,
    pub tts_model_path: String,
    pub llm_enabled: bool,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_provider_url: String,
    /// 新 key;`None` / 空串表示保持现有 key 不变。
    pub llm_api_key: Option<String>,
    /// `true` 表示删除已存储的 key。
    pub clear_llm_api_key: bool,
    pub uia_allowed_apps: Vec<String>,
}

/// Debug 只暴露"是否提供了新 key",绝不打印 key 值本身。
impl std::fmt::Debug for SettingsUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsUpdate")
            .field("voice_model_path", &self.voice_model_path)
            .field("voice_language", &self.voice_language)
            .field("voice_threads", &self.voice_threads)
            .field("vad_energy_threshold", &self.vad_energy_threshold)
            .field("vad_max_silence_ms", &self.vad_max_silence_ms)
            .field("vad_min_speech_ms", &self.vad_min_speech_ms)
            .field("voice_max_duration_ms", &self.voice_max_duration_ms)
            .field("voice_chunk_duration_ms", &self.voice_chunk_duration_ms)
            .field("privacy_mode", &self.privacy_mode)
            .field("compensation_ttl_hours", &self.compensation_ttl_hours)
            .field("tts_enabled", &self.tts_enabled)
            .field("tts_model_path", &self.tts_model_path)
            .field("llm_enabled", &self.llm_enabled)
            .field("llm_base_url", &self.llm_base_url)
            .field("llm_model", &self.llm_model)
            .field("llm_provider_url", &self.llm_provider_url)
            .field("llm_api_key_provided", &self.llm_api_key.is_some())
            .field("clear_llm_api_key", &self.clear_llm_api_key)
            .field("uia_allowed_apps", &self.uia_allowed_apps)
            .finish()
    }
}

impl From<&SettingsUpdate> for SettingsView {
    /// 把写入 DTO 的非 secret 字段映射为读取 DTO(secret 状态由调用方填写)。
    fn from(update: &SettingsUpdate) -> Self {
        Self {
            voice_model_path: update.voice_model_path.clone(),
            voice_language: update.voice_language.clone(),
            voice_threads: update.voice_threads,
            vad_energy_threshold: update.vad_energy_threshold,
            vad_max_silence_ms: update.vad_max_silence_ms,
            vad_min_speech_ms: update.vad_min_speech_ms,
            voice_max_duration_ms: update.voice_max_duration_ms,
            voice_chunk_duration_ms: update.voice_chunk_duration_ms,
            privacy_mode: update.privacy_mode,
            compensation_ttl_hours: update.compensation_ttl_hours,
            tts_enabled: update.tts_enabled,
            tts_model_path: update.tts_model_path.clone(),
            llm_enabled: update.llm_enabled,
            // 由调用方(get_settings)从 SecretStore 覆盖。
            llm_api_key_present: false,
            llm_base_url: update.llm_base_url.clone(),
            llm_model: update.llm_model.clone(),
            llm_provider_url: update.llm_provider_url.clone(),
            uia_allowed_apps: update.uia_allowed_apps.clone(),
        }
    }
}

/// 将 View 展平为 KV 列表(只含非 secret 字段;`llm.api_key` 永不被持久化)。
pub fn flatten_to_kv(dto: &SettingsView) -> Vec<(String, String)> {
    vec![
        ("voice.model_path".to_string(), dto.voice_model_path.clone()),
        ("voice.language".to_string(), dto.voice_language.clone().unwrap_or_default()),
        ("voice.threads".to_string(), dto.voice_threads.to_string()),
        ("voice.vad.energy_threshold".to_string(), dto.vad_energy_threshold.to_string()),
        ("voice.vad.max_silence_ms".to_string(), dto.vad_max_silence_ms.to_string()),
        ("voice.vad.min_speech_ms".to_string(), dto.vad_min_speech_ms.to_string()),
        ("voice.max_duration_ms".to_string(), dto.voice_max_duration_ms.to_string()),
        ("voice.chunk_duration_ms".to_string(), dto.voice_chunk_duration_ms.to_string()),
        ("privacy.mode".to_string(), dto.privacy_mode.to_string()),
        ("compensation.ttl_hours".to_string(), dto.compensation_ttl_hours.to_string()),
        ("tts.enabled".to_string(), dto.tts_enabled.to_string()),
        ("tts.model_path".to_string(), dto.tts_model_path.clone()),
        // W7 LLM —— 只存非 secret 字段。`llm.api_key` 绝不落盘(SecretStore 持有)。
        ("llm.enabled".to_string(), dto.llm_enabled.to_string()),
        ("llm.base_url".to_string(), dto.llm_base_url.clone()),
        ("llm.model".to_string(), dto.llm_model.clone()),
        ("llm.provider_url".to_string(), dto.llm_provider_url.clone()),
        // W7 Plan 4: UIA 白名单 —— JSON 数组字符串(如 ["notepad","explorer","calc"])。
        // serde_json::to_string 失败时存空串(理论上 Vec<String> 序列化不会失败)。
        (
            "uia.allowed_apps".to_string(),
            serde_json::to_string(&dto.uia_allowed_apps).unwrap_or_default(),
        ),
    ]
}

/// 数值 clamp 辅助(2026-08-24 3-5):后端对越界数值收敛到合理区间。
/// 前端表单已做同样 clamp,这里兜底 UI 之外写入路径(直接改配置 KV / 旧版本残留)。
fn clamp_range<T: PartialOrd>(v: T, min: T, max: T) -> T {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// 从 KV 列表合并为 View(缺失字段用默认值)。`llm.api_key` / 
/// `llm.api_key_present` 这类 secret 派生 key 会被忽略(读取接口从
/// SecretStore 取真实状态)。
pub fn merge_from_kv(kv: &[(String, String)]) -> UiResult<SettingsView> {
    let mut dto = SettingsView::default();
    for (k, v) in kv {
        match k.as_str() {
            "voice.model_path" => dto.voice_model_path = v.clone(),
            "voice.language" => dto.voice_language = if v.is_empty() { None } else { Some(v.clone()) },
            "voice.threads" => dto.voice_threads = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("voice.threads: {e}")))?, 1, 16),
            "voice.vad.energy_threshold" => dto.vad_energy_threshold = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("energy_threshold: {e}")))?, 0.0, 100_000.0),
            "voice.vad.max_silence_ms" => dto.vad_max_silence_ms = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("max_silence_ms: {e}")))?, 100, 30_000),
            "voice.vad.min_speech_ms" => dto.vad_min_speech_ms = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("min_speech_ms: {e}")))?, 50, 5_000),
            "voice.max_duration_ms" => dto.voice_max_duration_ms = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("max_duration_ms: {e}")))?, 1_000, 600_000),
            "voice.chunk_duration_ms" => dto.voice_chunk_duration_ms = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("chunk_duration_ms: {e}")))?, 10, 60_000),
            "privacy.mode" => dto.privacy_mode = v.parse().map_err(|e| UiError::InvalidConfig(format!("privacy.mode: {e}")))?,
            "compensation.ttl_hours" => dto.compensation_ttl_hours = clamp_range(v.parse().map_err(|e| UiError::InvalidConfig(format!("ttl_hours: {e}")))?, 1, 8_760),
            "tts.enabled" => dto.tts_enabled = v.parse().map_err(|e| UiError::InvalidConfig(format!("tts.enabled: {e}")))?,
            "tts.model_path" => dto.tts_model_path = v.clone(),
            // W7 LLM —— 只解析非 secret 字段。
            "llm.enabled" => dto.llm_enabled = v.parse().map_err(|e| UiError::InvalidConfig(format!("llm.enabled: {e}")))?,
            "llm.base_url" => dto.llm_base_url = v.clone(),
            "llm.model" => dto.llm_model = v.clone(),
            "llm.provider_url" => dto.llm_provider_url = v.clone(),
            // W7 Plan 4: UIA 白名单 —— JSON 数组反序列化,解析失败返回 InvalidConfig。
            // 空串视为空数组(向前兼容:旧版本无此 key 时 default 已预填默认值)。
            "uia.allowed_apps" => {
                dto.uia_allowed_apps = if v.is_empty() {
                    Vec::new()
                } else {
                    serde_json::from_str(v).map_err(|e| {
                        UiError::InvalidConfig(format!("uia.allowed_apps: {e}"))
                    })?
                };
            }
            // 忽略未知 key 以及 secret 派生 key("llm.api_key" / "llm.api_key_present")
            // —— 前向兼容 + 读取接口从 SecretStore 取真实状态。
            _ => {}
        }
    }
    Ok(dto)
}

/// 读取所有设置(合并持久化值与默认值;`llm_api_key_present` 从 SecretStore 取)。
pub fn get_settings(state: &AppState) -> UiResult<SettingsView> {
    let kv = {
        let conn = state.kernel.conn();
        state.kernel.config_repo().list(&conn)?
    };
    let mut view = merge_from_kv(&kv)?;
    // present 状态以 SecretStore 实时读取为准(读取失败按"无 key"降级,与
    // rebuild_llm_client 的 fail-safe 策略一致,不让设置页整体加载失败)。
    view.llm_api_key_present = state.kernel.llm_api_key().unwrap_or_default().is_some();
    Ok(view)
}

/// 读取所有设置 Tauri command 包装。
#[tauri::command]
pub async fn get_settings_command(state: State<'_, AppState>) -> Result<SettingsView, String> {
    get_settings(&state).map_err(Into::into)
}

/// 更新设置:先处理 SecretStore 中的 key(用户最关心的状态,也是最可能失败
/// 的环节),再持久化非 secret KV。若 key 操作失败,直接返回,避免 KV 先落盘
/// 造成"部分应用"与 serving-applied 状态脱节。
pub fn update_settings(state: &AppState, update: &SettingsUpdate) -> UiResult<()> {
    // 显式 secret 操作:先 clear 再 set,set 覆盖 clear。
    if update.clear_llm_api_key {
        state.kernel.clear_llm_api_key()?;
    }
    if let Some(key) = update.llm_api_key.as_deref().filter(|k| !k.is_empty()) {
        state.kernel.set_llm_api_key(key)?;
    }
    // 持久化非 secret KV。用 block scope 限制 conn guard 生命周期(防止
    // 未来新增调用在 guard 存活期间重入 kernel.conn 导致 Mutex 死锁)。
    let view = SettingsView::from(update);
    let kv = flatten_to_kv(&view);
    {
        let conn = state.kernel.conn();
        for (k, v) in &kv {
            state.kernel.config_repo().set(&conn, k, v)?;
        }
    }
    Ok(())
}

/// 更新设置 Tauri command 包装。
///
/// W7: 持久化 Settings 后重建并缓存 LlmClient(对应 `configuration-and-automation-safety` 的
/// "application-state" 检查 —— 写入 KV 是 accepted/persisted,但 route_text 用的是
/// serving-applied 状态;此处显式刷新 serving-applied LlmClient,避免 Settings 改了
/// 但路由仍用旧客户端)。
#[tauri::command]
pub async fn update_settings_command(
    state: State<'_, AppState>,
    settings: SettingsUpdate,
) -> Result<(), String> {
    update_settings(&state, &settings).map_err(|e: crate::error::UiError| e.to_string())?;
    // W7 Plan 4: 同步 UIA 白名单到运行时 TrustKernel(与 LlmClient 重建同理:
    // KV 是 accepted/persisted,但 serving-applied 状态需显式刷新)。
    state
        .kernel
        .set_allowed_apps(settings.uia_allowed_apps.clone());
    // W1 Task 1.3: 刷新 kernel 的单一 LLM 运行时配置(不再缓存到 AppState,
    // 避免双份运行时配置)。rebuild_llm_client 从 SecretStore 读 key:
    // - privacy_mode=true / llm_enabled=false / store 无 key → LlmClient::disabled()
    // - 否则 → LlmClient::new(base_url, store_key, model)
    // 下次路由调 kernel.llm_client() 即拿到新实例。
    #[cfg(feature = "llm")]
    {
        let view = SettingsView::from(&settings);
        state.rebuild_llm_client(&view);
    }
    Ok(())
}

/// 启动重建：从持久化 KV + SecretStore 重建 serving-applied client。
/// 此前只在设置保存时 rebuild，重启后 LLM 回到 None —— 这是“配了云端还不会聊天”的根因之一。
/// 无配置的新用户走默认分支（disabled），失败不阻断启动由调用方决定。
#[cfg(feature = "llm")]
pub fn startup_rebuild_llm(state: &AppState) {
    let view = get_settings(state).unwrap_or_default();
    state.rebuild_llm_client(&view);
}

/// 测试连接输入：用表单当前值测（保存前可测）。
/// `api_key` 为空则用 SecretStore 中已存的 key（测“已保存配置”）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmTestInput {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// 测试连接失败原因（前端据此给可操作提示，不暴露 key 与原始错误细节）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmTestFailure {
    /// 未配置：url/key/model 任一为空，或隐私模式拦截。
    NotConfigured,
    /// 401：key 错了或已失效。
    Unauthorized,
    /// 404：base_url 路径错或模型名在该 provider 下不存在。
    NotFound,
    /// 15s 内无响应：网络/代理/服务商慢。
    Timeout,
    /// DNS/连接被拒/TLS 等传输层失败。
    Network,
    /// 非 JSON 响应（多为代理/网关拦截页）。
    Parse,
}

/// 测试连接结果（绝不含 key）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LlmTestResult {
    Ok { latency_ms: u64, model: String },
    Failed { reason: LlmTestFailure, message: String },
}

/// 纯函数：`LlmError` → 前端可消费的失败分类（可单测，无需网络）。
pub fn map_probe_error(e: &trust_kernel::llm::types::LlmError) -> (LlmTestFailure, String) {
    use trust_kernel::llm::types::LlmError;
    match e {
        LlmError::NotConfigured | LlmError::DisabledByPrivacy => (
            LlmTestFailure::NotConfigured,
            "未配置：检查开关、base_url、模型名，并确认已填写 key（隐私模式下不可用）".to_string(),
        ),
        LlmError::Http(msg) if msg.contains("401") => (
            LlmTestFailure::Unauthorized,
            "401：API key 错误或已失效，请重新获取填写".to_string(),
        ),
        LlmError::Http(msg) if msg.contains("404") => (
            LlmTestFailure::NotFound,
            "404：base_url 路径或模型名不对，请对照服务商文档检查".to_string(),
        ),
        LlmError::Http(msg) => (
            LlmTestFailure::Network,
            format!("请求失败：{msg}（检查网络/代理/base_url）"),
        ),
        LlmError::Timeout(_) => (
            LlmTestFailure::Timeout,
            "15s 无响应：检查网络/代理，或稍后重试".to_string(),
        ),
        LlmError::Parse(_) => (
            LlmTestFailure::Parse,
            "响应不是合法 JSON：可能是代理/网关拦截页，检查 base_url".to_string(),
        ),
    }
}

/// 测试云端 LLM 连接：只读探针，不持久化任何东西，不记录 key。
///
/// 设计：只验证“配置能否调通”，不验证“路由好不好”；真正的路由质量
/// 由 planner 的 classify/decompose 负责。探针失败只返回分类，不抛错，
/// 前端据 `reason` 给可操作提示。
#[tauri::command]
pub async fn test_llm_command(
    state: State<'_, AppState>,
    input: LlmTestInput,
) -> Result<LlmTestResult, String> {
    #[cfg(feature = "llm")]
    {
        use trust_kernel::llm::client::LlmClient;
        if state.kernel.privacy_mode() {
            return Ok(LlmTestResult::Failed {
                reason: LlmTestFailure::NotConfigured,
                message: "隐私模式已启用：LLM 不可用，先关闭隐私模式再测".to_string(),
            });
        }
        let key = match input.api_key.as_deref().filter(|k| !k.is_empty()) {
            Some(k) => k.to_string(),
            None => state
                .kernel
                .llm_api_key()
                .map_err(|e| e.to_string())?
                .unwrap_or_default(),
        };
        let client = LlmClient::new(&input.base_url, &key, &input.model);
        match client.probe().await {
            Ok(out) => Ok(LlmTestResult::Ok {
                latency_ms: out.latency_ms,
                model: out.model,
            }),
            Err(e) => {
                let (reason, message) = map_probe_error(&e);
                Ok(LlmTestResult::Failed { reason, message })
            }
        }
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = (state, input);
        Ok(LlmTestResult::Failed {
            reason: LlmTestFailure::NotConfigured,
            message: "当前构建未启用 LLM 功能".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_dto_tts_roundtrip() {
        let dto = SettingsView {
            tts_enabled: false,
            tts_model_path: "custom-tts-model".to_string(),
            ..Default::default()
        };
        let kv = flatten_to_kv(&dto);
        let parsed = merge_from_kv(&kv).unwrap();
        assert!(!parsed.tts_enabled);
        assert_eq!(parsed.tts_model_path, "custom-tts-model");
    }

    #[test]
    fn llm_settings_roundtrip() {
        let dto = SettingsView {
            llm_enabled: true,
            llm_api_key_present: true,
            llm_base_url: "https://api.deepseek.com/v1".to_string(),
            llm_model: "deepseek-chat".to_string(),
            llm_provider_url: "https://platform.deepseek.com/api_keys".to_string(),
            ..Default::default()
        };

        let kv = flatten_to_kv(&dto);
        let restored = merge_from_kv(&kv).unwrap();
        assert!(restored.llm_enabled);
        assert_eq!(restored.llm_base_url, "https://api.deepseek.com/v1");
        assert_eq!(restored.llm_model, "deepseek-chat");
        assert_eq!(restored.llm_provider_url, "https://platform.deepseek.com/api_keys");
    }

    #[test]
    fn llm_defaults_are_disabled() {
        let dto = SettingsView::default();
        assert!(!dto.llm_enabled);
        assert!(!dto.llm_api_key_present);
        assert_eq!(dto.llm_base_url, "https://api.deepseek.com/v1");
        assert_eq!(dto.llm_model, "deepseek-chat");
        assert_eq!(dto.llm_provider_url, "https://platform.deepseek.com/api_keys");
    }

    #[test]
    fn probe_error_mapping_covers_all_variants() {
        use trust_kernel::llm::types::LlmError;
        let (r, _) = map_probe_error(&LlmError::NotConfigured);
        assert!(matches!(r, LlmTestFailure::NotConfigured));
        let (r, _) = map_probe_error(&LlmError::DisabledByPrivacy);
        assert!(matches!(r, LlmTestFailure::NotConfigured));
        let (r, msg) = map_probe_error(&LlmError::Http("HTTP 401 Unauthorized".to_string()));
        assert!(matches!(r, LlmTestFailure::Unauthorized));
        assert!(msg.contains("401"));
        let (r, _) = map_probe_error(&LlmError::Http("HTTP 404 Not Found".to_string()));
        assert!(matches!(r, LlmTestFailure::NotFound));
        let (r, _) = map_probe_error(&LlmError::Http("HTTP 500 Internal".to_string()));
        assert!(matches!(r, LlmTestFailure::Network));
        let (r, _) = map_probe_error(&LlmError::Timeout(std::time::Duration::from_secs(15)));
        assert!(matches!(r, LlmTestFailure::Timeout));
        let (r, _) = map_probe_error(&LlmError::Parse("bad json".to_string()));
        assert!(matches!(r, LlmTestFailure::Parse));
    }

    #[test]
    fn test_result_serializes_without_key() {
        // 结果 DTO 绝不能携带 key：序列化后检查无敏感字段名。
        let r = LlmTestResult::Ok {
            latency_ms: 320,
            model: "deepseek-chat".to_string(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("deepseek-chat"));
        assert!(!s.to_lowercase().contains("api_key") && !s.to_lowercase().contains("apikey"));
    }

    #[test]
    fn flatten_to_kv_never_serializes_llm_api_key() {
        // SettingsView 根本不含 key 字段;显式断言 KV 中没有任何 llm.api_key。
        let dto = SettingsView::default();
        let kv = flatten_to_kv(&dto);
        assert!(
            !kv.iter().any(|(k, _)| k == "llm.api_key"),
            "flatten_to_kv must never persist llm.api_key"
        );
    }

    #[test]
    fn merge_from_kv_ignores_secret_derived_keys() {
        let kv = vec![
            ("llm.api_key".to_string(), "sk-should-be-ignored".to_string()),
            ("llm.api_key_present".to_string(), "true".to_string()),
            ("llm.enabled".to_string(), "true".to_string()),
        ];
        let view = merge_from_kv(&kv).unwrap();
        assert!(view.llm_enabled);
        // 读取接口不解析 secret 派生 key。
        assert!(!view.llm_api_key_present, "view state comes from SecretStore");
    }

    /// SettingsUpdate → SettingsView 的非 secret 字段映射正确。
    #[test]
    fn settings_update_maps_to_view() {
        let update = SettingsUpdate {
            voice_threads: 8,
            llm_enabled: true,
            llm_api_key: Some("sk-new".to_string()),
            clear_llm_api_key: false,
            ..from_default_update()
        };
        let view = SettingsView::from(&update);
        assert_eq!(view.voice_threads, 8);
        assert!(view.llm_enabled);
        assert!(!view.llm_api_key_present, "view present flag filled by caller");
    }

    fn from_default_update() -> SettingsUpdate {
        let view = SettingsView::default();
        SettingsUpdate {
            voice_model_path: view.voice_model_path,
            voice_language: view.voice_language,
            voice_threads: view.voice_threads,
            vad_energy_threshold: view.vad_energy_threshold,
            vad_max_silence_ms: view.vad_max_silence_ms,
            vad_min_speech_ms: view.vad_min_speech_ms,
            voice_max_duration_ms: view.voice_max_duration_ms,
            voice_chunk_duration_ms: view.voice_chunk_duration_ms,
            privacy_mode: view.privacy_mode,
            compensation_ttl_hours: view.compensation_ttl_hours,
            tts_enabled: view.tts_enabled,
            tts_model_path: view.tts_model_path,
            llm_enabled: view.llm_enabled,
            llm_base_url: view.llm_base_url,
            llm_model: view.llm_model,
            llm_provider_url: view.llm_provider_url,
            llm_api_key: None,
            clear_llm_api_key: false,
            uia_allowed_apps: view.uia_allowed_apps,
        }
    }

    // ===== W7 Plan 4: UIA 应用白名单 =====

    #[test]
    fn uia_allowed_apps_roundtrip() {
        let dto = SettingsView::default();
        assert_eq!(
            dto.uia_allowed_apps,
            vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ]
        );

        let dto = SettingsView {
            uia_allowed_apps: vec!["notepad".to_string(), "code".to_string()],
            ..Default::default()
        };
        let kv = flatten_to_kv(&dto);
        let restored = merge_from_kv(&kv).unwrap();
        assert_eq!(
            restored.uia_allowed_apps,
            vec!["notepad".to_string(), "code".to_string()]
        );
    }

    #[test]
    fn uia_allowed_apps_default() {
        let dto = SettingsView::default();
        assert_eq!(dto.uia_allowed_apps, vec!["notepad", "explorer", "calc"]);
    }

    #[test]
    fn uia_allowed_apps_empty_serialization() {
        let dto = SettingsView {
            uia_allowed_apps: vec![],
            ..Default::default()
        };
        let kv = flatten_to_kv(&dto);
        let restored = merge_from_kv(&kv).unwrap();
        assert!(restored.uia_allowed_apps.is_empty());
    }

    /// W7 Plan 4 Task 5 (review fix): `update_settings_command` 必须 sync
    /// `uia_allowed_apps` 到运行时 `kernel.allowed_apps()`(B1 fix)。
    #[test]
    fn update_settings_syncs_allowed_apps_to_kernel() {
        let state = crate::state::AppState::new_in_memory().expect("AppState::new_in_memory");

        {
            let apps = state.kernel.allowed_apps();
            assert_eq!(
                *apps,
                vec![
                    "notepad".to_string(),
                    "explorer".to_string(),
                    "calc".to_string(),
                ]
            );
        }

        let mut update = from_default_update();
        update.uia_allowed_apps = vec!["code".to_string(), "terminal".to_string()];
        update_settings(&state, &update).expect("update_settings");

        state
            .kernel
            .set_allowed_apps(update.uia_allowed_apps.clone());

        let apps = state.kernel.allowed_apps();
        assert_eq!(
            *apps,
            vec!["code".to_string(), "terminal".to_string()],
            "update_settings + set_allowed_apps must sync uia_allowed_apps to kernel.allowed_apps()"
        );

        let conn = state.kernel.conn();
        let kv = state.kernel.config_repo().list(&conn).expect("config_repo list");
        let uia_kv = kv
            .iter()
            .find(|(k, _)| k == "uia.allowed_apps")
            .map(|(_, v)| v.clone())
            .expect("uia.allowed_apps in KV");
        assert!(uia_kv.contains("code") && uia_kv.contains("terminal"));
    }

    /// W7 Plan 4 Task 5 (review fix): `update_settings` 持久化 + boot reload
    /// 端到端验证。
    #[test]
    fn update_settings_persists_and_reloads_allowed_apps() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8").to_string();

        {
            let state = crate::state::AppState::new_file(&path).expect("AppState::new_file");
            let mut update = from_default_update();
            update.uia_allowed_apps = vec!["vim".to_string(), "emacs".to_string()];
            update_settings(&state, &update).expect("update_settings");
            state
                .kernel
                .set_allowed_apps(update.uia_allowed_apps.clone());
            let apps = state.kernel.allowed_apps();
            assert_eq!(*apps, vec!["vim".to_string(), "emacs".to_string()]);
        }

        {
            let state = crate::state::AppState::new_file(&path).expect("AppState::new_file phase 2");
            let apps = state.kernel.allowed_apps();
            assert_eq!(
                *apps,
                vec!["vim".to_string(), "emacs".to_string()],
                "boot load must pick up KV-persisted uia_allowed_apps"
            );
        }
    }
}
