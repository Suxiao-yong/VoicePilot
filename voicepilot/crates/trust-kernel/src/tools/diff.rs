//! Diff Preview 工具(W6b-3a Task 1)。
//! 用 `similar` crate 计算 unified diff,Rust 端完成,文件内容不经过 IPC。
//!
//! 路径约定:`source_path` 和 `dest_path` 都是完整文件路径(非目录)。
//! files.organize 场景下 `manifest.destination` 是目录,前端调用时需拼接:
//! `dest_path = manifest.destination + "/" + basename(source_path)`。

use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use similar::TextDiff;

/// Diff 计算结果。
///
/// 注意:不加 `#[serde(rename_all = "camelCase")]` —— 与现有 `FileSnapshot`
/// 等类型保持一致(全 snake_case 序列化),前端 TS interface 也用 snake_case。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffResult {
    /// 源文件路径(canonical)
    pub source_path: String,
    /// 目标文件路径(canonical,可能不存在=新文件)
    pub dest_path: String,
    /// 文件类型(影响展示)
    pub file_kind: FileKind,
    /// unified diff 文本(可空,如二进制文件)
    pub diff_text: Option<String>,
    /// 截断提示(若文件 > 50MB 软上限)
    pub truncated: bool,
    /// 截断原因(若 truncated=true)
    pub truncate_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// 目标不存在,纯新增
    NewFile,
    /// 文本文件,有 diff
    Text,
    /// 二进制文件(前 8KB 含 NUL byte)
    Binary,
}

/// 软上限 50MB,防止 OOM。
const MAX_FILE_SIZE_BYTES: u64 = 50 * 1024 * 1024;

/// 计算单文件 diff。
///
/// - `source_path` 必须存在(读取内容)
/// - `dest_path` 可不存在(返回 FileKind::NewFile,diff 为全文件内容)
/// - 超过 50MB 返回 truncated=true,diff_text=None
/// - 二进制文件返回 FileKind::Binary,diff_text=None
pub fn compute_file_diff(
    source_path: &Path,
    dest_path: &Path,
) -> crate::error::Result<DiffResult> {
    let source_meta = fs::metadata(source_path)?;
    if !source_meta.is_file() {
        return Err(crate::error::KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("source is not a regular file: {}", source_path.display()),
        )));
    }
    let source_size = source_meta.len();

    // 路径规范化(纯字符串变换,不触碰文件系统)
    let source_canonical = crate::tools::fs_paths::canonicalize(&source_path.to_string_lossy());
    let dest_canonical = crate::tools::fs_paths::canonicalize(&dest_path.to_string_lossy());

    // 软上限检查
    if source_size > MAX_FILE_SIZE_BYTES {
        return Ok(DiffResult {
            source_path: source_canonical,
            dest_path: dest_canonical,
            file_kind: FileKind::Text,
            diff_text: None,
            truncated: true,
            truncate_reason: Some(format!(
                "文件超过 50MB 软上限({} bytes)",
                source_size
            )),
        });
    }

    let source_content = fs::read(source_path)?;

    // 二进制检测:前 8KB 含 NUL byte
    let is_binary = source_content.iter().take(8 * 1024).any(|&b| b == 0);

    if is_binary {
        return Ok(DiffResult {
            source_path: source_canonical,
            dest_path: dest_canonical,
            file_kind: FileKind::Binary,
            diff_text: None,
            truncated: false,
            truncate_reason: None,
        });
    }

    // dest 不存在 = 新文件
    if !dest_path.exists() {
        let source_text = String::from_utf8_lossy(&source_content);
        return Ok(DiffResult {
            source_path: source_canonical,
            dest_path: dest_canonical,
            file_kind: FileKind::NewFile,
            diff_text: Some(format_new_file_diff(&source_text)),
            truncated: false,
            truncate_reason: None,
        });
    }

    // dest 必须是常规文件
    let dest_meta = fs::metadata(dest_path)?;
    if !dest_meta.is_file() {
        return Err(crate::error::KernelError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("dest is not a regular file: {}", dest_path.display()),
        )));
    }

    // dest 软上限检查
    let dest_size = dest_meta.len();
    if dest_size > MAX_FILE_SIZE_BYTES {
        return Ok(DiffResult {
            source_path: source_canonical,
            dest_path: dest_canonical,
            file_kind: FileKind::Text,
            diff_text: None,
            truncated: true,
            truncate_reason: Some(format!(
                "目标文件超过 50MB 软上限({} bytes)",
                dest_size
            )),
        });
    }

    // 双向 diff
    let dest_content = fs::read(dest_path)?;
    let source_text = String::from_utf8_lossy(&source_content);
    let dest_text = String::from_utf8_lossy(&dest_content);

    let diff = TextDiff::from_lines(&source_text, &dest_text)
        .unified_diff()
        .context_radius(3)
        .to_string();

    Ok(DiffResult {
        source_path: source_canonical,
        dest_path: dest_canonical,
        file_kind: FileKind::Text,
        diff_text: Some(diff),
        truncated: false,
        truncate_reason: None,
    })
}

/// 新文件用 unified diff 格式呈现全内容(+ 前缀)。
fn format_new_file_diff(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("+{}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn compute_file_diff_new_file() {
        let src = write_temp(b"line1\nline2\nline3\n");
        let src_path = src.path();
        let dest_path = std::path::Path::new("/nonexistent/dest/path.txt");

        let result = compute_file_diff(src_path, dest_path).unwrap();

        assert_eq!(result.file_kind, FileKind::NewFile);
        assert!(result.diff_text.is_some());
        let diff = result.diff_text.unwrap();
        assert!(diff.contains("+line1"));
        assert!(diff.contains("+line2"));
        assert!(diff.contains("+line3"));
        assert!(!result.truncated);
    }

    #[test]
    fn compute_file_diff_text_diff() {
        let src = write_temp(b"line1\nline2 old\nline3\n");
        let dst = write_temp(b"line1\nline2 new\nline3\n");

        let result = compute_file_diff(src.path(), dst.path()).unwrap();

        assert_eq!(result.file_kind, FileKind::Text);
        let diff = result.diff_text.unwrap();
        assert!(diff.contains("-line2 old"));
        assert!(diff.contains("+line2 new"));
    }

    #[test]
    fn compute_file_diff_binary_detection() {
        let mut src = NamedTempFile::new().unwrap();
        src.write_all(b"binary\x00data\x00here").unwrap();
        src.flush().unwrap();
        let dst = write_temp(b"some text");

        let result = compute_file_diff(src.path(), dst.path()).unwrap();

        assert_eq!(result.file_kind, FileKind::Binary);
        assert!(result.diff_text.is_none());
        assert!(!result.truncated);
    }

    #[test]
    fn compute_file_diff_truncated_50mb() {
        // 创建一个伪 51MB 文件(用 sparse file 避免实际占盘)
        let src = NamedTempFile::new().unwrap();
        let src_path = src.path().to_path_buf();
        // 先写 1 byte,然后用 truncate 设大小为 51MB
        // 注意:sparse file 在 fs::read 时会实际读取 51MB(全 0),
        // 所以这里不 read,只触发 metadata().len() > 50MB 的分支
        let dst = write_temp(b"dest");
        src.as_file().set_len(51 * 1024 * 1024).unwrap();

        let result = compute_file_diff(&src_path, dst.path()).unwrap();

        assert!(result.truncated);
        assert!(result.diff_text.is_none());
        assert!(result.truncate_reason.unwrap().contains("50MB"));
    }
}
