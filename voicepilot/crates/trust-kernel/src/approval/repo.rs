//! Approval repository — V1.1 §8.1 `approvals` table.

use crate::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use crate::error::Result;
use crate::policy::types::{DLevel, ELevel};
use rusqlite::{params, Connection};

/// Column list shared by `get` and `list_for_task` SELECT queries.
/// Must stay in sync with `ApprovalRow` field order.
const SELECT_COLS: &str = "approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                    decided_at, E_level, D_level, destination, egress_approved,
                    approval_scope, policy_bundle_hash";

/// Row tuple shape produced by the rusqlite `query_map` closures.
/// Field order matches `SELECT_COLS`.
type ApprovalRow = (
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
);

/// Convert a raw row tuple into an `ApprovalRecord`, parsing enum columns.
fn row_to_record(row: ApprovalRow) -> Result<ApprovalRecord> {
    let (
        approval_id,
        task_id,
        step_id,
        risk_level,
        args_hash,
        user_decision_str,
        decided_at,
        e_level_str,
        d_level_str,
        destination,
        egress_approved,
        approval_scope_str,
        policy_bundle_hash,
    ) = row;
    let user_decision = ApprovalDecision::parse(&user_decision_str)
        .ok_or_else(|| crate::error::KernelError::Approval(format!(
            "invalid user_decision: {}", user_decision_str
        )))?;
    let e_level = ELevel::as_enum_from_str(&e_level_str)
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
    Ok(ApprovalRecord {
        approval_id,
        task_id,
        step_id,
        risk_level,
        args_hash,
        user_decision,
        decided_at,
        e_level,
        d_level,
        destination,
        egress_approved: egress_approved != 0,
        approval_scope,
        policy_bundle_hash,
    })
}

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
                rec.e_level.as_str(),
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
        let sql = format!(
            "SELECT {} FROM approvals WHERE approval_id = ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![approval_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            Ok(Some(row_to_record(row_result?)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_for_task(&self, conn: &Connection, task_id: &str) -> Result<Vec<ApprovalRecord>> {
        let sql = format!(
            "SELECT {} FROM approvals WHERE task_id = ?1 ORDER BY decided_at",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![task_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            out.push(row_to_record(row_result?)?);
        }
        Ok(out)
    }
}
