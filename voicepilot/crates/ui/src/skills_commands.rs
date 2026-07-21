//! Skills Manager Tauri commands —— V1.1.2 §8.3 Skills Manager 后端。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDto {
    pub skill_id: String,
    pub version: String,
    pub enabled: bool,
    pub success_count: i64,
    pub avg_latency_ms: f64,
    /// 从 manifest_json 解析出的风险等级(E×D),用于前端展示。
    pub risk_label: String,
}

impl From<trust_kernel::skills::repo::SkillRecord> for SkillDto {
    fn from(r: trust_kernel::skills::repo::SkillRecord) -> Self {
        let risk_label = serde_json::from_str::<serde_json::Value>(&r.manifest_json)
            .ok()
            .and_then(|v| v.get("risk").and_then(|r| r.as_str().map(String::from)))
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            skill_id: r.skill_id,
            version: r.version.to_string(),
            enabled: r.enabled,
            success_count: r.success_count,
            avg_latency_ms: r.avg_latency_ms,
            risk_label,
        }
    }
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn list_skills(state: &AppState) -> UiResult<Vec<SkillDto>> {
    let records = state.kernel.list_skills()?;
    Ok(records.into_iter().map(SkillDto::from).collect())
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn toggle_skill(state: &AppState, skill_id: &str, enabled: bool) -> UiResult<()> {
    state.kernel.toggle_skill(skill_id, enabled)?;
    Ok(())
}

#[tauri::command]
pub async fn list_skills_command(
    state: State<'_, AppState>,
) -> Result<Vec<SkillDto>, String> {
    list_skills(&state).map_err(Into::into)
}

#[tauri::command]
pub async fn toggle_skill_command(
    state: State<'_, AppState>,
    skill_id: String,
    enabled: bool,
) -> Result<(), String> {
    toggle_skill(&state, &skill_id, enabled).map_err(Into::into)
}
