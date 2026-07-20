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
