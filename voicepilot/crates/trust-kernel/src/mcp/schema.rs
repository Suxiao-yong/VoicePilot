//! MCP tool schema — V1.1 §6.1 + Appendix B.
//!
//! Mirrors the MCP Tool schema: name, description, inputSchema (JSON Schema),
//! outputSchema (JSON Schema), annotations (readOnlyHint etc.).
//! Annotations are treated as untrusted hints per V1.1 §6.1 — the Rust-side
//! Action Gateway is the authoritative source of risk classification.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub annotations: McpAnnotations,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAnnotations {
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}

// ===== W11 Plan 4: annotation 一致性校验(spec §6.1)=====

/// 写操作 tool 名标记 —— 名字含任一标记即视为会产生副作用的写工具。
/// 覆盖 mock 恶意 server 与真实 MCP 生态常见写工具命名。
const WRITE_TOOL_MARKERS: &[&str] = &[
    "write", "delete", "remove", "move", "create", "mkdir", "rmdir", "append",
    "edit", "update", "patch", "save", "upload", "send", "export", "rm", "mv", "cp",
];

/// 校验 MCP tool annotation 与实际行为的一致性(spec §6.1)。
///
/// 接收 `tools/list` 返回的**单个 tool 原始 JSON**(含 `name` / `annotations` /
/// 自定义 `effectManifest` 字段)。MCP annotation 是**不可信提示** —— 由外部
/// MCP server 声明,Trust Kernel 不信任。检测两类谎报:
///
///   1. `readOnlyHint=true` 但 tool 名明显是写操作(write/delete/move/...)
///   2. 自定义 `effectManifest.read=true` 声称只读,但 tool 名明显是写操作
///
/// 返回 `Ok(())` 表示 annotation 与行为一致;返回 `Err(KernelError::MaliciousServer)`
/// 表示谎报只读。调用方(评测 harness / 未来 dispatcher)在检测到时拦截调用
/// 并审计 `malicious_server_detected` 事件。
pub fn verify_mcp_annotations(tool_json: &serde_json::Value) -> crate::error::Result<()> {
    let name = tool_json.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let name_lower = name.to_lowercase();
    let is_write_tool = WRITE_TOOL_MARKERS.iter().any(|m| name_lower.contains(m));
    if !is_write_tool {
        return Ok(());
    }
    let read_only_hint = tool_json
        .pointer("/annotations/readOnlyHint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let effect_read_only = tool_json
        .pointer("/effectManifest/read")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if read_only_hint || effect_read_only {
        return Err(crate::error::KernelError::MaliciousServer(format!(
            "tool '{}' is a write tool but advertises read-only (readOnlyHint={}, effectManifest.read={})",
            name, read_only_hint, effect_read_only
        )));
    }
    Ok(())
}
