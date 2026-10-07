//! JSON-RPC 2.0 message types — V1.1 §6.1 MCP wire protocol.
//!
//! Per MCP 2025-11-25 spec, messages are JSON-RPC 2.0 objects with camelCase
//! field names. stdio transport uses NDJSON (one message per line).

use serde::de::Error as SerdeError;
use serde::{Deserialize, Serialize};
use std::io::Write;

/// JSON-RPC id — can be a number, string, or null (for notifications).
/// Per spec, notifications carry no id at all (separate type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
    Null,
}

/// JSON-RPC 2.0 Request — has id, expects a Response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Notification — no id, no Response expected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 successful Response — has id and result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub result: serde_json::Value,
}

/// JSON-RPC 2.0 error Response — has id and error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub error: JsonRpcErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorBody {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Standard JSON-RPC 2.0 error codes (per spec §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum JsonRpcErrorCode {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
}

impl JsonRpcError {
    pub fn new(id: JsonRpcId, code: JsonRpcErrorCode, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            error: JsonRpcErrorBody {
                code: code as i32,
                message: message.into(),
                data: None,
            },
        }
    }

    pub fn with_data(
        id: JsonRpcId,
        code: JsonRpcErrorCode,
        message: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            error: JsonRpcErrorBody {
                code: code as i32,
                message: message.into(),
                data: Some(data),
            },
        }
    }
}

impl JsonRpcResponse {
    pub fn new(id: JsonRpcId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result,
        }
    }
}

/// Discriminated union for parsed incoming messages.
#[derive(Debug, Clone)]
pub enum IncomingMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
    Error(JsonRpcError),
}

/// Parse one NDJSON line into an IncomingMessage.
/// Returns Ok(None) for empty/whitespace-only input (allows trailing newline).
pub fn parse_line(line: &str) -> Result<Option<IncomingMessage>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)?;
    // Discriminate by fields present:
    //   - has "method" + no "id" → Notification
    //   - has "method" + "id"    → Request
    //   - has "result"           → Response
    //   - has "error"            → Error
    if value.get("method").is_some() {
        if value.get("id").is_some() {
            let req: JsonRpcRequest = serde_json::from_value(value)?;
            Ok(Some(IncomingMessage::Request(req)))
        } else {
            let notif: JsonRpcNotification = serde_json::from_value(value)?;
            Ok(Some(IncomingMessage::Notification(notif)))
        }
    } else if value.get("result").is_some() {
        let resp: JsonRpcResponse = serde_json::from_value(value)?;
        Ok(Some(IncomingMessage::Response(resp)))
    } else if value.get("error").is_some() {
        let err: JsonRpcError = serde_json::from_value(value)?;
        Ok(Some(IncomingMessage::Error(err)))
    } else {
        // Not a valid JSON-RPC message — re-parse as InvalidRequest for the error path.
        Err(SerdeError::custom(
            "message missing method/result/error field",
        ))
    }
}

/// Serialize a message and write it as a single line + '\n'.
/// Works for any Serialize type (Response, Error, Notification).
pub fn write_message<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> std::io::Result<()> {
    let json = serde_json::to_string(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}
