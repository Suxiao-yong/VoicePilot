//! MCP server — V1.1 §6.1.
//!
//! Owns a McpHandler + dispatches JSON-RPC 2.0 requests to method handlers.
//! Tasks 7-8 add the real method handlers (initialize, tools/list, tools/call).

use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::mcp::handler::McpHandler;
use crate::mcp::transport::{JsonRpcErrorCode, JsonRpcError, JsonRpcRequest};
use serde::Serialize;

/// Outgoing message — either a successful Response or an Error.
#[derive(Debug, Clone)]
pub enum OutgoingMessage {
    Response(crate::mcp::transport::JsonRpcResponse),
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
    kernel: TrustKernel,
}

impl McpServer {
    pub fn new(handler: McpHandler, kernel: TrustKernel) -> Self {
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
            // tools/list and tools/call added in Task 8.
            _ => Ok(OutgoingMessage::Error(JsonRpcError::new(
                id,
                JsonRpcErrorCode::MethodNotFound,
                format!("method not found: {}", req.method),
            ))),
        }
    }

    fn handle_initialize(
        &self,
        id: crate::mcp::transport::JsonRpcId,
        _params: Option<serde_json::Value>,
    ) -> Result<OutgoingMessage> {
        let result = serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": "voicepilot",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        Ok(OutgoingMessage::Response(
            crate::mcp::transport::JsonRpcResponse::new(id, result),
        ))
    }

    pub fn handler(&self) -> &McpHandler {
        &self.handler
    }

    pub fn kernel(&self) -> &TrustKernel {
        &self.kernel
    }
}
