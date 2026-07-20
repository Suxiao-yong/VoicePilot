use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use trust_kernel::approval::approver::{AutoApprover, AutoDenier};
use trust_kernel::compensation::types::CompensationLevel;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill, SkillExecution};
use trust_kernel::toolresult::{EvidenceStrength, ToolStatus};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-exec-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn files_organize_skill_executes_full_pipeline_on_allow() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "把下载目录里的 PDF 移到论文文件夹").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("src"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf1").unwrap();
    fs::write(src_dir.join("b.pdf"), b"pdf2").unwrap();
    fs::write(src_dir.join("c.txt"), b"txt").unwrap();

    let input = FilesOrganizeInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };

    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let result: SkillExecution = skill.execute(&kernel, &input, approver.as_ref()).unwrap();

    // ToolResult V2 fields.
    assert_eq!(result.tool_result.status, ToolStatus::Succeeded);
    assert_eq!(result.tool_result.evidence_strength, EvidenceStrength::Strong);
    assert_eq!(result.tool_result.compensation_level, CompensationLevel::Strong);
    assert!(result.tool_result.compensation_ref.is_some());

    // Filesystem state.
    assert!(!src_dir.join("a.pdf").exists());
    assert!(!src_dir.join("b.pdf").exists());
    assert!(src_dir.join("c.txt").exists(), "non-matching file must stay");
    assert!(dest_dir.join("a.pdf").exists());
    assert!(dest_dir.join("b.pdf").exists());

    // Kernel persistence.
    let comp_id = result.tool_result.compensation_ref.as_ref().unwrap();
    let comp = kernel.get_compensation(comp_id).unwrap().expect("compensation must exist");
    assert_eq!(comp.level, CompensationLevel::Strong);
    assert_eq!(comp.status, "active");

    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert!(step.prepare_token.is_some());
    assert!(step.preconditions_hash.is_some());
    assert!(step.effect_manifest.is_some());
    assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
    assert!(step.compensation_ref.is_some());

    let approvals = kernel.list_approvals_for_task("t1").unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].user_decision, trust_kernel::approval::types::ApprovalDecision::Allow);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn files_organize_skill_aborts_on_deny_without_commit() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "整理").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("src"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf1").unwrap();

    let input = FilesOrganizeInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };

    let approver = Arc::new(AutoDenier);
    let skill = FilesOrganizeSkill::new();
    let result = skill.execute(&kernel, &input, approver.as_ref()).unwrap();

    assert_eq!(result.tool_result.status, ToolStatus::Cancelled);
    assert!(result.tool_result.compensation_ref.is_none());

    // Source must still exist (no commit happened).
    assert!(src_dir.join("a.pdf").exists());

    let approvals = kernel.list_approvals_for_task("t1").unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].user_decision, trust_kernel::approval::types::ApprovalDecision::Deny);

    // No compensation should have been created.
    let active_comps = kernel.list_active_compensations().unwrap();
    assert!(active_comps.is_empty());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn files_organize_skill_fails_when_no_files_match_filter() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "整理").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("src"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.txt"), b"txt").unwrap();

    let input = FilesOrganizeInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };

    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let result = skill.execute(&kernel, &input, approver.as_ref());

    // Empty search → executor returns Err (Skill failure_policy.on_fail = "stop").
    assert!(result.is_err());

    fs::remove_dir_all(&dir).ok();
}
