//! MCP server — V1.1 §6.1.
//!
//! Owns a McpHandler + dispatches JSON-RPC 2.0 requests to method handlers.
//! Holds the kernel via Arc<TrustKernel> so multiple owners (server + test
//! harness) can share the same audit log / DB state.

use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::mcp::handler::{McpCallResult, McpHandler};
use crate::mcp::transport::{
    JsonRpcError, JsonRpcErrorCode, JsonRpcId, JsonRpcRequest, JsonRpcResponse,
};
use serde::Serialize;
use std::sync::Arc;

/// Outgoing message — either a successful Response or an Error.
#[derive(Debug, Clone)]
pub enum OutgoingMessage {
    Response(JsonRpcResponse),
    Error(JsonRpcError),
}

impl Serialize for OutgoingMessage {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            OutgoingMessage::Response(r) => r.serialize(serializer),
            OutgoingMessage::Error(e) => e.serialize(serializer),
        }
    }
}

pub struct McpServer {
    handler: McpHandler,
    kernel: Arc<TrustKernel>,
}

impl McpServer {
    pub fn new(handler: McpHandler, kernel: TrustKernel) -> Self {
        Self {
            handler,
            kernel: Arc::new(kernel),
        }
    }

    pub fn with_arc(handler: McpHandler, kernel: Arc<TrustKernel>) -> Self {
        Self { handler, kernel }
    }

    /// Dispatch a single JSON-RPC request. Returns an OutgoingMessage
    /// (Response or Error) that the caller serializes and writes to the
    /// transport. Notifications (no id) are handled separately by the
    /// transport loop — this method is only called for Requests.
    pub fn handle_request(&self, req: JsonRpcRequest) -> Result<OutgoingMessage> {
        let id = req.id.clone();
        match req.method.as_str() {
            "initialize" => self.handle_initialize(id, req.params),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, req.params),
            _ => Ok(OutgoingMessage::Error(JsonRpcError::new(
                id,
                JsonRpcErrorCode::MethodNotFound,
                format!("method not found: {}", req.method),
            ))),
        }
    }

    fn handle_initialize(
        &self,
        id: JsonRpcId,
        _params: Option<serde_json::Value>,
    ) -> Result<OutgoingMessage> {
        let result = serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "voicepilot",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        Ok(OutgoingMessage::Response(JsonRpcResponse::new(id, result)))
    }

    fn handle_tools_list(&self, id: JsonRpcId) -> Result<OutgoingMessage> {
        let tools = self.handler.list_tools();
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();
        let result = serde_json::json!({ "tools": tools_json });
        Ok(OutgoingMessage::Response(JsonRpcResponse::new(id, result)))
    }

    fn handle_tools_call(
        &self,
        id: JsonRpcId,
        params: Option<serde_json::Value>,
    ) -> Result<OutgoingMessage> {
        let params = params.unwrap_or(serde_json::Value::Null);
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                KernelError::Mcp("tools/call requires 'name' field".to_string())
            })?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // Extract task_id + step_id for audit logging (stateful calls).
        let task_id = arguments
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let step_id = arguments
            .get("step_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        let result = self.handler.call_tool(&self.kernel, name, &arguments);

        // Kernel-level errors from call_tool (e.g., move_files rejected)
        // become JSON-RPC error responses — the transport expects a
        // response for every request, and `?` here would propagate as a
        // Rust `Result::Err` causing the caller's `unwrap()` to panic.
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                return Ok(OutgoingMessage::Error(JsonRpcError::new(
                    id,
                    JsonRpcErrorCode::InternalError,
                    e.to_string(),
                )));
            }
        };

        // Audit log if task_id is present (stateful call).
        if let Some(tid) = &task_id {
            self.kernel.audit_append_external(
                tid,
                step_id.as_deref(),
                "MCP_TOOLS_CALL",
                serde_json::json!({
                    "tool": name,
                    "arguments": arguments,
                    "success": matches!(result, McpCallResult::Ok(_)),
                }),
            )?;
        }

        let (content, is_error) = match result {
            McpCallResult::Ok(value) => (value, false),
            McpCallResult::Err(msg) => (serde_json::json!({ "error": msg }), true),
        };
        let result_json = serde_json::json!({
            "content": [{
                "type": "text",
                "text": content.to_string()
            }],
            "isError": is_error
        });
        Ok(OutgoingMessage::Response(JsonRpcResponse::new(id, result_json)))
    }

    pub fn handler(&self) -> &McpHandler {
        &self.handler
    }

    pub fn kernel(&self) -> &TrustKernel {
        &self.kernel
    }
}
