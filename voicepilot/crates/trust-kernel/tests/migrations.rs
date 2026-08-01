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

#[test]
fn migration_007_converts_screaming_snake_case_audit_event_types() {
    // W10 清理项 3:migration 007 必须把 14 种历史 SCREAMING_SNAKE_CASE event_type
    // 转换为 lower_snake_case,保证带历史数据的库与新代码一致。
    //
    // 测试策略:
    // 1. 跑完所有 migration(含 007)。
    // 2. 直接 INSERT 几条 SCREAMING_SNAKE_CASE event_type 行(模拟 W1-W8 历史数据)。
    // 3. 重新跑 migration 007(单独 execute_batch)验证幂等性 + 转换。
    // 4. 查询验证所有行已被转换为 lower_snake_case。
    //
    // 注:run_migrations 已在 setup 阶段执行过 007,但当时 audit_logs 表空,
    // UPDATE 影响 0 行。本测试手动 INSERT 历史数据后再次执行 007 验证转换逻辑。
    use trust_kernel::db;
    let conn = db::open_in_memory().expect("open db");
    db::run_migrations(&conn).expect("migrations");

    // 插入 14 种 SCREAMING_SNAKE_CASE 历史事件(模拟 W1-W8 旧版本写入)。
    // audit_logs 表结构(参考 001_init.sql):log_id / task_id / step_id / event_type
    // / details / timestamp / prev_hash / hash / otlp_trace_id / otlp_span_id
    // / data_classification_redacted。task_id 必须 REFERENCES tasks(task_id),
    // 故先插一个 task。
    conn.execute(
        "INSERT INTO tasks (task_id, user_goal, status, created_at, updated_at) \
         VALUES ('t-mig7', 'migration 007 test', 'PENDING', '2026-08-01T00:00:00Z', '2026-08-01T00:00:00Z')",
        [],
    )
    .expect("insert task");

    let legacy_events = [
        "TASK_CREATED",
        "STEP_CREATED",
        "STEP_STATUS_CHANGED",
        "STEP_PREPARED",
        "STEP_COMMITTED",
        "STEP_STARTED",
        "STEP_SUCCEEDED",
        "STEP_FAILED",
        "STATE_TRANSITION",
        "COMPENSATION_CREATED",
        "COMPENSATION_STATUS_CHANGED",
        "APPROVAL_RECORDED",
        "MCP_TOOLS_CALL",
        "MCP_CALL_FAILED",
    ];
    for (i, et) in legacy_events.iter().enumerate() {
        conn.execute(
            "INSERT INTO audit_logs (log_id, task_id, event_type, details, timestamp, hash) \
             VALUES (?1, 't-mig7', ?2, '{}', '2026-08-01T00:00:00Z', 'hash-placeholder')",
            rusqlite::params![format!("log-{}", i), et],
        )
        .expect("insert legacy audit log");
    }

    // 重新执行 migration 007(幂等性验证 + 实际转换)。
    // MIGRATION_007 是 const &str,但 db 模块未导出,直接 re-run_migrations 即可
    // (run_migrations 内部会再次 execute_batch MIGRATION_007,UPDATE CASE WHEN
    // 对 SCREAMING_SNAKE_CASE 行生效,对已 lower_snake_case 行不匹配,影响 0 行)。
    db::run_migrations(&conn).expect("re-run migrations (idempotent + converts)");

    // 验证所有 14 种 SCREAMING_SNAKE_CASE 已被转换为 lower_snake_case。
    let remaining_screaming: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs \
             WHERE event_type GLOB '[A-Z]*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        remaining_screaming, 0,
        "migration 007 must convert all SCREAMING_SNAKE_CASE event_type to lower_snake_case"
    );

    // 验证每种 lower_snake_case 都存在一行。
    let lower_count: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT event_type) FROM audit_logs \
             WHERE event_type IN ('task_created','step_created','step_status_changed',\
             'step_prepared','step_committed','step_started','step_succeeded','step_failed',\
             'state_transition','compensation_created','compensation_status_changed',\
             'approval_recorded','mcp_tools_call','mcp_call_failed')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        lower_count, 14,
        "all 14 lower_snake_case event_type must be present after migration 007, got {}",
        lower_count
    );
}
