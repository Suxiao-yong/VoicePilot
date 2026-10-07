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
use crate::voice::model::{
    SENSE_VOICE_ARCHIVE_SHA256, SENSE_VOICE_ARCHIVE_SIZE_BYTES, SENSE_VOICE_DIR_NAME,
    SENSE_VOICE_SIZE_MB, SENSE_VOICE_URL,
};
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
    pub archive_size_bytes: u64,
    pub expected_sha256: Option<String>,
    pub size_hint_mb: u32,
    /// Wave 3 Task 3.2:下载来源抽象(URL 或本地文件)。
    pub source: DownloadSource,
}

/// Wave 3 Task 3.2:下载来源抽象。
///
/// - `Url`:真实 HTTP(S) 下载(sherpa-onnx GitHub releases)。
/// - `File`:本地文件源(离线 / 测试,便于验证断点续传与原子安装)。
#[derive(Debug, Clone)]
pub enum DownloadSource {
    Url { url: String },
    File { path: PathBuf },
}

/// Wave 3 Task 3.2:模型就绪状态(UI ModelDownloadBar 消费)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelState {
    /// voice feature 未启用。
    Disabled,
    /// 模型目录不存在(未下载 / 已被移除)。
    Missing,
    /// 存在 `.part` 下载临时文件(下载中或可续传)。
    Downloading,
    /// 归档已下载,正在校验 / 解压。
    Verifying,
    /// 模型完整可用(`model.onnx` + `tokens.txt`)。
    Ready,
    /// 上次下载 / 校验失败(残留标记)。
    Failed,
}

/// 默认模型信息(sherpa-onnx SenseVoice,约 1 GB tar.bz2)。
pub fn default_model_info() -> ModelInfo {
    ModelInfo {
        name: SENSE_VOICE_DIR_NAME.to_string(),
        download_url: SENSE_VOICE_URL.to_string(),
        archive_size_bytes: SENSE_VOICE_ARCHIVE_SIZE_BYTES,
        expected_sha256: Some(SENSE_VOICE_ARCHIVE_SHA256.to_string()),
        size_hint_mb: SENSE_VOICE_SIZE_MB,
        source: DownloadSource::Url {
            url: SENSE_VOICE_URL.to_string(),
        },
    }
}

/// Wave 3 Task 3.2:根据 `<models_dir>/<name>` 的文件系统状态解析模型状态。
///
/// 顺序:完整目录 → Ready;存在 `.part` → Downloading;否则 Missing。
pub fn model_state(models_dir: &Path, name: &str) -> VoiceResult<ModelState> {
    let final_dir = models_dir.join(name);
    if model_dir_is_complete(&final_dir) {
        return Ok(ModelState::Ready);
    }
    let part_path = models_dir.join(format!("{name}.tar.bz2.part"));
    if part_path.exists() {
        return Ok(ModelState::Downloading);
    }
    Ok(ModelState::Missing)
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

/// 单一进程内下载锁:任意时刻只有一个模型下载在进行(Task 3.2)。
static MODEL_DOWNLOAD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 下载 sherpa-onnx 模型 tar.bz2 并解压到 `models_dir/<name>/`(原子安装)。
///
/// 流程:
/// 1. 若 `<models_dir>/<name>/` 已完整(`model.onnx` + `tokens.txt`),直接返回(幂等)
/// 2. 用 `download_to_part` 下载(已有 `.part` 时断点续传;网络失败保留 `.part`)
/// 3. (可选)SHA256 校验下载的 tar.bz2;失败清理 `.part`
/// 4. 解压到临时目录 `<models_dir>/.<name>.install-<pid>`(原子安装)
/// 5. 校验临时目录完整后,rename 到最终目录(正在使用的 ASR/TTS engine 不受影响)
/// 6. 删除 `.part` 临时文件
///
/// - 下载期间持有进程内单一下载锁。
/// - 错误信息包含下载路径与恢复操作(断点续传 / 检查磁盘空间),不只是底层异常。
pub fn download_model_from<F>(
    info: &ModelInfo,
    models_dir: PathBuf,
    on_progress: F,
) -> VoiceResult<PathBuf>
where
    F: Fn(DownloadProgress),
{
    // 单一下载锁(同一进程内只允许一个下载;锁是诊断性的,不用作跨进程锁)。
    let _lock = MODEL_DOWNLOAD_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    fs::create_dir_all(&models_dir).map_err(|e| {
        VoiceError::DownloadFailed(format!(
            "create models_dir failed: {} (path: {})",
            e,
            models_dir.display()
        ))
    })?;

    let final_dir = models_dir.join(&info.name);
    let part_path = models_dir.join(format!("{}.tar.bz2.part", info.name));

    // 幂等:若模型目录已完整(model.onnx + tokens.txt),直接返回
    if model_dir_is_complete(&final_dir) {
        return Ok(final_dir);
    }

    // 磁盘空间预检(尽力而为):URL 下载前检查目标卷剩余空间是否足够。
    if let DownloadSource::Url { .. } = &info.source {
        check_disk_space_for(&models_dir, info.archive_size_bytes);
    }

    // === 阶段 1:下载 tar.bz2 到 .part 文件(已有 .part 时断点续传) ===
    download_to_part(&info.source, &part_path, &on_progress)?;

    // === 阶段 2:(可选)SHA256 校验下载的 tar.bz2 ===
    if let Some(expected) = &info.expected_sha256 {
        let actual = sha256_of_file(&part_path)?;
        if actual != expected.to_ascii_lowercase() {
            let _ = fs::remove_file(&part_path);
            return Err(VoiceError::DownloadFailed(format!(
                "模型归档 SHA256 校验失败:期望 {expected},实际 {actual}。\
                 归档可能损坏;请删除 `{}` 后重新下载(会自动断点续传)。",
                part_path.display()
            )));
        }
    }

    // === 阶段 3:原子安装 —— 解压到临时目录,校验后 rename 到最终目录 ===
    let install_tmp = models_dir.join(format!(".{}.install-{}", info.name, std::process::id()));
    // 清理上次残留的临时安装目录。
    if install_tmp.exists() {
        let _ = fs::remove_dir_all(&install_tmp);
    }
    fs::create_dir_all(&install_tmp).map_err(|e| {
        VoiceError::DownloadFailed(format!(
            "create install temp dir failed: {} (path: {})",
            e,
            install_tmp.display()
        ))
    })?;
    extract_tar_bz2(&part_path, &install_tmp)?;

    // 解压结果校验。
    let extracted_dir = install_tmp.join(&info.name);
    if !model_dir_is_complete(&extracted_dir) {
        let _ = fs::remove_dir_all(&install_tmp);
        let _ = fs::remove_file(&part_path);
        return Err(VoiceError::DownloadFailed(format!(
            "模型解压不完整:解压后缺少 {}/model.onnx 或 tokens.txt。\
             归档可能损坏或与预期不符;请删除 `{}` 后重新下载。",
            extracted_dir.display(),
            part_path.display()
        )));
    }

    // 原子替换最终目录,消除"删除旧目录 → rename"之间的丢失窗口:
    //   1. final_dir → .<name>.old-<pid>(旧目录保留,直到新目录就位)
    //   2. extracted_dir → final_dir(此刻旧目录仍存在,崩溃可恢复)
    //   3. 删除 .old
    // 正在使用的 ASR/TTS engine 持有的旧句柄不受影响。
    let old_dir = models_dir.join(format!(".{}.old-{}", info.name, std::process::id()));
    if final_dir.exists() {
        if old_dir.exists() {
            let _ = fs::remove_dir_all(&old_dir);
        }
        fs::rename(&final_dir, &old_dir).map_err(|e| {
            VoiceError::DownloadFailed(format!(
                "备份旧模型目录失败: {} (从 {} 到 {})",
                e,
                final_dir.display(),
                old_dir.display()
            ))
        })?;
    }
    if let Err(e) = fs::rename(&extracted_dir, &final_dir) {
        // 回滚:把旧目录还原到最终位置。
        let _ = fs::rename(&old_dir, &final_dir);
        let _ = fs::remove_dir_all(&install_tmp);
        return Err(VoiceError::DownloadFailed(format!(
            "安装模型目录失败: {} (从 {} 到 {})",
            e,
            extracted_dir.display(),
            final_dir.display()
        )));
    }
    let _ = fs::remove_dir_all(&old_dir);
    let _ = fs::remove_dir_all(&install_tmp);

    // === 阶段 4:删除 .part tar.bz2 临时文件 ===
    let _ = fs::remove_file(&part_path);

    Ok(final_dir)
}

/// 下载 sherpa-onnx 模型到 canonical `~/.voicepilot/models/`(Task 3.2 入口)。
pub fn download_model<F>(info: &ModelInfo, on_progress: F) -> VoiceResult<PathBuf>
where
    F: Fn(DownloadProgress),
{
    download_model_from(info, models_dir(), on_progress)
}

/// 已有 `.part` 文件的字节数(不存在返回 0)。
fn existing_part_len(part_path: &Path) -> VoiceResult<u64> {
    match fs::metadata(part_path) {
        Ok(meta) if meta.is_file() => Ok(meta.len()),
        Ok(_) => Ok(0),
        Err(_) => Ok(0),
    }
}

/// 截断 `.part` 文件(服务器忽略 Range 或范围失效时安全重启动)。
fn truncate_part(part_path: &Path) -> VoiceResult<()> {
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(part_path)
        .map(|_| ())
        .map_err(|e| VoiceError::DownloadFailed(format!("truncate .part failed: {}", e)))
}

/// 下载 tar.bz2 到 `.part` 文件,带进度回调 + 断点续传(Task 3.2)。
///
/// - 已有 `.part` 时:
///   - `Url` 源:发送 `Range: bytes=<len>-`;服务器返回 206 则追加,200 则
///     安全重启(截断重下),416(范围失效)则截断重启。
///   - `File` 源:从已有偏移继续复制本地文件。
/// - 网络 / 读取失败:返回 Err 但**保留 `.part`**(下次可续传)。
/// - 用户取消(`on_progress` 由调用方控制):保持可续传状态。
fn download_to_part<F>(
    source: &DownloadSource,
    part_path: &Path,
    on_progress: &F,
) -> VoiceResult<()>
where
    F: Fn(DownloadProgress),
{
    match source {
        DownloadSource::File { path } => download_part_from_file(path, part_path, on_progress),
        DownloadSource::Url { url } => download_part_from_url(url, part_path, on_progress),
    }
}

/// 从本地文件源复制到 `.part`(支持偏移续传)。
fn download_part_from_file<F>(src_path: &Path, part_path: &Path, on_progress: &F) -> VoiceResult<()>
where
    F: Fn(DownloadProgress),
{
    let src_meta = fs::metadata(src_path).map_err(|e| {
        VoiceError::DownloadFailed(format!(
            "模型源文件缺失:{} (error: {})。请检查下载路径 `{}` 或重新发起下载(可断点续传)。",
            src_path.display(),
            e,
            part_path.display()
        ))
    })?;
    let src_len = src_meta.len();
    let existing = existing_part_len(part_path)?;
    if existing > src_len {
        // 旧 `.part` 比源文件还长(源被替换过):截断重下,避免污染归档。
        truncate_part(part_path)?;
    }
    let existing = existing_part_len(part_path)?;
    if existing >= src_len {
        // 本地源已完整落到 .part。
        on_progress(DownloadProgress {
            downloaded_bytes: src_len,
            total_bytes: Some(src_len),
            percent: Some(100.0),
        });
        return Ok(());
    }

    let mut src_file = fs::File::open(src_path)
        .map_err(|e| VoiceError::DownloadFailed(format!("打开模型源文件失败:{}", e)))?;
    use std::io::Seek;
    src_file
        .seek(std::io::SeekFrom::Start(existing))
        .map_err(|e| VoiceError::DownloadFailed(format!("seek 源文件失败:{}", e)))?;

    let mut part_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .map_err(|e| VoiceError::DownloadFailed(format!("打开 .part 失败:{}", e)))?;

    let mut buf = [0u8; 32 * 1024];
    let mut downloaded = existing;
    let mut last_progress = Instant::now();
    let progress_throttle = Duration::from_millis(100);
    loop {
        let n = src_file
            .read(&mut buf)
            .map_err(|e| VoiceError::DownloadFailed(format!("读取模型源文件失败:{}", e)))?;
        if n == 0 {
            break;
        }
        part_file
            .write_all(&buf[..n])
            .map_err(|e| VoiceError::DownloadFailed(format!("写入 .part 失败:{}", e)))?;
        downloaded += n as u64;
        if last_progress.elapsed() >= progress_throttle {
            let percent = Some(downloaded as f32 / src_len as f32 * 100.0);
            on_progress(DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes: Some(src_len),
                percent,
            });
            last_progress = Instant::now();
        }
    }
    let _ = part_file.sync_all();
    on_progress(DownloadProgress {
        downloaded_bytes: downloaded,
        total_bytes: Some(src_len),
        percent: Some(100.0),
    });
    Ok(())
}

/// 从 URL 下载到 `.part`(Range 断点续传)。
fn download_part_from_url<F>(url: &str, part_path: &Path, on_progress: &F) -> VoiceResult<()>
where
    F: Fn(DownloadProgress),
{
    let agent = ureq::AgentBuilder::new()
        .timeout_read(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600))
        .build();

    let existing = existing_part_len(part_path)?;

    // 发起请求:已有部分时带 Range,否则从头。
    // 注意:ureq 2.x 把 4xx 视为 Err,因此 416(范围失效)在请求层处理。
    let resp = if existing > 0 {
        agent
            .get(url)
            .set("Range", &format!("bytes={existing}-"))
            .call()
    } else {
        agent.get(url).call()
    };
    let resp = match resp {
        Ok(resp) => resp,
        Err(ureq::Error::Status(416, _)) => {
            // 范围失效(offset 超出文件尾):安全重启 —— 截断 `.part`,下次
            // 调用从头下载。
            truncate_part(part_path)?;
            return Err(VoiceError::DownloadFailed(format!(
                "下载范围失效(HTTP 416);已重置下载进度,请重新下载。`.part` 已保留:{}",
                part_path.display()
            )));
        }
        Err(e) => {
            return Err(VoiceError::DownloadFailed(format!(
                "HTTP 下载失败:{}(URL: {})。请检查网络后重试;`.part` 已保留可断点续传:{}",
                e,
                url,
                part_path.display()
            )));
        }
    };

    // 处理 Range 语义。
    let status = resp.status();
    let append = match status {
        206 => true,
        200 => {
            // 服务器忽略 Range:安全重启(截断从头)。
            if existing > 0 {
                truncate_part(part_path)?;
            }
            false
        }
        other => {
            return Err(VoiceError::DownloadFailed(format!(
                "服务器返回 HTTP {};下载失败。请检查 URL:{} 后重试;`.part` 已保留可续传:{}",
                other,
                url,
                part_path.display()
            )));
        }
    };

    // 总字节数:206 时 = 偏移 + 本次 Content-Length;200 时 = Content-Length。
    let content_length = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let total_bytes = if append {
        existing.saturating_add(content_length)
    } else {
        content_length
    };

    let mut part_file = if append {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(part_path)
            .map_err(|e| VoiceError::DownloadFailed(format!("open .part append failed: {}", e)))?
    } else {
        fs::File::create(part_path)
            .map_err(|e| VoiceError::DownloadFailed(format!("create .part failed: {}", e)))?
    };

    let mut buf = [0u8; 32 * 1024]; // 32KB buffer
    // 206(续传)从已有偏移累计;200(服务器忽略 Range,已截断重下)从 0 计。
    let mut downloaded = if append { existing } else { 0 };
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
        downloaded += n as u64;

        // 节流:每 100ms 报告一次进度
        if last_progress.elapsed() >= progress_throttle {
            let percent = if total_bytes > 0 {
                Some(downloaded as f32 / total_bytes as f32 * 100.0)
            } else {
                None
            };
            on_progress(DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes: if total_bytes > 0 {
                    Some(total_bytes)
                } else {
                    None
                },
                percent,
            });
            last_progress = Instant::now();
        }
    }

    // 最终进度报告
    let percent = if total_bytes > 0 {
        Some(downloaded as f32 / total_bytes as f32 * 100.0)
    } else {
        None
    };
    on_progress(DownloadProgress {
        downloaded_bytes: downloaded,
        total_bytes: if total_bytes > 0 {
            Some(total_bytes)
        } else {
            None
        },
        percent,
    });

    // flush + sync
    part_file
        .sync_all()
        .map_err(|e| VoiceError::DownloadFailed(format!("sync failed: {}", e)))?;

    Ok(())
}

/// 磁盘空间预检(Windows):目标卷剩余空间不足以放下归档时提前给出友好错误。
///
/// 尽力而为:查询失败时静默跳过(真正的空间不足会在写入时报 OS 错误)。
#[cfg(windows)]
fn check_disk_space_for(path: &Path, needed_bytes: u64) {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct UnsignedLargeInteger {
        low_part: u32,
        high_part: u32,
    }

    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut UnsignedLargeInteger,
            lp_total_number_of_free_bytes: *mut UnsignedLargeInteger,
            lp_total_number_of_bytes: *mut UnsignedLargeInteger,
        ) -> i32;
    }

    let dir = if path.exists() {
        path.to_path_buf()
    } else {
        std::path::Path::new(".").to_path_buf()
    };
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_avail = UnsignedLargeInteger {
        low_part: 0,
        high_part: 0,
    };
    let rc = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_avail,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc == 0 {
        return; // 查询失败:尽力而为,跳过预检
    }
    let free_bytes = ((free_avail.high_part as u64) << 32) | free_avail.low_part as u64;
    if free_bytes < needed_bytes {
        tracing::warn!(
            free_bytes,
            needed_bytes,
            "模型下载磁盘空间不足,但继续尝试(下载会在写入失败时报错)"
        );
    }
}

/// 磁盘空间预检(非 Windows):no-op。
#[cfg(not(windows))]
fn check_disk_space_for(_path: &Path, _needed_bytes: u64) {}

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
        VoiceError::DownloadFailed(format!(
            "open archive failed: {}: {}",
            archive_path.display(),
            e
        ))
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
        assert_eq!(info.archive_size_bytes, SENSE_VOICE_ARCHIVE_SIZE_BYTES);
        assert_eq!(info.size_hint_mb, 1048);
        assert_eq!(
            info.expected_sha256.as_deref(),
            Some(SENSE_VOICE_ARCHIVE_SHA256)
        );
    }

    #[test]
    fn check_model_present_nonexistent() {
        // 用一个肯定不存在的名字
        assert!(!check_model_present("definitely_nonexistent_model_dir_xxx"));
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
                .set_path(format!("{}/model.onnx", model_name))
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
                .set_path(format!("{}/tokens.txt", model_name))
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
        assert!(
            model_dir.is_dir(),
            "model dir should exist after extraction"
        );
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
        assert!(
            result.is_err(),
            "extracting nonexistent archive should fail"
        );

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
