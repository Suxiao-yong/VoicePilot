//! MCP client — W7 Plan 5 Task 0.
//!
//! Spawns an external MCP server subprocess (e.g. `npx @playwright/mcp@latest`)
//! and invokes its tools via JSON-RPC 2.0 over stdio (NDJSON framing).
//!
//! Lifecycle: created per Skill execution (e.g. one McpClient for one
//! `research.save_markdown` run). Drop kills the subprocess.
//!
//! Errors: all I/O / protocol errors map to `KernelError::Mcp(String)`.

use crate::error::{KernelError, Result};
use crate::mcp::transport::{parse_line, write_message, IncomingMessage, JsonRpcId, JsonRpcRequest};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: AtomicI64,
}

impl McpClient {
    /// Spawn an MCP server subprocess. Does NOT do the `initialize` handshake
    /// — call `initialize()` after construction.
    pub fn spawn(command: &str, args: &[String], env: &serde_json::Value) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        // Apply env overrides (JSON object). Inherits parent env otherwise.
        if let Some(obj) = env.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    cmd.env(k, s);
                }
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd.spawn().map_err(|e| {
            KernelError::Mcp(format!("failed to spawn MCP server '{command}': {e}"))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            KernelError::Mcp("MCP server stdin not captured".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            KernelError::Mcp("MCP server stdout not captured".to_string())
        })?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: AtomicI64::new(1),
        })
    }

    /// Perform the MCP `initialize` handshake. Must be called once after
    /// spawn before any `tools/call`. Returns Ok(()) on success.
    pub fn initialize(&mut self) -> Result<()> {
        let result = self.request("initialize", serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {
                "name": "voicepilot",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))?;
        // Verify protocolVersion is present
        if result.get("protocolVersion").is_none() {
            return Err(KernelError::Mcp(
                "initialize response missing protocolVersion".to_string(),
            ));
        }
        // Send initialized notification (no response expected)
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        write_message(&mut self.stdin, &notif)?;
        Ok(())
    }

    /// Invoke a tool. Returns the tool result (parsed from `content[0].text`)
    /// or an error if the server returns `isError: true` or a JSON-RPC error.
    pub fn invoke_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });
        let result = self.request("tools/call", params)?;
        // Check isError flag
        if result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(KernelError::Mcp(format!(
                "tool '{name}' returned isError: {}",
                result
            )));
        }
        // Extract content[0].text as JSON
        let content = result
            .get("content")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| {
                KernelError::Mcp(format!("tool '{name}' response missing content[0]"))
            })?;
        let text = content
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                KernelError::Mcp(format!("tool '{name}' content[0] missing text field"))
            })?;
        // Try to parse text as JSON; fall back to string value
        Ok(serde_json::from_str(text).unwrap_or(serde_json::Value::String(text.to_string())))
    }

    /// Invoke `tools/list` and return the advertised tools (raw JSON array).
    ///
    /// W11 Plan 4:评测 harness 用它在调用前检查外部 server 的 tool annotation
    /// (`verify_mcp_annotations`),检测谎报只读的恶意 server。
    pub fn list_tools(&mut self) -> Result<serde_json::Value> {
        let result = self.request("tools/list", serde_json::json!({}))?;
        Ok(result
            .get("tools")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    /// Send a JSON-RPC request and wait for the matching response.
    /// Skips notifications and unmatched ids on the read side.
    fn request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: JsonRpcId::Number(id),
            method: method.to_string(),
            params: Some(params),
        };
        write_message(&mut self.stdin, &req)?;
        // Read lines until we find a Response or Error with matching id
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).map_err(|e| {
                KernelError::Mcp(format!("failed to read MCP response: {e}"))
            })?;
            if n == 0 {
                return Err(KernelError::Mcp(
                    "MCP server stdout closed before response".to_string(),
                ));
            }
            match parse_line(&line)? {
                Some(IncomingMessage::Response(resp)) => {
                    if matches!(resp.id, JsonRpcId::Number(n) if n == id) {
                        return Ok(resp.result);
                    }
                    // Wrong id — keep reading
                }
                Some(IncomingMessage::Error(err)) => {
                    if matches!(err.id, JsonRpcId::Number(n) if n == id) {
                        return Err(KernelError::Mcp(format!(
                            "JSON-RPC error from MCP server: code={} message={}",
                            err.error.code, err.error.message
                        )));
                    }
                }
                _ => continue, // notifications / unmatched ids
            }
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Best-effort: close stdin, then kill, then wait
        let _ = self.stdin.flush();
        // Try graceful kill first (Windows: taskkill /F via kill())
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
