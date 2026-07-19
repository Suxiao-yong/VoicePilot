//! Policy repository — CRUD against SQLite `policies` table.

use crate::error::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRecord {
    pub policy_id: String,
    pub version: i64,
    pub rules_json: String,
    pub hash: String,
    pub enabled: bool,
    pub cedar_policies: Option<String>,
    pub cedar_schema: Option<String>,
    pub rust_constraints: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PolicyRepo;

impl PolicyRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, policy: &PolicyRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO policies
                (policy_id, version, rules_json, hash, enabled,
                 cedar_policies, cedar_schema, rust_constraints)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                policy.policy_id,
                policy.version,
                policy.rules_json,
                policy.hash,
                policy.enabled as i64,
                policy.cedar_policies,
                policy.cedar_schema,
                policy.rust_constraints,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, policy_id: &str) -> Result<Option<PolicyRecord>> {
        let mut stmt = conn.prepare(
            "SELECT policy_id, version, rules_json, hash, enabled,
                    cedar_policies, cedar_schema, rust_constraints
             FROM policies WHERE policy_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![policy_id], |r| {
            let policy_id: String = r.get(0)?;
            let version: i64 = r.get(1)?;
            let rules_json: String = r.get(2)?;
            let hash: String = r.get(3)?;
            let enabled: i64 = r.get(4)?;
            let cedar_policies: Option<String> = r.get(5)?;
            let cedar_schema: Option<String> = r.get(6)?;
            let rust_constraints: Option<String> = r.get(7)?;
            Ok((
                policy_id, version, rules_json, hash, enabled,
                cedar_policies, cedar_schema, rust_constraints,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (policy_id, version, rules_json, hash, enabled,
                 cedar_policies, cedar_schema, rust_constraints) = row_result?;
            Ok(Some(PolicyRecord {
                policy_id, version, rules_json, hash, enabled: enabled != 0,
                cedar_policies, cedar_schema, rust_constraints,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_enabled(&self, conn: &Connection) -> Result<Vec<PolicyRecord>> {
        let mut stmt = conn.prepare(
            "SELECT policy_id, version, rules_json, hash, enabled,
                    cedar_policies, cedar_schema, rust_constraints
             FROM policies WHERE enabled = 1 ORDER BY policy_id",
        )?;
        let rows = stmt.query_map([], |r| {
            let policy_id: String = r.get(0)?;
            let version: i64 = r.get(1)?;
            let rules_json: String = r.get(2)?;
            let hash: String = r.get(3)?;
            let enabled: i64 = r.get(4)?;
            let cedar_policies: Option<String> = r.get(5)?;
            let cedar_schema: Option<String> = r.get(6)?;
            let rust_constraints: Option<String> = r.get(7)?;
            Ok((
                policy_id, version, rules_json, hash, enabled,
                cedar_policies, cedar_schema, rust_constraints,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (policy_id, version, rules_json, hash, enabled,
                 cedar_policies, cedar_schema, rust_constraints) = row_result?;
            out.push(PolicyRecord {
                policy_id, version, rules_json, hash, enabled: enabled != 0,
                cedar_policies, cedar_schema, rust_constraints,
            });
        }
        Ok(out)
    }

    pub fn update_hash(&self, conn: &Connection, policy_id: &str, new_hash: &str) -> Result<()> {
        conn.execute(
            "UPDATE policies SET hash = ?1 WHERE policy_id = ?2",
            params![new_hash, policy_id],
        )?;
        Ok(())
    }
}
