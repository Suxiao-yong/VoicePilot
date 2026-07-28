use trust_kernel::db;

#[test]
fn migrations_apply_cleanly_on_fresh_db() {
    let conn = db::open_in_memory().expect("open in-memory db");
    db::run_migrations(&conn).expect("migrations must apply");
    // Verify all 10 tables exist (V1.1 spec §8.1)
    let expected_tables = [
        "tasks", "steps", "policies", "approvals", "compensations",
        "audit_logs", "skills", "taints", "mcp_servers", "egress_log",
    ];
    for t in &expected_tables {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![t],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("table {} should exist", t));
        assert_eq!(count, 1, "table {} missing after migration", t);
    }
}

#[test]
fn migrations_are_idempotent() {
    let conn = db::open_in_memory().expect("open db");
    db::run_migrations(&conn).expect("first migration run");
    db::run_migrations(&conn).expect("second migration run (idempotent)");
}

#[test]
fn foreign_keys_are_enforced() {
    let conn = db::open_in_memory().expect("open db");
    db::run_migrations(&conn).expect("migrations");
    let fk_enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .expect("PRAGMA foreign_keys");
    assert_eq!(fk_enabled, 1, "foreign_keys pragma must be ON");
}

#[test]
fn migration_004_creates_dag_tables() {
    let conn = db::open_in_memory().expect("open db");
    db::run_migrations(&conn).expect("migrations");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('dag_plans','dag_nodes','task_explanations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3, "W8 migration 004 must create dag_plans + dag_nodes + task_explanations");

    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN ('idx_dag_nodes_plan','idx_dag_plans_status','idx_task_explanations_step')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(idx_count, 3, "W8 migration 004 must create 3 indexes");
}
