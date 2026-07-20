//! Approval repository — V1.1 §8.1 `approvals` table.

use crate::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use crate::error::Result;
use crate::policy::types::{DLevel, ELevel};
use rusqlite::{params, Connection};

#[derive(Debug, Clone, Default)]
pub struct ApprovalRepo;

impl ApprovalRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, rec: &ApprovalRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO approvals
                (approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                 decided_at, E_level, D_level, destination, egress_approved,
                 approval_scope, policy_bundle_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rec.approval_id,
                rec.task_id,
                rec.step_id,
                rec.risk_level,
                rec.args_hash,
                rec.user_decision.as_str(),
                rec.decided_at,
                format!("{:?}", rec.e_level),
                rec.d_level.as_str(),
                rec.destination,
                rec.egress_approved as i64,
                rec.approval_scope.as_str(),
                rec.policy_bundle_hash,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, approval_id: &str) -> Result<Option<ApprovalRecord>> {
        let mut stmt = conn.prepare(
            "SELECT approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                    decided_at, E_level, D_level, destination, egress_approved,
                    approval_scope, policy_bundle_hash
             FROM approvals WHERE approval_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![approval_id], |r| {
            let approval_id: String = r.get(0)?;
            let task_id: String = r.get(1)?;
            let step_id: Option<String> = r.get(2)?;
            let risk_level: String = r.get(3)?;
            let args_hash: String = r.get(4)?;
            let user_decision: String = r.get(5)?;
            let decided_at: String = r.get(6)?;
            let e_level_str: String = r.get(7)?;
            let d_level_str: String = r.get(8)?;
            let destination: String = r.get(9)?;
            let egress_approved: i64 = r.get(10)?;
            let approval_scope_str: String = r.get(11)?;
            let policy_bundle_hash: String = r.get(12)?;
            Ok((
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (
                approval_id, task_id, step_id, risk_level, args_hash, user_decision_str,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ) = row_result?;
            let user_decision = ApprovalDecision::parse(&user_decision_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid user_decision: {}", user_decision_str
                )))?;
            let e_level = parse_e_level(&e_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid E_level: {}", e_level_str
                )))?;
            let d_level = DLevel::as_enum_from_str(&d_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid D_level: {}", d_level_str
                )))?;
            let approval_scope = ApprovalScope::parse(&approval_scope_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid approval_scope: {}", approval_scope_str
                )))?;
            Ok(Some(ApprovalRecord {
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level, d_level, destination,
                egress_approved: egress_approved != 0,
                approval_scope, policy_bundle_hash,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_for_task(&self, conn: &Connection, task_id: &str) -> Result<Vec<ApprovalRecord>> {
        let mut stmt = conn.prepare(
            "SELECT approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                    decided_at, E_level, D_level, destination, egress_approved,
                    approval_scope, policy_bundle_hash
             FROM approvals WHERE task_id = ?1 ORDER BY decided_at",
        )?;
        let rows = stmt.query_map(params![task_id], |r| {
            let approval_id: String = r.get(0)?;
            let task_id: String = r.get(1)?;
            let step_id: Option<String> = r.get(2)?;
            let risk_level: String = r.get(3)?;
            let args_hash: String = r.get(4)?;
            let user_decision: String = r.get(5)?;
            let decided_at: String = r.get(6)?;
            let e_level_str: String = r.get(7)?;
            let d_level_str: String = r.get(8)?;
            let destination: String = r.get(9)?;
            let egress_approved: i64 = r.get(10)?;
            let approval_scope_str: String = r.get(11)?;
            let policy_bundle_hash: String = r.get(12)?;
            Ok((
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (
                approval_id, task_id, step_id, risk_level, args_hash, user_decision_str,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ) = row_result?;
            let user_decision = ApprovalDecision::parse(&user_decision_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid user_decision: {}", user_decision_str
                )))?;
            let e_level = parse_e_level(&e_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid E_level: {}", e_level_str
                )))?;
            let d_level = DLevel::as_enum_from_str(&d_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid D_level: {}", d_level_str
                )))?;
            let approval_scope = ApprovalScope::parse(&approval_scope_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid approval_scope: {}", approval_scope_str
                )))?;
            out.push(ApprovalRecord {
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level, d_level, destination,
                egress_approved: egress_approved != 0,
                approval_scope, policy_bundle_hash,
            });
        }
        Ok(out)
    }
}

/// Parse "E0".."E3" string into ELevel. Used because rusqlite stores E_level as TEXT.
fn parse_e_level(s: &str) -> Option<ELevel> {
    match s {
        "E0" => Some(ELevel::E0),
        "E1" => Some(ELevel::E1),
        "E2" => Some(ELevel::E2),
        "E3" => Some(ELevel::E3),
        _ => None,
    }
}
