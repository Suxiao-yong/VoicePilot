//! Diff Preview Tauri commands(W6b-3a Task 3)。
//!
//! 桥接前端 `compute_diff_command` invoke 到 `trust_kernel::tools::diff::compute_file_diff`。
//! 路径必须通过 `FilesystemTool::assert_path_allowed` 白名单校验。

use std::path::PathBuf;
use tauri::State;
use trust_kernel::tools::diff::{compute_file_diff, DiffResult};

use crate::error::UiError;
use crate::state::AppState;

/// 计算单文件 diff。
///
/// 前端在 ApprovalModal 中点击"Diff"按钮调用。
/// 路径必须通过 allowed_paths 白名单(由 FilesystemTool 强制)。
#[tauri::command]
pub fn compute_diff_command(
    state: State<'_, AppState>,
    source_path: String,
    dest_path: String,
) -> Result<DiffResult, String> {
    compute_diff_impl(&state, &source_path, &dest_path).map_err(Into::into)
}

pub fn compute_diff_impl(
    state: &AppState,
    source_path: &str,
    dest_path: &str,
) -> Result<DiffResult, UiError> {
    let src = PathBuf::from(source_path);
    let dst = PathBuf::from(dest_path);

    // 通过 FilesystemTool 走 allowed_paths 检查(沿用 §6.1 安全模型)
    // assert_path_allowed 返 KernelError,通过 #[from] 自动转 UiError::Kernel
    state
        .kernel
        .filesystem()
        .assert_path_allowed(&src)?;
    state
        .kernel
        .filesystem()
        .assert_path_allowed(&dst)?;

    let result = compute_file_diff(&src, &dst)?;
    Ok(result)
}
