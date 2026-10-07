#![cfg(feature = "tauri")]

//! W6b-3a Task 3:compute_diff_command 集成测试。
//!
//! 验证 allowed_paths 白名单生效:
//! - 在白名单内的路径:Ok(DiffResult)
//! - 不在白名单的路径:Err

use tempfile::NamedTempFile;
use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::kernel::TrustKernel;
use voicepilot_ui::state::AppState;

#[test]
fn compute_diff_command_allowed_path() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 不设 allowed_paths —— 开放访问
    let state = AppState::new(kernel);

    let src = NamedTempFile::new().unwrap();
    std::fs::write(src.path(), b"line1\nline2\n").unwrap();
    let dst = NamedTempFile::new().unwrap();
    std::fs::write(dst.path(), b"line1\nline2 modified\n").unwrap();

    // 直接调用 impl 函数(避免 Tauri State 包装)
    let result = voicepilot_ui::diff_commands::compute_diff_impl(
        &state,
        &src.path().to_string_lossy(),
        &dst.path().to_string_lossy(),
    );
    assert!(result.is_ok(), "err = {:?}", result.err());
    let diff = result.unwrap();
    assert_eq!(diff.file_kind, trust_kernel::tools::diff::FileKind::Text);
    assert!(diff.diff_text.unwrap().contains("+line2 modified"));
}

#[test]
fn compute_diff_command_blocked_path() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    // 设 allowed_paths 为 C:/safe_area,使 E:/ 路径被拒
    let allowed = AllowedPaths::new(vec!["C:/safe_area".to_string()]);
    kernel.replace_filesystem_with_allowed_paths(allowed);
    let state = AppState::new(kernel);

    let result = voicepilot_ui::diff_commands::compute_diff_impl(
        &state,
        "E:/definitely_nonexistent/source.txt",
        "E:/definitely_nonexistent/dest.txt",
    );
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("path") || err_msg.contains("not allowed"),
        "err = {}",
        err_msg
    );
}
