use chrono::Utc;
use trust_kernel::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};

fn fresh_kernel() -> TrustKernel {
    TrustKernel::open_in_memory().unwrap()
}

fn sample_comp(comp_id: &str, step_id: &str) -> CompensationRecord {
    CompensationRecord {
        comp_id: comp_id.to_string(),
        step_id: step_id.to_string(),
        level: CompensationLevel::Strong,
        snapshot_encrypted: None,
        ttl_expires: (Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339(),
        status: "active".to_string(),
        snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: r#"{"moves":[]}"#.to_string(),
    }
}

#[test]
fn kernel_create_compensation_persists() {
    let k = fresh_kernel();
    k.create_task("t1", "test goal").unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    k.create_compensation(&sample_comp("c1", "s1")).unwrap();
    let loaded = k.get_compensation("c1").unwrap().expect("must exist");
    assert_eq!(loaded.level, CompensationLevel::Strong);
    assert_eq!(loaded.status, "active");
}

#[test]
fn kernel_list_active_compensations() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    k.create_compensation(&sample_comp("c1", "s1")).unwrap();
    k.create_compensation(&sample_comp("c2", "s1")).unwrap();
    k.mark_compensation_status("c2", "consumed").unwrap();
    let active = k.list_active_compensations().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].comp_id, "c1");
}

#[test]
fn kernel_step_lifecycle_persists_prepare_and_post_commit() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    let step = StepRecord::new("s1", "t1", 1);
    k.create_step(&step).unwrap();

    // Update prepare state.
    let manifest_json = serde_json::json!({"sources": [], "destination": "c:/out"});
    k.update_step_prepare_state("s1", "prt_token", "sha256:abc", &manifest_json)
        .unwrap();
    let loaded = k.get_step("s1").unwrap().expect("must exist");
    assert_eq!(loaded.prepare_token.as_deref(), Some("prt_token"));
    assert_eq!(loaded.preconditions_hash.as_deref(), Some("sha256:abc"));
    assert!(loaded.effect_manifest.is_some());

    // Update post-commit state.
    k.update_step_post_commit("s1", "strong", Some("c1")).unwrap();
    let loaded = k.get_step("s1").unwrap().unwrap();
    assert_eq!(loaded.evidence_strength.as_deref(), Some("strong"));
    assert_eq!(loaded.compensation_ref.as_deref(), Some("c1"));

    // Status transition.
    k.update_step_status("s1", StepStatus::Succeeded).unwrap();
    let loaded = k.get_step("s1").unwrap().unwrap();
    assert_eq!(loaded.status, StepStatus::Succeeded);
}

#[test]
fn kernel_list_steps_for_task_returns_in_order() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    k.create_step(&StepRecord::new("s2", "t1", 2)).unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    let steps = k.list_steps_for_task("t1").unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].step_id, "s1");
    assert_eq!(steps[1].step_id, "s2");
}

use trust_kernel::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use trust_kernel::policy::types::{DLevel, ELevel};

#[test]
fn kernel_record_approval_persists_and_audits() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let rec = ApprovalRecord {
        approval_id: "a1".to_string(),
        task_id: "t1".to_string(),
        step_id: Some("s1".to_string()),
        risk_level: "E2".to_string(),
        args_hash: "sha256:args".to_string(),
        user_decision: ApprovalDecision::Allow,
        decided_at: "2026-07-20T10:00:00Z".to_string(),
        e_level: ELevel::E2,
        d_level: DLevel::D2,
        destination: "local_file".to_string(),
        egress_approved: false,
        approval_scope: ApprovalScope::Single,
        policy_bundle_hash: "sha256:bundle".to_string(),
    };
    k.record_approval(&rec).unwrap();

    let loaded = k.get_approval("a1").unwrap().expect("must exist");
    assert_eq!(loaded.user_decision, ApprovalDecision::Allow);

    let list = k.list_approvals_for_task("t1").unwrap();
    assert_eq!(list.len(), 1);

    // Audit trail must include approval_recorded.
    let audit_count = k.audit_count_for_task("t1").unwrap();
    assert!(audit_count >= 2, "task + approval events expected");
}

#[test]
fn kernel_check_approval_scope_returns_single_in_w3b() {
    // W3b always returns Single per V1.1 §8.1 — batch lands in W7 with Skill context.
    let k = fresh_kernel();
    let scope = k.check_approval_scope("t1", "files.organize", "sha256:args", "sha256:bundle", 3);
    assert_eq!(scope, ApprovalScope::Single);
}

#[test]
fn kernel_audit_append_external_logs_event() {
    let k = trust_kernel::kernel::TrustKernel::open_in_memory().unwrap();
    k.create_task("t-ext", "external audit test").unwrap();
    let before = k.audit_count_for_task("t-ext").unwrap();
    k.audit_append_external(
        "t-ext",
        None,
        "mcp_tools_call",
        serde_json::json!({
            "tool": "filesystem.search_files",
            "args": {"root": "C:/Users", "pattern": "*.pdf"}
        }),
    )
    .unwrap();
    let after = k.audit_count_for_task("t-ext").unwrap();
    assert_eq!(after, before + 1);
}

#[test]
fn kernel_audit_append_external_rejects_unknown_task() {
    let k = trust_kernel::kernel::TrustKernel::open_in_memory().unwrap();
    let result = k.audit_append_external(
        "nonexistent-task",
        None,
        "mcp_tools_call",
        serde_json::json!({}),
    );
    // FK constraint — audit_logs.task_id REFERENCES tasks(task_id).
    assert!(result.is_err());
}

#[test]
fn kernel_replace_filesystem_enforces_allowed_paths() {
    use trust_kernel::allowed_paths::AllowedPaths;
    use std::path::Path;

    let k = trust_kernel::kernel::TrustKernel::open_in_memory().unwrap();
    let allowed = AllowedPaths::new(vec!["C:/Users".to_string()]);
    k.replace_filesystem_with_allowed_paths(allowed);

    // Path outside whitelist must be rejected.
    let result = k.filesystem().search_files(Path::new("E:/elsewhere"), "*.pdf");
    assert!(result.is_err(), "search_files outside allowed_paths must fail");
}
