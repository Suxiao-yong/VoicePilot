//! W7 Plan 2 Task 7 — end-to-end smoke tests for the 3 new fs Skills:
//! `task.repeat_verified`, `task.explain`, `task.compensate`.
//!
//! Test 1 exercises the full lifecycle:
//!   files.organize → task.repeat_verified → task.compensate
//! verifying that (a) the effect_manifest from step 1 can be re-verified,
//! (b) the compensation record from step 1 can be reversed, and (c) files
//! end up back at the original source location.
//!
//! Test 2 verifies that `task.explain` reads the audit log correctly after
//! multiple prior operations, and that the audit log grows as operations
//! are executed.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::skills::task_compensate::{TaskCompensateInput, execute_compensate};
use trust_kernel::skills::task_explain::{TaskExplainInput, execute_explain};
use trust_kernel::skills::task_repeat::{TaskRepeatVerifiedInput, execute_repeat_verified};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w7p2-smoke-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn e2e_files_organize_then_repeat_verified_then_compensate() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dir = tmp_dir();
    let src_dir = dir.join("downloads");
    fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("papers");
    fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf-a-content").unwrap();
    fs::write(src_dir.join("b.pdf"), b"pdf-b-content").unwrap();

    let approver = Arc::new(AutoApprover);

    // ===== Step 1: files.organize moves PDFs from src to dest =====
    let task1 = "e2e-task-1".to_string();
    let step1 = "e2e-step-1".to_string();
    kernel.create_task(&task1, "整理 PDF").unwrap();
    kernel
        .create_step(&StepRecord::new(&step1, &task1, 1))
        .unwrap();
    let input1 = FilesOrganizeInput {
        task_id: task1.clone(),
        step_id: step1.clone(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };
    let skill = FilesOrganizeSkill::new();
    let execution1 = skill
        .execute(&kernel, &input1, approver.as_ref())
        .expect("files.organize must succeed with AutoApprover");
    let comp_id = execution1
        .tool_result
        .compensation_ref
        .as_ref()
        .expect("compensation_ref must be set after files.organize")
        .clone();

    // Files moved src → dest.
    assert!(!src_dir.join("a.pdf").exists());
    assert!(!src_dir.join("b.pdf").exists());
    assert!(dest_dir.join("a.pdf").exists());
    assert!(dest_dir.join("b.pdf").exists());

    // ===== Step 2: task.repeat_verified re-verifies the move =====
    // `execute_repeat_verified` derives source_dir from the previous
    // manifest's first source path (src_dir) and calls search_files on
    // it. After step 1, src_dir is empty — drop a fresh PDF in src_dir
    // to satisfy search_files. verify_move still checks dest_dir against
    // the original manifest hashes (which still match).
    fs::write(src_dir.join("c.pdf"), b"pdf-c-content").unwrap();

    let task2 = "e2e-task-2".to_string();
    let step2 = "e2e-step-2".to_string();
    let input2 = TaskRepeatVerifiedInput {
        task_id: task2.clone(),
        step_id: step2.clone(),
        target_task_id: task1.clone(),
        source_filter: "*.pdf".to_string(),
    };
    let result2 = execute_repeat_verified(&kernel, &input2, approver.as_ref())
        .expect("task.repeat_verified must succeed");
    assert_eq!(result2, task2);

    // Step 2 record: Succeeded with strong evidence.
    // W10 Plan 1: task.repeat_verified verifier strategy 升级为 "strong"
    // (verify_task_repeat 重读 sha256+size),evidence_strength 跟随升级。
    let step2_rec = kernel.get_step(&step2).unwrap().unwrap();
    assert_eq!(step2_rec.status, StepStatus::Succeeded);
    assert_eq!(step2_rec.evidence_strength.as_deref(), Some("strong"));

    // ===== Step 3: task.compensate reverses step 1's move =====
    let task3 = "e2e-task-3".to_string();
    let step3 = "e2e-step-3".to_string();
    let input3 = TaskCompensateInput {
        task_id: task3.clone(),
        step_id: step3.clone(),
        target_step_id: step1.clone(),
    };
    let result3 = execute_compensate(&kernel, &input3, approver.as_ref())
        .expect("task.compensate must succeed");
    assert_eq!(result3, task3);

    // Files moved dest → src (back to original locations).
    assert!(
        src_dir.join("a.pdf").exists(),
        "a.pdf must be back at src after compensate"
    );
    assert!(
        src_dir.join("b.pdf").exists(),
        "b.pdf must be back at src after compensate"
    );
    assert!(
        !dest_dir.join("a.pdf").exists(),
        "a.pdf must be removed from dest after compensate"
    );
    assert!(
        !dest_dir.join("b.pdf").exists(),
        "b.pdf must be removed from dest after compensate"
    );

    // Compensation record marked "reversed".
    let comp_rec = kernel.get_compensation(&comp_id).unwrap().unwrap();
    assert_eq!(comp_rec.status, "reversed");

    // Step 3 record: Succeeded with strong evidence + comp_ref.
    let step3_rec = kernel.get_step(&step3).unwrap().unwrap();
    assert_eq!(step3_rec.status, StepStatus::Succeeded);
    assert_eq!(step3_rec.evidence_strength.as_deref(), Some("strong"));
    assert_eq!(
        step3_rec.compensation_ref.as_deref(),
        Some(comp_id.as_str())
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn e2e_explain_reads_audit_log_after_multiple_operations() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dir = tmp_dir();
    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();

    // Run 2 files.organize operations — each creates task_created +
    // step_created + ... + compensation_created audit events.
    for i in 1..=2 {
        let src_dir = dir.join(format!("src{}", i));
        fs::create_dir_all(&src_dir).unwrap();
        let dest_dir = dir.join(format!("dest{}", i));
        fs::create_dir_all(&dest_dir).unwrap();
        fs::write(
            src_dir.join(format!("file{}.pdf", i)),
            format!("content{}", i).as_bytes(),
        )
        .unwrap();

        let task_id = format!("prep-task-{}", i);
        let step_id = format!("prep-step-{}", i);
        kernel
            .create_task(&task_id, &format!("整理 {}", i))
            .unwrap();
        kernel
            .create_step(&StepRecord::new(&step_id, &task_id, 1))
            .unwrap();
        let input = FilesOrganizeInput {
            task_id: task_id.clone(),
            step_id: step_id.clone(),
            source: src_dir.clone(),
            filter: "*.pdf".to_string(),
            destination: dest_dir.clone(),
        };
        skill
            .execute(&kernel, &input, approver.as_ref())
            .expect("files.organize must succeed");
    }

    // Count audit events before explain.
    let audit_before = kernel.list_audit_recent(100).unwrap().len();

    // Run task.explain.
    let task_explain_id = "explain-task-1".to_string();
    let step_explain_id = "explain-step-1".to_string();
    let input = TaskExplainInput {
        task_id: task_explain_id.clone(),
        step_id: step_explain_id.clone(),
        limit: 10,
    };
    let result =
        execute_explain(&kernel, &input, approver.as_ref()).expect("task.explain must succeed");
    assert_eq!(result, task_explain_id);

    // Explain step: Succeeded with weak evidence (read-only).
    let step_rec = kernel.get_step(&step_explain_id).unwrap().unwrap();
    assert_eq!(step_rec.status, StepStatus::Succeeded);
    assert_eq!(step_rec.evidence_strength.as_deref(), Some("weak"));

    // Audit log grew (explain itself adds task_created + step_created + ... events).
    let audit_after = kernel.list_audit_recent(100).unwrap().len();
    assert!(
        audit_after > audit_before,
        "audit log must grow after explain ({} > {})",
        audit_after,
        audit_before
    );
    assert!(
        audit_after >= 2,
        "audit log must contain events from prior operations, got {}",
        audit_after
    );

    fs::remove_dir_all(&dir).ok();
}
