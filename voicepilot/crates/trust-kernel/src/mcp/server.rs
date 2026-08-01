//! MCP server — V1.1 §6.1.
//!
//! Owns a McpHandler + dispatches JSON-RPC 2.0 requests to method handlers.
//! Holds the kernel via Arc<TrustKernel> so multiple owners (server + test
//! harness) can share the same audit log / DB state.

use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::mcp::handler::{McpCallResult, McpHandler};
use crate::mcp::transport::{
    JsonRpcError, JsonRpcErrorCode, JsonRpcId, JsonRpcRequest, JsonRpcResponse,
};
use crate::policy::taint_repo::{compute_value_hash, make_taint_record, TaintRepo};
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
    /// W9 Plan 3: MCP server 标识(用于 `mcp_tool:<server_id>` taint provenance)。
    /// 默认 UUID v4(构造时生成),生产代码可用 `with_server_id` 链式方法覆盖。
    server_id: String,
}

impl McpServer {
    pub fn new(handler: McpHandler, kernel: TrustKernel) -> Self {
        Self {
            handler,
            kernel: Arc::new(kernel),
            server_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn with_arc(handler: McpHandler, kernel: Arc<TrustKernel>) -> Self {
        Self {
            handler,
            kernel,
            server_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// W9 Plan 3: 链式设置 `server_id`(用于 `mcp_tool:<server_id>` taint provenance)。
    /// 生产代码(`mcp-serve` CLI)用此方法设置有意义的标识(如 "voicepilot-stdio"),
    /// 测试代码用默认 UUID 即可。
    pub fn with_server_id(mut self, server_id: impl Into<String>) -> Self {
        self.server_id = server_id.into();
        self
    }

    /// W9 Plan 3: 获取 `server_id`(供 taint 标记 + 审计使用)。
    pub fn server_id(&self) -> &str {
        &self.server_id
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
        // Spec issue #37: missing 'name' must return InvalidParams -32602,
        // not propagate as KernelError::Mcp and crash the stdio loop.
        let name = match params
            .get("name")
            .and_then(|v| v.as_str())
        {
            Some(n) => n,
            None => {
                return Ok(OutgoingMessage::Error(JsonRpcError::new(
                    id,
                    JsonRpcErrorCode::InvalidParams,
                    "tools/call requires 'name' field",
                )));
            }
        };
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
        // Spec issue #37: audit failure (e.g. FK violation on unknown task_id)
        // must emit InternalError -32603 and let the loop continue, not
        // propagate as KernelError::Db and crash the stdio loop.
        if let Some(tid) = &task_id {
            if let Err(e) = self.kernel.audit_append_external(
                tid,
                step_id.as_deref(),
                "mcp_tools_call",
                serde_json::json!({
                    "tool": name,
                    "arguments": arguments,
                    "success": matches!(result, McpCallResult::Ok(_)),
                }),
            ) {
                return Ok(OutgoingMessage::Error(JsonRpcError::new(
                    id,
                    JsonRpcErrorCode::InternalError,
                    format!("audit log failure: {}", e),
                )));
            }
        }

        let (content, is_error) = match result {
            McpCallResult::Ok(value) => (value, false),
            McpCallResult::Err(msg) => (serde_json::json!({ "error": msg }), true),
        };

        // W9 Plan 3: MCP tool 返回值标 `mcp_tool:<server_id>` taint(spec §2.3)。
        // 仅对成功结果标 taint(错误结果不流入下游节点,无需追踪)。
        // taint upsert 失败不阻断 MCP 响应(taint 是安全增强,非硬约束)。
        if !is_error {
            let result_hash = compute_value_hash(&content);
            let server_taint = format!("mcp_tool:{}", self.server_id);
            let source_ref = match (&task_id, &step_id) {
                (Some(t), Some(s)) => Some(format!("{}:{}", t, s)),
                (Some(t), None) => Some(t.clone()),
                _ => None,
            };
            let record = make_taint_record(
                result_hash.clone(),
                server_taint.clone(),
                vec![server_taint.clone()],
                source_ref.clone(),
            );
            {
                let conn = self.kernel.conn();
                let _ = TaintRepo::new().upsert(&conn, &record);
            }
            // 审计 taint_propagated(仅当 task_id 存在,FK 约束 audit_logs.task_id)。
            if let Some(tid) = &task_id {
                let _ = self.kernel.audit_append_external(
                    tid,
                    step_id.as_deref(),
                    "taint_propagated",
                    serde_json::json!({
                        "source_ref": source_ref,
                        "input_hash": null,
                        "output_hash": result_hash,
                        "taints": [server_taint],
                    }),
                );
            }
        }

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

    /// Run the stdio transport loop. Reads NDJSON lines from `reader`,
    /// dispatches Requests, and writes Responses/Errors to `writer`.
    /// Returns when `reader` reaches EOF.
    ///
    /// Notifications are silently dropped (client-to-server only).
    /// Parse errors emit a -32700 ParseError response and continue.
    pub fn run_stdio<R: std::io::BufRead, W: std::io::Write>(
        &self,
        reader: R,
        writer: &mut W,
    ) -> Result<()> {
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break, // EOF or read error
            };
            let parsed = match crate::mcp::transport::parse_line(&line) {
                Ok(Some(msg)) => msg,
                Ok(None) => continue, // blank line
                Err(_) => {
                    // Parse error — emit -32700 response with null id.
                    let err = JsonRpcError::new(
                        JsonRpcId::Null,
                        JsonRpcErrorCode::ParseError,
                        "parse error",
                    );
                    crate::mcp::transport::write_message(writer, &err)?;
                    continue;
                }
            };
            match parsed {
                crate::mcp::transport::IncomingMessage::Request(req) => {
                    let outgoing = self.handle_request(req)?;
                    match outgoing {
                        OutgoingMessage::Response(resp) => {
                            crate::mcp::transport::write_message(writer, &resp)?;
                        }
                        OutgoingMessage::Error(err) => {
                            crate::mcp::transport::write_message(writer, &err)?;
                        }
                    }
                }
                // Notifications, Responses, Errors from client are ignored.
                _ => continue,
            }
        }
        Ok(())
    }
}
