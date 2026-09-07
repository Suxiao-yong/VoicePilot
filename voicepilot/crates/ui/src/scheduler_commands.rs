//! Phase C: 后台定时作业 Tauri 命令（UI 列表面，R4 核 5 降级路径）。
//!
//! 3 个命令:
//! - `list_agent_jobs_command`:列出所有作业（含下次运行时间）
//! - `list_job_runs_command`:查某作业的执行历史
//! - `disable_agent_job_command`:停用作业（task.unschedule 的 UI 等价物）
//!
//! 安全规则:WebView 只读 scheduler 的 repo 函数，不直接持有 DB 连接。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;
use trust_kernel::scheduler::{AgentJob, JobRun};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentJobDto {
    pub job_id: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub last_run_at_ms: Option<i64>,
    pub next_run_at_ms: Option<i64>,
}

impl From<AgentJob> for AgentJobDto {
    fn from(j: AgentJob) -> Self {
        Self {
            job_id: j.job_id,
            prompt: j.prompt,
            schedule: j.schedule,
            enabled: j.enabled,
            created_at_ms: j.created_at_ms,
            last_run_at_ms: j.last_run_at_ms,
            next_run_at_ms: j.next_run_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRunDto {
    pub run_id: String,
    pub job_id: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub outcome: String,
    pub result: String,
}

impl From<JobRun> for JobRunDto {
    fn from(r: JobRun) -> Self {
        Self {
            run_id: r.run_id,
            job_id: r.job_id,
            started_at_ms: r.started_at_ms,
            finished_at_ms: r.finished_at_ms,
            outcome: r.outcome,
            result: r.result,
        }
    }
}

/// 逻辑函数：列出所有定时作业（供测试直接调用）。
pub fn list_agent_jobs(state: &AppState) -> UiResult<Vec<AgentJobDto>> {
    let conn = state.kernel.conn();
    Ok(trust_kernel::scheduler::list_jobs(&conn)?
        .into_iter()
        .map(AgentJobDto::from)
        .collect())
}

/// 逻辑函数：查某作业执行历史（供测试直接调用）。
pub fn list_job_runs(state: &AppState, job_id: &str, limit: usize) -> UiResult<Vec<JobRunDto>> {
    let conn = state.kernel.conn();
    Ok(trust_kernel::scheduler::list_runs(&conn, job_id, limit)?
        .into_iter()
        .map(JobRunDto::from)
        .collect())
}

/// 逻辑函数：停用作业（供测试直接调用）。不存在 → 显式错误。
pub fn disable_agent_job(state: &AppState, job_id: &str) -> UiResult<()> {
    let conn = state.kernel.conn();
    if trust_kernel::scheduler::set_job_enabled(&conn, job_id, false)? {
        Ok(())
    } else {
        Err(crate::error::UiError::InvalidConfig(format!(
            "job '{job_id}' not found"
        )))
    }
}

#[tauri::command]
pub async fn list_agent_jobs_command(
    state: State<'_, AppState>,
) -> Result<Vec<AgentJobDto>, String> {
    list_agent_jobs(&state).map_err(Into::into)
}

#[tauri::command]
pub async fn list_job_runs_command(
    state: State<'_, AppState>,
    job_id: String,
    limit: Option<usize>,
) -> Result<Vec<JobRunDto>, String> {
    list_job_runs(&state, &job_id, limit.unwrap_or(20)).map_err(Into::into)
}

#[tauri::command]
pub async fn disable_agent_job_command(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    disable_agent_job(&state, &job_id).map_err(Into::into)
}
