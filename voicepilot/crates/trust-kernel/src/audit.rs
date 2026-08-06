//! Audit log writer with hash chain — tamper-evident event record.
//!
//! Each event's hash = SHA256(prev_hash || canonical_json(event_fields)).
//! V1.1 §1.4 requires 100% audit coverage for every tool call.
//!
//! The logger shares a single `Arc<Mutex<Connection>>` with the rest of the
//! kernel so that audit events see the same DB state as task/step writes.

use crate::error::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub log_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub details: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Hash of the previous event in the same task (None for the first event).
    /// If `None` on append, the logger auto-resolves the last hash for the task.
    pub prev_hash: Option<String>,
    /// Computed by the logger on append. Empty when caller constructs the event.
    pub hash: String,
}

pub trait AuditLogger: Send + Sync {
    fn append(&self, event: &AuditEvent) -> Result<()>;

    /// 列出最近的 N 条审计事件(按 timestamp 降序)。
    fn list_recent(&self, limit: usize) -> Result<Vec<AuditEvent>>;

    /// 列出某任务的所有审计事件(按 timestamp 升序)。
    fn list_for_task(&self, task_id: &str) -> Result<Vec<AuditEvent>>;
}

/// SQLite-backed audit logger. Shares its connection with the kernel via
/// `Arc<Mutex<Connection>>` so audit writes are visible to subsequent reads
/// in the same transaction sequence.
pub struct SqliteAuditLogger {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteAuditLogger {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Look up the most recent hash for a given task, used to chain the next event.
    fn last_hash_for_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
        let hash: Option<String> = conn
            .query_row(
                "SELECT hash FROM audit_logs WHERE task_id=?1 ORDER BY timestamp DESC, log_id DESC LIMIT 1",
                params![task_id],
                |r| r.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(hash)
    }

    /// Compute SHA256(prev_hash || canonical_json(payload)).
    fn compute_hash(event: &AuditEvent) -> String {
        let mut hasher = Sha256::new();
        if let Some(prev) = &event.prev_hash {
            hasher.update(prev.as_bytes());
        }
        // Canonical JSON: stable key order, no whitespace.
        let payload = serde_json::json!({
            "log_id": event.log_id,
            "task_id": event.task_id,
            "step_id": event.step_id,
            "event_type": event.event_type,
            "details": event.details,
            "timestamp": event.timestamp.to_rfc3339(),
        });
        let canonical = serde_json::to_string(&payload).unwrap_or_default();
        hasher.update(canonical.as_bytes());
        let digest = hasher.finalize();
        format!("{:x}", digest)
    }

    /// Test helper: query (hash, prev_hash) rows for a task.
    pub fn query_rows(
        &self,
        sql: &str,
        task_id: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Test helper: run a query that returns a single Optional<String> column.
    pub fn query_single(&self, sql: &str, param: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let value: Option<String> = conn
            .query_row(sql, params![param], |r| r.get(0))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(value)
    }
}

impl AuditLogger for SqliteAuditLogger {
    fn append(&self, event: &AuditEvent) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Resolve prev_hash if caller didn't supply one (auto-chain).
        let prev_hash = match &event.prev_hash {
            Some(h) => Some(h.clone()),
            None => Self::last_hash_for_task(&conn, &event.task_id)?,
        };
        let mut to_write = event.clone();
        to_write.prev_hash = prev_hash;
        to_write.hash = Self::compute_hash(&to_write);

        conn.execute(
            "INSERT INTO audit_logs
                (log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                to_write.log_id,
                to_write.task_id,
                to_write.step_id,
                to_write.event_type,
                serde_json::to_string(&to_write.details)?,
                to_write.timestamp.to_rfc3339(),
                to_write.prev_hash,
                to_write.hash,
            ],
        )?;
        Ok(())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().expect("conn poisoned");
        let mut stmt = conn.prepare(
            "SELECT log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash
             FROM audit_logs ORDER BY timestamp DESC, log_id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let details_str: String = row.get(4)?;
            let ts_str: String = row.get(5)?;
            Ok(AuditEvent {
                log_id: row.get(0)?,
                task_id: row.get(1)?,
                step_id: row.get(2)?,
                event_type: row.get(3)?,
                details: serde_json::from_str(&details_str).unwrap_or(serde_json::Value::Null),
                timestamp: chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                prev_hash: row.get(6)?,
                hash: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn list_for_task(&self, task_id: &str) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock().expect("conn poisoned");
        let mut stmt = conn.prepare(
            "SELECT log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash
             FROM audit_logs WHERE task_id = ?1 ORDER BY timestamp ASC, log_id ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| {
            let details_str: String = row.get(4)?;
            let ts_str: String = row.get(5)?;
            Ok(AuditEvent {
                log_id: row.get(0)?,
                task_id: row.get(1)?,
                step_id: row.get(2)?,
                event_type: row.get(3)?,
                details: serde_json::from_str(&details_str).unwrap_or(serde_json::Value::Null),
                timestamp: chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                prev_hash: row.get(6)?,
                hash: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

// ===== W10 Plan 5: AUDIT_EVENT_TYPE_REGISTRY + is_valid_event_type =====

/// W10 Plan 5: 审计事件类型注册表(spec §7.2)。
///
/// 基于 Grep 核对的全部实际 emit callsite,共 28 种:
/// - W1-W3 基础(kernel.rs):9 种
/// - W4 MCP(mcp/server.rs):1 种
/// - W8 DAG + LLM(dag_executor.rs, llm/client.rs, task_explain.rs):10 种
/// - W9 Stronghold / Taint(kernel.rs, common.rs, task_compensate.rs, gateway.rs, dispatcher.rs, mcp/server.rs):5 种
/// - W10 新增(Plan 3 voice_started + Plan 4 kill_switch_triggered / task_cancelled):3 种
///
/// **v2 修订 #11 修正:** spec §7.1 列出 27 种,但遗漏了 W8 Plan 3 在
/// `task_explain.rs:219` 新增的 `llm_explain_called`。本 registry 补入,共 28 种。
///
/// 运行时校验:`kernel.rs::audit_append` 调用 `is_valid_event_type` 校验,
/// 若无效则 `tracing::warn!` 但继续写入(不阻塞,spec §7.2 v2 修订 #3)。
pub const AUDIT_EVENT_TYPE_REGISTRY: &[&str] = &[
    // W1-W3 基础(kernel.rs)
    "task_created",
    "state_transition",
    "step_created",
    "step_status_changed",
    "step_prepared",
    "step_committed",
    "compensation_created",
    "compensation_status_changed",
    "approval_recorded",
    // W4 MCP(mcp/server.rs)
    "mcp_tools_call",
    // W8 DAG + LLM(dag_executor.rs, llm/client.rs, task_explain.rs)
    "llm_decompose_called",
    "llm_explain_called",
    "dag_plan_created",
    "dag_skeleton_approved",
    "dag_skeleton_modified",
    "dag_modify_limit_exceeded",
    "dag_node_started",
    "dag_node_succeeded",
    "dag_node_failed",
    "dag_completed",
    // W9 Stronghold / Taint
    "stronghold_degraded_mode_entered",
    "stronghold_snapshot_encrypted",
    "stronghold_snapshot_decrypt_failed",
    "taint_propagated",
    "taint_blocked",
    // W10 新增(Plan 3/4)
    "voice_started",
    "kill_switch_triggered",
    "task_cancelled",
];

/// W10 Plan 5: 校验 event_type 是否在 AUDIT_EVENT_TYPE_REGISTRY 中(spec §7.2)。
///
/// 供 `kernel.rs::audit_append` 在构造 AuditEvent 前调用。若返回 false,
/// caller 应 `tracing::warn!` 但继续写入(不返回 Err,避免回归现有 callsite)。
pub fn is_valid_event_type(name: &str) -> bool {
    AUDIT_EVENT_TYPE_REGISTRY.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_28_event_types() {
        assert_eq!(
            AUDIT_EVENT_TYPE_REGISTRY.len(),
            28,
            "registry must contain exactly 28 event types (spec §7.1 + llm_explain_called)"
        );
    }

    #[test]
    fn registry_has_no_duplicates() {
        let mut sorted = AUDIT_EVENT_TYPE_REGISTRY.to_vec();
        sorted.sort();
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(
            sorted.len(),
            deduped.len(),
            "registry must not contain duplicate event types"
        );
    }

    #[test]
    fn is_valid_event_type_recognizes_known_events() {
        assert!(is_valid_event_type("task_created"));
        assert!(is_valid_event_type("state_transition"));
        assert!(is_valid_event_type("voice_started"));
        assert!(is_valid_event_type("kill_switch_triggered"));
        assert!(is_valid_event_type("task_cancelled"));
        assert!(is_valid_event_type("llm_explain_called"));
    }

    #[test]
    fn is_valid_event_type_rejects_unknown_events() {
        assert!(!is_valid_event_type("unknown_event"));
        assert!(!is_valid_event_type(""));
        assert!(!is_valid_event_type("TASK_CREATED")); // SCREAMING_SNAKE_CASE 不应匹配
        assert!(!is_valid_event_type("task_created ")); // 带空格
    }
}
