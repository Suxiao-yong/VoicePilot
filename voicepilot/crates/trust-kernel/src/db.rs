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
    tracing::info!("migrations applied");
    Ok(())
}
