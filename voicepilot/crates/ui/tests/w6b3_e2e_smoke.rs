#![cfg(feature = "tauri")]

//! W6b-3a Task 9: E2E 全链路冒烟测试。
//!
//! 验证完整链路(无 voice):
//! - route_text("整理下载目录的图片") → Routed { skill_id: "files.organize" }
//! - organize_files(AutoApprover) → commit_move 成功 + 审计链
//! - submit_approval(allow) 在无 pending request 时返回 false
//! - compute_diff_impl(白名单内路径) → DiffResult
//! - is_voice_enabled_command / check_model_command 在非 voice build 下的行为
//!
//! 由于 voice feature 在 CI 环境不稳定(whisper-rs bindgen issue #49),
//! 本测试**不**依赖 voice。voice 链路在 w6b1_voice_smoke.rs 单独覆盖。

use tempfile::tempdir;
use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::kernel::TrustKernel;
use voicepilot_ui::commands::{
    OrganizeInput, RouteTextResult, organize_files, route_text, submit_approval,
};
use voicepilot_ui::state::AppState;

/// route_text 通过关键词匹配路由到 files.organize。
/// "整理下载目录的图片" 同时命中 keywords "整理" 和 "下载目录"。
///
/// W7: route_text 改为 async fn,测试用 `#[tokio::test]` + `.await`。
#[tokio::test]
async fn e2e_route_text_matches_files_organize() {
    let state = AppState::new_in_memory().unwrap();
    let result = route_text(&state, "整理下载目录的图片").await.unwrap();
    match result {
        RouteTextResult::Routed { skill_id, .. } => {
            assert_eq!(skill_id, "files.organize");
        }
        other => panic!("expected Routed, got {:?}", other),
    }
}

/// 完整 organize_files 链路:用 AutoApprover 跳过 IPC。
/// 验证 commit 成功 + 文件实际移动 + audit chain 包含关键事件。
#[test]
fn e2e_organize_files_with_auto_approver() {
    let src_dir = tempdir().unwrap();
    let dst_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("test.txt");
    std::fs::write(&src_file, b"hello e2e").unwrap();

    let kernel = TrustKernel::open_in_memory().unwrap();
    // 注入 allowed_paths 白名单(src + dst),模拟生产 FilesystemTool 配置。
    let allowed_roots = vec![
        src_dir.path().to_string_lossy().to_string(),
        dst_dir.path().to_string_lossy().to_string(),
    ];
    let allowed = AllowedPaths::new(allowed_roots);
    kernel.replace_filesystem_with_allowed_paths(allowed);
    let state = AppState::new(kernel);

    let input = OrganizeInput {
        task_id: "e2e-task-1".to_string(),
        step_id: "e2e-step-1".to_string(),
        source: src_dir.path().to_string_lossy().to_string(),
        filter: "*.txt".to_string(),
        destination: dst_dir.path().to_string_lossy().to_string(),
    };

    let result = organize_files(&state, &AutoApprover, &input).unwrap();
    assert!(
        result.committed,
        "expected committed=true, err={:?}",
        result.error
    );
    assert_eq!(result.moved_paths.len(), 1);

    let moved_dst = dst_dir.path().join("test.txt");
    assert!(moved_dst.is_file(), "dest file should exist after commit");
    let content = std::fs::read_to_string(&moved_dst).unwrap();
    assert_eq!(content, "hello e2e");

    // 负面断言:src 文件应已移动走
    assert!(
        !src_file.exists(),
        "src file should be moved away after organize"
    );

    // 审计链验证:list_audit_recent 返回全局最近 N 条(此处 fresh DB 仅含本任务事件)。
    // 成功路径 8 个事件(参考 w6a_e2e_smoke.rs:52-60):
    //   task_created / step_created / step_status_changed(Running) /
    //   step_prepared / approval_recorded / compensation_created /
    //   step_committed / step_status_changed(Succeeded)
    let audit = state.kernel.list_audit_recent(50).unwrap();
    assert!(!audit.is_empty(), "audit should have events");

    // 精确断言关键事件类型存在(参考 kernel.rs: step_prepared / step_committed)。
    let event_types: Vec<String> = audit.iter().map(|e| e.event_type.clone()).collect();
    assert!(
        event_types.iter().any(|t| t == "step_prepared"),
        "audit should contain step_prepared event, got {:?}",
        event_types
    );
    assert!(
        event_types.iter().any(|t| t == "step_committed"),
        "audit should contain step_committed event, got {:?}",
        event_types
    );
}

/// submit_approval 在无 pending request 时返回 false(一次性语义,§8.2)。
#[test]
fn e2e_submit_approval_no_pending_returns_false() {
    let state = AppState::new_in_memory().unwrap();
    let result = submit_approval(&state, "nonexistent-id", ApprovalDecision::Allow).unwrap();
    assert!(!result, "expected false for nonexistent approval_id");
}

/// compute_diff_impl 集成验证(白名单内路径)。
/// 路径必须在 allowed_paths 内,否则 assert_path_allowed 拒绝。
#[test]
fn e2e_compute_diff_command_allowed() {
    use voicepilot_ui::diff_commands::compute_diff_impl;

    let src_dir = tempdir().unwrap();
    let src_file = src_dir.path().join("src.txt");
    std::fs::write(&src_file, b"line1\nline2\n").unwrap();
    let dst_file = src_dir.path().join("dst.txt");
    std::fs::write(&dst_file, b"line1\nline2 modified\n").unwrap();

    let kernel = TrustKernel::open_in_memory().unwrap();
    let allowed = AllowedPaths::new(vec![src_dir.path().to_string_lossy().to_string()]);
    kernel.replace_filesystem_with_allowed_paths(allowed);
    let state = AppState::new(kernel);

    let result = compute_diff_impl(
        &state,
        &src_file.to_string_lossy(),
        &dst_file.to_string_lossy(),
    );
    assert!(result.is_ok(), "err = {:?}", result.err());
    let diff = result.unwrap();
    let diff_text = diff
        .diff_text
        .expect("diff_text should be Some for text files");
    assert!(
        diff_text.contains("+line2 modified"),
        "diff_text should contain '+line2 modified', got: {}",
        diff_text
    );
}

/// is_voice_enabled_command 在非 voice build 下返回 false。
#[cfg(not(feature = "voice"))]
#[test]
fn e2e_is_voice_enabled_returns_false_without_feature() {
    let result = voicepilot_ui::model_download_commands::is_voice_enabled_command();
    assert!(!result, "voice should be disabled without --features voice");
}

/// check_model_command 在非 voice build 下返回 Disabled。
#[cfg(not(feature = "voice"))]
#[test]
fn e2e_check_model_returns_disabled_without_feature() {
    use voicepilot_ui::model_download_commands::{ModelStatus, check_model_command};
    let status = check_model_command();
    assert!(
        matches!(status, ModelStatus::Disabled),
        "expected Disabled without voice feature, got {:?}",
        status
    );
}
