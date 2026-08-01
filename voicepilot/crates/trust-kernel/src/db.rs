//! SQLite connection + migration runner.
//!
//! Migrations are embedded SQL files under `src/migrations/`.
//! Each migration is idempotent (`CREATE TABLE IF NOT EXISTS`).

use crate::error::Result;
use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("migrations/001_init.sql");
const MIGRATION_002: &str = include_str!("migrations/002_app_config.sql");
// W7 Plan 5 Task 0: adds command/args/env columns to mcp_servers.
const MIGRATION_003: &str = include_str!("migrations/003_mcp_servers_command.sql");
// W8 Plan 1 Task 1: DAG orchestration tables (dag_plans + dag_nodes + task_explanations).
const MIGRATION_004: &str = include_str!("migrations/004_dag_plans.sql");
// W9 Plan 2 Task 3a: adds reverse_payload + compensate_fn real columns to compensations.
const MIGRATION_005: &str = include_str!("migrations/005_compensations_reverse_payload_columns.sql");
// W9 Plan 3 Task 1: taints 表 value_hash UNIQUE 约束(供 TaintRepo::upsert 的
// ON CONFLICT(value_hash) DO UPDATE 路径依赖)。CREATE UNIQUE INDEX IF NOT EXISTS
// 幂等,可重复执行。
const MIGRATION_006: &str = include_str!("migrations/006_taints_unique_index.sql");
// W10 清理项 3: audit_logs.event_type 历史数据 SCREAMING_SNAKE_CASE → lower_snake_case。
// 单条 UPDATE ... CASE WHEN 语句,无 schema 变更。CASE WHEN 不匹配 lower_snake_case
// 值,重复执行影响 0 行,天然幂等。
const MIGRATION_007: &str = include_str!("migrations/007_audit_logs_event_type_lower_snake_case.sql");

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

pub fn open_file(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Apply all migrations. Idempotent — safe to call on every startup.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(MIGRATION_001)?;
    conn.execute_batch(MIGRATION_002)?;
    // W7 Plan 5 Task 0: 003 adds columns to mcp_servers via ALTER TABLE.
    // SQLite doesn't support `ADD COLUMN IF NOT EXISTS`, and re-running
    // migrations against an already-migrated DB would fail with
    // "duplicate column name". Split by ';', strip `--` comment lines
    // from each chunk, execute each non-empty statement individually,
    // and swallow the duplicate-column error so re-runs are idempotent.
    for stmt in MIGRATION_003.split(';') {
        // Strip SQL line comments (`-- ...`) from each chunk so a comment
        // block above an ALTER statement doesn't cause us to skip it.
        let stripped: String = stmt
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join(" ");
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Err(e) = conn.execute(trimmed, []) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(e.into());
            }
        }
    }
    // W8 Plan 1 Task 1: 004 creates new tables (dag_plans + dag_nodes +
    // task_explanations) plus 3 indexes. All statements are
    // `CREATE ... IF NOT EXISTS`, so a single `execute_batch` is sufficient
    // and idempotent.
    conn.execute_batch(MIGRATION_004)?;
    // W9 Plan 2 Task 3a: 005 adds reverse_payload + compensate_fn columns
    // to compensations via ALTER TABLE. SQLite doesn't support
    // `ADD COLUMN IF NOT EXISTS`, so re-runs would fail with
    // "duplicate column name". Split by ';', strip `--` comment lines,
    // execute each non-empty statement individually, and swallow the
    // duplicate-column error so re-runs are idempotent (same pattern as 003).
    for stmt in MIGRATION_005.split(';') {
        let stripped: String = stmt
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join(" ");
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Err(e) = conn.execute(trimmed, []) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(e.into());
            }
        }
    }
    // W9 Plan 2 Task 3a: 应用层数据迁移 hook — 把 W3a PoC stash(存于
    // snapshot_vault_ref 列的 `{"compensate_fn":...,"reverse_payload":...}` JSON)
    // 精确解析到真实列 reverse_payload + compensate_fn,然后清空 snapshot_vault_ref。
    // 幂等:已迁移的行 snapshot_vault_ref 不再以 '{' 开头,SELECT 不会命中。
    migrate_005_compensations_stash(conn)?;
    // W9 Plan 3 Task 1: 006 创建 taints.value_hash UNIQUE 索引。CREATE ...
    // IF NOT EXISTS 幂等,单次 execute_batch 足够。
    conn.execute_batch(MIGRATION_006)?;
    // W10 清理项 3: 007 把 audit_logs.event_type 历史数据从 SCREAMING_SNAKE_CASE
    // 转换为 lower_snake_case。单条 UPDATE ... CASE WHEN,CASE 不匹配
    // lower_snake_case 值,重复执行影响 0 行,天然幂等,单次 execute_batch 足够。
    conn.execute_batch(MIGRATION_007)?;
    tracing::info!("migrations applied");
    Ok(())
}

/// W9 Plan 2 Task 3a: 应用层数据迁移 hook。
///
/// 把 W3a PoC stash(存于 `snapshot_vault_ref` 列的
/// `{"compensate_fn":...,"reverse_payload":...}` JSON)精确解析后写入真实列
/// `reverse_payload` + `compensate_fn`,然后清空 `snapshot_vault_ref`(置 NULL)。
///
/// 幂等性:已迁移的行 `snapshot_vault_ref` 不再以 `{` 开头(为 NULL 或 UUID / "degraded"),
/// SELECT WHERE 子句过滤后不会重复命中。
pub fn migrate_005_compensations_stash(conn: &Connection) -> Result<()> {
    use rusqlite::params;
    let mut stmt = conn.prepare(
        "SELECT comp_id, snapshot_vault_ref FROM compensations WHERE snapshot_vault_ref LIKE '{%'",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    for (comp_id, stash_json) in rows {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stash_json) {
            let compensate_fn = v
                .get("compensate_fn")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            // W3a PoC 把 reverse_payload 以 JSON value(已被 serde_json::to_string
            // 二次转义)存入 stash,这里取出后保留其 JSON 字符串形式。
            let reverse_payload = v
                .get("reverse_payload")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "{}".to_string());
            conn.execute(
                "UPDATE compensations SET reverse_payload = ?1, compensate_fn = ?2, snapshot_vault_ref = NULL WHERE comp_id = ?3",
                params![reverse_payload, compensate_fn, comp_id],
            )?;
        }
    }
    Ok(())
}
