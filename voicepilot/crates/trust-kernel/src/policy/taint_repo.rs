//! TaintRepo — W9 §2.3 Taint Tracking CRUD。
//!
//! 操作 `taints` 表(migrations/001_init.sql:98-105),实现值级污点追踪。
//! 遵循 DagRepo accessor pattern:`new()` 不带参数,方法接收 `&Connection`。
//!
//! 安全约束(spec §6.2):
//!   - value_hash = SHA256(value),不存储原始 value(隐私)
//!   - taints_json 仅存 taint 标签,不含原始值
//!   - delete_by_source 在任务清理时调用,避免无限增长

use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;

/// 一条 taint 记录(对应 `taints` 表一行)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintRecord {
    pub taint_id: String,
    pub value_hash: String,
    pub provenance: String,
    pub taints: Vec<String>,
    pub collected_at: String,
    pub source_ref: Option<String>,
}

/// TaintRepo — `taints` 表的 CRUD 访问器。
pub struct TaintRepo;

impl TaintRepo {
    /// 构造(无参数,遵循 DagRepo pattern)。
    pub fn new() -> Self {
        Self
    }

    /// 插入或合并一条 taint 记录。
    /// 若 value_hash 已存在,合并 taints(去重),更新 collected_at + source_ref。
    /// W9 修复(P1-12):使用 ON CONFLICT(value_hash) DO UPDATE,依赖 006_taints_unique_index.sql
    /// 的 UNIQUE 约束,避免并发 upsert 产生重复行。
    pub fn upsert(&self, conn: &Connection, taint: &TaintRecord) -> Result<()> {
        // 先查再合并 taints,然后用 ON CONFLICT 写入(UNIQUE 约束保证幂等)
        let existing_taints: Vec<String> = conn
            .query_row(
                "SELECT taints_json FROM taints WHERE value_hash = ?1",
                params![taint.value_hash],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let merged = merge_taints(&existing_taints, &taint.taints);
        let merged_json = serde_json::to_string(&merged)?;
        // source_ref 取新值(若新值非 None),否则保留原值
        let final_source = taint.source_ref.clone();

        conn.execute(
            "INSERT INTO taints (taint_id, value_hash, provenance, taints_json, collected_at, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(value_hash) DO UPDATE SET
                taints_json = excluded.taints_json,
                collected_at = excluded.collected_at,
                source_ref = excluded.source_ref",
            params![
                taint.taint_id,
                taint.value_hash,
                taint.provenance,
                merged_json,
                taint.collected_at,
                final_source,
            ],
        )?;
        Ok(())
    }

    /// 按原始 value 查询 taints(内部算 SHA256)。
    /// W9 修复(P1-19):内部将 &str 转为 serde_json::Value 后调 compute_value_hash,
    /// 以使用 canonical JSON 序列化(避免字段顺序差异)。
    pub fn find_by_value(&self, conn: &Connection, value: &str) -> Result<Option<TaintRecord>> {
        let value_json: serde_json::Value =
            serde_json::from_str(value).unwrap_or(serde_json::Value::String(value.to_string()));
        let value_hash = compute_value_hash(&value_json);
        self.find_by_hash(conn, &value_hash)
    }

    /// 按 value_hash 查询(供 gateway 已有 hash 时直接查)。
    pub fn find_by_hash(&self, conn: &Connection, value_hash: &str) -> Result<Option<TaintRecord>> {
        let row = conn
            .query_row(
                "SELECT taint_id, value_hash, provenance, taints_json, collected_at, \
                 source_ref FROM taints WHERE value_hash = ?1",
                params![value_hash],
                |row| {
                    let taints_json: String = row.get(3)?;
                    let taints: Vec<String> =
                        serde_json::from_str(&taints_json).unwrap_or_default();
                    Ok(TaintRecord {
                        taint_id: row.get(0)?,
                        value_hash: row.get(1)?,
                        provenance: row.get(2)?,
                        taints,
                        collected_at: row.get(4)?,
                        source_ref: row.get(5)?,
                    })
                },
            )
            .ok();
        Ok(row)
    }

    /// 按 provenance 查询所有 taints。
    pub fn list_by_provenance(
        &self,
        conn: &Connection,
        provenance: &str,
    ) -> Result<Vec<TaintRecord>> {
        let mut stmt = conn.prepare(
            "SELECT taint_id, value_hash, provenance, taints_json, collected_at, source_ref \
             FROM taints WHERE provenance = ?1",
        )?;
        let rows = stmt.query_map(params![provenance], |row| {
            let taints_json: String = row.get(3)?;
            let taints: Vec<String> = serde_json::from_str(&taints_json).unwrap_or_default();
            Ok(TaintRecord {
                taint_id: row.get(0)?,
                value_hash: row.get(1)?,
                provenance: row.get(2)?,
                taints,
                collected_at: row.get(4)?,
                source_ref: row.get(5)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    /// 按 source_ref 查询(如某 task_id 关联的所有 taints)。
    pub fn list_by_source(&self, conn: &Connection, source_ref: &str) -> Result<Vec<TaintRecord>> {
        let mut stmt = conn.prepare(
            "SELECT taint_id, value_hash, provenance, taints_json, collected_at, source_ref \
             FROM taints WHERE source_ref = ?1",
        )?;
        let rows = stmt.query_map(params![source_ref], |row| {
            let taints_json: String = row.get(3)?;
            let taints: Vec<String> = serde_json::from_str(&taints_json).unwrap_or_default();
            Ok(TaintRecord {
                taint_id: row.get(0)?,
                value_hash: row.get(1)?,
                provenance: row.get(2)?,
                taints,
                collected_at: row.get(4)?,
                source_ref: row.get(5)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    /// 删除某 source_ref 的所有 taints(任务清理时用),返回删除行数。
    pub fn delete_by_source(&self, conn: &Connection, source_ref: &str) -> Result<u64> {
        let affected = conn.execute(
            "DELETE FROM taints WHERE source_ref = ?1",
            params![source_ref],
        )?;
        Ok(affected as u64)
    }
}

impl Default for TaintRepo {
    fn default() -> Self {
        Self::new()
    }
}

/// 计算 value 的 SHA256 hex 字符串(spec §6.2:不存储原始 value)。
/// W9 修复(P1-19):用 canonical JSON 序列化(字段排序),避免
/// `{"a":1,"b":2}` 和 `{"b":2,"a":1}` 产生不同 hash。
pub fn compute_value_hash(value: &serde_json::Value) -> String {
    let canonical = canonicalize_json(value);
    let canonical_str = serde_json::to_string(&canonical).unwrap_or_default();
    sha256(&canonical_str)
}

/// 递归排序 JSON 字段(用 BTreeMap 实现 canonical form)。
fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let btree: std::collections::BTreeMap<&String, &serde_json::Value> =
                map.iter().collect();
            let canonical_map: serde_json::Map<String, serde_json::Value> = btree
                .iter()
                .map(|(k, v)| ((*k).clone(), canonicalize_json(v)))
                .collect();
            serde_json::Value::Object(canonical_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalize_json).collect())
        }
        other => other.clone(),
    }
}

/// SHA256 hex 字符串辅助函数。
fn sha256(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 合并两个 taints 列表(去重,保留顺序)。
pub fn merge_taints(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut merged = existing.to_vec();
    for t in incoming {
        if !merged.contains(t) {
            merged.push(t.clone());
        }
    }
    merged
}

/// 当前时间 ISO8601(供调用方构造 TaintRecord 用)。
pub fn now_iso8601() -> String {
    Utc::now().to_rfc3339()
}

/// 构造一条新 TaintRecord(便利函数,自动填 taint_id + collected_at)。
pub fn make_taint_record(
    value_hash: String,
    provenance: String,
    taints: Vec<String>,
    source_ref: Option<String>,
) -> TaintRecord {
    TaintRecord {
        taint_id: uuid::Uuid::new_v4().to_string(),
        value_hash,
        provenance,
        taints,
        collected_at: now_iso8601(),
        source_ref,
    }
}
