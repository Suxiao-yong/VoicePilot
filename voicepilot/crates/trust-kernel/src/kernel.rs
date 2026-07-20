//! TrustKernel facade — the single entry point for CLI/Tauri/UI.
//!
//! W1: text entry → state machine + audit. No real tools yet.

use crate::audit::{AuditEvent, AuditLogger, SqliteAuditLogger};
use crate::compensation::types::CompensationRecord;
use crate::db;
use crate::error::{KernelError, Result};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::repo::task_repo::{TaskRecord, TaskRepo};
use crate::state::TaskState;
use chrono::Utc;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
    gateway: Arc<crate::gateway::ActionGateway>,
    fs: Arc<crate::tools::fs::FilesystemTool>,
    comp_repo: Arc<crate::compensation::repo::CompensationRepo>,
    txn_mgr: Arc<crate::policy::transaction::TransactionManager>,
}

impl TrustKernel {
    pub fn open_in_memory() -> Result<Self> {
        let conn = db::open_in_memory()?;
        db::run_migrations(&conn)?;
        Ok(Self::with_conn(conn))
    }

    pub fn open_file(path: &str) -> Result<Self> {
        let conn = db::open_file(path)?;
        db::run_migrations(&conn)?;
        Ok(Self::with_conn(conn))
    }

    fn with_conn(conn: Connection) -> Self {
        let shared = Arc::new(Mutex::new(conn));
        let cedar_src = include_str!("policies/default.cedar");
        let gateway = Arc::new(
            crate::gateway::ActionGateway::new(cedar_src)
                .expect("default cedar policy must parse"),
        );
        Self {
            conn: shared.clone(),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(shared)),
            gateway,
            fs: Arc::new(crate::tools::fs::FilesystemTool::new()),
            comp_repo: Arc::new(crate::compensation::repo::CompensationRepo::new()),
            txn_mgr: Arc::new(crate::policy::transaction::TransactionManager::new()),
        }
    }

    /// Access the Action Gateway for policy decisions.
    pub fn gateway(&self) -> &crate::gateway::ActionGateway {
        &self.gateway
    }

    /// Access the FilesystemTool adapter.
    pub fn filesystem(&self) -> &crate::tools::fs::FilesystemTool {
        &self.fs
    }

    /// Access the Compensation repository.
    pub fn compensation_repo(&self) -> &crate::compensation::repo::CompensationRepo {
        &self.comp_repo
    }

    /// Access the TransactionManager (for prepare/commit lifecycle).
    pub fn transaction_manager(&self) -> &crate::policy::transaction::TransactionManager {
        &self.txn_mgr
    }

    pub fn create_task(&self, task_id: &str, user_goal: &str) -> Result<TaskRecord> {
        let task = TaskRecord::new(task_id, user_goal);
        {
            let conn = self.conn.lock().unwrap();
            self.task_repo.create(&conn, &task)?;
        }
        self.audit_append(&task.task_id, None, "TASK_CREATED", serde_json::json!({
            "user_goal": task.user_goal,
        }))?;
        Ok(task)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        self.task_repo.get(&conn, task_id)
    }

    /// Transition a task to a new state. Rejects illegal transitions.
    /// Emits a `STATE_TRANSITION` audit event on success.
    pub fn transition(&self, task_id: &str, target: TaskState) -> Result<()> {
        let current = self
            .get_task(task_id)?
            .ok_or_else(|| KernelError::TaskNotFound(task_id.to_string()))?;
        if !current.status.can_transition_to(target) {
            return Err(KernelError::InvalidTransition {
                from: current.status,
                to: target,
            });
        }
        {
            let conn = self.conn.lock().unwrap();
            self.task_repo.update_status(&conn, task_id, target)?;
        }
        self.audit_append(task_id, None, "STATE_TRANSITION", serde_json::json!({
            "from": current.status,
            "to": target,
        }))?;
        Ok(())
    }

    pub fn audit_count_for_task(&self, task_id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE task_id = ?1",
            rusqlite::params![task_id],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    // ===== Compensation accessors (W3b) =====

    pub fn create_compensation(&self, rec: &CompensationRecord) -> Result<()> {
        {
            let conn = self.conn.lock().unwrap();
            self.comp_repo.create(&conn, rec)?;
        }
        // Resolve task_id from steps.step_id so audit_logs.task_id FK is
        // satisfied. The original W3b plan used a "unknown-task" placeholder
        // here, but audit_logs.task_id REFERENCES tasks(task_id) (migration
        // 001_init.sql line 77) so any non-existent task_id violates FK.
        let task_id = self
            .task_id_for_step(&rec.step_id)?
            .unwrap_or_else(|| "unknown-task".to_string());
        self.audit_append(
            &task_id,
            Some(&rec.step_id),
            "COMPENSATION_CREATED",
            serde_json::json!({
                "comp_id": rec.comp_id,
                "level": rec.level.as_str(),
                "conflict_policy": rec.conflict_policy.as_str(),
                "ttl_expires": rec.ttl_expires,
            }),
        )?;
        Ok(())
    }

    pub fn get_compensation(&self, comp_id: &str) -> Result<Option<CompensationRecord>> {
        let conn = self.conn.lock().unwrap();
        self.comp_repo.get(&conn, comp_id)
    }

    pub fn list_active_compensations(&self) -> Result<Vec<CompensationRecord>> {
        let conn = self.conn.lock().unwrap();
        self.comp_repo.list_active(&conn)
    }

    pub fn mark_compensation_status(&self, comp_id: &str, new_status: &str) -> Result<()> {
        let step_id_opt = {
            let conn = self.conn.lock().unwrap();
            self.comp_repo.mark_status(&conn, comp_id, new_status)?;
            self.comp_repo.get(&conn, comp_id)?.map(|c| c.step_id)
        };
        let task_id = match &step_id_opt {
            Some(sid) => self
                .task_id_for_step(sid)?
                .unwrap_or_else(|| "unknown-task".to_string()),
            None => "unknown-task".to_string(),
        };
        self.audit_append(
            &task_id,
            step_id_opt.as_deref(),
            "COMPENSATION_STATUS_CHANGED",
            serde_json::json!({
                "comp_id": comp_id,
                "new_status": new_status,
            }),
        )?;
        Ok(())
    }

    // ===== Step accessors (W3b) =====

    pub fn create_step(&self, step: &StepRecord) -> Result<()> {
        {
            let conn = self.conn.lock().unwrap();
            let repo = crate::repo::step_repo::StepRepo::new();
            repo.create(&conn, step)?;
        }
        self.audit_append(
            &step.task_id,
            Some(&step.step_id),
            "STEP_CREATED",
            serde_json::json!({
                "step_id": step.step_id,
                "step_order": step.step_order,
                "tool_name": step.tool_name,
            }),
        )?;
        Ok(())
    }

    pub fn get_step(&self, step_id: &str) -> Result<Option<StepRecord>> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.get(&conn, step_id)
    }

    pub fn list_steps_for_task(&self, task_id: &str) -> Result<Vec<StepRecord>> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.list_for_task(&conn, task_id)
    }

    pub fn update_step_status(&self, step_id: &str, new_status: StepStatus) -> Result<()> {
        {
            let conn = self.conn.lock().unwrap();
            let repo = crate::repo::step_repo::StepRepo::new();
            repo.update_status(&conn, step_id, new_status)?;
        }
        let task_id = self
            .task_id_for_step(step_id)?
            .unwrap_or_else(|| "unknown-task".to_string());
        self.audit_append(
            &task_id,
            Some(step_id),
            "STEP_STATUS_CHANGED",
            serde_json::json!({
                "step_id": step_id,
                "new_status": new_status.as_str(),
            }),
        )?;
        Ok(())
    }

    pub fn update_step_prepare_state(
        &self,
        step_id: &str,
        prepare_token: &str,
        preconditions_hash: &str,
        effect_manifest: &serde_json::Value,
    ) -> Result<()> {
        {
            let conn = self.conn.lock().unwrap();
            let repo = crate::repo::step_repo::StepRepo::new();
            repo.update_prepare_state(
                &conn, step_id, prepare_token, preconditions_hash, effect_manifest,
            )?;
        }
        let task_id = self
            .task_id_for_step(step_id)?
            .unwrap_or_else(|| "unknown-task".to_string());
        self.audit_append(
            &task_id,
            Some(step_id),
            "STEP_PREPARED",
            serde_json::json!({
                "step_id": step_id,
                "prepare_token": prepare_token,
                "preconditions_hash": preconditions_hash,
            }),
        )?;
        Ok(())
    }

    pub fn update_step_post_commit(
        &self,
        step_id: &str,
        evidence_strength: &str,
        compensation_ref: Option<&str>,
    ) -> Result<()> {
        {
            let conn = self.conn.lock().unwrap();
            let repo = crate::repo::step_repo::StepRepo::new();
            repo.update_post_commit(&conn, step_id, evidence_strength, compensation_ref)?;
        }
        let task_id = self
            .task_id_for_step(step_id)?
            .unwrap_or_else(|| "unknown-task".to_string());
        self.audit_append(
            &task_id,
            Some(step_id),
            "STEP_COMMITTED",
            serde_json::json!({
                "step_id": step_id,
                "evidence_strength": evidence_strength,
                "compensation_ref": compensation_ref,
            }),
        )?;
        Ok(())
    }

    /// Resolve the task_id for a given step_id by querying the steps table.
    /// Used by step-scoped audit events where only step_id is available;
    /// satisfies the audit_logs.task_id FK to tasks.task_id.
    /// The W3b plan originally proposed "unknown-task" placeholders and
    /// deferred the lookup to W7, but FK enforcement (foreign_keys=ON in
    /// db.rs) requires the lookup now.
    fn task_id_for_step(&self, step_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let task_id: Option<String> = conn
            .query_row(
                "SELECT task_id FROM steps WHERE step_id = ?1",
                rusqlite::params![step_id],
                |r| r.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(KernelError::Db(other)),
            })?;
        Ok(task_id)
    }

    fn audit_append(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        details: serde_json::Value,
    ) -> Result<()> {
        let event = AuditEvent {
            log_id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            step_id: step_id.map(String::from),
            event_type: event_type.to_string(),
            details,
            timestamp: Utc::now(),
            prev_hash: None, // auto-chained by logger
            hash: String::new(), // computed by logger
        };
        self.audit.append(&event)
    }
}
