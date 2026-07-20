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
