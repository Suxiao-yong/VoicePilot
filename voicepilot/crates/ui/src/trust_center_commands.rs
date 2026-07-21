//! Trust Center Tauri commands —— V1.1.2 §8.3 Trust Center 后端。
//!
//! 展示 MCP Server 列表 + 一键停用/启用(§8.3 V1.1 新界面)。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::UiResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDto {
    pub server_id: String,
    pub name: String,
    pub version: String,
    pub transport: String,
    pub enabled: bool,
    pub trusted: bool,
    pub protocol_version: Option<String>,
    pub allowed_origins: Option<String>,
    pub allowed_paths: Option<String>,
}

impl From<trust_kernel::mcp::repo::McpServerRecord> for McpServerDto {
    fn from(r: trust_kernel::mcp::repo::McpServerRecord) -> Self {
        Self {
            server_id: r.server_id,
            name: r.name,
            version: r.version,
            transport: r.transport,
            enabled: r.enabled,
            trusted: r.trusted,
            protocol_version: r.protocol_version,
            allowed_origins: r.allowed_origins,
            allowed_paths: r.allowed_paths,
        }
    }
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn list_mcp_servers(state: &AppState) -> UiResult<Vec<McpServerDto>> {
    let records = state.kernel.list_mcp_servers()?;
    Ok(records.into_iter().map(McpServerDto::from).collect())
}

/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn toggle_mcp_server(state: &AppState, server_id: &str, enabled: bool) -> UiResult<()> {
    state.kernel.toggle_mcp_server(server_id, enabled)?;
    Ok(())
}

#[tauri::command]
pub async fn list_mcp_servers_command(
    state: State<'_, AppState>,
) -> Result<Vec<McpServerDto>, String> {
    list_mcp_servers(&state).map_err(Into::into)
}

#[tauri::command]
pub async fn toggle_mcp_server_command(
    state: State<'_, AppState>,
    server_id: String,
    enabled: bool,
) -> Result<(), String> {
    toggle_mcp_server(&state, &server_id, enabled).map_err(Into::into)
}
