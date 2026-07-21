//! SQLite connection + migration runner.
//!
//! Migrations are embedded SQL files under `src/migrations/`.
//! Each migration is idempotent (`CREATE TABLE IF NOT EXISTS`).

use crate::error::Result;
use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("migrations/001_init.sql");
const MIGRATION_002: &str = include_str!("migrations/002_app_config.sql");

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
    tracing::info!("migrations applied");
    Ok(())
}
