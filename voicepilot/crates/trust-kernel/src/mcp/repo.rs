//! MCP server config repo — V1.1 §8.1 mcp_servers table.
//!
//! Stores per-server metadata (name, version, transport, protocol_version)
//! and security config (allowed_origins, allowed_paths). W4 loads
//! allowed_paths at startup and injects into FilesystemTool via
//! new_with_allowed_paths(), resolving spec issue #31.

use crate::allowed_paths::AllowedPaths;
use crate::error::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerRecord {
    pub server_id: String,
    pub name: String,
    pub version: String,
    pub transport: String,
    pub enabled: bool,
    pub trusted: bool,
    pub protocol_version: Option<String>,
    pub allowed_origins: Option<String>, // JSON array, raw text
    pub allowed_paths: Option<String>,   // JSON array, raw text
}

pub struct McpServerRepo;

impl McpServerRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, rec: &McpServerRecord) -> Result<()> {
        conn.execute(
            r#"INSERT INTO mcp_servers
               (server_id, name, version, transport, enabled, trusted,
                protocol_version, allowed_origins, allowed_paths)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                rec.server_id,
                rec.name,
                rec.version,
                rec.transport,
                rec.enabled as i32,
                rec.trusted as i32,
                rec.protocol_version,
                rec.allowed_origins,
                rec.allowed_paths,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, server_id: &str) -> Result<Option<McpServerRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT server_id, name, version, transport, enabled, trusted,
                      protocol_version, allowed_origins, allowed_paths
               FROM mcp_servers WHERE server_id = ?1"#,
        )?;
        let mut rows = stmt.query(params![server_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list(&self, conn: &Connection) -> Result<Vec<McpServerRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT server_id, name, version, transport, enabled, trusted,
                      protocol_version, allowed_origins, allowed_paths
               FROM mcp_servers ORDER BY server_id"#,
        )?;
        let records = stmt
            .query_map([], row_to_record)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn update(&self, conn: &Connection, rec: &McpServerRecord) -> Result<()> {
        conn.execute(
            r#"UPDATE mcp_servers SET
                 name = ?2, version = ?3, transport = ?4, enabled = ?5,
                 trusted = ?6, protocol_version = ?7, allowed_origins = ?8,
                 allowed_paths = ?9
               WHERE server_id = ?1"#,
            params![
                rec.server_id,
                rec.name,
                rec.version,
                rec.transport,
                rec.enabled as i32,
                rec.trusted as i32,
                rec.protocol_version,
                rec.allowed_origins,
                rec.allowed_paths,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, conn: &Connection, server_id: &str) -> Result<()> {
        conn.execute(r#"DELETE FROM mcp_servers WHERE server_id = ?1"#, params![server_id])?;
        Ok(())
    }

    /// Load and parse the allowed_paths JSON column for a server.
    /// Returns Ok(None) if the server has no allowed_paths configured
    /// (caller may treat as "no whitelist enforced").
    /// Returns Err on missing server with invalid JSON.
    pub fn load_allowed_paths(
        &self,
        conn: &Connection,
        server_id: &str,
    ) -> Result<Option<AllowedPaths>> {
        let rec = match self.get(conn, server_id)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let json_str = match rec.allowed_paths {
            Some(s) => s,
            None => return Ok(None),
        };
        let roots: Vec<String> = serde_json::from_str(&json_str)?;
        if roots.is_empty() {
            return Ok(None);
        }
        Ok(Some(AllowedPaths::new(roots)))
    }

    /// Ensure the builtin voicepilot-filesystem server row exists.
    /// Idempotent — does not overwrite an existing row (preserves user
    /// customizations to allowed_paths).
    pub fn seed_builtin_filesystem(&self, conn: &Connection) -> Result<()> {
        if self.get(conn, "voicepilot-filesystem")?.is_some() {
            return Ok(());
        }
        let default_paths = serde_json::json!(["C:/Users", "D:/"]).to_string();
        let rec = McpServerRecord {
            server_id: "voicepilot-filesystem".to_string(),
            name: "VoicePilot Filesystem".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            transport: "stdio".to_string(),
            enabled: true,
            trusted: true,
            protocol_version: Some("2025-11-25".to_string()),
            allowed_origins: None,
            allowed_paths: Some(default_paths),
        };
        self.create(conn, &rec)
    }
}

impl Default for McpServerRepo {
    fn default() -> Self {
        Self::new()
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<McpServerRecord> {
    Ok(McpServerRecord {
        server_id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        transport: row.get(3)?,
        enabled: row.get::<_, i32>(4)? != 0,
        trusted: row.get::<_, i32>(5)? != 0,
        protocol_version: row.get(6)?,
        allowed_origins: row.get(7)?,
        allowed_paths: row.get(8)?,
    })
}
