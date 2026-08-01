//! AuditLogger 查询方法单元测试 —— list_recent / list_for_task(V1.1.2 §8.3 Audit Viewer)。

use std::sync::{Arc, Mutex};

use trust_kernel::audit::{AuditEvent, AuditLogger, SqliteAuditLogger};
use trust_kernel::db;
use trust_kernel::repo::step_repo::{StepRecord, StepRepo};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};

fn setup() -> SqliteAuditLogger {
    let conn = db::open_in_memory().expect("open_in_memory");
    db::run_migrations(&conn).expect("run_migrations");
    // 预创建 task 行以满足 audit_logs.task_id 外键
    // (audit_logs.task_id REFERENCES tasks(task_id) ON DELETE CASCADE)。
    let task_repo = TaskRepo::new();
    for tid in ["task-1", "task-2"] {
        let task = TaskRecord::new(tid, "test goal");
        task_repo.create(&conn, &task).expect("create task");
    }
    // 预创建 step 行以满足 audit_logs.step_id 外键(测试用 step-1 归属 task-1)。
    let step_repo = StepRepo::new();
    let step = StepRecord::new("step-1", "task-1", 1);
    step_repo.create(&conn, &step).expect("create step");
    SqliteAuditLogger::new(Arc::new(Mutex::new(conn)))
}

fn make_event(task_id: &str, step_id: Option<&str>, event_type: &str) -> AuditEvent {
    AuditEvent {
        log_id: format!("log-{}", uuid::Uuid::new_v4()),
        task_id: task_id.to_string(),
        step_id: step_id.map(|s| s.to_string()),
        event_type: event_type.to_string(),
        details: serde_json::json!({"test": true}),
        timestamp: chrono::Utc::now(),
        prev_hash: None,
        hash: "deadbeef".to_string(),
    }
}

#[test]
fn list_recent_returns_events_in_desc_order() {
    let logger = setup();
    let e1 = make_event("task-1", None, "task_created");
    let e2 = make_event("task-1", Some("step-1"), "step_started");
    let e3 = make_event("task-2", None, "task_created");
    logger.append(&e1).expect("append");
    std::thread::sleep(std::time::Duration::from_millis(10));
    logger.append(&e2).expect("append");
    std::thread::sleep(std::time::Duration::from_millis(10));
    logger.append(&e3).expect("append");

    let recent = logger.list_recent(2).expect("list_recent");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].task_id, "task-2");
    assert_eq!(recent[1].task_id, "task-1");
}

#[test]
fn list_for_task_returns_all_events_for_task() {
    let logger = setup();
    logger.append(&make_event("task-1", None, "task_created")).expect("append");
    logger.append(&make_event("task-2", None, "task_created")).expect("append");
    logger.append(&make_event("task-1", Some("step-1"), "step_started")).expect("append");
    logger.append(&make_event("task-1", Some("step-1"), "step_succeeded")).expect("append");

    let events = logger.list_for_task("task-1").expect("list_for_task");
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].event_type, "task_created");
    assert_eq!(events[1].event_type, "step_started");
    assert_eq!(events[2].event_type, "step_succeeded");
}

#[test]
fn list_for_task_returns_empty_for_unknown_task() {
    let logger = setup();
    logger.append(&make_event("task-1", None, "task_created")).expect("append");
    let events = logger.list_for_task("nonexistent").expect("list_for_task");
    assert!(events.is_empty());
}

#[test]
fn list_recent_with_zero_limit_returns_empty() {
    let logger = setup();
    logger.append(&make_event("task-1", None, "task_created")).expect("append");
    let events = logger.list_recent(0).expect("list_recent");
    assert!(events.is_empty());
}
