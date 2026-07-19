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
