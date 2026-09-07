//! ConfigRepo —— V1.1.2 §8.3 Settings 持久化 KV 存储。
//!
//! 简单 key-value 表,value 为字符串(调用方负责 JSON 序列化)。
//! key 命名空间约定见 migrations/002_app_config.sql 注释。

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct ConfigRepo;

impl ConfigRepo {
    pub fn new() -> Self {
        Self
    }

    /// 获取某个配置值。返回 None 表示 key 不存在。
    pub fn get(&self, conn: &Connection, key: &str) -> Result<Option<String>> {
        let mut stmt = conn.prepare("SELECT value FROM app_config WHERE key = ?1")?;
        let val = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(val)
    }

    /// 设置(或覆盖)某个配置值。
    pub fn set(&self, conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO app_config (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// 列出所有配置项(按 key 字典序)。
    pub fn list(&self, conn: &Connection) -> Result<Vec<(String, String)>> {
        let mut stmt = conn.prepare("SELECT key, value FROM app_config ORDER BY key ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 删除某个配置项。key 不存在时为 no-op。
    pub fn delete(&self, conn: &Connection, key: &str) -> Result<()> {
        conn.execute("DELETE FROM app_config WHERE key = ?1", params![key])?;
        Ok(())
    }
}
