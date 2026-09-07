//! Trust Center Tauri commands —— V1.1.2 §8.3 Trust Center 后端。
//!
//! 展示 MCP Server 列表 + 一键停用/启用(§8.3 V1.1 新界面)。

use serde::{Deserialize, Serialize};
use tauri::State;

use trust_kernel::mcp::repo::McpServerRecord;

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
    /// W7 Plan 5: spawn command (e.g. "npx"). `None` for in-process servers.
    pub command: Option<String>,
    /// W7 Plan 5: JSON array of args (e.g. `["-y","@playwright/mcp@latest"]`).
    pub args: Option<String>,
    /// W7 Plan 5: JSON object of env overrides (e.g. `{"FOO":"bar"}`).
    /// Values are configuration secrets: never emitted into audit/log output.
    pub env: Option<String>,
}

impl From<McpServerRecord> for McpServerDto {
    fn from(r: McpServerRecord) -> Self {
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
            command: r.command,
            args: r.args,
            env: r.env,
        }
    }
}

impl McpServerDto {
    /// Map the input DTO onto the persisted record. JSON `args` / `env` are
    /// validated at the kernel boundary (`register_mcp_server`) before any
    /// write.
    pub fn into_record(&self) -> McpServerRecord {
        McpServerRecord {
            server_id: self.server_id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            transport: self.transport.clone(),
            enabled: self.enabled,
            trusted: self.trusted,
            protocol_version: self.protocol_version.clone(),
            allowed_origins: self.allowed_origins.clone(),
            allowed_paths: self.allowed_paths.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
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

/// Wave 2 Task 2.2: register a new MCP plugin. Validation happens in the
/// kernel before any write; success rebuilds the extension catalog.
/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn register_mcp_server(state: &AppState, dto: McpServerDto) -> UiResult<()> {
    state.kernel.register_mcp_server(dto.into_record())?;
    Ok(())
}

/// Wave 2 Task 2.2: remove an MCP plugin. Removal fails when the server is
/// still referenced by a User Skill execution target (error lists the
/// referencing Skill IDs). Note: a currently running task that already took
/// its extension snapshot is not tracked per snapshot today — see plan
/// Task 2.2 running-task note.
/// 逻辑函数:返回 UiResult,供测试直接调用。
pub fn remove_mcp_server(state: &AppState, server_id: &str) -> UiResult<()> {
    state.kernel.remove_mcp_server(server_id)?;
    Ok(())
}

// ===== 主流标准 MCP 配置兼容（2026-08-24 统一） =====
// 标准 JSON:`{"mcpServers":{name:{command,args,env,type?}}}`；兼容 VS Code
// 变体 `servers` 根键。导入默认 enabled=true / trusted=false（安全默认，
// 需用户在 UI 显式标记信任后才可被 Skill 执行）；单条失败逐条报错不整体回滚。

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportErrorItem {
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportMcpResult {
    pub imported: usize,
    pub errors: Vec<ImportErrorItem>,
}

/// 解析标准 mcpServers JSON 并逐条注册。
pub fn import_mcp_servers(state: &AppState, json: &str) -> UiResult<ImportMcpResult> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| crate::error::UiError::InvalidConfig(format!("mcp json: {e}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| crate::error::UiError::InvalidConfig("mcp json must be an object".into()))?;
    let servers = obj
        .get("mcpServers")
        .or_else(|| obj.get("servers")) // VS Code 变体
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            crate::error::UiError::InvalidConfig(
                "missing mcpServers (or servers) object in JSON".into(),
            )
        })?;

    let mut imported = 0usize;
    let mut errors = Vec::new();
    for (name, cfg) in servers {
        let mut err = |msg: String| errors.push(ImportErrorItem {
            name: name.clone(),
            error: msg,
        });
        let Some(cfg) = cfg.as_object() else {
            err("server entry must be an object".into());
            continue;
        };
        // transport:本期仅 stdio;http/streamable-http 明确报错而非静默降级
        let transport = cfg
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("stdio");
        if transport != "stdio" {
            err(format!("transport '{transport}' not supported yet (stdio only)"));
            continue;
        }
        let Some(command) = cfg.get("command").and_then(|v| v.as_str()) else {
            err("missing string field 'command' for stdio server".into());
            continue;
        };
        let args = cfg
            .get("args")
            .map(|v| v.to_string());
        // env：复用 import_external_mcp 的 Phase A keyring 收编 —— DB 只存引用
        // `{"$keyring": "voicepilot/mcp/<server>/<KEY>"}`，非明文密钥。
        // 逐条报错：某 entry 的 env 畸形或 keyring 写失败只进该条 errors，不阻断其余。
        let env = if let Some(env_val) = cfg.get("env") {
            let obj = match env_val.as_object() {
                Some(o) => o,
                None => {
                    err("'env' must be an object".into());
                    continue;
                }
            };
            let mut env_map = std::collections::BTreeMap::new();
            let mut malformed = false;
            for (k, v) in obj {
                match v.as_str() {
                    Some(s) => {
                        env_map.insert(k.clone(), s.to_string());
                    }
                    None => {
                        err(format!("env value for key '{k}' must be a string"));
                        malformed = true;
                        break;
                    }
                }
            }
            if malformed {
                continue;
            }
            match collect_external_mcp_env(state, name, &env_map) {
                Ok((json, _)) => Some(json),
                Err(e) => {
                    err(e.to_string());
                    continue;
                }
            }
        } else {
            None
        };
        let dto = McpServerDto {
            server_id: name.clone(),
            name: name.clone(),
            version: "1.0.0".to_string(),
            transport: "stdio".to_string(),
            // 安全默认:启用展示但不可被执行,须 UI 显式标记 trusted
            enabled: true,
            trusted: false,
            protocol_version: Some("2025-11-25".to_string()),
            allowed_origins: None,
            allowed_paths: None,
            command: Some(command.to_string()),
            args,
            env,
        };
        match register_mcp_server(state, dto) {
            Ok(()) => imported += 1,
            Err(e) => err(e.to_string()),
        }
    }
    Ok(ImportMcpResult { imported, errors })
}

/// 导出标准 `{"mcpServers":{...}}` JSON（剔除 trusted/allowed_paths 等私有字段）。
/// 注意:env 可能含密钥——导出是用户主动行为,与社区格式一致原样输出。
pub fn export_mcp_servers(state: &AppState) -> UiResult<String> {
    let records = state.kernel.list_mcp_servers()?;
    let mut servers = serde_json::Map::new();
    for r in records {
        let mut entry = serde_json::Map::new();
        if let Some(c) = r.command {
            entry.insert("command".to_string(), serde_json::Value::String(c));
        }
        if let Some(a) = r.args {
            entry.insert(
                "args".to_string(),
                serde_json::from_str(&a).unwrap_or(serde_json::Value::String(a)),
            );
        }
        if let Some(e) = r.env {
            entry.insert(
                "env".to_string(),
                serde_json::from_str(&e).unwrap_or(serde_json::Value::String(e)),
            );
        }
        if r.transport == "http" {
            entry.insert("type".to_string(), serde_json::Value::String("http".into()));
        }
        servers.insert(r.server_id, serde_json::Value::Object(entry));
    }
    let doc = serde_json::json!({ "mcpServers": serde_json::Value::Object(servers) });
    serde_json::to_string_pretty(&doc)
        .map_err(|e| crate::error::UiError::InvalidConfig(format!("serialize: {e}")))
}

/// 外部发现 MCP 候选项 DTO：只读扫描结果，不代表已注册/已启用。
/// 前端用已注册列表的 server_id 自行标注“已导入”。只带 env key 名：
/// 值是第三方密钥，展示与导入都不需要，绝不进 renderer 内存。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpDto {
    pub source_file: String,
    /// 跨文件去重后的来源列表（`"<file> (<format>)"`），同内容多来源合并。
    #[serde(default)]
    pub sources: Vec<String>,
    pub format: String,
    pub server_id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env_keys: Vec<String>,
}

/// 被跳过的条目（原因随附，前端展示“跳过 N 个”时不再是黑盒）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSkippedDto {
    pub server_id: String,
    pub reason: String,
}

/// 外部 MCP 扫描结果：命中 + 跳过记账。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpScanResult {
    pub hits: Vec<ExternalMcpDto>,
    pub skipped: Vec<McpSkippedDto>,
}

/// 逻辑函数:扫描全局第三方 MCP 配置（Claude Desktop / Cursor / VS Code）。
/// 纯只读：文件缺失/损坏逐个跳过，不写 DB、不起进程、不触网络。
pub fn scan_external_mcp(_state: &AppState) -> UiResult<ExternalMcpScanResult> {
    use trust_kernel::external_scan::default_windows_mcp_files;
    Ok(scan_external_mcp_with_files(&default_windows_mcp_files()))
}

/// 可注入 files 的扫描实现（单测用 TempDir 固件逐字段断言映射）。
pub fn scan_external_mcp_with_files(
    files: &[(std::path::PathBuf, trust_kernel::external_scan::McpConfigFormat)],
) -> ExternalMcpScanResult {
    use trust_kernel::external_scan::scan_external_mcp_files;
    let report = scan_external_mcp_files(files);
    ExternalMcpScanResult {
        hits: report
            .hits
            .into_iter()
            .map(|h| ExternalMcpDto {
                source_file: h.source_file.to_string_lossy().into_owned(),
                sources: h.sources,
                format: h.format,
                server_id: h.server_id,
                name: h.name,
                command: h.command,
                args: h.args,
                env_keys: h.env_keys,
            })
            .collect(),
        skipped: report
            .skipped
            .into_iter()
            .map(|s| McpSkippedDto {
                server_id: s.server_id,
                reason: s.reason,
            })
            .collect(),
    }
}

/// 逻辑函数:按引用导入外部 MCP 候选（source_file + format + server_id）。
/// secret（env 值）由后端从源文件重读，绝不经过 renderer：受损渲染器
/// 只能让用户导入扫描见过的条目，换不了 command/env。
/// 姿势与手动粘贴导入完全一致：enabled=true（可见可管理）+
/// trusted=false（不可被执行，须在 Trust Center 显式标记信任）。
/// 重复 server_id 由 register 显式拒绝。
pub fn import_external_mcp(
    state: &AppState,
    source_file: &str,
    format: &str,
    server_id: &str,
) -> UiResult<()> {
    import_external_mcp_with_store(state, source_file, format, server_id).map(|_| ())
}

/// 与 `import_external_mcp` 相同，额外返回已收编进 keyring 的 env key 名
/// （前端展示“N 个凭据已迁移进系统凭据库”）。
pub fn import_external_mcp_with_store(
    state: &AppState,
    source_file: &str,
    format: &str,
    server_id: &str,
) -> UiResult<((), Vec<String>)> {
    use trust_kernel::external_scan::{read_external_mcp_entry, McpConfigFormat};
    let parsed_format = match format {
        "claude-desktop" => McpConfigFormat::ClaudeDesktop,
        "cursor" => McpConfigFormat::Cursor,
        "vscode" => McpConfigFormat::VsCode,
        other => {
            return Err(crate::error::UiError::InvalidConfig(format!(
                "unknown MCP config format: {other}"
            )))
        }
    };
    // env 值来自内核侧重读（下方 entry.env），非前端供给：受损 renderer
    // 只能引用扫描见过的 (file, id)，换不了 command/env。
    let entry = read_external_mcp_entry(
        std::path::Path::new(source_file),
        parsed_format,
        server_id,
    )
    .ok_or_else(|| {
        crate::error::UiError::InvalidConfig(format!(
            "server '{server_id}' not found on re-read of {source_file}"
        ))
    })?;
    let (env_json, migrated) = collect_external_mcp_env(state, server_id, &entry.env)?;
    let dto = McpServerDto {
        server_id: entry.server_id.clone(),
        name: entry.name,
        version: "external".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: None,
        command: Some(entry.command),
        args: Some(serde_json::to_string(&entry.args).map_err(|e| {
            crate::error::UiError::InvalidConfig(format!("args serialize: {e}"))
        })?),
        env: Some(env_json),
    };
    register_mcp_server(state, dto)?;
    Ok(((), migrated))
}

/// Phase A 密钥收编：把 entry.env 里疑似凭据的值迁移进 SecretStore，
/// DB 只存字符串引用 `"$keyring:voicepilot/mcp/<server_id>/<KEY>"`（满足
/// `validate_mcp_server_record` 的 string→string env 校验，对象形式会在
/// register 时被拒）；非凭据值（如 BASE_URL）原样保留。spawn 前
/// `skills::common::resolve_mcp_env` 把引用换回真实值，缺失 fail-closed。
/// 返回 (DB env JSON 字符串, 已收编的 key 名列表)。错误信息绝不含值本身。
pub(crate) fn collect_external_mcp_env(
    state: &AppState,
    server_id: &str,
    env: &std::collections::BTreeMap<String, String>,
) -> UiResult<(String, Vec<String>)> {
    use trust_kernel::skills::common::{is_credential_env_key, MCP_ENV_KEYRING_PREFIX};
    let mut out = serde_json::Map::new();
    let mut migrated = Vec::new();
    for (key, value) in env {
        if is_credential_env_key(key) {
            let ref_name = format!("{MCP_ENV_KEYRING_PREFIX}{server_id}/{key}");
            state
                .kernel
                .secret_store()
                .set_secret(&ref_name, value)
                .map_err(|e| crate::error::UiError::Tauri(format!("keyring: {e}")))?;
            out.insert(
                key.clone(),
                serde_json::json!(format!("$keyring:{ref_name}")),
            );
            migrated.push(key.clone());
        } else {
            out.insert(key.clone(), serde_json::json!(value));
        }
    }
    Ok((serde_json::to_string(&serde_json::Value::Object(out)).unwrap_or_else(|_| "{}".to_string()), migrated))
}

#[tauri::command]
pub async fn scan_external_mcp_command(
    state: State<'_, AppState>,
) -> Result<ExternalMcpScanResult, String> {
    scan_external_mcp(&state).map_err(Into::into)
}

#[tauri::command]
pub async fn import_external_mcp_command(
    state: State<'_, AppState>,
    source_file: String,
    format: String,
    server_id: String,
) -> Result<(), String> {
    import_external_mcp(&state, &source_file, &format, &server_id).map_err(Into::into)
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

#[tauri::command]
pub async fn register_mcp_server_command(
    state: State<'_, AppState>,
    server: McpServerDto,
) -> Result<(), String> {
    register_mcp_server(&state, server).map_err(Into::into)
}

#[tauri::command]
pub async fn remove_mcp_server_command(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<(), String> {
    remove_mcp_server(&state, &server_id).map_err(Into::into)
}

#[tauri::command]
pub async fn import_mcp_servers_command(
    state: State<'_, AppState>,
    json: String,
) -> Result<ImportMcpResult, String> {
    import_mcp_servers(&state, &json).map_err(Into::into)
}

#[tauri::command]
pub async fn export_mcp_servers_command(
    state: State<'_, AppState>,
) -> Result<String, String> {
    export_mcp_servers(&state).map_err(Into::into)
}
