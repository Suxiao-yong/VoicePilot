//! TaskExplanationRepo — W8 §2.5 / §2.6.
//!
//! CRUD for `task_explanations` table.
//! Stores LLM failure analysis results from `task.explain` Skill.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExplanationRecord {
    pub explanation_id: String,
    pub step_id: String,
    pub root_cause_zh: String,
    pub category: String,
    pub suggested_fix: Option<String>,
    pub confidence: f32,
    pub llm_model: Option<String>,
    pub created_at: String,
}

/// 失败分类(W8 §2.5)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    McpUnavailable,
    PathNotAllowed,
    ApprovalDenied,
    NetworkError,
    Unknown,
}

impl FailureCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::McpUnavailable => "mcp_unavailable",
            Self::PathNotAllowed => "path_not_allowed",
            Self::ApprovalDenied => "approval_denied",
            Self::NetworkError => "network_error",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "mcp_unavailable" => Some(Self::McpUnavailable),
            "path_not_allowed" => Some(Self::PathNotAllowed),
            "approval_denied" => Some(Self::ApprovalDenied),
            "network_error" => Some(Self::NetworkError),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

pub struct TaskExplanationRepo;

impl TaskExplanationRepo {
    pub fn new() -> Self {
        Self
    }

    /// 创建失败归因记录。
    /// `rec.created_at` 由本方法覆写为当前 UTC RFC3339 时间戳,
    /// 调用方可传 `String::new()` 占位。
    pub fn create(
        &self,
        conn: &Connection,
        rec: &TaskExplanationRecord,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO task_explanations
               (explanation_id, step_id, root_cause_zh, category,
                suggested_fix, confidence, llm_model, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            params![
                rec.explanation_id,
                rec.step_id,
                rec.root_cause_zh,
                rec.category,
                rec.suggested_fix,
                rec.confidence,
                rec.llm_model,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_by_id(
        &self,
        conn: &Connection,
        explanation_id: &str,
    ) -> Result<Option<TaskExplanationRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT explanation_id, step_id, root_cause_zh, category,
                      suggested_fix, confidence, llm_model, created_at
               FROM task_explanations WHERE explanation_id = ?1"#,
        )?;
        let mut rows = stmt.query(params![explanation_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    /// W8 §2.5:按 step_id 查询最新归因(UI 显示 task.explain 结果)。
    pub fn get_by_step_id(
        &self,
        conn: &Connection,
        step_id: &str,
    ) -> Result<Option<TaskExplanationRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT explanation_id, step_id, root_cause_zh, category,
                      suggested_fix, confidence, llm_model, created_at
               FROM task_explanations
               WHERE step_id = ?1
               ORDER BY created_at DESC
               LIMIT 1"#,
        )?;
        let mut rows = stmt.query(params![step_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn delete(&self, conn: &Connection, explanation_id: &str) -> Result<()> {
        conn.execute(
            r#"DELETE FROM task_explanations WHERE explanation_id = ?1"#,
            params![explanation_id],
        )?;
        Ok(())
    }
}

impl Default for TaskExplanationRepo {
    fn default() -> Self {
        Self::new()
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskExplanationRecord> {
    Ok(TaskExplanationRecord {
        explanation_id: row.get(0)?,
        step_id: row.get(1)?,
        root_cause_zh: row.get(2)?,
        category: row.get(3)?,
        suggested_fix: row.get(4)?,
        confidence: row.get(5)?,
        llm_model: row.get(6)?,
        created_at: row.get(7)?,
    })
}
