//! SkillRepo —— V1.1.2 §8.3 Skills Manager 后端。
//!
//! 持久化 Skill manifest + 成功统计(success_count + avg_latency_ms)。
//! incr_success 用增量平均:avg = (avg * n + new) / (n + 1)。
//!
//! **注意:** `skills` 表已在 `migrations/001_init.sql` 创建,
//! 不需要新建迁移。DB schema `version INTEGER`,所以 SkillRecord.version 是 i64。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub skill_id: String,
    pub version: i64,
    pub manifest_json: String,
    pub enabled: bool,
    pub success_count: i64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRepo;

impl SkillRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn upsert(&self, conn: &Connection, rec: &SkillRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO skills (skill_id, version, manifest_json, enabled, success_count, avg_latency_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(skill_id) DO UPDATE SET
               version = excluded.version,
               manifest_json = excluded.manifest_json",
            params![rec.skill_id, rec.version, rec.manifest_json, rec.enabled, rec.success_count, rec.avg_latency_ms],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, skill_id: &str) -> Result<Option<SkillRecord>> {
        let mut stmt = conn.prepare(
            "SELECT skill_id, version, manifest_json, enabled, success_count, avg_latency_ms FROM skills WHERE skill_id = ?1",
        )?;
        let rec = stmt
            .query_row(params![skill_id], |row| {
                Ok(SkillRecord {
                    skill_id: row.get(0)?,
                    version: row.get(1)?,
                    manifest_json: row.get(2)?,
                    enabled: row.get(3)?,
                    success_count: row.get(4)?,
                    avg_latency_ms: row.get(5)?,
                })
            })
            .optional()?;
        Ok(rec)
    }

    pub fn list(&self, conn: &Connection) -> Result<Vec<SkillRecord>> {
        let mut stmt = conn.prepare(
            "SELECT skill_id, version, manifest_json, enabled, success_count, avg_latency_ms FROM skills ORDER BY skill_id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillRecord {
                skill_id: row.get(0)?,
                version: row.get(1)?,
                manifest_json: row.get(2)?,
                enabled: row.get(3)?,
                success_count: row.get(4)?,
                avg_latency_ms: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn toggle(&self, conn: &Connection, skill_id: &str, enabled: bool) -> Result<()> {
        conn.execute(
            "UPDATE skills SET enabled = ?1 WHERE skill_id = ?2",
            params![enabled, skill_id],
        )?;
        Ok(())
    }

    /// 记录一次成功执行,增量更新 success_count 与 avg_latency_ms。
    pub fn incr_success(&self, conn: &Connection, skill_id: &str, latency_ms: f64) -> Result<()> {
        conn.execute(
            "UPDATE skills
             SET success_count = success_count + 1,
                 avg_latency_ms = (avg_latency_ms * success_count + ?1) / (success_count + 1)
             WHERE skill_id = ?2",
            params![latency_ms, skill_id],
        )?;
        Ok(())
    }
}
