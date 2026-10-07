//! 模型 auto-download Tauri commands(W6b-3a Task 7)。
//!
//! 模块整体编译(无 voice feature 也能编译),但 voice 相关 import 在 cfg 内。

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(feature = "voice")]
use tauri::Emitter;
#[cfg(feature = "voice")]
use trust_kernel::voice::model_download::{
    DownloadProgress, default_model_info, download_model, model_state, models_dir,
};

/// 模型状态(前端 ModelDownloadBar 用)。
///
/// Wave 3 Task 3.2:从二值(Present/Absent)扩展为完整状态机:
/// Disabled / Missing / Downloading / Verifying / Ready / Failed。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Disabled,
    Missing,
    Downloading,
    Verifying,
    Ready,
    Failed,
}

/// 下载进度事件 payload。
///
/// 注意:不加 `#[serde(rename_all = "camelCase")]` —— 与现有
/// `TranscriptionPartialPayload` 等保持一致(全 snake_case)。
#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgressPayload {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
}

/// `is_voice_enabled` command。
#[tauri::command]
pub fn is_voice_enabled_command() -> bool {
    cfg!(feature = "voice")
}

/// `check_model` command。
#[tauri::command]
pub fn check_model_command() -> ModelStatus {
    #[cfg(feature = "voice")]
    {
        let info = default_model_info();
        match model_state(&models_dir(), &info.name) {
            Ok(trust_kernel::voice::model_download::ModelState::Ready) => ModelStatus::Ready,
            Ok(trust_kernel::voice::model_download::ModelState::Downloading) => {
                ModelStatus::Downloading
            }
            Ok(trust_kernel::voice::model_download::ModelState::Missing) => ModelStatus::Missing,
            Ok(trust_kernel::voice::model_download::ModelState::Disabled) => ModelStatus::Disabled,
            Ok(trust_kernel::voice::model_download::ModelState::Verifying) => {
                ModelStatus::Verifying
            }
            Ok(trust_kernel::voice::model_download::ModelState::Failed) => ModelStatus::Failed,
            Err(_) => ModelStatus::Missing,
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        ModelStatus::Disabled
    }
}

/// `download_model` command。
#[tauri::command]
pub async fn download_model_command(app: AppHandle) -> Result<String, String> {
    #[cfg(feature = "voice")]
    {
        let info = default_model_info();
        let info_clone = info.clone();

        let result = tokio::task::spawn_blocking(move || {
            let app_clone = app.clone();
            download_model(&info_clone, |progress: DownloadProgress| {
                let payload = DownloadProgressPayload {
                    downloaded_bytes: progress.downloaded_bytes,
                    total_bytes: progress.total_bytes,
                    percent: progress.percent,
                };
                let _ = app_clone.emit("model-download-progress", payload);
            })
        })
        .await
        .map_err(|e| format!("task join error: {}", e))?;

        match result {
            Ok(path) => Ok(path.to_string_lossy().into_owned()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        let _ = app;
        Err("voice feature not enabled".to_string())
    }
}
