use trust_kernel::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use trust_kernel::compensation::repo::CompensationRepo;
use trust_kernel::db;
use trust_kernel::repo::step_repo::{StepRecord, StepRepo};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};

fn fresh_conn() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    // Pre-create parent task + step rows so compensations.step_id FK is satisfied.
    // (compensations.step_id REFERENCES steps(step_id) ON DELETE CASCADE)
    let task_repo = TaskRepo::new();
    task_repo.create(&conn, &TaskRecord::new("task-comp", "compensation test")).unwrap();
    let step_repo = StepRepo::new();
    step_repo.create(&conn, &StepRecord::new("step-1", "task-comp", 0)).unwrap();
    step_repo.create(&conn, &StepRecord::new("s", "task-comp", 1)).unwrap();
    conn
}

#[test]
fn create_compensation_persists_and_can_be_loaded() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    let rec = CompensationRecord {
        comp_id: "comp-1".to_string(),
        step_id: "step-1".to_string(),
        level: CompensationLevel::Strong,
        snapshot_encrypted: Some(b"encrypted-blob".to_vec()),
        ttl_expires: "2026-07-19T16:00:00Z".to_string(),
        status: "active".to_string(),
        // Deviation from plan: was Some("stronghold://voicepilot/comp/abc").
        // The PoC stash hack (gotcha #1) only persists compensate_fn/reverse_payload
        // when snapshot_vault_ref is None. Setting to None so the round-trip
        // preserves compensate_fn as the test asserts.
        snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: r#"{"sources":["c:/out/a.txt"],"dest":"c:/orig"}"#.to_string(),
    };
    repo.create(&conn, &rec).unwrap();

    let loaded = repo.get(&conn, "comp-1").unwrap().expect("must exist");
    assert_eq!(loaded.level, CompensationLevel::Strong);
    assert_eq!(loaded.conflict_policy, ConflictPolicy::AutoReverse);
    assert_eq!(loaded.compensate_fn, "filesystem.reverse_move");
    assert_eq!(loaded.status, "active");
}

#[test]
fn get_missing_returns_none() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    assert!(repo.get(&conn, "nope").unwrap().is_none());
}

#[test]
fn mark_consumed_updates_status() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    let rec = CompensationRecord {
        comp_id: "comp-2".to_string(), step_id: "s".to_string(),
        level: CompensationLevel::BestEffort,
        snapshot_encrypted: None, ttl_expires: "2026-07-19T16:00:00Z".to_string(),
        status: "active".to_string(), snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::RequireConfirmation,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: "{}".to_string(),
    };
    repo.create(&conn, &rec).unwrap();
    repo.mark_status(&conn, "comp-2", "consumed").unwrap();
    let loaded = repo.get(&conn, "comp-2").unwrap().unwrap();
    assert_eq!(loaded.status, "consumed");
}

#[test]
fn list_active_returns_only_active() {
    let conn = fresh_conn();
    let repo = CompensationRepo::new();
    for (id, status) in [("c1", "active"), ("c2", "consumed"), ("c3", "active")] {
        let rec = CompensationRecord {
            comp_id: id.to_string(), step_id: "s".to_string(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None, ttl_expires: "2026-07-19T16:00:00Z".to_string(),
            status: status.to_string(), snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "filesystem.reverse_move".to_string(),
            reverse_payload: "{}".to_string(),
        };
        repo.create(&conn, &rec).unwrap();
    }
    let active = repo.list_active(&conn).unwrap();
    assert_eq!(active.len(), 2);
}
