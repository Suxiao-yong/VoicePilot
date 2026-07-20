//! MCP tool dispatcher — V1.1 §6.1.
//!
//! W3b scope: list_tools() returns schemas for filesystem.* tools;
//! call_tool() dispatches read-only tools (search_files, verify_move) directly.
//! move_files is rejected here — the only legitimate entry to a move_files
//! transaction is through the Skill executor, which enforces the approve phase.
//! W4 will expose prepare_move + commit_move as separate MCP tools so external
//! MCP clients can drive the two-phase protocol themselves.

use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::mcp::schema::{McpAnnotations, McpToolSchema};

#[derive(Debug, Clone, Default)]
pub struct McpHandler;

#[derive(Debug, Clone)]
pub enum McpCallResult {
    Ok(serde_json::Value),
    Err(String),
}

impl McpHandler {
    pub fn new() -> Self {
        Self
    }

    /// List all registered MCP tools with full schemas.
    /// V1.1 §6.1 + Appendix B.
    pub fn list_tools(&self) -> Vec<McpToolSchema> {
        vec![
            McpToolSchema {
                name: "filesystem.search_files".to_string(),
                description: "Walk a directory recursively and return files matching a glob pattern."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["root", "pattern"],
                    "properties": {
                        "root": { "type": "string" },
                        "pattern": { "type": "string" }
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "matches": { "type": "array", "items": { "type": "string" } }
                    }
                }),
                annotations: McpAnnotations {
                    read_only_hint: true,
                    destructive_hint: false,
                    idempotent_hint: true,
                    open_world_hint: false,
                },
            },
            McpToolSchema {
                name: "filesystem.move_files".to_string(),
                description: "Move files from sources to destination (two-phase: prepare+commit)."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["task_id", "step_id", "sources", "destination"],
                    "properties": {
                        "task_id": { "type": "string" },
                        "step_id": { "type": "string" },
                        "sources": { "type": "array", "items": { "type": "string" }, "maxItems": 100 },
                        "destination": { "type": "string" }
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "prepare_token": { "type": "string" },
                        "effect_manifest": { "type": "object" },
                        "preconditions_hash": { "type": "string" }
                    }
                }),
                annotations: McpAnnotations {
                    read_only_hint: false,
                    destructive_hint: false,
                    idempotent_hint: true,
                    open_world_hint: false,
                },
            },
            McpToolSchema {
                name: "filesystem.verify_move".to_string(),
                description: "Re-read destination files and verify sha256+size match the manifest."
                    .to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["effect_manifest"],
                    "properties": {
                        "effect_manifest": { "type": "object" }
                    }
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "verified": { "type": "boolean" },
                        "evidence_strength": { "type": "string" }
                    }
                }),
                annotations: McpAnnotations {
                    read_only_hint: true,
                    destructive_hint: false,
                    idempotent_hint: true,
                    open_world_hint: false,
                },
            },
        ]
    }

    /// Dispatch a tool call. Read-only tools execute directly; move_files
    /// is rejected — callers must go through the Skill executor.
    pub fn call_tool(
        &self,
        kernel: &TrustKernel,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<McpCallResult> {
        match name {
            "filesystem.search_files" => self.call_search_files(kernel, args),
            "filesystem.verify_move" => self.call_verify_move(kernel, args),
            "filesystem.move_files" => Err(KernelError::Mcp(
                "move_files must be invoked through the files.organize Skill executor (V1.1 §6.2 approve phase required). W4 will expose prepare_move + commit_move as separate MCP tools."
                    .to_string(),
            )),
            other => Err(KernelError::Mcp(format!("unknown tool: {}", other))),
        }
    }

    fn call_search_files(
        &self,
        kernel: &TrustKernel,
        args: &serde_json::Value,
    ) -> Result<McpCallResult> {
        let root = args
            .get("root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KernelError::Mcp("missing 'root' argument".to_string()))?;
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| KernelError::Mcp("missing 'pattern' argument".to_string()))?;

        let matches = kernel
            .filesystem()
            .search_files(std::path::Path::new(root), pattern)?;
        let match_strs: Vec<String> = matches
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        Ok(McpCallResult::Ok(serde_json::json!({
            "matches": match_strs,
        })))
    }

    fn call_verify_move(
        &self,
        kernel: &TrustKernel,
        args: &serde_json::Value,
    ) -> Result<McpCallResult> {
        let manifest_value = args
            .get("effect_manifest")
            .ok_or_else(|| KernelError::Mcp("missing 'effect_manifest' argument".to_string()))?;
        let manifest: crate::policy::transaction::EffectManifest =
            serde_json::from_value(manifest_value.clone())?;
        let result = kernel.filesystem().verify_move(&manifest)?;
        Ok(McpCallResult::Ok(serde_json::json!({
            "verified": result.verified,
            "evidence_strength": result.evidence_strength.as_str(),
        })))
    }
}
