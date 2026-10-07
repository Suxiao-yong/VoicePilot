//! MCP server config repo — V1.1 §8.1 mcp_servers table.
//!
//! Stores per-server metadata (name, version, transport, protocol_version)
//! and security config (allowed_origins, allowed_paths). W4 loads
//! allowed_paths at startup and injects into FilesystemTool via
//! new_with_allowed_paths(), resolving spec issue #31.
//!
//! W7 Plan 5 Task 0: extended with `command` / `args` / `env` columns to
//! support spawning external MCP server subprocesses (e.g.
//! `npx @playwright/mcp@latest`). In-process servers (e.g. the builtin
//! voicepilot-filesystem) leave these as `None`.

use crate::allowed_paths::AllowedPaths;
use crate::error::{KernelError, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

/// Transport allowlist for registered MCP plugins (Wave 2 Task 2.2).
/// Only `stdio` is currently supported; extending the protocol means adding
/// an entry here — an explicit allowlist, never a free-form string.
pub const MCP_TRANSPORT_ALLOWLIST: &[&str] = &["stdio"];

/// The locked MCP protocol version (V1.1 §8.1 `mcp_servers.protocol_version`).
/// A registered plugin's `protocol_version`, when present, must equal this
/// exactly — the same value every seeded server row uses.
pub const MCP_PROTOCOL_VERSION_LOCKED: &str = "2025-11-25";

/// The existing `mcp_servers` row is the single source of truth for MCP
/// configuration; do not duplicate it in a separate extension/plugin table.
/// Environment values are configuration secrets and must never be emitted in
/// audit or log output.
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
    /// W7 Plan 5: spawn command (e.g. "npx"). `None` for in-process servers.
    pub command: Option<String>,
    /// W7 Plan 5: JSON array of args (e.g. `["-y","@playwright/mcp@latest"]`).
    pub args: Option<String>,
    /// W7 Plan 5: JSON object of env overrides (e.g. `{"FOO":"bar"}`).
    /// Values must never be emitted into audit or log output.
    pub env: Option<String>,
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
                protocol_version, allowed_origins, allowed_paths,
                command, args, env)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"#,
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
                rec.command,
                rec.args,
                rec.env,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, server_id: &str) -> Result<Option<McpServerRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT server_id, name, version, transport, enabled, trusted,
                      protocol_version, allowed_origins, allowed_paths,
                      command, args, env
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
                      protocol_version, allowed_origins, allowed_paths,
                      command, args, env
               FROM mcp_servers ORDER BY server_id"#,
        )?;
        let records = stmt
            .query_map([], row_to_record)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// 切换 MCP Server 启用状态(V1.1.2 §8.3 Trust Center 一键停用)。
    /// 若 server_id 不存在,SQLite UPDATE 0 行受影响,不报错(noop)。
    pub fn toggle_enabled(&self, conn: &Connection, server_id: &str, enabled: bool) -> Result<()> {
        conn.execute(
            "UPDATE mcp_servers SET enabled = ?1 WHERE server_id = ?2",
            params![enabled, server_id],
        )?;
        Ok(())
    }

    pub fn update(&self, conn: &Connection, rec: &McpServerRecord) -> Result<()> {
        conn.execute(
            r#"UPDATE mcp_servers SET
                 name = ?2, version = ?3, transport = ?4, enabled = ?5,
                 trusted = ?6, protocol_version = ?7, allowed_origins = ?8,
                 allowed_paths = ?9, command = ?10, args = ?11, env = ?12
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
                rec.command,
                rec.args,
                rec.env,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, conn: &Connection, server_id: &str) -> Result<()> {
        conn.execute(
            r#"DELETE FROM mcp_servers WHERE server_id = ?1"#,
            params![server_id],
        )?;
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
    ///
    /// W7 Plan 5: `command` / `args` / `env` are `None` because the
    /// voicepilot-filesystem server runs in-process (no subprocess).
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
            command: None,
            args: None,
            env: None,
        };
        self.create(conn, &rec)
    }

    /// Ensure the default `playwright` MCP server row exists.
    /// Idempotent — does not overwrite an existing row (preserves user
    /// customizations such as `enabled = false` from Settings Trust Center).
    ///
    /// W7 Plan 5 Task 1: per spec §2.7, inserts the spawn spec
    /// `npx -y @playwright/mcp@latest`. `allowed_paths` is `[]` (empty)
    /// because Playwright MCP must NOT directly access the filesystem —
    /// all write operations go through `filesystem.write` (V1.1 §4.4).
    ///
    /// Plan deviation: the plan text says "if mcp_servers table is empty",
    /// but `seed_builtin_filesystem` always inserts `voicepilot-filesystem`
    /// first, so the table is never empty when this runs. The right
    /// behavior is "if playwright row does not exist" — matches the
    /// `seed_builtin_filesystem` pattern and preserves user customizations.
    pub fn insert_default_servers(&self, conn: &Connection) -> Result<()> {
        if self.get(conn, "playwright")?.is_none() {
            let rec = McpServerRecord {
                server_id: "playwright".to_string(),
                name: "Playwright MCP".to_string(),
                version: "latest".to_string(),
                transport: "stdio".to_string(),
                enabled: true,
                trusted: false,
                protocol_version: Some("2025-11-25".to_string()),
                allowed_origins: None,
                // Empty array — Playwright MCP has no filesystem access.
                allowed_paths: Some("[]".to_string()),
                command: Some("npx".to_string()),
                args: Some(r#"["-y","@playwright/mcp@latest"]"#.to_string()),
                env: Some("{}".to_string()),
            };
            self.create(conn, &rec)?;
        }
        // UIA backend (sbroenne/mcp-windows, MIT): the `McpUiaAdapter`
        // spawns this command per call. The exe is NOT bundled — download
        // `Sbroenne.WindowsMcp.exe` (releases) onto PATH, or edit this row
        // in Trust Center with the full path. Missing exe fails closed at
        // spawn with a readable error; never silently.
        if self.get(conn, MCP_WINDOWS_SERVER_ID)?.is_none() {
            let rec = McpServerRecord {
                server_id: MCP_WINDOWS_SERVER_ID.to_string(),
                name: "Windows UI Automation (mcp-windows)".to_string(),
                version: "1.3.22".to_string(),
                transport: "stdio".to_string(),
                enabled: true,
                trusted: false,
                protocol_version: Some("2025-11-25".to_string()),
                allowed_origins: None,
                allowed_paths: Some("[]".to_string()),
                command: Some("Sbroenne.WindowsMcp.exe".to_string()),
                args: Some("[]".to_string()),
                env: Some("{}".to_string()),
            };
            self.create(conn, &rec)?;
        }
        Ok(())
    }
}

/// `mcp_servers.server_id` convention for the bundled UIA backend
/// (sbroenne/mcp-windows). Single source of truth for the adapter,
/// seed, docs, and setup-hint error strings.
pub const MCP_WINDOWS_SERVER_ID: &str = "mcp-windows";

impl McpServerRepo {
    /// Resolve a stdio spawn spec `(command, args, env)` for `server_id`.
    ///
    /// Shared by `invoke_mcp_tool`, the dispatcher MCP arm, and the UIA
    /// adapter so the lookup + error wording exists exactly once.
    /// Error strings are pinned by existing tests — keep them verbatim.
    pub fn spawn_config(
        &self,
        conn: &Connection,
        server_id: &str,
    ) -> Result<(String, Vec<String>, serde_json::Value)> {
        let rec = self.get(conn, server_id)?.ok_or_else(|| {
            KernelError::Mcp(format!(
                "MCP server '{server_id}' not found in mcp_servers table"
            ))
        })?;
        if !rec.enabled {
            return Err(KernelError::Mcp(format!(
                "MCP server '{server_id}' is disabled"
            )));
        }
        let command = rec.command.clone().ok_or_else(|| {
            KernelError::Mcp(format!("MCP server '{server_id}' missing command field"))
        })?;
        let args_vec: Vec<String> = serde_json::from_str(
            rec.args
                .clone()
                .unwrap_or_else(|| "[]".to_string())
                .as_str(),
        )
        .map_err(|e| KernelError::Mcp(format!("MCP server '{server_id}' args parse error: {e}")))?;
        let env_json: serde_json::Value =
            serde_json::from_str(rec.env.clone().unwrap_or_else(|| "{}".to_string()).as_str())
                .map_err(|e| {
                    KernelError::Mcp(format!("MCP server '{server_id}' env parse error: {e}"))
                })?;
        Ok((command, args_vec, env_json))
    }
}

impl Default for McpServerRepo {
    fn default() -> Self {
        Self::new()
    }
}

/// Wave 2 Task 2.2: validate a plugin record before it is written to
/// `mcp_servers`. Called by `TrustKernel::register_mcp_server` before any
/// write — failure leaves the DB and the runtime catalog untouched.
///
/// Rules:
/// - `server_id` / `name` / `version` / `transport` non-blank
/// - `transport` in `MCP_TRANSPORT_ALLOWLIST` ("stdio" only)
/// - `protocol_version`, when present, must be exactly
///   `MCP_PROTOCOL_VERSION_LOCKED` ("2025-11-25")
/// - `command` required for stdio spawn servers (non-blank)
/// - `args` must parse as a JSON array of strings
/// - `env` must parse as a JSON string→string object
/// - `allowed_paths` / `allowed_origins` must parse as JSON string arrays
///
/// Security: env values are configuration secrets. This function never logs
/// the record and never writes audit events, so env never appears in audit or
/// log output (redaction is by omission — nothing here emits env).
pub fn validate_mcp_server_record(rec: &McpServerRecord) -> Result<()> {
    if rec.server_id.trim().is_empty() {
        return Err(KernelError::Mcp("server_id must not be blank".to_string()));
    }
    if rec.name.trim().is_empty() {
        return Err(KernelError::Mcp("name must not be blank".to_string()));
    }
    if rec.version.trim().is_empty() {
        return Err(KernelError::Mcp("version must not be blank".to_string()));
    }
    if rec.transport.trim().is_empty() {
        return Err(KernelError::Mcp("transport must not be blank".to_string()));
    }
    if !MCP_TRANSPORT_ALLOWLIST.contains(&rec.transport.as_str()) {
        return Err(KernelError::Mcp(format!(
            "MCP server '{}' uses unsupported transport '{}' (allowlist: {})",
            rec.server_id,
            rec.transport,
            MCP_TRANSPORT_ALLOWLIST.join(", "),
        )));
    }
    if let Some(protocol_version) = &rec.protocol_version {
        if protocol_version != MCP_PROTOCOL_VERSION_LOCKED {
            return Err(KernelError::Mcp(format!(
                "MCP server '{}' protocol_version '{}' does not match locked value '{}'",
                rec.server_id, protocol_version, MCP_PROTOCOL_VERSION_LOCKED
            )));
        }
    }
    if rec
        .command
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        return Err(KernelError::Mcp(format!(
            "MCP server '{}' missing command field (required for stdio spawn servers)",
            rec.server_id
        )));
    }
    if let Some(raw) = &rec.args {
        if serde_json::from_str::<Vec<String>>(raw).is_err() {
            return Err(KernelError::Mcp(format!(
                "MCP server '{}' args must be a JSON array of strings",
                rec.server_id
            )));
        }
    }
    if let Some(raw) = &rec.env {
        if serde_json::from_str::<std::collections::HashMap<String, String>>(raw).is_err() {
            return Err(KernelError::Mcp(format!(
                "MCP server '{}' env must be a JSON object of string→string values",
                rec.server_id
            )));
        }
    }
    if let Some(raw) = &rec.allowed_paths {
        if serde_json::from_str::<Vec<String>>(raw).is_err() {
            return Err(KernelError::Mcp(format!(
                "MCP server '{}' allowed_paths must be a JSON array of strings",
                rec.server_id
            )));
        }
    }
    if let Some(raw) = &rec.allowed_origins {
        if serde_json::from_str::<Vec<String>>(raw).is_err() {
            return Err(KernelError::Mcp(format!(
                "MCP server '{}' allowed_origins must be a JSON array of strings",
                rec.server_id
            )));
        }
    }
    Ok(())
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
        command: row.get(9)?,
        args: row.get(10)?,
        env: row.get(11)?,
    })
}
