//! Task 2 —— 一键撤销（Undo）Tauri commands（契约 §2.3）。
//!
//! 两个命令：
//! - `list_compensable_actions_command`：列出当前 `status='active'` 的补偿记录
//!   （`kernel.list_active_compensations()`），前端据此渲染撤销入口。
//! - `undo_compensation_command`：用户主动点击撤销 → 走 `task.compensate`
//!   executor 完整管道（prepare → 审批门 → auto_reverse → verify → finalize）。
//!
//! 审批用 `AutoApprover`：这是**用户主动点击**的撤销，不是自动执行 —— 语义上
//! 已有人工确认；`task.compensate` 当前也不在审批白名单内（见
//! `skills::simple::approval_required`），注入 `TauriApprover` 反而会挂起等窗。
//!
//! 成功标记：补偿记录 status = **`"reversed"`**（契约勘误 #6，见
//! `task_compensate.rs` 的 `mark_compensation_status(comp_id, "reversed")`）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

/// 可补偿动作 DTO（映射 `CompensationRecord`，前端撤销列表 / 结果卡片用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensableAction {
    /// 补偿记录 ID。
    pub comp_id: String,
    /// 被补偿（原始）步骤 ID —— 撤销时作为 `target_step_id`。
    pub step_id: String,
    /// 补偿等级：none | best_effort | strong。
    pub level: String,
    /// 记录状态：active | reversed | consumed | expired | failed。
    pub status: String,
    /// 补偿 TTL 过期时刻（RFC3339）。
    pub ttl_expires: String,
    /// 人类可读描述（补偿函数名 + 等级；不含快照密文 / 密钥）。
    pub description: String,
}

/// 撤销入参：`task_id` / `step_id` 是本次撤销操作新建的 task/step，
/// `target_step_id` 指向要回滚的原始步骤（契约 §2.3）。
#[derive(Debug, Clone, Deserialize)]
pub struct UndoInput {
    pub task_id: String,
    pub step_id: String,
    pub target_step_id: String,
}

/// 撤销结果（IPC 级故障才走 `Err`，执行级失败走 `error` 字段）。
#[derive(Debug, Clone, Serialize)]
pub struct UndoResult {
    pub committed: bool,
    /// 成功固定为 `"reversed"`；失败为 `"failed"`。
    pub compensation_status: String,
    pub summary: String,
    pub error: Option<String>,
}

/// 组装 `CompensationRecord` → 前端 DTO 的可读描述（无密文、无路径明文）。
fn describe(rec: &trust_kernel::compensation::types::CompensationRecord) -> String {
    if rec.compensate_fn.trim().is_empty() {
        format!("回滚该步骤（{}）", rec.level.as_str())
    } else {
        format!("{}（{}）", rec.compensate_fn, rec.level.as_str())
    }
}

/// 列出当前可撤销的补偿记录（只读）。
pub fn list_compensable_actions(state: &AppState) -> UiResult<Vec<CompensableAction>> {
    let records = state.kernel.list_active_compensations()?;
    Ok(records
        .into_iter()
        .map(|rec| CompensableAction {
            description: describe(&rec),
            comp_id: rec.comp_id,
            step_id: rec.step_id,
            level: rec.level.as_str().to_string(),
            status: rec.status,
            ttl_expires: rec.ttl_expires,
        })
        .collect())
}

/// 执行一键撤销：调 `task.compensate` executor（E2 + 强验证）。
///
/// 返回 `UndoResult`；executor 的任何失败（无 active 记录 / 回滚失败 /
/// 验证不通过）都收敛到 `error` 字段，`committed=false`。
pub fn undo_compensation(state: &AppState, input: &UndoInput) -> UiResult<UndoResult> {
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::skills::task_compensate::{TaskCompensateInput, execute_compensate};

    let compensate_input = TaskCompensateInput {
        task_id: input.task_id.clone(),
        step_id: input.step_id.clone(),
        target_step_id: input.target_step_id.clone(),
    };
    match execute_compensate(&state.kernel, &compensate_input, &AutoApprover) {
        Ok(_new_task_id) => Ok(UndoResult {
            committed: true,
            compensation_status: "reversed".to_string(),
            summary: format!("已回滚 {} 的副作用", input.target_step_id),
            error: None,
        }),
        Err(e) => Ok(UndoResult {
            committed: false,
            compensation_status: "failed".to_string(),
            summary: String::new(),
            error: Some(e.to_string()),
        }),
    }
}

/// 列出可撤销动作（前端撤销列表）。
#[tauri::command]
pub async fn list_compensable_actions_command(
    state: State<'_, AppState>,
) -> Result<Vec<CompensableAction>, String> {
    list_compensable_actions(&state).map_err(Into::into)
}

/// 一键撤销：回滚 `target_step_id` 对应补偿记录的副作用。
#[tauri::command]
pub async fn undo_compensation_command(
    state: State<'_, AppState>,
    input: UndoInput,
) -> Result<UndoResult, String> {
    undo_compensation(&state, &input).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trust_kernel::compensation::types::{
        CompensationLevel, CompensationRecord, ConflictPolicy,
    };
    use trust_kernel::repo::step_repo::StepRecord;

    fn seed_active_compensation(kernel: &trust_kernel::kernel::TrustKernel) {
        kernel.create_task("task-1", "organize").unwrap();
        kernel
            .create_step(&StepRecord::new("step-1", "task-1", 1))
            .unwrap();
        kernel
            .create_compensation(&CompensationRecord {
                comp_id: "comp-1".to_string(),
                step_id: "step-1".to_string(),
                level: CompensationLevel::Strong,
                snapshot_encrypted: None,
                ttl_expires: "2099-01-01T00:00:00+00:00".to_string(),
                status: "active".to_string(),
                snapshot_vault_ref: None,
                conflict_policy: ConflictPolicy::AutoReverse,
                compensate_fn: "filesystem.reverse_move".to_string(),
                reverse_payload: r#"{"moves":[]}"#.to_string(),
            })
            .unwrap();
    }

    #[test]
    fn list_compensable_actions_maps_active_records() {
        let state = AppState::new_in_memory().unwrap();
        seed_active_compensation(&state.kernel);

        let actions = list_compensable_actions(&state).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].comp_id, "comp-1");
        assert_eq!(actions[0].step_id, "step-1");
        assert_eq!(actions[0].level, "strong");
        assert_eq!(actions[0].status, "active");
        assert!(actions[0].description.contains("filesystem.reverse_move"));
    }

    #[test]
    fn undo_compensation_reports_error_when_no_active_record() {
        let state = AppState::new_in_memory().unwrap();
        let input = UndoInput {
            task_id: "undo-task".to_string(),
            step_id: "undo-step".to_string(),
            target_step_id: "missing-step".to_string(),
        };
        let result = undo_compensation(&state, &input).unwrap();
        assert!(!result.committed);
        assert_eq!(result.compensation_status, "failed");
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("no active compensation")
        );
    }
}
