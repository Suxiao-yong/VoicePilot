//! 模型 auto-download + 解压(W6b-3b Task 7)。
//!
//! 用 `ureq` HTTPS 下载 sherpa-onnx SenseVoice 模型 tar.bz2 到 `~/.voicepilot/models/`,
//! 然后用 `bzip2` + `tar` 解压:
//! - 100ms 节流进度回调(下载阶段)
//! - .part 临时文件下载,完成后解压到目标目录
//! - SHA256 校验(若 expected_sha256 提供,对下载的 tar.bz2 校验)
//! - 解压后校验 `model.onnx` + `tokens.txt` 存在
//! - 解压成功后删除 .part tar.bz2 临时文件
//!
//! 整个模块在 `#[cfg(feature = "voice")]` 下(由 voice/mod.rs 控制)。

use crate::voice::error::{VoiceError, VoiceResult};
use crate::voice::model::{SENSE_VOICE_DIR_NAME, SENSE_VOICE_SIZE_MB, SENSE_VOICE_URL};
use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 模型信息(下载用)。
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// 模型目录名(解压后顶层目录名,即 `~/.voicepilot/models/<name>/`)。
    pub name: String,
    pub download_url: String,
    pub expected_sha256: Option<String>,
    pub size_hint_mb: u32,
}

/// 默认模型信息(sherpa-onnx SenseVoice,~234MB tar.bz2)。
pub fn default_model_info() -> ModelInfo {
    ModelInfo {
        name: SENSE_VOICE_DIR_NAME.to_string(),
        download_url: SENSE_VOICE_URL.to_string(),
        // GitHub releases 不提供官方 SHA256,这里用 None —— 解压后由
        // `SherpaAsrEngine::new` 加载 model.onnx 时验证(失败会返回 ModelLoadFailed)
        expected_sha256: None,
        size_hint_mb: SENSE_VOICE_SIZE_MB,
    }
}

/// 返回模型存储目录:`~/.voicepilot/models/`。
///
/// 用 `dirs` crate 跨平台获取 home 目录。
pub fn models_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".voicepilot").join("models")
}

/// 检查指定模型(目录形式)是否已存在。
///
/// sherpa-onnx 模型是目录而非单文件,故用 `is_dir()`。
pub fn check_model_present(name: &str) -> bool {
    models_dir().join(name).is_dir()
}

/// 检查指定模型目录是否包含 `model.onnx` + `tokens.txt`(sherpa-onnx 加载所需)。
fn model_dir_is_complete(model_dir: &Path) -> bool {
    model_dir.is_dir()
        && model_dir.join("model.onnx").is_file()
        && model_dir.join("tokens.txt").is_file()
}

/// 下载进度回调参数。
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
}

/// 下载 sherpa-onnx 模型 tar.bz2 并解压到 `~/.voicepilot/models/<name>/`。
///
/// 流程:
/// 1. 若 `<models_dir>/<name>/` 已存在且包含 `model.onnx` + `tokens.txt`,直接返回(幂等)
/// 2. 用 `ureq` 下载 tar.bz2 到 `<models_dir>/<name>.tar.bz2.part`
/// 3. (可选)SHA256 校验下载的 tar.bz2
/// 4. 用 `bzip2` + `tar` 解压到 `<models_dir>/`
/// 5. 校验 `<models_dir>/<name>/model.onnx` + `tokens.txt` 存在
/// 6. 删除 .part tar.bz2 临时文件
/// 7. 返回 `<models_dir>/<name>/` 路径
///
/// - `on_progress`:每 100ms 调用一次(节流),传入下载进度
/// - 失败时清理 .part 文件
pub fn download_model<F>(info: &ModelInfo, on_progress: F) -> VoiceResult<PathBuf>
where
    F: Fn(DownloadProgress),
{
    let dir = models_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| VoiceError::DownloadFailed(format!("create models_dir failed: {}", e)))?;

    let final_dir = dir.join(&info.name);

    // 幂等:若模型目录已完整(model.onnx + tokens.txt),直接返回
    if model_dir_is_complete(&final_dir) {
        return Ok(final_dir);
    }

    // tar.bz2 临时文件路径(.part 后缀,避免与最终模型目录混淆)
    let archive_name = format!("{}.tar.bz2", info.name);
    let part_path = dir.join(format!("{}.part", archive_name));

    // 清理可能残留的 .part 文件
    if part_path.exists() {
        let _ = fs::remove_file(&part_path);
    }

    // === 阶段 1:下载 tar.bz2 到 .part 文件 ===
    download_to_part(&info.download_url, &part_path, &on_progress)?;

    // === 阶段 2:(可选)SHA256 校验下载的 tar.bz2 ===
    if let Some(expected) = &info.expected_sha256 {
        let actual = sha256_of_file(&part_path)?;
        if actual != expected.to_ascii_lowercase() {
            let _ = fs::remove_file(&part_path);
            return Err(VoiceError::DownloadFailed(format!(
                "SHA256 mismatch: expected={} actual={}",
                expected, actual
            )));
        }
    }

    // === 阶段 3:解压 tar.bz2 到 models_dir ===
    // 若目标目录已存在但不完整(残留),先清理避免解压冲突
    if final_dir.exists() {
        let _ = fs::remove_dir_all(&final_dir);
    }
    if let Err(e) = extract_tar_bz2(&part_path, &dir) {
        // 解压失败:立即清理 .part + 残留目录,避免占用磁盘 + 半解压状态
        let _ = fs::remove_dir_all(&final_dir);
        let _ = fs::remove_file(&part_path);
        return Err(e);
    }

    // === 阶段 4:校验解压结果 ===
    if !model_dir_is_complete(&final_dir) {
        // 解压失败:清理残留
        let _ = fs::remove_dir_all(&final_dir);
        let _ = fs::remove_file(&part_path);
        return Err(VoiceError::DownloadFailed(format!(
            "extraction incomplete: expected {}/model.onnx + tokens.txt after extracting {}",
            final_dir.display(),
            part_path.file_name().and_then(|s| s.to_str()).unwrap_or("?")
        )));
    }

    // === 阶段 5:删除 .part tar.bz2 临时文件 ===
    let _ = fs::remove_file(&part_path);

    Ok(final_dir)
}

/// 用 `ureq` 下载 URL 到 .part 文件,带进度回调 + SHA256 增量计算。
fn download_to_part<F>(url: &str, part_path: &Path, on_progress: &F) -> VoiceResult<()>
where
    F: Fn(DownloadProgress),
{
    let agent = ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600))
        .build();

    let resp = agent
        .get(url)
        .call()
        .map_err(|e| VoiceError::DownloadFailed(format!("HTTP request failed: {}", e)))?;

    let total_bytes = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    let mut part_file = fs::File::create(part_path)
        .map_err(|e| VoiceError::DownloadFailed(format!("create .part file failed: {}", e)))?;

    let mut hasher = Sha256::new();
    let mut buf = [0u8; 32 * 1024]; // 32KB buffer
    let mut downloaded: u64 = 0;
    let mut last_progress = Instant::now();
    let progress_throttle = Duration::from_millis(100);

    let mut reader = resp.into_reader();
    loop {
        let n = reader
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

    // 丢弃 hasher 结果(此处不校验,由调用方按需校验);保留计算是为将来扩展
    let _ = hasher.finalize();
    Ok(())
}

/// 计算文件 SHA256(小写十六进制)。
fn sha256_of_file(path: &Path) -> VoiceResult<String> {
    let mut f = fs::File::open(path)
        .map_err(|e| VoiceError::DownloadFailed(format!("open for sha256 failed: {}", e)))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| VoiceError::DownloadFailed(format!("read for sha256 failed: {}", e)))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 解压 tar.bz2 到目标目录。
///
/// `archive_path`:tar.bz2 文件路径
/// `dest_dir`:解压目标目录(会保留 tar 内的顶层目录结构)
pub fn extract_tar_bz2(archive_path: &Path, dest_dir: &Path) -> VoiceResult<()> {
    let archive_file = fs::File::open(archive_path).map_err(|e| {
        VoiceError::DownloadFailed(format!("open archive failed: {}: {}", archive_path.display(), e))
    })?;
    let bz_decoder = BzDecoder::new(archive_file);
    let mut tar_archive = tar::Archive::new(bz_decoder);
    tar_archive.unpack(dest_dir).map_err(|e| {
        VoiceError::DownloadFailed(format!(
            "extract tar.bz2 failed: {}: {}",
            archive_path.display(),
            e
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bzip2::write::BzEncoder;
    use std::io::Cursor;

    #[test]
    fn default_model_info_correct() {
        let info = default_model_info();
        assert_eq!(info.name, SENSE_VOICE_DIR_NAME);
        assert!(info.download_url.contains("github.com"));
        assert!(info.download_url.contains("sherpa-onnx-sense-voice"));
        assert!(info.download_url.ends_with(".tar.bz2"));
        assert!(info.size_hint_mb > 0);
        assert!(info.expected_sha256.is_none());
    }

    #[test]
    fn check_model_present_nonexistent() {
        // 用一个肯定不存在的名字
        assert!(!check_model_present(
            "definitely_nonexistent_model_dir_xxx"
        ));
    }

    #[test]
    fn models_dir_ends_with_voicepilot_models() {
        let dir = models_dir();
        let s = dir.to_string_lossy();
        assert!(s.contains(".voicepilot"), "dir = {}", s);
        assert!(s.contains("models"), "dir = {}", s);
    }

    /// 构造一个内存中的 tar.bz2,内容为 `<model_name>/model.onnx` + `<model_name>/tokens.txt`。
    fn build_synthetic_tar_bz2(model_name: &str) -> Vec<u8> {
        // 1. 在内存里构建 tar(顶层目录 + model.onnx + tokens.txt)
        let mut tar_buf: Vec<u8> = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_buf);

            // 添加目录条目
            let mut dir_header = tar::Header::new_gnu();
            dir_header.set_path(model_name).unwrap();
            dir_header.set_size(0);
            dir_header.set_mode(0o755);
            dir_header.set_entry_type(tar::EntryType::Directory);
            dir_header.set_cksum();
            builder.append(&dir_header, std::io::empty()).unwrap();

            // 添加 model.onnx
            let model_bytes = b"FAKE_ONNX_MODEL_BYTES";
            let mut file_header = tar::Header::new_gnu();
            file_header
                .set_path(&format!("{}/model.onnx", model_name))
                .unwrap();
            file_header.set_size(model_bytes.len() as u64);
            file_header.set_mode(0o644);
            file_header.set_entry_type(tar::EntryType::Regular);
            file_header.set_cksum();
            builder
                .append(&file_header, Cursor::new(model_bytes))
                .unwrap();

            // 添加 tokens.txt
            let tokens_bytes = b"<silence>\n<unk>\nhello\nworld\n";
            let mut tokens_header = tar::Header::new_gnu();
            tokens_header
                .set_path(&format!("{}/tokens.txt", model_name))
                .unwrap();
            tokens_header.set_size(tokens_bytes.len() as u64);
            tokens_header.set_mode(0o644);
            tokens_header.set_entry_type(tar::EntryType::Regular);
            tokens_header.set_cksum();
            builder
                .append(&tokens_header, Cursor::new(tokens_bytes))
                .unwrap();

            builder.finish().unwrap();
        }

        // 2. bz2 压缩 tar
        let mut encoder = BzEncoder::new(Vec::new(), bzip2::Compression::default());
        encoder.write_all(&tar_buf).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn extract_tar_bz2_unpacks_model_dir_with_onnx_and_tokens() {
        let tmp = std::env::temp_dir().join("vp-w6b3b-extract-test");
        std::fs::remove_dir_all(&tmp).ok();
        std::fs::create_dir_all(&tmp).unwrap();

        let model_name = "fake-sense-voice-model";
        let bz2_bytes = build_synthetic_tar_bz2(model_name);
        let archive_path = tmp.join("fake.tar.bz2");
        std::fs::write(&archive_path, &bz2_bytes).unwrap();

        // 解压到 tmp/
        extract_tar_bz2(&archive_path, &tmp).expect("extract should succeed");

        // 校验解压结果
        let model_dir = tmp.join(model_name);
        assert!(model_dir.is_dir(), "model dir should exist after extraction");
        assert!(
            model_dir.join("model.onnx").is_file(),
            "model.onnx should exist"
        );
        assert!(
            model_dir.join("tokens.txt").is_file(),
            "tokens.txt should exist"
        );

        // 校验内容
        let onnx = std::fs::read(model_dir.join("model.onnx")).unwrap();
        assert_eq!(onnx, b"FAKE_ONNX_MODEL_BYTES");
        let tokens = std::fs::read_to_string(model_dir.join("tokens.txt")).unwrap();
        assert!(tokens.contains("hello"));
        assert!(tokens.contains("world"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn extract_tar_bz2_fails_for_nonexistent_archive() {
        let tmp = std::env::temp_dir().join("vp-w6b3b-extract-fail-test");
        std::fs::remove_dir_all(&tmp).ok();
        std::fs::create_dir_all(&tmp).unwrap();

        let result = extract_tar_bz2(&tmp.join("nonexistent.tar.bz2"), &tmp);
        assert!(result.is_err(), "extracting nonexistent archive should fail");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn model_dir_is_complete_checks_onnx_and_tokens() {
        let tmp = std::env::temp_dir().join("vp-w6b3b-complete-test");
        std::fs::remove_dir_all(&tmp).ok();
        std::fs::create_dir_all(&tmp).unwrap();

        // 空目录 → 不完整
        assert!(!model_dir_is_complete(&tmp));

        // 只有 model.onnx → 不完整
        std::fs::write(tmp.join("model.onnx"), b"fake").unwrap();
        assert!(!model_dir_is_complete(&tmp));

        // 加上 tokens.txt → 完整
        std::fs::write(tmp.join("tokens.txt"), b"tokens").unwrap();
        assert!(model_dir_is_complete(&tmp));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
