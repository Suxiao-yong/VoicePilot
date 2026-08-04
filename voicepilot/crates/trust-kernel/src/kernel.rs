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
    // W7 Plan 4: UIA 应用白名单(quick.app_control / note.capture 可启动的应用列表)。
    // 默认 ["notepad", "explorer", "calc"]。Settings 面板可编辑,持久化到 app_config。
    // 即使 `uia` feature 关闭此字段也存在(纯数据,无害)——避免 DTO 形状随 feature 变化。
    allowed_apps: Arc<std::sync::Mutex<Vec<String>>>,
    // W8 Plan 4: LLM 客户端(可选,None = 不调 LLM)。
    // `#[cfg(feature = "llm")]` 门控:无 llm feature 时不持有 LlmClient,
    // route_text_with_dag 直接走关键词 + Planner 回退(spec §6)。
    // 用 `Mutex<Option<Arc<LlmClient>>>` 而非 `Arc<Mutex<...>>`:kernel 是唯一 owner,
    // 不需要 Arc 共享;LlmClient 内部有 reqwest::Client(不可 Clone),
    // 用 Arc<LlmClient> 让 setter / getter 不需要 ownership transfer。
    #[cfg(feature = "llm")]
    llm_client: std::sync::Mutex<Option<Arc<crate::llm::client::LlmClient>>>,
    // W9 Plan 1: Stronghold vault(可选,None = 未注入 / feature 未启用)。
    // 用 `Mutex<Option<Arc<StrongholdVault>>>` 而非 `Arc<Mutex<...>>`:kernel 是唯一 owner,
    // 不需要 Arc 共享;StrongholdVault 内部已有 Mutex<Option<Stronghold>>,
    // 外层 Mutex 仅保护 "是否已注入" 状态的替换(set_stronghold_vault)。
    //
    // 门控决策:`#[cfg(feature = "stronghold")]` 门控(与 `llm_client` 模式一致)。
    // vault 不是 DTO(不跨进程边界 / 不序列化),字段形状随 feature 变化可接受;
    // `set_stronghold_vault` / `stronghold_vault` 方法同样门控;
    // `stronghold_enabled` / `ensure_stronghold_ready_for_privacy` 不门控(用内部 #[cfg] 分支)。
    #[cfg(feature = "stronghold")]
    stronghold_vault: std::sync::Mutex<Option<Arc<crate::crypto::stronghold::StrongholdVault>>>,
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
            // W7 Plan 4: UIA 白名单默认值 —— V1.1 §8.1 quick.app_control / note.capture
            // 仅可启动此列表内应用。Settings 面板可改,持久化在 app_config "uia.allowed_apps"。
            allowed_apps: Arc::new(std::sync::Mutex::new(vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ])),
            // W8 Plan 4: 默认无 LLM 客户端(None)。Settings 面板在 LLM 启用时
            // 调 `set_llm_client(Some(Arc::new(LlmClient::new(...))))` 注入。
            #[cfg(feature = "llm")]
            llm_client: std::sync::Mutex::new(None),
            // W9 Plan 1: 默认无 Stronghold vault(None)。Settings 面板或启动逻辑
            // 在用户输入密码后调 `set_stronghold_vault(Some(Arc::new(StrongholdVault::create(...))))` 注入。
            #[cfg(feature = "stronghold")]
            stronghold_vault: std::sync::Mutex::new(None),
        };
        // W7 Plan 4: 从 KV 加载 allowed_apps(Settings 持久化值)覆盖默认值。
        // 缺失 / 空串 / 反序列化失败时保持默认 ["notepad", "explorer", "calc"]。
        // 错误经 tracing::warn! 记录,不向上传播(一个损坏的 KV 值不能让 kernel 构造失败)。
        // 与 LlmClient 重建同理:KV 是 accepted/persisted,运行时 serving-applied
        // 状态需显式刷新 —— 此处刷新保证 boot 后 allowed_apps 与 Settings 一致。
        {
            let conn_guard = kernel.conn.lock().unwrap();
            match kernel.config_repo().get(&conn_guard, "uia.allowed_apps") {
                Ok(Some(kv_str)) if !kv_str.is_empty() => {
                    match serde_json::from_str::<Vec<String>>(&kv_str) {
                        Ok(apps) => {
                            drop(conn_guard);
                            kernel.set_allowed_apps(apps);
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = ?e,
                                "failed to deserialize uia.allowed_apps from KV; using default whitelist"
                            );
                        }
                    }
                }
                Ok(_) => { /* key missing or empty — keep default */ }
                Err(e) => {
                    tracing::warn!(
                        error = ?e,
                        "failed to read uia.allowed_apps from KV; using default whitelist"
                    );
                }
            }
        }
        // W7 Plan 3: best-effort user skill loading at boot. Errors are
        // logged via tracing::warn! and never propagate — a malformed user
        // skill file must not crash kernel construction.
        if let Err(e) = kernel.load_user_skills() {
            tracing::warn!(error = ?e, "load_user_skills failed at boot");
        }
        // W7 Plan 5 Task 2: best-effort insert default playwright MCP server
        // row. Idempotent — preserves user customizations (e.g. disabled via
        // Trust Center). Errors are logged and never propagate — a DB issue
        // here must not crash kernel construction. Unlike `seed_builtin_filesystem`
        // (called only from CLI mcp-serve), playwright is needed in all modes
        // (UI can trigger research.save_markdown / form.prepare).
        {
            let conn_guard = kernel.conn.lock().unwrap();
            if let Err(e) = crate::mcp::repo::McpServerRepo::new()
                .insert_default_servers(&conn_guard)
            {
                tracing::warn!(
                    error = ?e,
                    "insert_default_servers (playwright) failed at boot"
                );
            }
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

    /// W7 Plan 4: 读取 UIA 应用白名单(quick.app_control / note.capture
    /// 可启动的应用列表)。返回 `MutexGuard`,与 `filesystem()` / `conn()`
    /// 模式一致 —— 调用方持有 guard 期间不要调用其它会锁 `allowed_apps`
    /// 的 kernel 方法(无重入)。
    pub fn allowed_apps(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.allowed_apps.lock().unwrap()
    }

    /// W7 Plan 4: 替换 UIA 应用白名单。Settings 面板 `update_settings`
    /// 调用此方法把 KV 中持久化的列表写入运行时状态。
    /// 接受 `Vec<String>` 而非 `&[String]` —— 调用方通常已 own 数据
    /// (从 `serde_json::from_str` 反序列化得到),传 Vec 避免额外 clone。
    pub fn set_allowed_apps(&self, apps: Vec<String>) {
        *self.allowed_apps.lock().unwrap() = apps;
    }

    // ===== W8 Plan 4: LLM client + privacy_mode accessors =====

    /// W8 Plan 4: 返回当前 LLM 客户端的 Arc 克隆(若有)。
    /// `route_text_with_dag` 用此方法判断是否调 LLM 拆解 DAG。
    /// 返回 `Option<Arc<LlmClient>>` 而非 `Option<&LlmClient>`:内部用
    /// `Mutex<Option<Arc<LlmClient>>>` 存储,返回引用需要持有 guard,API 不便;
    /// 克隆 Arc(参考计数 +1)开销极低,调用方拿到 Arc 后可自由持有。
    #[cfg(feature = "llm")]
    pub fn llm_client(&self) -> Option<Arc<crate::llm::client::LlmClient>> {
        let guard = self.llm_client.lock().unwrap();
        guard.clone()
    }

    /// W8 Plan 4: 注入或清除 LLM 客户端。
    /// Settings 面板 `update_settings_command` 在 LLM 启用且 api_key 非空时
    /// 调 `set_llm_client(Some(Arc::new(LlmClient::new(...))))`;
    /// privacy_mode 切换为 true 或 LLM 禁用时调 `set_llm_client(None)`。
    #[cfg(feature = "llm")]
    pub fn set_llm_client(&self, client: Option<Arc<crate::llm::client::LlmClient>>) {
        let mut guard = self.llm_client.lock().unwrap();
        *guard = client;
    }

    // ===== W9 Plan 1: Stronghold vault accessors =====

    /// W9 Plan 1: 注入或清除 Stronghold vault。
    /// Settings 面板 / 启动逻辑在用户输入密码后调
    /// `set_stronghold_vault(Some(Arc::new(StrongholdVault::create(password, &conn))))`;
    /// 降级模式调 `set_stronghold_vault(Some(Arc::new(StrongholdVault::degraded(&conn))))`;
    /// 退出登录调 `set_stronghold_vault(None)`(内部会先 lock 旧 vault 清零 key material)。
    #[cfg(feature = "stronghold")]
    pub fn set_stronghold_vault(
        &self,
        vault: Option<Arc<crate::crypto::stronghold::StrongholdVault>>,
    ) {
        // 若已有 vault,先 lock()(清零 key material)再替换。单次 lock 获取,避免双锁。
        let mut guard = self.stronghold_vault.lock().unwrap();
        if let Some(old) = guard.take() {
            old.lock();
        }
        *guard = vault;
    }

    /// W9 Plan 1: 返回当前 Stronghold vault 的 Arc 克隆(若有)。
    /// Plan 2 `create_post_commit_compensation` / `reverse_compensation` 用此方法
    /// 判断是否调 vault.encrypt() / vault.decrypt()。
    #[cfg(feature = "stronghold")]
    pub fn stronghold_vault(&self) -> Option<Arc<crate::crypto::stronghold::StrongholdVault>> {
        self.stronghold_vault.lock().unwrap().clone()
    }

    /// W9 Plan 1: Stronghold feature 是否启用 + 配置是否启用。
    /// - feature 关闭(编译时):返回 false
    /// - feature 启用 + app_config.stronghold.enabled 缺失 / "true":返回 true
    /// - feature 启用 + app_config.stronghold.enabled = "false":返回 false(测试用)
    pub fn stronghold_enabled(&self) -> bool {
        #[cfg(feature = "stronghold")]
        {
            let conn = self.conn();
            crate::crypto::stronghold::is_stronghold_enabled_in_config(&conn)
        }
        #[cfg(not(feature = "stronghold"))]
        {
            false
        }
    }

    /// W9 Plan 1: privacy_mode 联动校验(spec §2.1 与 privacy_mode 联动)。
    ///
    /// 调用时机:启动逻辑 / Settings 切换 privacy_mode=true 时 / route_text_with_dag 入口。
    ///
    /// 规则:
    /// - privacy_mode = false:直接返回 Ok(())(允许 Stronghold 降级模式启动)
    /// - privacy_mode = true + stronghold_enabled = false:返回 Err(StrongholdRequired)
    ///   (高隐私模式必须启用 Stronghold,防止 reverse_payload 明文落盘)
    /// - privacy_mode = true + stronghold_enabled = true + vault 未注入:返回 Err(StrongholdRequired)
    /// - privacy_mode = true + stronghold_enabled = true + vault 已注入但未解锁:返回 Err(StrongholdRequired)
    /// - privacy_mode = true + stronghold_enabled = true + vault 已解锁:返回 Ok(())
    ///
    /// 注意:此方法不门控 #[cfg(feature = "stronghold")],因为 privacy_mode 在所有
    /// feature 组合下都存在(W8 Plan 4 实现);feature 关闭时 stronghold_enabled() 恒 false,
    /// privacy_mode=true 必然返回 Err(防绕过)。
    pub fn ensure_stronghold_ready_for_privacy(&self) -> Result<()> {
        if !self.privacy_mode() {
            return Ok(());
        }
        // privacy_mode = true:要求 stronghold_enabled + vault 已注入 + 已解锁
        if !self.stronghold_enabled() {
            return Err(KernelError::StrongholdRequired);
        }
        #[cfg(feature = "stronghold")]
        {
            let vault = self.stronghold_vault().ok_or(KernelError::StrongholdRequired)?;
            if !vault.is_unlocked() {
                return Err(KernelError::StrongholdRequired);
            }
        }
        // feature 关闭时 stronghold_enabled() 已返回 false,不会走到这里
        Ok(())
    }

    /// W8 Plan 4: 读取 privacy_mode(spec §6 安全约束)。
    /// 从 `app_config.privacy.mode` 读取,value="true" → true,其他 → false。
    /// 读取失败 / key 缺失 / value 非法 → 默认 false(保守策略)。
    /// `route_text_with_dag` 在 privacy_mode=true 时禁止调 LLM 拆解。
    pub fn privacy_mode(&self) -> bool {
        let conn = self.conn();
        match self.config_repo().get(&conn, "privacy.mode") {
            Ok(Some(v)) => v.trim().eq_ignore_ascii_case("true"),
            _ => false,
        }
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
        self.audit_append(&task.task_id, None, "task_created", serde_json::json!({
            "user_goal": task.user_goal,
        }))?;
        Ok(task)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        self.task_repo.get(&conn, task_id)
    }

    /// Transition a task to a new state. Rejects illegal transitions.
    /// Emits a `state_transition` audit event on success.
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
        self.audit_append(task_id, None, "state_transition", serde_json::json!({
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

    /// W9 Plan 1: 记录 Stronghold 降级模式进入事件(spec §6.4 审计事件表)。
    ///
    /// `reason` 仅取 "wrong_password" / "vault_corrupted" 等常量,不含密码 /
    /// derived_key / salt 等敏感字段(spec §6.1 第 4 条 + §6.4)。
    ///
    /// task_id 占位:降级模式无活跃 task,但 `audit_logs.task_id` 是 FK
    /// REFERENCES `tasks(task_id)`(migrations/001_init.sql:77),字面量
    /// "unknown-task" 会触发 FK 违约。本方法先创建占位 task 行满足 FK,
    /// 再用其 task_id 写审计。step_id = None。
    pub fn stronghold_enter_degraded_mode(&self, reason: &str) -> Result<()> {
        // FK 约束要求 task_id 必须存在于 tasks 表中。先创建占位 task。
        // 用 block scope 限制 MutexGuard 生命周期,避免 audit_append 二次加锁死锁。
        let placeholder_task_id = format!("stronghold-degraded-{}", Uuid::new_v4());
        {
            let conn = self.conn();
            let placeholder = TaskRecord::new(
                &placeholder_task_id,
                "stronghold degraded mode placeholder",
            );
            self.task_repo.create(&conn, &placeholder)?;
        }
        self.audit_append(
            &placeholder_task_id,
            None,
            "stronghold_degraded_mode_entered",
            serde_json::json!({
                "reason": reason,
            }),
        )
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
            "compensation_created",
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
            "compensation_status_changed",
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
            "approval_recorded",
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
            "step_created",
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
            "step_status_changed",
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
            "step_prepared",
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
            "step_committed",
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
    pub fn task_id_for_step(&self, step_id: &str) -> Result<Option<String>> {
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

    // ===== W10 Plan 3: Voice latency sample recording =====

    /// W10 Plan 3: 记录一条 voice latency 样本(spec §5.2/§5.3)。
    ///
    /// 1. 创建占位 task(满足 audit_logs.task_id FK 约束,与 stronghold_enter_degraded_mode 模式一致)
    /// 2. emit `voice_started` audit event(Plan 5 将注册到 AUDIT_EVENT_TYPE_REGISTRY)
    /// 3. INSERT 到 voice_latency_samples 表
    ///
    /// `started_at_ms`:VAD 检测首个 voiced chunk 的 epoch ms(t0)
    /// `latency_ms`:t1 - t0(t1 = 首个 partial transcript 回调)
    /// `model`:sherpa-rs 模型名
    /// `privacy_mode`:true=local only,false=cloud LLM
    pub fn record_voice_latency_sample(
        &self,
        started_at_ms: i64,
        latency_ms: i64,
        model: &str,
        privacy_mode: bool,
    ) -> Result<()> {
        // 1. 占位 task 满足 FK(与 stronghold_enter_degraded_mode 模式一致)。
        // 用 block scope 限制 MutexGuard 生命周期,避免 audit_append 二次加锁死锁。
        let placeholder_task_id = format!("voice-latency-{}", Uuid::new_v4());
        {
            let conn = self.conn();
            let placeholder = TaskRecord::new(&placeholder_task_id, "voice latency sample placeholder");
            self.task_repo.create(&conn, &placeholder)?;
        }
        // 2. emit voice_started audit event
        self.audit_append(
            &placeholder_task_id,
            None,
            "voice_started",
            serde_json::json!({
                "started_at_ms": started_at_ms,
                "latency_ms": latency_ms,
                "model": model,
                "privacy_mode": privacy_mode,
            }),
        )?;
        // 3. INSERT 样本
        {
            let conn = self.conn();
            conn.execute(
                "INSERT INTO voice_latency_samples (started_at_ms, latency_ms, model, privacy_mode)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![started_at_ms, latency_ms, model, privacy_mode as i64],
            )?;
        }
        Ok(())
    }

    /// W10 Plan 3: 计算 voice latency 统计(spec §5.2 compute_stats)。
    ///
    /// `since`:可选 epoch ms 下界,None = 全部样本。
    pub fn compute_voice_latency_stats(
        &self,
        since: Option<i64>,
    ) -> Result<crate::voice_latency::LatencyStats> {
        let conn = self.conn();
        crate::voice_latency::compute_stats(&conn, since)
    }

    /// W10 Plan 3: 清理旧 voice latency 样本(spec §5.2 prune_older_than)。
    ///
    /// `days`:保留天数。返回删除的行数。
    /// `now_ms` 由 caller 传入(便于测试注入固定时间),生产用 `Utc::now().timestamp_millis()`。
    pub fn prune_voice_latency_older_than(&self, days: u32, now_ms: i64) -> Result<u64> {
        let conn = self.conn();
        crate::voice_latency::prune_older_than(&conn, days, now_ms)
    }

    /// W10 Plan 4: 触发 Kill Switch(spec §6.2 Kill Switch 触发路径 step 2-3)。
    ///
    /// 1. 读取当前 task 状态
    /// 2. emit `kill_switch_triggered { timestamp_ms, from_state, source }` audit event
    /// 3. 若当前态为 Idle:直接 transition → Cancelled(无 Cancelling 中间态,≤ 100ms)
    ///    否则:transition → Cancelling(进入中间态,等待 complete_cancellation)
    /// 4. 返回 t0(epoch ms)供 complete_cancellation 计算 SLA
    ///
    /// `task_id`:目标任务 ID(必须已存在于 tasks 表)
    /// `source`:触发来源,如 "voice_command" / "cli" / "ui_button"
    ///
    /// 返回:t0 epoch ms(用于 SLA 计算)
    ///
    /// Audit events emitted:
    /// 1. `kill_switch_triggered { timestamp_ms, from_state, source }`
    /// 2. `state_transition { from: <current>, to: Cancelling | Cancelled }`(via self.transition)
    ///
    /// 注意:若 task 已在终态(Done/Failed/Cancelled),仅 emit kill_switch_triggered
    /// for audit,不 transition(返回 Err 会让 caller 难以继续)。返回当前 epoch ms。
    pub fn trigger_kill_switch(&self, task_id: &str, source: &str) -> Result<i64> {
        let now_ms = Utc::now().timestamp_millis();
        let current = self
            .get_task(task_id)?
            .ok_or_else(|| KernelError::TaskNotFound(task_id.to_string()))?;
        let from_state = current.status;

        // 1. emit kill_switch_triggered audit event
        self.audit_append(
            task_id,
            None,
            "kill_switch_triggered",
            serde_json::json!({
                "timestamp_ms": now_ms,
                "from_state": from_state,
                "source": source,
            }),
        )?;

        // 2. 若已终态,不再 transition
        let is_terminal = matches!(
            from_state,
            TaskState::Done | TaskState::Failed | TaskState::Cancelled
        );
        if !is_terminal {
            // 3. Idle 特殊处理:直接 → Cancelled(无 Cancelling 中间态)
            //    其他非终态:→ Cancelling(中间态,等 complete_cancellation)
            let target = if from_state == TaskState::Idle {
                TaskState::Cancelled
            } else {
                TaskState::Cancelling
            };
            // transition 内部会 emit state_transition audit event
            self.transition(task_id, target)?;
        }

        Ok(now_ms)
    }

    /// W10 Plan 4: 完成任务取消(spec §6.2 Kill Switch 触发路径 step 4-5)。
    ///
    /// 1. 读取当前 task 状态
    /// 2. 若当前态为 Cancelling:transition → Cancelled(emit state_transition)
    ///    若当前态已为 Cancelled(Idle 直跳路径):不重复 transition
    /// 3. 计算 duration_ms = now_ms - triggered_at_ms
    /// 4. 计算 sla_met = duration_ms <= 1000
    /// 5. emit `task_cancelled { duration_ms, sla_met }` audit event
    ///
    /// `task_id`:目标任务 ID
    /// `triggered_at_ms`:t0 epoch ms(来自 trigger_kill_switch 返回值)
    /// `now_ms`:t1 epoch ms(caller 传入便于测试注入;生产用 Utc::now().timestamp_millis())
    ///
    /// Audit events emitted:
    /// 1. `state_transition { from: Cancelling, to: Cancelled }`(若当前态为 Cancelling)
    /// 2. `task_cancelled { duration_ms, sla_met }`
    pub fn complete_cancellation(
        &self,
        task_id: &str,
        triggered_at_ms: i64,
        now_ms: i64,
    ) -> Result<()> {
        let current = self
            .get_task(task_id)?
            .ok_or_else(|| KernelError::TaskNotFound(task_id.to_string()))?;

        // 1. 若当前态为 Cancelling,transition → Cancelled
        //    若已为 Cancelled(Idle 直跳路径),跳过 transition
        if current.status == TaskState::Cancelling {
            self.transition(task_id, TaskState::Cancelled)?;
        }

        // 2. 计算 SLA
        let duration_ms = now_ms.saturating_sub(triggered_at_ms);
        let sla_met = duration_ms <= 1000;

        // 3. emit task_cancelled audit event
        self.audit_append(
            task_id,
            None,
            "task_cancelled",
            serde_json::json!({
                "duration_ms": duration_ms,
                "sla_met": sla_met,
            }),
        )?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_apps_default_and_setter() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let apps = kernel.allowed_apps();
        assert_eq!(
            *apps,
            vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ]
        );
        drop(apps);

        kernel.set_allowed_apps(vec!["code".to_string(), "terminal".to_string()]);
        let apps2 = kernel.allowed_apps();
        assert_eq!(
            *apps2,
            vec!["code".to_string(), "terminal".to_string()]
        );
    }

    /// W7 Plan 4: boot load —— Settings 持久化的 allowed_apps 必须在
    /// `TrustKernel::open_file` 时从 KV 读取并覆盖默认值。用 tempfile
    /// 模拟"先 Settings 写入 → 关闭进程 → 重启"流程。
    #[test]
    fn allowed_apps_loads_from_kv_at_boot() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");

        // Phase 1: 构造 kernel,把自定义白名单写入 KV(模拟 Settings 持久化)。
        {
            let kernel = TrustKernel::open_file(path).expect("open_file phase 1");
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(
                    &conn,
                    "uia.allowed_apps",
                    r#"["code","terminal","vim"]"#,
                )
                .expect("set uia.allowed_apps");
        }

        // Phase 2: 重新打开同一个 DB 文件(模拟进程重启)。with_conn 应该
        // 从 KV 加载 allowed_apps 并覆盖默认值。
        {
            let kernel = TrustKernel::open_file(path).expect("open_file phase 2");
            let apps = kernel.allowed_apps();
            assert_eq!(
                *apps,
                vec![
                    "code".to_string(),
                    "terminal".to_string(),
                    "vim".to_string(),
                ],
                "boot load should pick up KV-persisted allowed_apps"
            );
        }
    }

    /// W7 Plan 4: boot load —— KV 中 allowed_apps 为空串或缺失时,
    /// 保持默认 ["notepad", "explorer", "calc"]。
    #[test]
    fn allowed_apps_keeps_default_when_kv_missing() {
        let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
        let apps = kernel.allowed_apps();
        assert_eq!(
            *apps,
            vec![
                "notepad".to_string(),
                "explorer".to_string(),
                "calc".to_string(),
            ]
        );
    }

    /// W7 Plan 4: boot load —— KV 中 allowed_apps 损坏(非 JSON)时,
    /// 保持默认值并 warn!(不向上传播错误)。
    #[test]
    fn allowed_apps_keeps_default_when_kv_corrupted() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");

        // Phase 1: 写入损坏的 JSON 字符串到 KV。
        {
            let kernel = TrustKernel::open_file(path).expect("open_file phase 1");
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "uia.allowed_apps", "not-a-json-array")
                .expect("set corrupted uia.allowed_apps");
        }

        // Phase 2: 重新打开 —— 解析失败时保持默认值(不 panic、不返回 Err)。
        {
            let kernel = TrustKernel::open_file(path).expect("open_file phase 2");
            let apps = kernel.allowed_apps();
            assert_eq!(
                *apps,
                vec![
                    "notepad".to_string(),
                    "explorer".to_string(),
                    "calc".to_string(),
                ]
            );
        }
    }

    // ===== W8 Plan 4 Task 2: llm_client + privacy_mode accessors =====

    #[cfg(feature = "llm")]
    #[test]
    fn llm_client_default_is_none() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        assert!(kernel.llm_client().is_none(), "default llm_client must be None");
    }

    #[cfg(feature = "llm")]
    #[test]
    fn llm_client_setter_round_trip() {
        use crate::llm::client::LlmClient;
        let kernel = TrustKernel::open_in_memory().unwrap();
        let llm = Arc::new(LlmClient::new(
            "https://api.deepseek.com/v1",
            "sk-test",
            "deepseek-chat",
        ));
        kernel.set_llm_client(Some(llm.clone()));
        let got = kernel.llm_client();
        assert!(got.is_some());
        assert!(got.as_ref().unwrap().is_enabled());
        assert_eq!(got.as_ref().unwrap().base_url(), "https://api.deepseek.com/v1");
    }

    #[cfg(feature = "llm")]
    #[test]
    fn llm_client_setter_clears() {
        use crate::llm::client::LlmClient;
        let kernel = TrustKernel::open_in_memory().unwrap();
        let llm = Arc::new(LlmClient::new("https://x", "sk", "m"));
        kernel.set_llm_client(Some(llm));
        assert!(kernel.llm_client().is_some());
        kernel.set_llm_client(None);
        assert!(kernel.llm_client().is_none());
    }

    #[test]
    fn privacy_mode_default_is_false() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        assert!(!kernel.privacy_mode(), "default privacy_mode must be false");
    }

    #[test]
    fn privacy_mode_reads_true_from_kv() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");
        {
            let kernel = TrustKernel::open_file(path).unwrap();
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "privacy.mode", "true")
                .unwrap();
        }
        let kernel = TrustKernel::open_file(path).unwrap();
        assert!(kernel.privacy_mode(), "privacy_mode=true must be read from KV");
    }

    #[test]
    fn privacy_mode_reads_false_from_kv() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");
        {
            let kernel = TrustKernel::open_file(path).unwrap();
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "privacy.mode", "false")
                .unwrap();
        }
        let kernel = TrustKernel::open_file(path).unwrap();
        assert!(!kernel.privacy_mode());
    }

    #[test]
    fn privacy_mode_treats_invalid_as_false() {
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path = tmp.path().to_str().expect("tempfile path is utf-8");
        {
            let kernel = TrustKernel::open_file(path).unwrap();
            let conn = kernel.conn();
            kernel
                .config_repo()
                .set(&conn, "privacy.mode", "not-a-bool")
                .unwrap();
        }
        let kernel = TrustKernel::open_file(path).unwrap();
        assert!(!kernel.privacy_mode(), "invalid privacy.mode value must default to false");
    }

    // ===== W10 Plan 3: voice latency sample recording =====

    #[test]
    fn record_voice_latency_sample_writes_row_and_emits_audit_event() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel
            .record_voice_latency_sample(1_700_000_000_000, 120, "sense_voice", false)
            .unwrap();

        // 验证 voice_latency_samples 表有 1 行
        let conn = kernel.conn();
        let (latency_ms, model, privacy_mode): (i64, String, i64) = conn
            .query_row(
                "SELECT latency_ms, model, privacy_mode FROM voice_latency_samples",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(latency_ms, 120);
        assert_eq!(model, "sense_voice");
        assert_eq!(privacy_mode, 0);
        drop(conn);

        // 验证 voice_started audit event 已 emit
        let events = kernel.list_audit_recent(10).unwrap();
        let voice_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "voice_started")
            .collect();
        assert_eq!(
            voice_events.len(),
            1,
            "exactly 1 voice_started event expected, got {}",
            voice_events.len()
        );
    }

    #[test]
    fn record_voice_latency_sample_multiple_rows_accumulate() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        kernel
            .record_voice_latency_sample(1_000, 50, "m1", false)
            .unwrap();
        kernel
            .record_voice_latency_sample(2_000, 80, "m1", false)
            .unwrap();
        kernel
            .record_voice_latency_sample(3_000, 120, "m2", true)
            .unwrap();

        let conn = kernel.conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM voice_latency_samples",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3, "3 samples should be recorded");
        drop(conn);

        // 3 个 voice_started audit events
        let events = kernel.list_audit_recent(100).unwrap();
        let voice_count = events
            .iter()
            .filter(|e| e.event_type == "voice_started")
            .count();
        assert_eq!(voice_count, 3);
    }

    #[test]
    fn compute_voice_latency_stats_returns_correct_percentiles() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // 插入 100 样本,latency = 100..200
        for i in 0..100i64 {
            kernel
                .record_voice_latency_sample(1_000_000 + i, 100 + i, "m", false)
                .unwrap();
        }
        let stats = kernel.compute_voice_latency_stats(None).unwrap();
        assert_eq!(stats.sample_count, 100);
        assert_eq!(stats.p50_ms, 149);
        assert_eq!(stats.p95_ms, 194);
        assert_eq!(stats.p99_ms, 198);
        assert_eq!(stats.max_ms, 199);
    }

    #[test]
    fn prune_voice_latency_older_than_deletes_old_samples() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let now_ms: i64 = 10_000_000_000;
        // 60 天前样本(应删除)+ 当前样本(应保留)
        kernel
            .record_voice_latency_sample(
                now_ms - 60 * 86_400 * 1000,
                100,
                "m",
                false,
            )
            .unwrap();
        kernel
            .record_voice_latency_sample(now_ms, 200, "m", false)
            .unwrap();

        let deleted = kernel.prune_voice_latency_older_than(30, now_ms).unwrap();
        assert_eq!(deleted, 1, "should delete 1 old sample");

        let stats = kernel.compute_voice_latency_stats(None).unwrap();
        assert_eq!(stats.sample_count, 1);
        assert_eq!(stats.max_ms, 200);
    }
}
