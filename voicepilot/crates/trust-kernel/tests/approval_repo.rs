use trust_kernel::approval::repo::ApprovalRepo;
use trust_kernel::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use trust_kernel::db;
use trust_kernel::policy::types::{DLevel, ELevel};

fn fresh_conn() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

fn sample_approval(approval_id: &str, task_id: &str, step_id: &str) -> ApprovalRecord {
    ApprovalRecord {
        approval_id: approval_id.to_string(),
        task_id: task_id.to_string(),
        step_id: Some(step_id.to_string()),
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
    }
}

#[test]
fn create_approval_persists_and_loads() {
    let conn = fresh_conn();
    let repo = ApprovalRepo::new();
    // Parent rows required for FK constraints.
    conn.execute(
        "INSERT INTO tasks (task_id, user_goal, status, created_at, updated_at) VALUES ('t1', 'g', 'IDLE', '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO steps (step_id, task_id, step_order, status) VALUES ('s1', 't1', 1, 'PENDING')",
        [],
    ).unwrap();

    let rec = sample_approval("a1", "t1", "s1");
    repo.create(&conn, &rec).unwrap();

    let loaded = repo.get(&conn, "a1").unwrap().expect("must exist");
    assert_eq!(loaded.user_decision, ApprovalDecision::Allow);
    assert_eq!(loaded.approval_scope, ApprovalScope::Single);
    assert_eq!(loaded.e_level, ELevel::E2);
    assert_eq!(loaded.d_level, DLevel::D2);
    assert_eq!(loaded.policy_bundle_hash, "sha256:bundle");
}

#[test]
fn list_for_task_returns_all_approvals() {
    let conn = fresh_conn();
    let repo = ApprovalRepo::new();
    conn.execute(
        "INSERT INTO tasks (task_id, user_goal, status, created_at, updated_at) VALUES ('t1', 'g', 'IDLE', '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO steps (step_id, task_id, step_order, status) VALUES ('s1', 't1', 1, 'PENDING')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO steps (step_id, task_id, step_order, status) VALUES ('s2', 't1', 2, 'PENDING')",
        [],
    ).unwrap();

    repo.create(&conn, &sample_approval("a1", "t1", "s1"))
        .unwrap();
    repo.create(&conn, &sample_approval("a2", "t1", "s2"))
        .unwrap();

    let list = repo.list_for_task(&conn, "t1").unwrap();
    assert_eq!(list.len(), 2);
}

#[test]
fn approval_decision_round_trips() {
    for d in [
        ApprovalDecision::Allow,
        ApprovalDecision::Deny,
        ApprovalDecision::Modify,
    ] {
        let s = d.as_str();
        let back = ApprovalDecision::parse(s).expect("must round-trip");
        assert_eq!(d, back);
    }
}

#[test]
fn approval_scope_round_trips() {
    for s in [ApprovalScope::Single, ApprovalScope::Batch] {
        let str = s.as_str();
        let back = ApprovalScope::parse(str).expect("must round-trip");
        assert_eq!(s, back);
    }
}
