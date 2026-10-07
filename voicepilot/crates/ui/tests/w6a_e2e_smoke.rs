#![cfg(feature = "tauri")]

use tempfile::TempDir;
use trust_kernel::approval::approver::AutoApprover;
use voicepilot_ui::commands::{OrganizeInput, organize_files};
use voicepilot_ui::state::AppState;

/// W6a §11.1 gate:端到端 organize_files 完整管道。
/// 使用 AutoApprover(绕过 Tauri 事件系统)验证 Rust 侧 Skill executor
/// + 审计链 + 补偿记录。TauriApprover 特定的事件发射在 approver_unit.rs 中覆盖。
#[test]
fn end_to_end_organize_files_with_auto_approver_full_pipeline() {
    use trust_kernel::repo::step_repo::StepStatus;

    // Setup:临时目录包含 2 个 .txt 文件 + 1 个 .log 文件 + 目标目录
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    std::fs::write(src.join("b.txt"), "beta").unwrap();
    std::fs::write(src.join("c.log"), "gamma").unwrap(); // 不匹配

    // AppState 用内存 kernel(TauriApprover 的 registry 在 AppState 上,
    // 但此处未使用,因为直接注入 AutoApprover)
    let state = AppState::new_in_memory().unwrap();

    let result = organize_files(
        &state,
        &AutoApprover,
        &OrganizeInput {
            task_id: "t-w6a-smoke".to_string(),
            step_id: "s-w6a-smoke".to_string(),
            source: src.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest.to_string_lossy().into_owned(),
        },
    )
    .unwrap();

    // 验证:2 个文件已移动(a.txt + b.txt),c.log 未触碰
    assert!(result.committed, "tool_result should be committed");
    assert_eq!(result.moved_paths.len(), 2);
    assert!(dest.join("a.txt").exists());
    assert!(dest.join("b.txt").exists());
    assert!(src.join("c.log").exists(), "non-matching file untouched");

    // 验证:审计链有预期事件。成功路径共 8 个 audit 事件:
    //   1. task_created              (create_task)
    //   2. step_created              (create_step)
    //   3. step_status_changed       (Running,executor step 1)
    //   4. step_prepared             (executor step 2)
    //   5. approval_recorded         (executor step 3)
    //   6. compensation_created      (executor step 6)
    //   7. step_committed            (executor step 7,update_step_post_commit)
    //   8. step_status_changed       (Succeeded,executor step 7)
    // 精确断言(N=8)以尽早捕获回归(参考 w4_e2e_smoke.rs:87 风格)。
    let audit_count = state.kernel.audit_count_for_task("t-w6a-smoke").unwrap();
    assert_eq!(
        audit_count, 8,
        "expected 8 audit events on success path, got {}",
        audit_count
    );

    // 验证:step 处于 Succeeded 状态(V1.1 §6.2 commit 完成后的终态,
    // StepStatus 枚举中无 Committed 变体,Succeeded 即表示已提交并验证通过)
    let step = state.kernel.get_step("s-w6a-smoke").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);

    // 验证:补偿已记录(强 + auto_reverse 就绪)。compensation_ref 形如
    // "comp-<uuid>",非空字符串。
    let comp_ref = result
        .compensation_ref
        .as_ref()
        .expect("compensation_ref must be set after successful commit");
    assert!(!comp_ref.is_empty(), "compensation_ref must not be empty");

    // 验证:evidence_strength(V1.1 §7.2 补偿强度语义)。
    // 成功路径下 verify_move 返回 Strong(fs.rs:271),as_str() = "strong"。
    assert_eq!(
        result.evidence_strength, "strong",
        "evidence_strength should be 'strong' for committed moves with auto_reverse"
    );
}

/// 测试拒绝会取消操作而不提交。
#[test]
fn end_to_end_deny_cancels_commit() {
    use trust_kernel::approval::approver::AutoDenier;
    use trust_kernel::repo::step_repo::StepStatus;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();

    let state = AppState::new_in_memory().unwrap();
    let result = organize_files(
        &state,
        &AutoDenier,
        &OrganizeInput {
            task_id: "t-w6a-deny".to_string(),
            step_id: "s-w6a-deny".to_string(),
            source: src.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest.to_string_lossy().into_owned(),
        },
    )
    .unwrap();

    // 拒绝 → Skill executor 返回 Ok 但 status=Cancelled(跳过 commit 阶段)。
    // V1.1 §6.2:deny 不是错误,而是一个合法的取消路径。
    assert!(
        !result.committed,
        "deny must not produce a committed tool_result"
    );
    assert!(
        result.moved_paths.is_empty(),
        "no files should be moved on deny"
    );
    assert!(
        result.compensation_ref.is_none(),
        "no compensation should be created on deny"
    );

    // 验证:源文件未被触碰
    assert!(
        src.join("a.txt").exists(),
        "source file must not be moved on deny"
    );
    assert!(
        !dest.join("a.txt").exists(),
        "dest must not contain the file on deny"
    );

    // 验证:step 状态为 Cancelled
    let step = state.kernel.get_step("s-w6a-deny").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Cancelled);
}
