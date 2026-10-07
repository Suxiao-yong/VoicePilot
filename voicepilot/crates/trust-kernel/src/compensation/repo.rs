//! Compensation repository — V1.1 §8.1 `compensations` table.
//!
//! W9 Plan 2: `reverse_payload` + `compensate_fn` 现在是真实列(migration 005 落实),
//! 不再用 W3a PoC stash(把 JSON 塞进 `snapshot_vault_ref` 列)。

use crate::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use crate::error::Result;
use rusqlite::{Connection, params};

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
                 compensation_level, snapshot_vault_ref, conflict_policy,
                 reverse_payload, compensate_fn)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?3, ?7, ?8, ?9, ?10)",
            params![
                rec.comp_id,
                rec.step_id,
                rec.level.as_str(),
                rec.snapshot_encrypted,
                rec.ttl_expires,
                rec.status,
                rec.snapshot_vault_ref,
                rec.conflict_policy.as_str(),
                rec.reverse_payload,
                rec.compensate_fn,
            ],
        )?;
        // W9 Plan 2:移除 W3a PoC stash(reverse_payload + compensate_fn 现在是真实列,
        // 由 migration 005 落实)。spec §2.2 明文残留检测 SQL `WHERE reverse_payload != ''`
        // 现在可以直接执行。
        Ok(())
    }

    pub fn get(&self, conn: &Connection, comp_id: &str) -> Result<Option<CompensationRecord>> {
        let mut stmt = conn.prepare(
            "SELECT comp_id, step_id, level, snapshot_encrypted, ttl_expires, status,
                    snapshot_vault_ref, conflict_policy, reverse_payload, compensate_fn
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
            // W9 Plan 2: 从真实列读取(migration 005 之前可能为 NULL,
            // 用 Option<String> 兜底再 unwrap_or_default)。
            let reverse_payload: String = r.get::<_, Option<String>>(8)?.unwrap_or_default();
            let compensate_fn: String = r.get::<_, Option<String>>(9)?.unwrap_or_default();
            Ok((
                comp_id,
                step_id,
                level,
                snapshot_encrypted,
                ttl_expires,
                status,
                snapshot_vault_ref,
                conflict_policy,
                reverse_payload,
                compensate_fn,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (
                comp_id,
                step_id,
                level_str,
                snapshot_encrypted,
                ttl_expires,
                status,
                snapshot_vault_ref,
                conflict_policy_str,
                reverse_payload,
                compensate_fn,
            ) = row_result?;
            let level = CompensationLevel::parse(&level_str).ok_or_else(|| {
                crate::error::KernelError::Compensation(format!("invalid level: {}", level_str))
            })?;
            let conflict_policy = ConflictPolicy::parse(&conflict_policy_str).ok_or_else(|| {
                crate::error::KernelError::Compensation(format!(
                    "invalid conflict_policy: {}",
                    conflict_policy_str
                ))
            })?;

            // W9 Plan 2: 直接用真实列,不再调 parse_poc_payload。
            Ok(Some(CompensationRecord {
                comp_id,
                step_id,
                level,
                snapshot_encrypted,
                ttl_expires,
                status,
                snapshot_vault_ref,
                conflict_policy,
                compensate_fn,
                reverse_payload,
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
                    snapshot_vault_ref, conflict_policy, reverse_payload, compensate_fn
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
            let reverse_payload: String = r.get::<_, Option<String>>(8)?.unwrap_or_default();
            let compensate_fn: String = r.get::<_, Option<String>>(9)?.unwrap_or_default();
            Ok((
                comp_id,
                step_id,
                level,
                snapshot_encrypted,
                ttl_expires,
                status,
                snapshot_vault_ref,
                conflict_policy,
                reverse_payload,
                compensate_fn,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (
                comp_id,
                step_id,
                level_str,
                snapshot_encrypted,
                ttl_expires,
                status,
                snapshot_vault_ref,
                conflict_policy_str,
                reverse_payload,
                compensate_fn,
            ) = row_result?;
            let level = CompensationLevel::parse(&level_str).ok_or_else(|| {
                crate::error::KernelError::Compensation(format!("invalid level: {}", level_str))
            })?;
            let conflict_policy = ConflictPolicy::parse(&conflict_policy_str).ok_or_else(|| {
                crate::error::KernelError::Compensation(format!(
                    "invalid conflict_policy: {}",
                    conflict_policy_str
                ))
            })?;
            // W9 Plan 2: 直接用真实列,不再调 parse_poc_payload。
            out.push(CompensationRecord {
                comp_id,
                step_id,
                level,
                snapshot_encrypted,
                ttl_expires,
                status,
                snapshot_vault_ref,
                conflict_policy,
                compensate_fn,
                reverse_payload,
            });
        }
        Ok(out)
    }
}
