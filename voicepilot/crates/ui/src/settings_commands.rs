//! Settings Tauri commands —— V1.1.2 §8.3 Settings 面板后端。
//!
//! 持久化层:ConfigRepo KV 表(app_config)。
//! 前端 DTO:SettingsDto(扁平结构,serde JSON)。
//! 内部:flatten_to_kv / merge_from_kv 在 DTO 与 KV 之间转换。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{UiError, UiResult};
use crate::state::AppState;

/// Settings 面板 DTO(前端直接消费的扁平结构)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsDto {
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
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            voice_model_path: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17".to_string(),
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
            tts_model_path: "vits-icefall-zh-aishell3".to_string(),
        }
    }
}

/// 将 DTO 展平为 KV 列表(用于持久化到 app_config 表)。
pub fn flatten_to_kv(dto: &SettingsDto) -> Vec<(String, String)> {
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
    ]
}

/// 从 KV 列表合并为 DTO(缺失字段用默认值)。
pub fn merge_from_kv(kv: &[(String, String)]) -> UiResult<SettingsDto> {
    let mut dto = SettingsDto::default();
    for (k, v) in kv {
        match k.as_str() {
            "voice.model_path" => dto.voice_model_path = v.clone(),
            "voice.language" => dto.voice_language = if v.is_empty() { None } else { Some(v.clone()) },
            "voice.threads" => dto.voice_threads = v.parse().map_err(|e| UiError::InvalidConfig(format!("voice.threads: {e}")))?,
            "voice.vad.energy_threshold" => dto.vad_energy_threshold = v.parse().map_err(|e| UiError::InvalidConfig(format!("energy_threshold: {e}")))?,
            "voice.vad.max_silence_ms" => dto.vad_max_silence_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("max_silence_ms: {e}")))?,
            "voice.vad.min_speech_ms" => dto.vad_min_speech_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("min_speech_ms: {e}")))?,
            "voice.max_duration_ms" => dto.voice_max_duration_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("max_duration_ms: {e}")))?,
            "voice.chunk_duration_ms" => dto.voice_chunk_duration_ms = v.parse().map_err(|e| UiError::InvalidConfig(format!("chunk_duration_ms: {e}")))?,
            "privacy.mode" => dto.privacy_mode = v.parse().map_err(|e| UiError::InvalidConfig(format!("privacy.mode: {e}")))?,
            "compensation.ttl_hours" => dto.compensation_ttl_hours = v.parse().map_err(|e| UiError::InvalidConfig(format!("ttl_hours: {e}")))?,
            "tts.enabled" => dto.tts_enabled = v.parse().map_err(|e| UiError::InvalidConfig(format!("tts.enabled: {e}")))?,
            "tts.model_path" => dto.tts_model_path = v.clone(),
            _ => {} // 忽略未知 key(前向兼容)
        }
    }
    Ok(dto)
}

/// 读取所有设置(合并持久化值与默认值)。
pub fn get_settings(state: &AppState) -> UiResult<SettingsDto> {
    let conn = state.kernel.conn();
    let kv = state.kernel.config_repo().list(&conn)?;
    merge_from_kv(&kv)
}

/// 读取所有设置 Tauri command 包装。
#[tauri::command]
pub async fn get_settings_command(state: State<'_, AppState>) -> Result<SettingsDto, String> {
    get_settings(&state).map_err(Into::into)
}

/// 更新设置(全量覆盖:将 DTO 展平后逐条 set,不存在部分更新语义)。
pub fn update_settings(state: &AppState, settings: &SettingsDto) -> UiResult<()> {
    let conn = state.kernel.conn();
    let kv = flatten_to_kv(settings);
    for (k, v) in &kv {
        state.kernel.config_repo().set(&conn, k, v)?;
    }
    Ok(())
}

/// 更新设置 Tauri command 包装。
#[tauri::command]
pub async fn update_settings_command(state: State<'_, AppState>, settings: SettingsDto) -> Result<(), String> {
    update_settings(&state, &settings).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_dto_tts_roundtrip() {
        let mut dto = SettingsDto::default();
        dto.tts_enabled = false;
        dto.tts_model_path = "custom-tts-model".to_string();
        let kv = flatten_to_kv(&dto);
        let parsed = merge_from_kv(&kv).unwrap();
        assert_eq!(parsed.tts_enabled, false);
        assert_eq!(parsed.tts_model_path, "custom-tts-model");
    }
}
