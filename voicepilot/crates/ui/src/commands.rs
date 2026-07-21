//! Tauri commands —— V1.1 §8.2 webview 与 trust-kernel 之间的 IPC 桥接。
//!
//! 所有 commands 都用 `#[cfg(feature = "tauri")]` 门控。它们接收 `&AppState`
//! (由 Tauri 管理)并返回 `Result<T, String>` 供 webview 消费。

use crate::error::UiResult;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// 镜像 `trust_kernel::voice::router_bridge::RouteOutcome`,但带 Serialize
/// 作为 Tauri command 返回类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteTextResult {
    Routed { skill_id: String },
    Unmatched { text: String },
    Empty,
}

/// 通过 SkillRouter 路由转写文本(或任意文本输入)。
/// V1.1 §5.1 —— 纯关键词匹配(W7 将添加 LLM Planner fallback)。
pub fn route_text(state: &AppState, text: &str) -> UiResult<RouteTextResult> {
    use trust_kernel::skills::manifest::files_organize_manifest;
    use trust_kernel::skills::router::{RouteDecision, SkillRouter};

    // `state` 暂未在路由中使用 —— W7 Planner fallback 会用它访问 TrustKernel。
    let _ = state;

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteTextResult::Empty);
    }
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteTextResult::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteTextResult::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn route_text_command(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<RouteTextResult, String> {
    route_text(&state, &text).map_err(Into::into)
}
