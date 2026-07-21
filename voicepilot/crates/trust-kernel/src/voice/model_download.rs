//! 模型 auto-download(W6b-3a Task 6)。
//!
//! 用 `ureq` HTTPS 下载 Whisper 模型到 `~/.voicepilot/models/`:
//! - 100ms 节流进度回调
//! - .part 临时文件,原子 rename
//! - SHA256 校验(若 expected_sha256 提供)
//!
//! 整个模块在 `#[cfg(feature = "voice")]` 下(由 voice/mod.rs 控制)。

use crate::voice::error::{VoiceError, VoiceResult};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 模型信息(下载用)。
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub download_url: String,
    pub expected_sha256: Option<String>,
    pub size_hint_mb: u32,
}

/// 默认模型信息(ggml-tiny.bin,~75MB)。
pub fn default_model_info() -> ModelInfo {
    ModelInfo {
        name: "ggml-tiny.bin".to_string(),
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
                .to_string(),
        // HuggingFace 不提供官方 SHA256,这里用 None —— 下载后由
        // whisper-rs 加载时验证(失败会返回 ModelLoadFailed)
        expected_sha256: None,
        size_hint_mb: 75,
    }
}

/// 返回模型存储目录:`~/.voicepilot/models/`。
///
/// 用 `dirs` crate 跨平台获取 home 目录。
pub fn models_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".voicepilot").join("models")
}

/// 检查指定模型是否已存在。
pub fn check_model_present(name: &str) -> bool {
    models_dir().join(name).is_file()
}

/// 下载进度回调参数。
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
}

/// 下载模型到 `~/.voicepilot/models/<name>`。
///
/// - `on_progress`:每 100ms 调用一次(节流),传入下载进度
/// - 失败时清理 .part 文件
/// - 成功后原子 rename .part → 最终文件名
/// - 若 `expected_sha256` 提供,下载完成后校验 SHA256
pub fn download_model<F>(
    info: &ModelInfo,
    on_progress: F,
) -> VoiceResult<PathBuf>
where
    F: Fn(DownloadProgress),
{
    let dir = models_dir();
    fs::create_dir_all(&dir).map_err(|e| {
        VoiceError::DownloadFailed(format!("create models_dir failed: {}", e))
    })?;

    let final_path = dir.join(&info.name);
    let part_path = dir.join(format!("{}.part", info.name));

    // 若最终文件已存在,直接返回(幂等)
    if final_path.is_file() {
        return Ok(final_path);
    }

    // 清理可能残留的 .part 文件
    if part_path.exists() {
        let _ = fs::remove_file(&part_path);
    }

    // 发起 HTTP GET
    let resp = ureq::get(&info.download_url)
        .call()
        .map_err(|e| VoiceError::DownloadFailed(format!("HTTP request failed: {}", e)))?;

    let total_bytes = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    // 写入 .part 文件
    let mut part_file = fs::File::create(&part_path).map_err(|e| {
        VoiceError::DownloadFailed(format!("create .part file failed: {}", e))
    })?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 32 * 1024]; // 32KB buffer
    let mut downloaded: u64 = 0;
    let mut last_progress = Instant::now();
    let progress_throttle = Duration::from_millis(100);

    loop {
        let n = resp
            .into_reader()
            .read(&mut buf)
            .map_err(|e| VoiceError::DownloadFailed(format!("read failed: {}", e)))?;
        if n == 0 {
            break;
        }
        part_file
            .write_all(&buf[..n])
            .map_err(|e| VoiceError::DownloadFailed(format!("write failed: {}", e)))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;

        // 节流:每 100ms 报告一次进度
        if last_progress.elapsed() >= progress_throttle {
            let percent = total_bytes.map(|t| (downloaded as f32 / t as f32) * 100.0);
            on_progress(DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes,
                percent,
            });
            last_progress = Instant::now();
        }
    }

    // 最终进度报告
    let percent = total_bytes.map(|t| (downloaded as f32 / t as f32) * 100.0);
    on_progress(DownloadProgress {
        downloaded_bytes: downloaded,
        total_bytes,
        percent,
    });

    // flush + sync
    part_file
        .sync_all()
        .map_err(|e| VoiceError::DownloadFailed(format!("sync failed: {}", e)))?;
    drop(part_file);

    // SHA256 校验(若提供)
    if let Some(expected) = &info.expected_sha256 {
        let actual = format!("{:x}", hasher.finalize());
        if &actual != expected {
            let _ = fs::remove_file(&part_path);
            return Err(VoiceError::DownloadFailed(format!(
                "SHA256 mismatch: expected={} actual={}",
                expected, actual
            )));
        }
    }

    // 原子 rename
    fs::rename(&part_path, &final_path).map_err(|e| {
        VoiceError::DownloadFailed(format!("rename failed: {}", e))
    })?;

    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_info_correct() {
        let info = default_model_info();
        assert_eq!(info.name, "ggml-tiny.bin");
        assert!(info.download_url.contains("huggingface.co"));
        assert!(info.size_hint_mb > 0);
    }

    #[test]
    fn check_model_present_nonexistent() {
        // 用一个肯定不存在的名字
        assert!(!check_model_present("definitely_nonexistent_model_xxx.bin"));
    }

    #[test]
    fn models_dir_ends_with_voicepilot_models() {
        let dir = models_dir();
        let s = dir.to_string_lossy();
        assert!(s.contains(".voicepilot"), "dir = {}", s);
        assert!(s.contains("models"), "dir = {}", s);
    }
}
