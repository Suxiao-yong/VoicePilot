use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::compensation::types::CompensationLevel;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::skills::manifest::files_organize_manifest;
use trust_kernel::skills::router::SkillRouter;
use trust_kernel::toolresult::{EvidenceStrength, ToolStatus};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-e2e-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn end_to_end_files_organize_skill_smoke() {
    // ===== Setup: kernel + task + step =====
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "e2e-task-1".to_string();
    let step_id = "e2e-step-1".to_string();
    let goal = "把下载目录里的 PDF 移到论文文件夹";
    kernel.create_task(&task_id, goal).unwrap();
    kernel.create_step(&StepRecord::new(&step_id, &task_id, 1)).unwrap();

    // ===== Setup: temp filesystem with PDFs + a non-matching .txt =====
    let dir = tmp_dir();
    let src_dir = dir.join("downloads"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("papers"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf-a-content").unwrap();
    fs::write(src_dir.join("b.pdf"), b"pdf-b-content").unwrap();
    fs::write(src_dir.join("c.txt"), b"not-a-pdf").unwrap();

    // ===== Route: Skill Router must pick files.organize =====
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    let decision = router.route(goal);
    let skill_manifest = match decision {
        trust_kernel::skills::router::RouteDecision::Skill(m) => m,
        trust_kernel::skills::router::RouteDecision::Planner => {
            panic!("router should have picked files.organize Skill")
        }
        // 同步 route() 不返回 SkillWithSlots;若返回则契约被破坏。
        #[cfg(feature = "llm")]
        trust_kernel::skills::router::RouteDecision::SkillWithSlots(_, _) => {
            panic!("sync route() should not return SkillWithSlots")
        }
        // W8 Plan 4:同步 route() 不返回 Dag(它不做 LLM 拆解);若返回则契约被破坏。
        #[cfg(feature = "llm")]
        trust_kernel::skills::router::RouteDecision::Dag(_) => {
            panic!("sync route() should not return Dag")
        }
    };
    assert_eq!(skill_manifest.id, "files.organize");

    // ===== Execute: full pipeline with AutoApprover =====
    let input = FilesOrganizeInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };
    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(&kernel, &input, approver.as_ref())
        .expect("skill must succeed with AutoApprover");

    // ===== Assert: ToolResult V2 =====
    let tr = &execution.tool_result;
    assert_eq!(tr.status, ToolStatus::Succeeded);
    assert_eq!(tr.evidence_strength, EvidenceStrength::Strong);
    assert_eq!(tr.compensation_level, CompensationLevel::Strong);
    let comp_id: &str = tr.compensation_ref.as_ref().expect("compensation_ref must be set");
    assert!(!tr.idempotency_key.is_empty());

    // ===== Assert: filesystem state =====
    assert!(!src_dir.join("a.pdf").exists());
    assert!(!src_dir.join("b.pdf").exists());
    assert!(src_dir.join("c.txt").exists(), "non-matching file must stay in place");
    assert!(dest_dir.join("a.pdf").exists());
    assert!(dest_dir.join("b.pdf").exists());
    assert_eq!(fs::read(dest_dir.join("a.pdf")).unwrap(), b"pdf-a-content");

    // ===== Assert: step record fully populated =====
    let step = kernel.get_step(&step_id).unwrap().expect("step must exist");
    assert_eq!(step.status, StepStatus::Succeeded);
    assert!(step.prepare_token.is_some(), "prepare_token must be persisted");
    assert!(step.preconditions_hash.is_some(), "preconditions_hash must be persisted");
    assert!(step.effect_manifest.is_some(), "effect_manifest must be persisted");
    assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
    assert_eq!(step.compensation_ref.as_deref(), Some(comp_id));

    // ===== Assert: approval record persisted =====
    let approvals = kernel.list_approvals_for_task(&task_id).unwrap();
    assert_eq!(approvals.len(), 1);
    assert!(approvals[0].user_decision == trust_kernel::approval::types::ApprovalDecision::Allow);
    assert_eq!(approvals[0].approval_scope, trust_kernel::approval::types::ApprovalScope::Single);
    assert_eq!(approvals[0].e_level, trust_kernel::policy::types::ELevel::E2);
    assert_eq!(approvals[0].d_level, trust_kernel::policy::types::DLevel::D2);

    // ===== Assert: compensation record persisted =====
    let comp = kernel.get_compensation(comp_id).unwrap().expect("compensation must exist");
    assert_eq!(comp.level, CompensationLevel::Strong);
    assert_eq!(comp.status, "active");
    assert_eq!(comp.compensate_fn, "filesystem.reverse_move");
    assert!(!comp.reverse_payload.is_empty());

    // ===== Assert: audit chain (hash-chained events) =====
    let audit_count = kernel.audit_count_for_task(&task_id).unwrap();
    assert!(
        audit_count >= 4,
        "expected at least 4 audit events for task (task_created, step_created, step_prepared, approval_recorded, step_committed, compensation_created, step_status_changed), got {}",
        audit_count
    );

    // ===== Assert: compensation can be loaded + reverse_payload parses =====
    let payload: serde_json::Value = serde_json::from_str(&comp.reverse_payload).unwrap();
    let moves = payload.get("moves").and_then(|m| m.as_array()).unwrap();
    assert_eq!(moves.len(), 2, "reverse_payload must list 2 moves");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn end_to_end_compensation_can_be_reversed_via_auto_reverse() {
    // After the skill runs, the user invokes task.compensate (V1.1 §5.2)
    // to roll back. W3b verifies the auto_reverse_move function works against
    // the persisted CompensationRecord.
    use trust_kernel::compensation::executor::auto_reverse_move;

    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "e2e-task-2".to_string();
    let step_id = "e2e-step-2".to_string();
    kernel.create_task(&task_id, "整理").unwrap();
    kernel.create_step(&StepRecord::new(&step_id, &task_id, 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("dl"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("x.pdf"), b"pdf-x").unwrap();

    let input = FilesOrganizeInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };
    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(&kernel, &input, approver.as_ref()).unwrap();
    let comp_id = execution.tool_result.compensation_ref.as_ref().unwrap();

    // File is now at dest_dir/x.pdf, not at src_dir/x.pdf.
    assert!(!src_dir.join("x.pdf").exists());
    assert!(dest_dir.join("x.pdf").exists());

    // Load compensation + reverse it.
    let comp = kernel.get_compensation(comp_id).unwrap().unwrap();
    auto_reverse_move(&kernel, &comp).unwrap();

    // After reverse: file is back at src_dir/x.pdf.
    assert!(src_dir.join("x.pdf").exists(), "file must be back at original location");
    assert!(!dest_dir.join("x.pdf").exists(), "file must be removed from destination");

    // Mark compensation as consumed.
    kernel.mark_compensation_status(comp_id, "consumed").unwrap();
    let active = kernel.list_active_compensations().unwrap();
    assert!(active.is_empty(), "compensation must no longer be active after consume");

    fs::remove_dir_all(&dir).ok();
}
