//! classify 结果缓存：同模型 + 同 schema 版本 + 归一化文本 → 高置信 Skill 命中。
//!
//! 不调模型最省：重复指令（如“打开记事本”）一次命中省掉整包云端往返。
//! 归一化只做空白折叠 + 小写（中文不受影响）；槽位原样存取，不参与 key。

use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

/// 缓存 TTL：24h。候选清单/模型变化由 key 隔离（snapshot_id 不进 key，
/// 因为 skill 上下线会改变同一文本的正确路由 —— 误命中比 miss 更贵，
/// 跨清单复用一律不做）。
pub const ROUTE_CACHE_TTL_DAYS: u32 = 1;

#[derive(Debug, Clone)]
pub struct CachedRoute {
    pub skill_id: String,
    pub slots_json: String,
    pub confidence: f32,
}

pub fn normalize_text(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub fn route_cache_key(model: &str, schema_version: u32, text: &str) -> String {
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0u8]);
    h.update(schema_version.to_le_bytes());
    h.update([0u8]);
    h.update(normalize_text(text).as_bytes());
    format!("{:x}", h.finalize())
}

/// 命中且未过期才返回 Some；过期行视为 miss（由 prune 异步清理）。
pub fn lookup(conn: &Connection, key: &str, now_ms: i64) -> Result<Option<CachedRoute>> {
    let mut stmt = conn.prepare(
        "SELECT skill_id, slots_json, confidence FROM llm_route_cache
         WHERE cache_key = ?1 AND expires_at_ms > ?2",
    )?;
    let row = stmt
        .query_row(params![key, now_ms], |row| {
            Ok(CachedRoute {
                skill_id: row.get(0)?,
                slots_json: row.get(1)?,
                confidence: row.get(2)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn store(
    conn: &Connection,
    key: &str,
    skill_id: &str,
    slots_json: &str,
    confidence: f32,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO llm_route_cache
         (cache_key, skill_id, slots_json, confidence, created_at_ms, expires_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(cache_key) DO UPDATE SET
           skill_id = excluded.skill_id, slots_json = excluded.slots_json,
           confidence = excluded.confidence, created_at_ms = excluded.created_at_ms,
           expires_at_ms = excluded.expires_at_ms",
        params![
            key,
            skill_id,
            slots_json,
            confidence,
            now_ms,
            now_ms + ROUTE_CACHE_TTL_DAYS as i64 * 86_400_000
        ],
    )?;
    Ok(())
}

pub fn prune_expired(conn: &Connection, now_ms: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM llm_route_cache WHERE expires_at_ms <= ?1",
        params![now_ms],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::run_migrations;

    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn normalize_ignores_case_and_whitespace() {
        // 空白折叠（非删除）+ 小写：内部单空格保留，首尾/多余空白消失。
        assert_eq!(normalize_text("  打开 记事本\n"), "打开 记事本");
        assert_eq!(normalize_text("Open   Notepad"), normalize_text("open notepad"));
    }

    #[test]
    fn store_lookup_roundtrip_and_expiry() {
        let conn = migrated();
        let key = route_cache_key("m", 1, "打开记事本");
        assert!(lookup(&conn, &key, 1000).unwrap().is_none());
        store(&conn, &key, "quick.app_control", "[]", 0.9, 1000).unwrap();
        let hit = lookup(&conn, &key, 2000).unwrap().expect("hit");
        assert_eq!(hit.skill_id, "quick.app_control");
        assert!(lookup(&conn, &key, 1000 + 86_400_000 + 1).unwrap().is_none());
    }

    #[test]
    fn prune_expired_removes_only_expired_rows() {
        let conn = migrated();
        store(&conn, "k-old", "s", "[]", 0.9, 1000).unwrap();
        store(&conn, "k-new", "s", "[]", 0.9, 1000 + 86_400_000).unwrap();
        let deleted = prune_expired(&conn, 1000 + 86_400_000 + 1).unwrap();
        assert_eq!(deleted, 1);
    }
}
