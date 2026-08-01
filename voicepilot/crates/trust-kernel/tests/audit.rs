use chrono::Utc;
use std::sync::{Arc, Mutex};
use trust_kernel::audit::{AuditEvent, AuditLogger, SqliteAuditLogger};
use trust_kernel::db;
use trust_kernel::repo::step_repo::{StepRecord, StepRepo};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};

fn make_event(task_id: &str, event_type: &str, prev_hash: Option<String>) -> AuditEvent {
    AuditEvent {
        log_id: uuid::Uuid::new_v4().to_string(),
        task_id: task_id.to_string(),
        step_id: None,
        event_type: event_type.to_string(),
        details: serde_json::json!({"note": "test"}),
        timestamp: Utc::now(),
        prev_hash,
        hash: String::new(), // computed by logger
    }
}

fn fresh_logger() -> SqliteAuditLogger {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    // Pre-create parent task rows so audit_logs.task_id FK is satisfied.
    let task_repo = TaskRepo::new();
    for tid in ["task-1", "task-2", "task-3"] {
        let task = TaskRecord::new(tid, "test goal");
        task_repo.create(&conn, &task).unwrap();
    }
    // Pre-create a step row for task-3 so audit_logs.step_id FK is satisfied.
    let step_repo = StepRepo::new();
    let step = StepRecord::new("step-1", "task-3", 1);
    step_repo.create(&conn, &step).unwrap();
    SqliteAuditLogger::new(Arc::new(Mutex::new(conn)))
}

#[test]
fn append_writes_event_with_correct_hash() {
    let logger = fresh_logger();
    let event = make_event("task-1", "task_created", None);
    logger.append(&event).unwrap();

    let rows = logger
        .query_rows("SELECT hash, prev_hash FROM audit_logs WHERE task_id=?1", "task-1")
        .unwrap();
    assert_eq!(rows.len(), 1);
    let (hash, prev) = &rows[0];
    assert!(!hash.is_empty(), "hash must be non-empty");
    assert!(prev.is_none(), "first event has no prev_hash");
}

#[test]
fn hash_chain_links_consecutive_events() {
    let logger = fresh_logger();

    let e1 = make_event("task-2", "task_created", None);
    logger.append(&e1).unwrap();
    let first_hash: String = logger
        .query_rows(
            "SELECT hash, prev_hash FROM audit_logs WHERE task_id=?1 ORDER BY timestamp LIMIT 1",
            "task-2",
        )
        .unwrap()[0]
        .0
        .clone();

    let e2 = make_event("task-2", "state_transition", Some(first_hash.clone()));
    logger.append(&e2).unwrap();

    let rows = logger
        .query_rows("SELECT hash, prev_hash FROM audit_logs WHERE task_id=?1", "task-2")
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1].1.as_deref(),
        Some(first_hash.as_str()),
        "second event must link to first hash"
    );
    assert_ne!(rows[0].0, rows[1].0, "hashes must differ");
}

#[test]
fn audit_logger_records_step_id_when_provided() {
    let logger = fresh_logger();

    let mut event = make_event("task-3", "step_started", None);
    event.step_id = Some("step-1".to_string());
    logger.append(&event).unwrap();

    let step_id = logger
        .query_single(
            "SELECT step_id FROM audit_logs WHERE task_id=?1",
            "task-3",
        )
        .unwrap();
    assert_eq!(step_id.as_deref(), Some("step-1"));
}
