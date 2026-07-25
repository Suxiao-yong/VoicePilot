//! TrustKernel facade — the single entry point for CLI/Tauri/UI.
//!
//! W1: text entry → state machine + audit. No real tools yet.

use crate::approval::repo::ApprovalRepo;
use crate::approval::types::{ApprovalRecord, ApprovalScope};
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
    fs: Arc<std::sync::Mutex<crate::tools::fs::FilesystemTool>>,
    comp_repo: Arc<crate::compensation::repo::CompensationRepo>,
    approval_repo: Arc<ApprovalRepo>,
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
        let kernel = Self {
            conn: shared.clone(),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(shared)),
            gateway,
            fs: Arc::new(std::sync::Mutex::new(crate::tools::fs::FilesystemTool::new())),
            comp_repo: Arc::new(crate::compensation::repo::CompensationRepo::new()),
            approval_repo: Arc::new(ApprovalRepo::new()),
            txn_mgr: Arc::new(crate::policy::transaction::TransactionManager::new()),
        };
        // W7 Plan 3: best-effort user skill loading at boot. Errors are
        // logged via tracing::warn! and never propagate — a malformed user
        // skill file must not crash kernel construction.
        if let Err(e) = kernel.load_user_skills() {
            tracing::warn!(error = ?e, "load_user_skills failed at boot");
        }
        kernel
    }

    /// Access the Action Gateway for policy decisions.
    pub fn gateway(&self) -> &crate::gateway::ActionGateway {
        &self.gateway
    }

    /// Access the FilesystemTool adapter.
    /// Returns a MutexGuard — caller can call methods via deref coercion.
    /// The guard is short-lived; drop it before calling other kernel
    /// methods that may lock `fs` (no reentrancy).
    pub fn filesystem(&self) -> std::sync::MutexGuard<'_, crate::tools::fs::FilesystemTool> {
        self.fs.lock().unwrap()
    }

    /// Replace the internal FilesystemTool with one that enforces an
    /// AllowedPaths whitelist. Used by the CLI mcp-serve command to
    /// inject the whitelist loaded from mcp_servers.allowed_paths.
    /// V1.1 §4.4 + §8.1 — resolves spec issue #31.
    pub fn replace_filesystem_with_allowed_paths(&self, allowed: crate::allowed_paths::AllowedPaths) {
        let new_tool = crate::tools::fs::FilesystemTool::new_with_allowed_paths(allowed);
        *self.fs.lock().unwrap() = new_tool;
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

    /// 列出最近的 N 条审计事件(V1.1.2 §8.3 Audit Viewer)。
    pub fn list_audit_recent(&self, limit: usize) -> Result<Vec<crate::audit::AuditEvent>> {
        self.audit.list_recent(limit)
    }

    /// 列出某任务的所有审计事件(V1.1.2 §8.3 Audit Viewer)。
    pub fn list_audit_for_task(&self, task_id: &str) -> Result<Vec<crate::audit::AuditEvent>> {
        self.audit.list_for_task(task_id)
    }

    /// Public entry point for external modules (MCP server, future IPC
    /// layers) to append audit events. V1.1 §6.1 — every MCP tools/call
    /// must leave an audit trail.
    ///
    /// Caller must supply a valid task_id (FK enforced). For stateless
    /// calls (initialize, tools/list) where no task exists, skip audit
    /// logging — those calls carry no security-relevant state changes.
    pub fn audit_append_external(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        details: serde_json::Value,
    ) -> Result<()> {
        self.audit_append(task_id, step_id, event_type, details)
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

    // ===== Approval accessors (W3b) =====

    pub fn record_approval(&self, rec: &ApprovalRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        self.approval_repo.create(&conn, rec)?;
        drop(conn);
        self.audit_append(
            &rec.task_id,
            rec.step_id.as_deref(),
            "APPROVAL_RECORDED",
            serde_json::json!({
                "approval_id": rec.approval_id,
                "user_decision": rec.user_decision.as_str(),
                "approval_scope": rec.approval_scope.as_str(),
                "e_level": rec.e_level.as_str(),
                "d_level": rec.d_level.as_str(),
                "policy_bundle_hash": rec.policy_bundle_hash,
            }),
        )?;
        Ok(())
    }

    pub fn get_approval(&self, approval_id: &str) -> Result<Option<ApprovalRecord>> {
        let conn = self.conn.lock().unwrap();
        self.approval_repo.get(&conn, approval_id)
    }

    pub fn list_approvals_for_task(&self, task_id: &str) -> Result<Vec<ApprovalRecord>> {
        let conn = self.conn.lock().unwrap();
        self.approval_repo.list_for_task(&conn, task_id)
    }

    /// Determine whether this step qualifies for batch approval.
    /// V1.1 §8.1: batch requires (1) Skill manifest mode=batch_once,
    /// (2) same task_id + skill_id, (3) same args_hash, (4) same same policy_bundle_hash,
    /// (5) count < max_approval_scope.
    /// W3b: always returns Single. W7 enables batch when Skill context is wired.
    pub fn check_approval_scope(
        &self,
        _task_id: &str,
        _skill_id: &str,
        _args_hash: &str,
        _policy_bundle_hash: &str,
        _max_scope: u32,
    ) -> ApprovalScope {
        ApprovalScope::Single
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

    /// Borrow the underlying connection for repo operations.
    /// Used by MCP repo and other sub-repos that need direct DB access.
    /// W4 note: returns a MutexGuard — caller must drop before any other
    /// kernel method that locks conn (no reentrancy).
    pub fn conn(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().unwrap()
    }

    /// 获取 ConfigRepo(W6b-2 Settings 持久化)。
    /// 与 McpServerRepo 模式一致:ConfigRepo 无状态,每次返回新实例。
    /// 调用方用 `let conn = kernel.conn(); kernel.config_repo().set(&conn, ...)`。
    pub fn config_repo(&self) -> crate::repo::config_repo::ConfigRepo {
        crate::repo::config_repo::ConfigRepo::new()
    }

    /// 切换 MCP Server 启用状态(V1.1.2 §8.3 Trust Center)。
    pub fn toggle_mcp_server(&self, server_id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn();
        crate::mcp::repo::McpServerRepo::new().toggle_enabled(&conn, server_id, enabled)
    }

    /// 列出所有 MCP Server(V1.1.2 §8.3 Trust Center)。
    pub fn list_mcp_servers(&self) -> Result<Vec<crate::mcp::repo::McpServerRecord>> {
        let conn = self.conn();
        crate::mcp::repo::McpServerRepo::new().list(&conn)
    }

    /// Stateless `SkillRepo` 访问器(V1.1.2 §8.3 Skills Manager)。
    pub fn skill_repo(&self) -> crate::skills::repo::SkillRepo {
        crate::skills::repo::SkillRepo::new()
    }

    /// 列出所有 Skill(V1.1.2 §8.3 Skills Manager)。
    pub fn list_skills(&self) -> Result<Vec<crate::skills::repo::SkillRecord>> {
        let conn = self.conn();
        crate::skills::repo::SkillRepo::new().list(&conn)
    }

    /// 切换 Skill 启用状态(V1.1.2 §8.3 Skills Manager)。
    pub fn toggle_skill(&self, skill_id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn();
        crate::skills::repo::SkillRepo::new().toggle(&conn, skill_id, enabled)
    }

    /// W7 Plan 3: 扫描 `%APPDATA%\voicepilot\skills\*.md`,upsert 到
    /// `skills` 表。Best-effort:错误经 tracing::warn! 记录,不向上传播
    /// (一个损坏的用户文件不能让 kernel 构造失败)。返回成功加载的
    /// 用户 Skill 数量。
    pub fn load_user_skills(&self) -> Result<usize> {
        let dir = match crate::skills::user_loader::user_skills_dir() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = ?e, "user_skills_dir() failed; skipping user skill load");
                return Ok(0);
            }
        };
        let manifests = crate::skills::user_loader::scan_user_skills(&dir);
        let repo = crate::skills::repo::SkillRepo::new();
        {
            let conn = self.conn();
            for m in &manifests {
                let rec = crate::skills::repo::SkillRecord {
                    skill_id: m.id.clone(),
                    // DB row version 是计数器(SkillRecord.version: i64);
                    // manifest version 字符串保留在 manifest_json 内。
                    version: 1,
                    manifest_json: serde_json::to_string(m).map_err(|e| {
                        KernelError::Skill(format!("serde_json failed: {}", e))
                    })?,
                    enabled: true,
                    success_count: 0,
                    avg_latency_ms: 0.0,
                };
                if let Err(e) = repo.upsert(&conn, &rec) {
                    tracing::warn!(skill_id = %m.id, error = ?e, "failed to upsert user skill");
                }
            }
        }
        Ok(manifests.len())
    }

    /// W7 Plan 3:重新扫描 skills 目录,返回用户自定义 Skill manifests。
    /// `route_text` 调用此方法把用户 Skill 注册到 fresh SkillRouter
    /// (Task 3 覆盖语义保证用户 > built-in 优先级)。
    pub fn list_user_skill_manifests(
        &self,
    ) -> Result<Vec<crate::skills::manifest::SkillManifest>> {
        let dir = crate::skills::user_loader::user_skills_dir()?;
        Ok(crate::skills::user_loader::scan_user_skills(&dir))
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
