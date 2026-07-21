//! Audit Viewer Tauri commands —— V1.1.2 §8.3 Audit Viewer 后端。
//!
//! 仅暴露只读查询(§8.2 Audit 窗口权限:只读,禁止任何写操作)。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEventDto {
    pub log_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub details: serde_json::Value,
    pub timestamp: String,
    pub prev_hash: Option<String>,
    pub hash: String,
}

impl From<trust_kernel::audit::AuditEvent> for AuditEventDto {
    fn from(e: trust_kernel::audit::AuditEvent) -> Self {
        Self {
            log_id: e.log_id,
            task_id: e.task_id,
            step_id: e.step_id,
            event_type: e.event_type,
            details: e.details,
            timestamp: e.timestamp.to_rfc3339(),
            prev_hash: e.prev_hash,
            hash: e.hash,
        }
    }
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn list_audit_recent(state: &AppState, limit: usize) -> UiResult<Vec<AuditEventDto>> {
    let events = state.kernel.list_audit_recent(limit)?;
    Ok(events.into_iter().map(AuditEventDto::from).collect())
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn list_audit_for_task(state: &AppState, task_id: &str) -> UiResult<Vec<AuditEventDto>> {
    let events = state.kernel.list_audit_for_task(task_id)?;
    Ok(events.into_iter().map(AuditEventDto::from).collect())
}

#[tauri::command]
pub async fn list_audit_recent_command(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<AuditEventDto>, String> {
    list_audit_recent(&state, limit).map_err(Into::into)
}

#[tauri::command]
pub async fn list_audit_for_task_command(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Vec<AuditEventDto>, String> {
    list_audit_for_task(&state, &task_id).map_err(Into::into)
}
