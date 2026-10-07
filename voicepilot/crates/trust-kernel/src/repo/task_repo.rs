//! Task repository — CRUD against SQLite `tasks` table.

use crate::error::Result;
use crate::state::TaskState;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: String,
    pub user_goal: String,
    pub status: TaskState,
    pub skill_id: Option<String>,
    pub route_path: Option<String>,
    pub transcript_hash: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl TaskRecord {
    pub fn new(task_id: impl Into<String>, user_goal: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            task_id: task_id.into(),
            user_goal: user_goal.into(),
            status: TaskState::Idle,
            skill_id: None,
            route_path: None,
            transcript_hash: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TaskRepo;

impl TaskRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, task: &TaskRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO tasks
                (task_id, user_goal, status, skill_id, route_path, transcript_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task.task_id,
                task.user_goal,
                serde_json::to_string(&task.status)?,
                task.skill_id,
                task.route_path,
                task.transcript_hash,
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, task_id: &str) -> Result<Option<TaskRecord>> {
        let mut stmt = conn.prepare(
            "SELECT task_id, user_goal, status, skill_id, route_path, transcript_hash,
                    created_at, updated_at
             FROM tasks WHERE task_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![task_id], |r| {
            let task_id: String = r.get(0)?;
            let user_goal: String = r.get(1)?;
            let status_str: String = r.get(2)?;
            let skill_id: Option<String> = r.get(3)?;
            let route_path: Option<String> = r.get(4)?;
            let transcript_hash: Option<String> = r.get(5)?;
            let created_at_str: String = r.get(6)?;
            let updated_at_str: String = r.get(7)?;
            Ok((
                task_id,
                user_goal,
                status_str,
                skill_id,
                route_path,
                transcript_hash,
                created_at_str,
                updated_at_str,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (
                task_id,
                user_goal,
                status_str,
                skill_id,
                route_path,
                transcript_hash,
                created_at_str,
                updated_at_str,
            ) = row_result?;
            let status: TaskState = serde_json::from_str(&status_str)?;
            let created_at = parse_rfc3339_col(created_at_str, 6)?;
            let updated_at = parse_rfc3339_col(updated_at_str, 7)?;
            Ok(Some(TaskRecord {
                task_id,
                user_goal,
                status,
                skill_id,
                route_path,
                transcript_hash,
                created_at,
                updated_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_status(
        &self,
        conn: &Connection,
        task_id: &str,
        new_status: TaskState,
    ) -> Result<()> {
        conn.execute(
            "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE task_id = ?3",
            params![
                serde_json::to_string(&new_status)?,
                Utc::now().to_rfc3339(),
                task_id,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, conn: &Connection, task_id: &str) -> Result<()> {
        conn.execute("DELETE FROM tasks WHERE task_id = ?1", params![task_id])?;
        Ok(())
    }
}

fn parse_rfc3339_col(s: String, col: usize) -> Result<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
        })
        .map_err(Into::into)
}
