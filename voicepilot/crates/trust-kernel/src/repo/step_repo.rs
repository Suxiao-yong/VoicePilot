//! Step repository — CRUD against SQLite `steps` table.

use crate::error::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        use StepStatus::*;
        match self {
            Pending => "PENDING",
            Running => "RUNNING",
            Succeeded => "SUCCEEDED",
            Failed => "FAILED",
            Skipped => "SKIPPED",
            Cancelled => "CANCELLED",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        use StepStatus::*;
        Some(match s {
            "PENDING" => Pending,
            "RUNNING" => Running,
            "SUCCEEDED" => Succeeded,
            "FAILED" => Failed,
            "SKIPPED" => Skipped,
            "CANCELLED" => Cancelled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub step_id: String,
    pub task_id: String,
    pub step_order: i64,
    pub tool_name: Option<String>,
    pub args: Option<serde_json::Value>,
    pub args_hash: Option<String>,
    pub status: StepStatus,
    pub prepare_token: Option<String>,
    pub preconditions_hash: Option<String>,
    pub effect_manifest: Option<serde_json::Value>,
    pub evidence_strength: Option<String>,
    pub compensation_ref: Option<String>,
    pub egress_performed: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl StepRecord {
    pub fn new(step_id: impl Into<String>, task_id: impl Into<String>, step_order: i64) -> Self {
        Self {
            step_id: step_id.into(),
            task_id: task_id.into(),
            step_order,
            tool_name: None,
            args: None,
            args_hash: None,
            status: StepStatus::Pending,
            prepare_token: None,
            preconditions_hash: None,
            effect_manifest: None,
            evidence_strength: None,
            compensation_ref: None,
            egress_performed: false,
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StepRepo;

impl StepRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, step: &StepRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO steps
                (step_id, task_id, step_order, tool_name, args, args_hash, status,
                 prepare_token, preconditions_hash, effect_manifest, evidence_strength,
                 compensation_ref, egress_performed, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                step.step_id,
                step.task_id,
                step.step_order,
                step.tool_name,
                step.args.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                step.args_hash,
                step.status.as_str(),
                step.prepare_token,
                step.preconditions_hash,
                step.effect_manifest.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                step.evidence_strength,
                step.compensation_ref,
                step.egress_performed as i64,
                step.started_at,
                step.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, step_id: &str) -> Result<Option<StepRecord>> {
        let mut stmt = conn.prepare(
            "SELECT step_id, task_id, step_order, tool_name, args, args_hash, status,
                    prepare_token, preconditions_hash, effect_manifest, evidence_strength,
                    compensation_ref, egress_performed, started_at, finished_at
             FROM steps WHERE step_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![step_id], |r| {
            let step_id: String = r.get(0)?;
            let task_id: String = r.get(1)?;
            let step_order: i64 = r.get(2)?;
            let tool_name: Option<String> = r.get(3)?;
            let args_str: Option<String> = r.get(4)?;
            let args_hash: Option<String> = r.get(5)?;
            let status_str: String = r.get(6)?;
            let prepare_token: Option<String> = r.get(7)?;
            let preconditions_hash: Option<String> = r.get(8)?;
            let manifest_str: Option<String> = r.get(9)?;
            let evidence_strength: Option<String> = r.get(10)?;
            let compensation_ref: Option<String> = r.get(11)?;
            let egress_performed: i64 = r.get(12)?;
            let started_at: Option<String> = r.get(13)?;
            let finished_at: Option<String> = r.get(14)?;
            Ok((
                step_id,
                task_id,
                step_order,
                tool_name,
                args_str,
                args_hash,
                status_str,
                prepare_token,
                preconditions_hash,
                manifest_str,
                evidence_strength,
                compensation_ref,
                egress_performed,
                started_at,
                finished_at,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (
                step_id,
                task_id,
                step_order,
                tool_name,
                args_str,
                args_hash,
                status_str,
                prepare_token,
                preconditions_hash,
                manifest_str,
                evidence_strength,
                compensation_ref,
                egress_performed,
                started_at,
                finished_at,
            ) = row_result?;
            let status = StepStatus::parse(&status_str)
                .ok_or_else(|| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(std::fmt::Error)))?;
            let args = args_str.and_then(|s| serde_json::from_str(&s).ok());
            let effect_manifest = manifest_str.and_then(|s| serde_json::from_str(&s).ok());
            Ok(Some(StepRecord {
                step_id,
                task_id,
                step_order,
                tool_name,
                args,
                args_hash,
                status,
                prepare_token,
                preconditions_hash,
                effect_manifest,
                evidence_strength,
                compensation_ref,
                egress_performed: egress_performed != 0,
                started_at,
                finished_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_for_task(&self, conn: &Connection, task_id: &str) -> Result<Vec<StepRecord>> {
        let mut stmt = conn.prepare(
            "SELECT step_id, task_id, step_order, tool_name, args, args_hash, status,
                    prepare_token, preconditions_hash, effect_manifest, evidence_strength,
                    compensation_ref, egress_performed, started_at, finished_at
             FROM steps WHERE task_id = ?1 ORDER BY step_order ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |r| {
            let step_id: String = r.get(0)?;
            let task_id: String = r.get(1)?;
            let step_order: i64 = r.get(2)?;
            let tool_name: Option<String> = r.get(3)?;
            let args_str: Option<String> = r.get(4)?;
            let args_hash: Option<String> = r.get(5)?;
            let status_str: String = r.get(6)?;
            let prepare_token: Option<String> = r.get(7)?;
            let preconditions_hash: Option<String> = r.get(8)?;
            let manifest_str: Option<String> = r.get(9)?;
            let evidence_strength: Option<String> = r.get(10)?;
            let compensation_ref: Option<String> = r.get(11)?;
            let egress_performed: i64 = r.get(12)?;
            let started_at: Option<String> = r.get(13)?;
            let finished_at: Option<String> = r.get(14)?;
            Ok((
                step_id,
                task_id,
                step_order,
                tool_name,
                args_str,
                args_hash,
                status_str,
                prepare_token,
                preconditions_hash,
                manifest_str,
                evidence_strength,
                compensation_ref,
                egress_performed,
                started_at,
                finished_at,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (
                step_id,
                task_id,
                step_order,
                tool_name,
                args_str,
                args_hash,
                status_str,
                prepare_token,
                preconditions_hash,
                manifest_str,
                evidence_strength,
                compensation_ref,
                egress_performed,
                started_at,
                finished_at,
            ) = row_result?;
            let status = StepStatus::parse(&status_str)
                .ok_or_else(|| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(std::fmt::Error)))?;
            let args = args_str.and_then(|s| serde_json::from_str(&s).ok());
            let effect_manifest = manifest_str.and_then(|s| serde_json::from_str(&s).ok());
            out.push(StepRecord {
                step_id,
                task_id,
                step_order,
                tool_name,
                args,
                args_hash,
                status,
                prepare_token,
                preconditions_hash,
                effect_manifest,
                evidence_strength,
                compensation_ref,
                egress_performed: egress_performed != 0,
                started_at,
                finished_at,
            });
        }
        Ok(out)
    }

    pub fn update_status(
        &self,
        conn: &Connection,
        step_id: &str,
        new_status: StepStatus,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        match new_status {
            StepStatus::Running => {
                conn.execute(
                    "UPDATE steps SET status = ?1, started_at = COALESCE(started_at, ?2) WHERE step_id = ?3",
                    params![new_status.as_str(), now, step_id],
                )?;
            }
            StepStatus::Succeeded | StepStatus::Failed | StepStatus::Cancelled | StepStatus::Skipped => {
                conn.execute(
                    "UPDATE steps SET status = ?1, finished_at = ?2 WHERE step_id = ?3",
                    params![new_status.as_str(), now, step_id],
                )?;
            }
            StepStatus::Pending => {
                conn.execute(
                    "UPDATE steps SET status = ?1 WHERE step_id = ?2",
                    params![new_status.as_str(), step_id],
                )?;
            }
        }
        Ok(())
    }
}
