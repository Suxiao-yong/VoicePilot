use trust_kernel::db;
use trust_kernel::repo::step_repo::{StepRecord, StepRepo, StepStatus};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};
use trust_kernel::state::TaskState;

fn fresh_db() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

#[test]
fn create_task_persists_and_can_be_loaded() {
    let conn = fresh_db();
    let repo = TaskRepo::new();
    let task = TaskRecord::new("task-1", "open notepad and write hello");
    repo.create(&conn, &task).unwrap();

    let loaded = repo.get(&conn, "task-1").unwrap().expect("task must exist");
    assert_eq!(loaded.task_id, "task-1");
    assert_eq!(loaded.user_goal, "open notepad and write hello");
    assert_eq!(loaded.status, TaskState::Idle);
}

#[test]
fn update_status_transitions_state() {
    let conn = fresh_db();
    let repo = TaskRepo::new();
    let task = TaskRecord::new("task-2", "goal");
    repo.create(&conn, &task).unwrap();

    repo.update_status(&conn, "task-2", TaskState::Planning).unwrap();
    let loaded = repo.get(&conn, "task-2").unwrap().unwrap();
    assert_eq!(loaded.status, TaskState::Planning);
}

#[test]
fn create_step_persists_with_task_link() {
    let conn = fresh_db();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let task = TaskRecord::new("task-3", "goal");
    task_repo.create(&conn, &task).unwrap();

    let step = StepRecord {
        step_id: "step-1".to_string(),
        task_id: "task-3".to_string(),
        step_order: 0,
        tool_name: Some("filesystem.read".to_string()),
        args: Some(serde_json::json!({"path": "/tmp"})),
        args_hash: Some("abc123".to_string()),
        status: StepStatus::Pending,
        prepare_token: None,
        preconditions_hash: None,
        effect_manifest: None,
        evidence_strength: None,
        compensation_ref: None,
        egress_performed: false,
        started_at: None,
        finished_at: None,
    };
    step_repo.create(&conn, &step).unwrap();

    let steps = step_repo.list_for_task(&conn, "task-3").unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, "step-1");
    assert_eq!(steps[0].tool_name.as_deref(), Some("filesystem.read"));
}

#[test]
fn update_step_status_advances_lifecycle() {
    let conn = fresh_db();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    task_repo.create(&conn, &TaskRecord::new("task-4", "goal")).unwrap();

    let step = StepRecord::new("step-1", "task-4", 0);
    step_repo.create(&conn, &step).unwrap();

    step_repo.update_status(&conn, "step-1", StepStatus::Running).unwrap();
    step_repo.update_status(&conn, "step-1", StepStatus::Succeeded).unwrap();

    let loaded = step_repo.get(&conn, "step-1").unwrap().unwrap();
    assert_eq!(loaded.status, StepStatus::Succeeded);
}

#[test]
fn deleting_task_cascades_to_steps() {
    let conn = fresh_db();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    task_repo.create(&conn, &TaskRecord::new("task-5", "goal")).unwrap();
    let step = StepRecord::new("step-1", "task-5", 0);
    step_repo.create(&conn, &step).unwrap();

    task_repo.delete(&conn, "task-5").unwrap();
    assert!(task_repo.get(&conn, "task-5").unwrap().is_none());
    let steps = step_repo.list_for_task(&conn, "task-5").unwrap();
    assert!(steps.is_empty(), "steps must cascade-delete with task");
}
