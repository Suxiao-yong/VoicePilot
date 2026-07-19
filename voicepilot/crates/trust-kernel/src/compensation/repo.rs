//! Compensation repository — V1.1 §8.1 `compensations` table.

use crate::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use crate::error::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone, Default)]
pub struct CompensationRepo;

impl CompensationRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, rec: &CompensationRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO compensations
                (comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                 compensation_level, snapshot_vault_ref, conflict_policy)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?3, ?7, ?8)",
            params![
                rec.comp_id,
                rec.step_id,
                rec.level.as_str(),
                rec.snapshot_encrypted,
                rec.ttl_expires,
                rec.status,
                rec.snapshot_vault_ref,
                rec.conflict_policy.as_str(),
            ],
        )?;
        // Note: compensate_fn + reverse_payload are stored in the snapshot_encrypted
        // blob's JSON for W3a (avoids schema migration). W8 will add explicit columns
        // when wiring tauri-plugin-stronghold.
        // For W3a we stash them as a separate JSON in snapshot_vault_ref's place
        // when vault_ref is None. This is a PoC shortcut — clearly documented.
        if rec.snapshot_vault_ref.is_none() {
            conn.execute(
                "UPDATE compensations SET snapshot_vault_ref = ?1 WHERE comp_id = ?2",
                params![
                    format!(
                        "{{\"compensate_fn\":\"{}\",\"reverse_payload\":{}}}",
                        rec.compensate_fn, rec.reverse_payload
                    ),
                    rec.comp_id
                ],
            )?;
        }
        Ok(())
    }

    pub fn get(&self, conn: &Connection, comp_id: &str) -> Result<Option<CompensationRecord>> {
        let mut stmt = conn.prepare(
            "SELECT comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                    snapshot_vault_ref, conflict_policy
             FROM compensations WHERE comp_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![comp_id], |r| {
            let comp_id: String = r.get(0)?;
            let step_id: String = r.get(1)?;
            let level: String = r.get(2)?;
            let snapshot_encrypted: Option<Vec<u8>> = r.get(3)?;
            let ttl_expires: String = r.get(4)?;
            let status: String = r.get(5)?;
            let snapshot_vault_ref: Option<String> = r.get(6)?;
            let conflict_policy: String = r.get(7)?;
            Ok((
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (comp_id, step_id, level_str, snapshot_encrypted, ttl_expires, status,
                 snapshot_vault_ref, conflict_policy_str) = row_result?;
            let level = CompensationLevel::parse(&level_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid level: {}", level_str)))?;
            let conflict_policy = ConflictPolicy::parse(&conflict_policy_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid conflict_policy: {}", conflict_policy_str)))?;

            // Parse compensate_fn + reverse_payload from snapshot_vault_ref PoC stash.
            let (compensate_fn, reverse_payload) = parse_poc_payload(&snapshot_vault_ref);

            Ok(Some(CompensationRecord {
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy, compensate_fn, reverse_payload,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn mark_status(&self, conn: &Connection, comp_id: &str, new_status: &str) -> Result<()> {
        conn.execute(
            "UPDATE compensations SET status = ?1 WHERE comp_id = ?2",
            params![new_status, comp_id],
        )?;
        Ok(())
    }

    pub fn list_active(&self, conn: &Connection) -> Result<Vec<CompensationRecord>> {
        let mut stmt = conn.prepare(
            "SELECT comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                    snapshot_vault_ref, conflict_policy
             FROM compensations WHERE status = 'active' ORDER BY ttl_expires",
        )?;
        let rows = stmt.query_map([], |r| {
            let comp_id: String = r.get(0)?;
            let step_id: String = r.get(1)?;
            let level: String = r.get(2)?;
            let snapshot_encrypted: Option<Vec<u8>> = r.get(3)?;
            let ttl_expires: String = r.get(4)?;
            let status: String = r.get(5)?;
            let snapshot_vault_ref: Option<String> = r.get(6)?;
            let conflict_policy: String = r.get(7)?;
            Ok((
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (comp_id, step_id, level_str, snapshot_encrypted, ttl_expires, status,
                 snapshot_vault_ref, conflict_policy_str) = row_result?;
            let level = CompensationLevel::parse(&level_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid level: {}", level_str)))?;
            let conflict_policy = ConflictPolicy::parse(&conflict_policy_str)
                .ok_or_else(|| crate::error::KernelError::Compensation(format!("invalid conflict_policy: {}", conflict_policy_str)))?;
            let (compensate_fn, reverse_payload) = parse_poc_payload(&snapshot_vault_ref);
            out.push(CompensationRecord {
                comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                snapshot_vault_ref, conflict_policy, compensate_fn, reverse_payload,
            });
        }
        Ok(out)
    }
}

/// Parse the PoC JSON stash from snapshot_vault_ref.
/// Returns (compensate_fn, reverse_payload) or ("", "{}") if not parseable.
fn parse_poc_payload(stash: &Option<String>) -> (String, String) {
    match stash {
        Some(s) if s.starts_with('{') => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                let fn_name = v.get("compensate_fn").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let payload = v.get("reverse_payload").map(|x| x.to_string()).unwrap_or_else(|| "{}".to_string());
                (fn_name, payload)
            } else {
                ("".to_string(), "{}".to_string())
            }
        }
        _ => ("".to_string(), "{}".to_string()),
    }
}
