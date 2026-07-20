# W4: MCP Server Wrapping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wrap the W3b `McpHandler` in a JSON-RPC 2.0 wire protocol over stdio, persist MCP server config (including `allowed_paths`) to the `mcp_servers` table, and expose a CLI `mcp-serve` command so external MCP clients (Claude Desktop, Cursor) can connect to VoicePilot, list tools, and invoke read-only filesystem tools. Stateful `filesystem.move_files` is rejected at the MCP layer (must go through the Skill executor). End state: an external MCP client can connect via stdio, complete the `initialize` handshake, call `tools/list` to see 3 filesystem tools, call `tools/call` on `filesystem.search_files` to find files, and have each call logged to the audit trail.

**Architecture:** Three new modules under `trust-kernel/src/mcp/`:
- `transport.rs` — JSON-RPC 2.0 message types (`Request`, `Response`, `Error`, `Notification`) + NDJSON line framing (one JSON object per line, per MCP stdio transport convention).
- `server.rs` — `McpServer` struct that owns a `McpHandler` + dispatch table (method name → handler), runs the stdio read/write loop, and emits audit events for each `tools/call`.
- `repo.rs` — `McpServerRepo` CRUD against the existing `mcp_servers` table (created by `001_init.sql`), plus a `load_allowed_paths(server_id)` helper that returns an `AllowedPaths` instance for injection into `FilesystemTool::new_with_allowed_paths()`.

The CLI gains an `mcp-serve` command that opens the kernel, loads the builtin `voicepilot-filesystem` server row (auto-seeding it if missing), constructs an `McpServer` with the `AllowedPaths`-enforced `FilesystemTool`, and enters the stdio loop.

No new crate dependencies — JSON-RPC 2.0 is simple enough to hand-roll with `serde_json`, consistent with W3a's "no MCP SDK dependency" hard constraint. The official Rust SDK `rmcp` v1.6.1 is available but adds a transitive dep tree; W4 keeps the native Rust philosophy. A future W9+ may migrate to `rmcp` if interop issues arise.

**Tech Stack:** Rust 1.96, `rusqlite` 0.32, `serde` 1.0, `serde_json` 1.0, `thiserror` 2.0, `uuid` 1.10, `chrono` 0.4 — all already in the workspace. No new deps.

**Reference:** V1.1.1 spec at `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`. Relevant sections: §6.1 (内置与自研清单, MCP protocol_version=2025-11-25), §6.2 (prepare→approve→commit — move_files must go through Skill executor), §6.3 (ToolResult V2), §8.1 (mcp_servers table schema), §11.1 (W4 gate "MCP Server Wrapping"). Appendix B (Tool Schema V2 — already implemented in W3b Task 8).

**W3b prerequisites (already complete):** `McpHandler` skeleton with `list_tools()` returning 3 filesystem tool schemas + `call_tool()` dispatching search_files/verify_move and rejecting move_files; `McpToolSchema`/`McpAnnotations` with `#[serde(rename_all = "camelCase")]`; `AllowedPaths` whitelist + `FilesystemTool::new_with_allowed_paths()`; `mcp_servers` table in `001_init.sql` (server_id, name, version, transport, enabled, trusted, protocol_version, allowed_origins, allowed_paths).

---

## File Structure

**New files:**
- `voicepilot/crates/trust-kernel/src/mcp/transport.rs` — JSON-RPC 2.0 message types + NDJSON framing
- `voicepilot/crates/trust-kernel/src/mcp/server.rs` — `McpServer` struct + dispatch + stdio loop
- `voicepilot/crates/trust-kernel/src/mcp/repo.rs` — `McpServerRepo` CRUD + `load_allowed_paths(server_id)`
- `voicepilot/crates/trust-kernel/tests/mcp_transport.rs`
- `voicepilot/crates/trust-kernel/tests/mcp_server.rs`
- `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`
- `voicepilot/crates/trust-kernel/tests/w4_e2e_smoke.rs`

**Modified files:**
- `voicepilot/crates/trust-kernel/src/mcp/mod.rs` — add `pub mod transport; pub mod server; pub mod repo;`
- `voicepilot/crates/trust-kernel/src/kernel.rs` — add `pub fn audit_append_external(event_type, details)` for MCP audit logging + `pub fn mcp_server_repo()` accessor
- `voicepilot/crates/cli/src/main.rs` — add `mcp-serve` command + help text

---

## Task 1: JSON-RPC 2.0 message types

**Goal:** Define the four JSON-RPC 2.0 message types (Request, Response, Error, Notification) as serde structs with correct camelCase serialization, plus the standard error codes (-32700 parse error through -32603 internal error).

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/mcp/transport.rs`
- Modify: `voicepilot/crates/trust-kernel/src/mcp/mod.rs`
- Test: `voicepilot/crates/trust-kernel/tests/mcp_transport.rs`

- [ ] **Step 1: Write the failing test for message types round-trip**

Create `voicepilot/crates/trust-kernel/tests/mcp_transport.rs`:

```rust
use trust_kernel::mcp::transport::{
    JsonRpcError, JsonRpcErrorBody, JsonRpcId, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, JsonRpcErrorCode,
};
use serde_json::json;

#[test]
fn request_serializes_with_camel_case_and_id() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        method: "tools/list".to_string(),
        params: Some(json!({})),
    };
    let s = serde_json::to_string(&req).unwrap();
    assert_eq!(
        s,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#
    );
}

#[test]
fn request_with_string_id_round_trips() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("abc-123".to_string()),
        method: "initialize".to_string(),
        params: Some(json!({"client": "test"})),
    };
    let s = serde_json::to_string(&req).unwrap();
    let back: JsonRpcRequest = serde_json::from_str(&s).unwrap();
    assert_eq!(back.id, JsonRpcId::String("abc-123".to_string()));
    assert_eq!(back.method, "initialize");
}

#[test]
fn notification_has_no_id() {
    let notif = JsonRpcNotification {
        jsonrpc: "2.0".to_string(),
        method: "notifications/initialized".to_string(),
        params: None,
    };
    let s = serde_json::to_string(&notif).unwrap();
    // Notification must not contain "id" field.
    assert!(!s.contains(r#""id""#));
    assert_eq!(
        s,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
    );
}

#[test]
fn response_with_result_round_trips() {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(42),
        result: json!({"tools": []}),
    };
    let s = serde_json::to_string(&resp).unwrap();
    let back: JsonRpcResponse = serde_json::from_str(&s).unwrap();
    assert_eq!(back.id, JsonRpcId::Number(42));
    assert_eq!(back.result, json!({"tools": []}));
}

#[test]
fn error_response_uses_standard_codes() {
    let err = JsonRpcError {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        error: JsonRpcErrorBody {
            code: JsonRpcErrorCode::MethodNotFound as i32,
            message: "method not found: foo/bar".to_string(),
            data: None,
        },
    };
    let s = serde_json::to_string(&err).unwrap();
    assert!(s.contains(r#""code":-32601"#));
    assert!(s.contains(r#""message":"method not found: foo/bar""#));
}

#[test]
fn error_code_constants_match_json_rpc_spec() {
    assert_eq!(JsonRpcErrorCode::ParseError as i32, -32700);
    assert_eq!(JsonRpcErrorCode::InvalidRequest as i32, -32600);
    assert_eq!(JsonRpcErrorCode::MethodNotFound as i32, -32601);
    assert_eq!(JsonRpcErrorCode::InvalidParams as i32, -32602);
    assert_eq!(JsonRpcErrorCode::InternalError as i32, -32603);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_transport 2>&1`
Expected: FAIL — `unresolved module transport` (file does not exist yet).

- [ ] **Step 3: Create the transport module**

First, modify `voicepilot/crates/trust-kernel/src/mcp/mod.rs` to add `pub mod transport;`:

```rust
pub mod handler;
pub mod repo;
pub mod schema;
pub mod server;
pub mod transport;
```

Then create `voicepilot/crates/trust-kernel/src/mcp/transport.rs`:

```rust
//! JSON-RPC 2.0 message types — V1.1 §6.1 MCP wire protocol.
//!
//! Per MCP 2025-11-25 spec, messages are JSON-RPC 2.0 objects with camelCase
//! field names. stdio transport uses NDJSON (one message per line).

use serde::{Deserialize, Serialize};

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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_transport 2>&1`
Expected: PASS — 6 tests.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/transport.rs voicepilot/crates/trust-kernel/src/mcp/mod.rs voicepilot/crates/trust-kernel/tests/mcp_transport.rs
git commit -m "feat(mcp): JSON-RPC 2.0 message types (V1.1 §6.1 wire protocol)"
```

---

## Task 2: NDJSON line framing (parse + serialize)

**Goal:** Add `parse_line` and `write_message` helpers to the transport module that handle NDJSON framing — one JSON-RPC message per line, terminated by `\n`. Parse must handle Request, Response, Error, and Notification variants (discriminated by presence/absence of `id`, `method`, `result`, `error`).

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/transport.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/mcp_transport.rs`

- [ ] **Step 1: Write the failing test for line parsing**

Append to `voicepilot/crates/trust-kernel/tests/mcp_transport.rs`:

```rust
use trust_kernel::mcp::transport::{
    parse_line, IncomingMessage, write_message,
};
use std::io::Cursor;

#[test]
fn parse_line_handles_request() {
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Request(req) => {
            assert_eq!(req.method, "tools/list");
            assert_eq!(req.id, JsonRpcId::Number(1));
        }
        other => panic!("expected Request, got {:?}", other),
    }
}

#[test]
fn parse_line_handles_notification() {
    let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Notification(notif) => {
            assert_eq!(notif.method, "notifications/initialized");
        }
        other => panic!("expected Notification, got {:?}", other),
    }
}

#[test]
fn parse_line_handles_response_with_result() {
    let line = r#"{"jsonrpc":"2.0","id":5,"result":{"tools":[]}}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Response(resp) => {
            assert_eq!(resp.id, JsonRpcId::Number(5));
            assert_eq!(resp.result, json!({"tools": []}));
        }
        other => panic!("expected Response, got {:?}", other),
    }
}

#[test]
fn parse_line_handles_error_response() {
    let line = r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32601,"message":"not found"}}"#;
    let msg = parse_line(line).unwrap().expect("must parse");
    match msg {
        IncomingMessage::Error(err) => {
            assert_eq!(err.id, JsonRpcId::Number(9));
            assert_eq!(err.error.code, -32601);
        }
        other => panic!("expected Error, got {:?}", other),
    }
}

#[test]
fn parse_line_rejects_empty_input() {
    let result = parse_line("");
    assert!(matches!(result, Ok(None)));
}

#[test]
fn parse_line_returns_err_on_invalid_json() {
    let result = parse_line("{not valid json");
    assert!(result.is_err());
}

#[test]
fn write_message_emits_single_line_with_newline() {
    let resp = JsonRpcResponse::new(JsonRpcId::Number(1), json!({"ok": true}));
    let mut buf = Vec::new();
    write_message(&mut buf, &resp).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.ends_with('\n'));
    assert!(!s.contains('\n') || s.matches('\n').count() == 1);
    assert!(s.contains(r#""ok":true"#));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_transport 2>&1`
Expected: FAIL — `parse_line`, `IncomingMessage`, `write_message` do not exist.

- [ ] **Step 3: Add the framing helpers**

Append to `voicepilot/crates/trust-kernel/src/mcp/transport.rs`:

```rust
use std::io::Write;

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
        Err(serde::de::Error::custom("message missing method/result/error field"))
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_transport 2>&1`
Expected: PASS — 13 tests (6 from Task 1 + 7 new).

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/transport.rs voicepilot/crates/trust-kernel/tests/mcp_transport.rs
git commit -m "feat(mcp): NDJSON line framing for stdio transport (V1.1 §6.1)"
```

---

## Task 3: McpServerRepo CRUD for mcp_servers table

**Goal:** Persist MCP server configuration (name, version, transport, protocol_version, allowed_paths) to the existing `mcp_servers` table so that W4 can load `allowed_paths` at startup and inject into `FilesystemTool`. Resolves spec issue #31 (allowed_paths enforcement layer).

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/mcp/repo.rs`
- Modify: `voicepilot/crates/trust-kernel/src/mcp/mod.rs`
- Test: `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`

- [ ] **Step 1: Write the failing test for repo CRUD**

Create `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`:

```rust
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};

#[test]
fn repo_create_and_get_round_trips() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "voicepilot-filesystem".to_string(),
        name: "VoicePilot Filesystem".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["C:/Users","D:/"]"#.to_string()),
    };
    repo.create(&k.conn(), &rec).unwrap();
    let loaded = repo.get(&k.conn(), "voicepilot-filesystem").unwrap().expect("must exist");
    assert_eq!(loaded.name, "VoicePilot Filesystem");
    assert_eq!(loaded.transport, "stdio");
    assert!(loaded.enabled);
    assert!(loaded.trusted);
    assert_eq!(loaded.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(loaded.allowed_paths.as_deref(), Some(r#"["C:/Users","D:/"]"#));
}

#[test]
fn repo_list_returns_all_rows() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.create(&k.conn(), &sample("s1")).unwrap();
    repo.create(&k.conn(), &sample("s2")).unwrap();
    let list = repo.list(&k.conn()).unwrap();
    assert_eq!(list.len(), 2);
}

#[test]
fn repo_update_changes_fields() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.create(&k.conn(), &sample("s1")).unwrap();
    let mut rec = repo.get(&k.conn(), "s1").unwrap().unwrap();
    rec.enabled = false;
    rec.allowed_paths = Some(r#"["D:/only"]"#.to_string());
    repo.update(&k.conn(), &rec).unwrap();
    let loaded = repo.get(&k.conn(), "s1").unwrap().unwrap();
    assert!(!loaded.enabled);
    assert_eq!(loaded.allowed_paths.as_deref(), Some(r#"["D:/only"]"#));
}

#[test]
fn repo_delete_removes_row() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.create(&k.conn(), &sample("s1")).unwrap();
    repo.delete(&k.conn(), "s1").unwrap();
    assert!(repo.get(&k.conn(), "s1").unwrap().is_none());
}

#[test]
fn repo_get_returns_none_for_missing() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    assert!(repo.get(&k.conn(), "nonexistent").unwrap().is_none());
}

fn sample(id: &str) -> McpServerRecord {
    McpServerRecord {
        server_id: id.to_string(),
        name: format!("Server {}", id),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: None,
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_repo 2>&1`
Expected: FAIL — `unresolved module repo`.

- [ ] **Step 3: Add `pub mod repo;` to mcp/mod.rs**

Edit `voicepilot/crates/trust-kernel/src/mcp/mod.rs` to ensure `pub mod repo;` is present (added in Task 1 Step 3).

- [ ] **Step 4: Create the repo module**

Create `voicepilot/crates/trust-kernel/src/mcp/repo.rs`:

```rust
//! MCP server config repo — V1.1 §8.1 mcp_servers table.
//!
//! Stores per-server metadata (name, version, transport, protocol_version)
//! and security config (allowed_origins, allowed_paths). W4 loads
//! allowed_paths at startup and injects into FilesystemTool via
//! new_with_allowed_paths(), resolving spec issue #31.

use crate::error::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerRecord {
    pub server_id: String,
    pub name: String,
    pub version: String,
    pub transport: String,
    pub enabled: bool,
    pub trusted: bool,
    pub protocol_version: Option<String>,
    pub allowed_origins: Option<String>, // JSON array, raw text
    pub allowed_paths: Option<String>,   // JSON array, raw text
}

pub struct McpServerRepo;

impl McpServerRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, rec: &McpServerRecord) -> Result<()> {
        conn.execute(
            r#"INSERT INTO mcp_servers
               (server_id, name, version, transport, enabled, trusted,
                protocol_version, allowed_origins, allowed_paths)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                rec.server_id,
                rec.name,
                rec.version,
                rec.transport,
                rec.enabled as i32,
                rec.trusted as i32,
                rec.protocol_version,
                rec.allowed_origins,
                rec.allowed_paths,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, server_id: &str) -> Result<Option<McpServerRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT server_id, name, version, transport, enabled, trusted,
                      protocol_version, allowed_origins, allowed_paths
               FROM mcp_servers WHERE server_id = ?1"#,
        )?;
        let mut rows = stmt.query(params![server_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list(&self, conn: &Connection) -> Result<Vec<McpServerRecord>> {
        let mut stmt = conn.prepare(
            r#"SELECT server_id, name, version, transport, enabled, trusted,
                      protocol_version, allowed_origins, allowed_paths
               FROM mcp_servers ORDER BY server_id"#,
        )?;
        let records = stmt
            .query_map([], row_to_record)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn update(&self, conn: &Connection, rec: &McpServerRecord) -> Result<()> {
        conn.execute(
            r#"UPDATE mcp_servers SET
                 name = ?2, version = ?3, transport = ?4, enabled = ?5,
                 trusted = ?6, protocol_version = ?7, allowed_origins = ?8,
                 allowed_paths = ?9
               WHERE server_id = ?1"#,
            params![
                rec.server_id,
                rec.name,
                rec.version,
                rec.transport,
                rec.enabled as i32,
                rec.trusted as i32,
                rec.protocol_version,
                rec.allowed_origins,
                rec.allowed_paths,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, conn: &Connection, server_id: &str) -> Result<()> {
        conn.execute(r#"DELETE FROM mcp_servers WHERE server_id = ?1"#, params![server_id])?;
        Ok(())
    }
}

impl Default for McpServerRepo {
    fn default() -> Self {
        Self::new()
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> Result<McpServerRecord> {
    Ok(McpServerRecord {
        server_id: row.get(0)?,
        name: row.get(1)?,
        version: row.get(2)?,
        transport: row.get(3)?,
        enabled: row.get::<_, i32>(4)? != 0,
        trusted: row.get::<_, i32>(5)? != 0,
        protocol_version: row.get(6)?,
        allowed_origins: row.get(7)?,
        allowed_paths: row.get(8)?,
    })
}
```

- [ ] **Step 5: Add `kernel.conn()` public accessor**

The test uses `k.conn()`. Add this to `voicepilot/crates/trust-kernel/src/kernel.rs` inside `impl TrustKernel` (before the private `audit_append` helper):

```rust
    /// Borrow the underlying connection for repo operations.
    /// Used by MCP repo and other sub-repos that need direct DB access.
    /// W4 note: returns a MutexGuard — caller must drop before any other
    /// kernel method that locks conn (no reentrancy).
    pub fn conn(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.conn.lock().unwrap()
    }
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_repo 2>&1`
Expected: PASS — 5 tests.

- [ ] **Step 7: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/repo.rs voicepilot/crates/trust-kernel/src/mcp/mod.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/mcp_repo.rs
git commit -m "feat(mcp): McpServerRepo CRUD for mcp_servers table (V1.1 §8.1)"
```

---

## Task 4: Load allowed_paths from mcp_servers table

**Goal:** Add a `McpServerRepo::load_allowed_paths(server_id)` helper that parses the JSON `allowed_paths` column and returns an `AllowedPaths` instance ready for injection into `FilesystemTool::new_with_allowed_paths()`. This closes the loop on spec issue #31 — `allowed_paths` is now enforced at the FilesystemTool level, loaded from the MCP server config.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/repo.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`

- [ ] **Step 1: Write the failing test for load_allowed_paths**

Append to `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`:

```rust
use trust_kernel::allowed_paths::AllowedPaths;
use std::path::Path;

#[test]
fn load_allowed_paths_returns_canonicalized_roots() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "s1".to_string(),
        name: "S1".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["C:/Users","D:/voicepilot"]"#.to_string()),
    };
    repo.create(&k.conn(), &rec).unwrap();

    let allowed = repo.load_allowed_paths(&k.conn(), "s1").unwrap().expect("must exist");
    // Path under root C:/Users should be allowed.
    assert!(allowed.check(Path::new("C:/Users/me/file.txt")).is_ok());
    // Path outside all roots should be rejected.
    assert!(allowed.check(Path::new("E:/elsewhere")).is_err());
}

#[test]
fn load_allowed_paths_returns_none_when_column_empty() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "s2".to_string(),
        name: "S2".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: None,
        allowed_origins: None,
        allowed_paths: None,
    };
    repo.create(&k.conn(), &rec).unwrap();
    assert!(repo.load_allowed_paths(&k.conn(), "s2").unwrap().is_none());
}

#[test]
fn load_allowed_paths_returns_none_when_server_missing() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    assert!(repo.load_allowed_paths(&k.conn(), "nonexistent").unwrap().is_none());
}

#[test]
fn load_allowed_paths_returns_err_on_invalid_json() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "s3".to_string(),
        name: "S3".to_string(),
        version: "0.1.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: None,
        allowed_origins: None,
        allowed_paths: Some("not valid json".to_string()),
    };
    repo.create(&k.conn(), &rec).unwrap();
    assert!(repo.load_allowed_paths(&k.conn(), "s3").is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_repo 2>&1`
Expected: FAIL — `load_allowed_paths` does not exist.

- [ ] **Step 3: Add load_allowed_paths to McpServerRepo**

Append to `voicepilot/crates/trust-kernel/src/mcp/repo.rs` (inside `impl McpServerRepo`):

```rust
    /// Load and parse the allowed_paths JSON column for a server.
    /// Returns Ok(None) if the server has no allowed_paths configured
    /// (caller may treat as "no whitelist enforced").
    /// Returns Err on missing server with invalid JSON.
    pub fn load_allowed_paths(
        &self,
        conn: &Connection,
        server_id: &str,
    ) -> Result<Option<AllowedPaths>> {
        let rec = match self.get(conn, server_id)? {
            Some(r) => r,
            None => return Ok(None),
        };
        let json_str = match rec.allowed_paths {
            Some(s) => s,
            None => return Ok(None),
        };
        let roots: Vec<String> = serde_json::from_str(&json_str)?;
        if roots.is_empty() {
            return Ok(None);
        }
        Ok(Some(AllowedPaths::new(roots)))
    }
```

And add the import at the top of `repo.rs`:

```rust
use crate::allowed_paths::AllowedPaths;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_repo 2>&1`
Expected: PASS — 9 tests (5 from Task 3 + 4 new).

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/repo.rs voicepilot/crates/trust-kernel/tests/mcp_repo.rs
git commit -m "feat(mcp): load_allowed_paths helper for FilesystemTool injection (V1.1 §8.1, issue #31)"
```

---

## Task 5: Public kernel.audit_append_external for MCP audit

**Goal:** Expose a public method on `TrustKernel` that lets the MCP server log audit events for `tools/call` invocations. The existing `audit_append` is private; W3b's `audit_append_step` was renamed/merged into other public methods. W4 needs a generic external audit entry point because MCP calls may not map cleanly to task/step IDs (e.g. `tools/list`).

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Test: `voicepilot/crates/trust-kernel/tests/mcp_server.rs` (created in Task 7; for now, write a standalone test in `tests/kernel_accessors.rs`)

- [ ] **Step 1: Write the failing test for audit_append_external**

Append to `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`:

```rust
#[test]
fn kernel_audit_append_external_logs_event() {
    let k = trust_kernel::kernel::TrustKernel::open_in_memory().unwrap();
    k.create_task("t-ext", "external audit test").unwrap();
    let before = k.audit_count_for_task("t-ext").unwrap();
    k.audit_append_external(
        "t-ext",
        None,
        "MCP_TOOLS_CALL",
        serde_json::json!({
            "tool": "filesystem.search_files",
            "args": {"root": "C:/Users", "pattern": "*.pdf"}
        }),
    )
    .unwrap();
    let after = k.audit_count_for_task("t-ext").unwrap();
    assert_eq!(after, before + 1);
}

#[test]
fn kernel_audit_append_external_rejects_unknown_task() {
    let k = trust_kernel::kernel::TrustKernel::open_in_memory().unwrap();
    let result = k.audit_append_external(
        "nonexistent-task",
        None,
        "MCP_TOOLS_CALL",
        serde_json::json!({}),
    );
    // FK constraint — audit_logs.task_id REFERENCES tasks(task_id).
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: FAIL — `audit_append_external` does not exist.

- [ ] **Step 3: Add the public audit_append_external method**

Edit `voicepilot/crates/trust-kernel/src/kernel.rs`, adding inside `impl TrustKernel` (after `audit_count_for_task`):

```rust
    /// Public entry point for external modules (MCP server, future IPC
    /// layers) to append audit events. V1.1 §6.1 — every MCP tools/call
    /// must leave an audit trail.
    ///
    /// Caller must supply a valid task_id (FK enforced). For stateless
    /// calls (initialize, tools/list) where no task exists, skip audit
    /// logging — those calls carry no security-relevant state changes.
    pub fn audit_append_external(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        details: serde_json::Value,
    ) -> Result<()> {
        self.audit_append(task_id, step_id, event_type, details)
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: PASS — all tests including 2 new ones.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/kernel_accessors.rs
git commit -m "feat(kernel): public audit_append_external for MCP audit trail (V1.1 §6.1)"
```

---

## Task 6: McpServer struct + dispatch table

**Goal:** Define the `McpServer` struct that owns a `McpHandler` and dispatches incoming `JsonRpcRequest` messages to method handlers. This task sets up the skeleton (struct, constructor, dispatch method that returns MethodNotFound for unknown methods); Tasks 7-8 add the `initialize`, `tools/list`, and `tools/call` handlers.

**Files:**
- Create: `voicepilot/crates/trust-kernel/src/mcp/server.rs`
- Modify: `voicepilot/crates/trust-kernel/src/mcp/mod.rs`
- Test: `voicepilot/crates/trust-kernel/tests/mcp_server.rs`

- [ ] **Step 1: Write the failing test for dispatch**

Create `voicepilot/crates/trust-kernel/tests/mcp_server.rs`:

```rust
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::server::McpServer;
use trust_kernel::mcp::transport::{IncomingMessage, JsonRpcId, JsonRpcRequest, JsonRpcResponse};

#[test]
fn server_dispatch_unknown_method_returns_method_not_found() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(1),
        method: "foo/bar".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be an error response with code -32601.
    assert!(s.contains(r#""code":-32601"#));
    assert!(s.contains(r#""id":1"#));
}

#[test]
fn server_dispatch_returns_response_for_known_method_placeholder() {
    // initialize is added in Task 7; for now we test that the dispatch
    // path produces a Response (not an Error) when the method exists.
    // This test will be replaced in Task 7.
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("init-1".to_string()),
        method: "initialize".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    // Task 6 placeholder: initialize returns MethodNotFound (no handler yet).
    // Task 7 will flip this to a real Response.
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(s.contains(r#""code":-32601"#));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: FAIL — `unresolved module server`.

- [ ] **Step 3: Add `pub mod server;` to mcp/mod.rs**

Edit `voicepilot/crates/trust-kernel/src/mcp/mod.rs` to ensure `pub mod server;` is present (added in Task 1 Step 3).

- [ ] **Step 4: Create the McpServer skeleton**

Create `voicepilot/crates/trust-kernel/src/mcp/server.rs`:

```rust
//! MCP server — V1.1 §6.1.
//!
//! Owns a McpHandler + dispatches JSON-RPC 2.0 requests to method handlers.
//! Tasks 7-8 add the real method handlers (initialize, tools/list, tools/call).

use crate::error::Result;
use crate::kernel::TrustKernel;
use crate::mcp::handler::McpHandler;
use crate::mcp::transport::{JsonRpcErrorCode, JsonRpcError, JsonRpcRequest};

/// Outgoing message — either a successful Response or an Error.
#[derive(Debug, Clone)]
pub enum OutgoingMessage {
    Response(crate::mcp::transport::JsonRpcResponse),
    Error(JsonRpcError),
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
        // Tasks 7-8 will replace this match with real handlers.
        let id = req.id.clone();
        Ok(OutgoingMessage::Error(JsonRpcError::new(
            id,
            JsonRpcErrorCode::MethodNotFound,
            format!("method not found: {}", req.method),
        )))
    }

    pub fn handler(&self) -> &McpHandler {
        &self.handler
    }

    pub fn kernel(&self) -> &TrustKernel {
        &self.kernel
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: PASS — 2 tests (both expect MethodNotFound placeholder).

- [ ] **Step 6: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/server.rs voicepilot/crates/trust-kernel/src/mcp/mod.rs voicepilot/crates/trust-kernel/tests/mcp_server.rs
git commit -m "feat(mcp): McpServer struct + dispatch skeleton (V1.1 §6.1)"
```

---

## Task 7: `initialize` method (handshake)

**Goal:** Implement the MCP `initialize` method — returns protocol version `2025-11-25`, server capabilities (tools), and server info (name + version). Per MCP spec, the client sends `initialize` first, then `notifications/initialized`, then any method calls.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/server.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/mcp_server.rs`

- [ ] **Step 1: Write the failing test for initialize**

Replace the second test in `voicepilot/crates/trust-kernel/tests/mcp_server.rs` with:

```rust
#[test]
fn server_initialize_returns_protocol_2025_11_25_and_capabilities() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::String("init-1".to_string()),
        method: "initialize".to_string(),
        params: Some(serde_json::json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": {"name": "test-client", "version": "0.1.0"}
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be a Response (not Error).
    assert!(!s.contains(r#""code""#), "initialize must succeed, got: {}", s);
    assert!(s.contains(r#""protocolVersion":"2025-11-25""#));
    // Must advertise tools capability.
    assert!(s.contains(r#""tools""#));
    // Must include serverInfo with name + version.
    assert!(s.contains(r#""serverInfo""#));
    assert!(s.contains(r#""voicepilot""#));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: FAIL — initialize still returns MethodNotFound (test now expects a Response).

- [ ] **Step 3: Implement the initialize handler**

Edit `voicepilot/crates/trust-kernel/src/mcp/server.rs`, replacing the `handle_request` body:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: PASS — 2 tests.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/server.rs voicepilot/crates/trust-kernel/tests/mcp_server.rs
git commit -m "feat(mcp): initialize handshake with protocol 2025-11-25 (V1.1 §6.1)"
```

---

## Task 8: `tools/list` and `tools/call` methods

**Goal:** Wire the `tools/list` and `tools/call` JSON-RPC methods to the existing `McpHandler::list_tools()` and `McpHandler::call_tool()`. For `tools/call`, log an audit event via `kernel.audit_append_external()` when the call carries a `task_id` parameter (stateful tools only). Stateless calls (no task_id) skip audit logging.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/server.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/mcp_server.rs`

- [ ] **Step 1: Write the failing tests for tools/list and tools/call**

Append to `voicepilot/crates/trust-kernel/tests/mcp_server.rs`:

```rust
use trust_kernel::mcp::handler::McpCallResult;

#[test]
fn server_tools_list_returns_3_filesystem_tools() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(2),
        method: "tools/list".to_string(),
        params: None,
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(!s.contains(r#""code""#), "tools/list must succeed: {}", s);
    assert!(s.contains(r#""filesystem.search_files""#));
    assert!(s.contains(r#""filesystem.move_files""#));
    assert!(s.contains(r#""filesystem.verify_move""#));
}

#[test]
fn server_tools_call_dispatches_search_files() {
    use std::fs;
    let kernel = TrustKernel::open_in_memory().unwrap();
    // Set up a temp dir with a file.
    let dir = std::env::temp_dir().join(format!("vp-w4-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.pdf"), b"pdf").unwrap();

    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(3),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.search_files",
            "arguments": {
                "root": dir.to_string_lossy(),
                "pattern": "*.pdf"
            }
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    assert!(!s.contains(r#""code""#), "tools/call search_files must succeed: {}", s);
    assert!(s.contains("a.pdf"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn server_tools_call_rejects_move_files() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(4),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.move_files",
            "arguments": {
                "task_id": "t1",
                "step_id": "s1",
                "sources": ["C:/tmp/a.pdf"],
                "destination": "C:/tmp/out/"
            }
        })),
    };
    let outgoing = server.handle_request(req).unwrap();
    let s = serde_json::to_string(&outgoing).unwrap();
    // Must be an error — move_files must go through the Skill executor.
    assert!(s.contains(r#""code""#));
}

#[test]
fn server_tools_call_logs_audit_when_task_id_present() {
    use std::fs;
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t-audit", "audit test").unwrap();
    let before = kernel.audit_count_for_task("t-audit").unwrap();

    let dir = std::env::temp_dir().join(format!("vp-w4-audit-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("x.pdf"), b"x").unwrap();

    let server = McpServer::new(McpHandler::new(), kernel.clone_for_test());
    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: JsonRpcId::Number(5),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "filesystem.search_files",
            "arguments": {
                "root": dir.to_string_lossy(),
                "pattern": "*.pdf",
                "task_id": "t-audit",
                "step_id": "s-audit"
            }
        })),
    };
    server.handle_request(req).unwrap();
    let after = kernel.audit_count_for_task("t-audit").unwrap();
    assert_eq!(after, before + 1, "tools/call with task_id must log audit event");

    fs::remove_dir_all(&dir).ok();
}
```

Note: the last test uses `kernel.clone_for_test()` — see Step 3 for the helper. If the kernel cannot be cheaply cloned, the test must instead use an `Arc<TrustKernel>` (the W3b kernel uses `Arc<Mutex<Connection>>` internally, so cloning the facade is feasible).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: FAIL — `tools/list` and `tools/call` still return MethodNotFound.

- [ ] **Step 3: Implement tools/list and tools/call handlers**

Edit `voicepilot/crates/trust-kernel/src/mcp/server.rs`. First, add `Clone` derive to `TrustKernel` if not already present — check `kernel.rs` for `#[derive(Clone)]` on the struct. If missing, add it (the kernel holds `Arc<Mutex<Connection>>` + `Arc<Mutex<...>>` for sub-repos, so cloning is cheap and shares state). Add a `clone_for_test` method:

In `voicepilot/crates/trust-kernel/src/kernel.rs`, ensure the struct derives Clone:

```rust
#[derive(Clone)]
pub struct TrustKernel {
    // ... existing fields
}
```

If the struct does not derive Clone because it holds a non-Clone field, instead change `McpServer` to hold `Arc<TrustKernel>` and update the constructor. For W4 we prefer `Arc<TrustKernel>` — see Step 4.

Now edit `voicepilot/crates/trust-kernel/src/mcp/server.rs`:

```rust
use std::sync::Arc;
use crate::mcp::handler::McpCallResult;
use crate::mcp::schema::McpToolSchema;

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
        id: crate::mcp::transport::JsonRpcId,
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
        Ok(OutgoingMessage::Response(
            crate::mcp::transport::JsonRpcResponse::new(id, result),
        ))
    }

    fn handle_tools_list(
        &self,
        id: crate::mcp::transport::JsonRpcId,
    ) -> Result<OutgoingMessage> {
        let tools = self.handler.list_tools();
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();
        let result = serde_json::json!({ "tools": tools_json });
        Ok(OutgoingMessage::Response(
            crate::mcp::transport::JsonRpcResponse::new(id, result),
        ))
    }

    fn handle_tools_call(
        &self,
        id: crate::mcp::transport::JsonRpcId,
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
        let task_id = arguments.get("task_id").and_then(|v| v.as_str()).map(String::from);
        let step_id = arguments.get("step_id").and_then(|v| v.as_str()).map(String::from);

        let result = self.handler.call_tool(&self.kernel, name, &arguments)?;

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
        Ok(OutgoingMessage::Response(
            crate::mcp::transport::JsonRpcResponse::new(id, result_json),
        ))
    }

    pub fn handler(&self) -> &McpHandler {
        &self.handler
    }

    pub fn kernel(&self) -> &TrustKernel {
        &self.kernel
    }
}
```

Also add the import at the top of `server.rs`:

```rust
use crate::error::{KernelError, Result};
```

For the `clone_for_test` helper used in the test, instead update the test to use `Arc`:

Update the failing test `server_tools_call_logs_audit_when_task_id_present` to read:

```rust
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    kernel.create_task("t-audit", "audit test").unwrap();
    let before = kernel.audit_count_for_task("t-audit").unwrap();

    let dir = std::env::temp_dir().join(format!("vp-w4-audit-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("x.pdf"), b"x").unwrap();

    let server = McpServer::with_arc(McpHandler::new(), kernel.clone());
    // ... rest of test unchanged, using `kernel` (Arc) for audit_count_for_task.
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: PASS — 5 tests (2 from Task 7 + 3 new).

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/server.rs voicepilot/crates/trust-kernel/tests/mcp_server.rs
git commit -m "feat(mcp): tools/list + tools/call methods with audit logging (V1.1 §6.1, §6.3)"
```

---

## Task 9: stdio transport loop

**Goal:** Add a `McpServer::run_stdio(reader, writer)` method that implements the read-dispatch-write loop: read a line from `reader`, parse it as an IncomingMessage, dispatch Requests (ignore Notifications/Responses — those are client-to-server only on stdio), write the OutgoingMessage to `writer` as NDJSON.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/server.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/mcp_server.rs`

- [ ] **Step 1: Write the failing test for run_stdio**

Append to `voicepilot/crates/trust-kernel/tests/mcp_server.rs`:

```rust
use std::io::Cursor;
use trust_kernel::mcp::transport::write_message;

#[test]
fn run_stdio_handles_initialize_then_tools_list() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // Two NDJSON lines on stdin: initialize + tools/list.
    let init_line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let list_line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":null}"#;
    let input = format!("{}\n{}\n", init_line, list_line);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    // Two response lines.
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 responses, got: {:?}", lines);
    assert!(lines[0].contains(r#""protocolVersion":"2025-11-25""#));
    assert!(lines[1].contains(r#""filesystem.search_files""#));
}

#[test]
fn run_stdio_skips_notifications() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // A notification (no id) should be silently dropped — no response written.
    let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let input = format!("{}\n", notif);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    assert!(writer.is_empty(), "notification must not produce a response");
}

#[test]
fn run_stdio_continues_after_parse_error() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let server = McpServer::new(McpHandler::new(), kernel);

    // Invalid JSON line, then a valid request.
    let input = "{not valid json\n".to_string()
        + r#"{"jsonrpc":"2.0","id":99,"method":"tools/list"}"#
        + "\n";
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    let output = String::from_utf8(writer).unwrap();
    // Parse error response for the bad line, then tools/list response.
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 2);
    assert!(lines[0].contains(r#""code":-32700"#), "first must be parse error");
    assert!(lines[1].contains(r#""filesystem.search_files""#));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: FAIL — `run_stdio` does not exist.

- [ ] **Step 3: Implement run_stdio**

Append to `voicepilot/crates/trust-kernel/src/mcp/server.rs` (inside `impl McpServer`):

```rust
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
        use std::io::BufRead;
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
                        crate::mcp::transport::JsonRpcId::Null,
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_server 2>&1`
Expected: PASS — 8 tests (5 from Task 8 + 3 new).

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/server.rs voicepilot/crates/trust-kernel/tests/mcp_server.rs
git commit -m "feat(mcp): stdio transport loop with NDJSON framing (V1.1 §6.1)"
```

---

## Task 10: Auto-seed builtin voicepilot-filesystem server row

**Goal:** When the CLI `mcp-serve` command starts, ensure the `voicepilot-filesystem` builtin server row exists in `mcp_servers` (auto-create if missing). The row carries the default `allowed_paths` whitelist. This makes the CLI "just work" on first run without manual SQL.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/mcp/repo.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`

- [ ] **Step 1: Write the failing test for auto-seed**

Append to `voicepilot/crates/trust-kernel/tests/mcp_repo.rs`:

```rust
#[test]
fn repo_seed_builtin_creates_row_if_missing() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    // Confirm row does not exist yet.
    assert!(repo.get(&k.conn(), "voicepilot-filesystem").unwrap().is_none());
    // Seed.
    repo.seed_builtin_filesystem(&k.conn()).unwrap();
    let rec = repo.get(&k.conn(), "voicepilot-filesystem")
        .unwrap()
        .expect("row must exist after seed");
    assert_eq!(rec.name, "VoicePilot Filesystem");
    assert_eq!(rec.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(rec.transport, "stdio");
    assert!(rec.enabled);
    assert!(rec.trusted);
    // allowed_paths should be a non-empty JSON array.
    assert!(rec.allowed_paths.is_some());
}

#[test]
fn repo_seed_builtin_is_idempotent() {
    let k = TrustKernel::open_in_memory().unwrap();
    let repo = McpServerRepo::new();
    repo.seed_builtin_filesystem(&k.conn()).unwrap();
    // Modify the row.
    let mut rec = repo.get(&k.conn(), "voicepilot-filesystem").unwrap().unwrap();
    rec.allowed_paths = Some(r#"["D:/custom"]"#.to_string());
    repo.update(&k.conn(), &rec).unwrap();
    // Seed again — must NOT overwrite the custom allowed_paths.
    repo.seed_builtin_filesystem(&k.conn()).unwrap();
    let loaded = repo.get(&k.conn(), "voicepilot-filesystem").unwrap().unwrap();
    assert_eq!(loaded.allowed_paths.as_deref(), Some(r#"["D:/custom"]"#));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_repo 2>&1`
Expected: FAIL — `seed_builtin_filesystem` does not exist.

- [ ] **Step 3: Implement seed_builtin_filesystem**

Append to `voicepilot/crates/trust-kernel/src/mcp/repo.rs` (inside `impl McpServerRepo`):

```rust
    /// Ensure the builtin voicepilot-filesystem server row exists.
    /// Idempotent — does not overwrite an existing row (preserves user
    /// customizations to allowed_paths).
    pub fn seed_builtin_filesystem(&self, conn: &Connection) -> Result<()> {
        if self.get(conn, "voicepilot-filesystem")?.is_some() {
            return Ok(());
        }
        let default_paths = serde_json::json!(["C:/Users", "D:/"]).to_string();
        let rec = McpServerRecord {
            server_id: "voicepilot-filesystem".to_string(),
            name: "VoicePilot Filesystem".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            transport: "stdio".to_string(),
            enabled: true,
            trusted: true,
            protocol_version: Some("2025-11-25".to_string()),
            allowed_origins: None,
            allowed_paths: Some(default_paths),
        };
        self.create(conn, &rec)
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_repo 2>&1`
Expected: PASS — 11 tests (9 from Task 4 + 2 new).

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/repo.rs voicepilot/crates/trust-kernel/tests/mcp_repo.rs
git commit -m "feat(mcp): seed_builtin_filesystem idempotent row creator (V1.1 §8.1)"
```

---

## Task 11: CLI `mcp-serve` command

**Goal:** Add a `mcp-serve` subcommand to the CLI that opens the kernel, seeds the builtin filesystem server row, loads `allowed_paths` from it, constructs a `FilesystemTool::new_with_allowed_paths()` and replaces the kernel's default filesystem tool (or wraps it via a new kernel method), then enters the `McpServer::run_stdio` loop on stdin/stdout.

Because `TrustKernel` owns its `FilesystemTool` internally and W3a did not expose a setter, W4 adds a `kernel.replace_filesystem_with_allowed_paths(allowed)` method that swaps the internal tool. This keeps the kernel's encapsulation while allowing the CLI to configure the whitelist at startup.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Modify: `voicepilot/crates/cli/src/main.rs`

- [ ] **Step 1: Write the failing test for replace_filesystem**

Append to `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`:

```rust
#[test]
fn kernel_replace_filesystem_enforces_allowed_paths() {
    use trust_kernel::allowed_paths::AllowedPaths;
    use std::path::Path;

    let k = trust_kernel::kernel::TrustKernel::open_in_memory().unwrap();
    let allowed = AllowedPaths::new(vec!["C:/Users".to_string()]);
    k.replace_filesystem_with_allowed_paths(allowed);

    // Path under C:/Users should work; outside should be rejected.
    // (We can't easily test prepare_move without real files, so we test
    // via the AllowedPaths directly via a search_files call with a
    // disallowed root.)
    let result = k.filesystem().search_files(Path::new("E:/elsewhere"), "*.pdf");
    assert!(result.is_err(), "search_files outside allowed_paths must fail");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: FAIL — `replace_filesystem_with_allowed_paths` does not exist.

- [ ] **Step 3: Add the kernel method**

Edit `voicepilot/crates/trust-kernel/src/kernel.rs`. First, check how `filesystem` is stored — if it's behind `Arc<Mutex<...>>`, we need to swap the inner value. Locate the `filesystem` field in the `TrustKernel` struct definition. If it's `Arc<Mutex<FilesystemTool>>`, add:

```rust
    /// Replace the internal FilesystemTool with one that enforces an
    /// AllowedPaths whitelist. Used by the CLI mcp-serve command to
    /// inject the whitelist loaded from mcp_servers.allowed_paths.
    /// V1.1 §4.4 + §8.1 — resolves spec issue #31.
    pub fn replace_filesystem_with_allowed_paths(&self, allowed: AllowedPaths) {
        let new_tool = crate::tools::fs::FilesystemTool::new_with_allowed_paths(allowed);
        // Swap the inner value. If the field is Arc<Mutex<FilesystemTool>>,
        // lock and replace. If it's a plain field behind &self, this won't
        // compile — in that case, change the field to Arc<Mutex<...>> or
        // use interior mutability.
        *self.filesystem.lock().unwrap() = new_tool;
    }
```

Add the import at the top of `kernel.rs`:

```rust
use crate::allowed_paths::AllowedPaths;
```

Note: the exact locking primitive depends on the existing field type. If the field is `Arc<Mutex<FilesystemTool>>`, the code above works. If it's a different type (e.g., `Rc<RefCell<...>>`), adjust accordingly. The implementer should read the existing `kernel.rs` struct definition first and match the pattern.

- [ ] **Step 4: Add the CLI mcp-serve command**

Edit `voicepilot/crates/cli/src/main.rs`. First, add the imports at the top:

```rust
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::repo::McpServerRepo;
use trust_kernel::mcp::server::McpServer;
```

Then add a new branch in the command dispatch loop (after the `organize` branch, before the catch-all):

```rust
        if line == "mcp-serve" {
            handle_mcp_serve_command(&kernel);
            continue;
        }
```

And add the help text (after the `organize` help line):

```rust
    println!("  mcp-serve       start MCP server on stdio (W4)");
```

Then define the handler function at the bottom of the file (after `handle_organize_command`):

```rust
fn handle_mcp_serve_command(kernel: &TrustKernel) {
    // Seed builtin server row (idempotent).
    let repo = McpServerRepo::new();
    if let Err(e) = repo.seed_builtin_filesystem(&*kernel.conn()) {
        eprintln!("error seeding builtin mcp_servers row: {}", e);
        return;
    }
    // Load allowed_paths from the builtin row.
    let allowed = repo
        .load_allowed_paths(&*kernel.conn(), "voicepilot-filesystem")
        .ok()
        .flatten();
    if let Some(allowed) = allowed {
        kernel.replace_filesystem_with_allowed_paths(allowed);
    }
    // Construct McpServer and run stdio loop.
    let server = McpServer::new(McpHandler::new(), kernel.clone());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    if let Err(e) = server.run_stdio(stdin.lock(), &mut stdout) {
        eprintln!("mcp-serve error: {}", e);
    }
}
```

Note: this requires `TrustKernel` to be `Clone` (added in Task 8 Step 3). If `TrustKernel` is not Clone, use `Arc<TrustKernel>` and `McpServer::with_arc`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: PASS — all tests including the new one.

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli 2>&1`
Expected: 0 warnings, 0 errors.

- [ ] **Step 6: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/kernel_accessors.rs voicepilot/crates/cli/src/main.rs
git commit -m "feat(cli): mcp-serve command with allowed_paths injection (V1.1 §6.1, §8.1, issue #31)"
```

---

## Task 12: End-to-end smoke test

**Goal:** Verify the entire W4 pipeline works: seed builtin server → load allowed_paths → construct McpServer → feed a multi-line NDJSON input (initialize + tools/list + tools/call search_files + tools/call move_files rejected) → assert responses match MCP spec + audit trail is populated.

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w4_e2e_smoke.rs`

- [ ] **Step 1: Write the end-to-end test**

Create `voicepilot/crates/trust-kernel/tests/w4_e2e_smoke.rs`:

```rust
use std::fs;
use std::io::Cursor;
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::repo::McpServerRepo;
use trust_kernel::mcp::server::McpServer;

fn tmp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w4-e2e-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn end_to_end_mcp_server_smoke() {
    // ===== Setup: kernel + seed builtin server + load allowed_paths =====
    let kernel = Arc::new(TrustKernel::open_in_memory().unwrap());
    let repo = McpServerRepo::new();
    repo.seed_builtin_filesystem(&*kernel.conn()).unwrap();

    // Create a task for the tools/call audit test.
    kernel.create_task("e2e-task", "mcp e2e test").unwrap();

    // ===== Setup: temp filesystem with a PDF =====
    let dir = tmp_dir();
    // Use a path under one of the default allowed_paths (C:/Users or D:/).
    // On Windows, std::env::temp_dir() is usually under C:/Users.
    fs::write(dir.join("a.pdf"), b"pdf-content").unwrap();

    // ===== Construct McpServer (no allowed_paths injection here —
    // the e2e test uses the kernel's default FilesystemTool which has
    // no whitelist, so search_files works on any path) =====
    let server = McpServer::with_arc(McpHandler::new(), kernel.clone());

    // ===== Build NDJSON input: 4 messages =====
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    let search = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"filesystem.search_files","arguments":{{"root":"{}","pattern":"*.pdf","task_id":"e2e-task","step_id":"e2e-step"}}}}}}"#,
        dir.to_string_lossy().replace('\\', "/").replace('"', "\\\"")
    );
    let reject = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"filesystem.move_files","arguments":{"task_id":"e2e-task","step_id":"e2e-step","sources":["C:/nonexistent"],"destination":"C:/out"}}}"#;
    let input = format!("{}\n{}\n{}\n{}\n", init, list, search, reject);
    let reader = Cursor::new(input.into_bytes());
    let mut writer = Vec::new();

    server.run_stdio(reader, &mut writer).unwrap();
    let output = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    // ===== Assert: 4 responses =====
    assert_eq!(lines.len(), 4, "expected 4 responses, got: {}", output);

    // ===== Assert: initialize response =====
    assert!(lines[0].contains(r#""protocolVersion":"2025-11-25""#));
    assert!(lines[0].contains(r#""serverInfo""#));

    // ===== Assert: tools/list response contains 3 filesystem tools =====
    assert!(lines[1].contains(r#""filesystem.search_files""#));
    assert!(lines[1].contains(r#""filesystem.move_files""#));
    assert!(lines[1].contains(r#""filesystem.verify_move""#));

    // ===== Assert: tools/call search_files response contains a.pdf =====
    assert!(lines[2].contains("a.pdf"));
    assert!(lines[2].contains(r#""isError":false"#));

    // ===== Assert: tools/call move_files response is an error =====
    assert!(lines[3].contains(r#""isError":true"#));

    // ===== Assert: audit trail has the MCP_TOOLS_CALL event =====
    let audit_count = kernel.audit_count_for_task("e2e-task").unwrap();
    // search_files carried task_id=e2e-task → 1 audit event.
    // move_files rejection happens before audit_append_external (call_tool
    // returns Err) → no audit event for the rejection.
    assert_eq!(audit_count, 1, "expected 1 MCP_TOOLS_CALL audit event");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn end_to_end_allowed_paths_enforced_after_replace() {
    use std::path::Path;
    use trust_kernel::allowed_paths::AllowedPaths;

    let kernel = TrustKernel::open_in_memory().unwrap();
    // Default filesystem tool — no whitelist, any path works.
    assert!(kernel.filesystem().search_files(Path::new("E:/nonexistent"), "*.pdf").is_ok());

    // Replace with a tool that enforces C:/Users whitelist.
    let allowed = AllowedPaths::new(vec!["C:/Users".to_string()]);
    kernel.replace_filesystem_with_allowed_paths(allowed);

    // Path outside whitelist must now be rejected.
    let result = kernel.filesystem().search_files(Path::new("E:/elsewhere"), "*.pdf");
    assert!(result.is_err(), "search_files outside allowed_paths must fail after replace");
}
```

- [ ] **Step 2: Run the end-to-end test**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w4_e2e_smoke 2>&1`
Expected: PASS — 2 tests.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --manifest-path voicepilot\Cargo.toml 2>&1`
Expected: ALL PASS — W1 + W2 + W3a + W3b + W4 (expected ~170+ tests total).

- [ ] **Step 4: Verify CLI builds with 0 warnings**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli 2>&1`
Expected: 0 warnings, 0 errors.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/tests/w4_e2e_smoke.rs
git commit -m "test(w4): end-to-end smoke test for MCP server (V1.1 §6.1, §11.1 W4 gate)"
```

---

## Self-Review

**1. Spec coverage (V1.1.1 spec sections referenced):**

| Spec section | Covered by |
|---|---|
| §6.1 MCP protocol_version=2025-11-25 | Task 7 (initialize returns protocolVersion) |
| §6.1 MCP tool schema (inputSchema/outputSchema/annotations) | W3b Task 8 (already done); W4 Task 8 (tools/list returns them) |
| §6.1 stdio transport (NDJSON) | Tasks 2, 9 |
| §6.1 JSON-RPC 2.0 wire protocol | Tasks 1, 2 |
| §6.2 prepare → approve → commit (move_files rejection at MCP layer) | Task 8 (move_files returns is_error=true) |
| §6.3 ToolResult V2 (tools/call returns content + is_error) | Task 8 |
| §8.1 mcp_servers table (CRUD + allowed_paths) | Tasks 3, 4, 10 |
| §8.1 audit_logs (MCP_TOOLS_CALL events) | Task 8 (audit_append_external) + Task 5 |
| §4.4 path canonicalization + allowed_paths enforcement | Task 11 (replace_filesystem_with_allowed_paths) + Task 4 (load_allowed_paths) |
| §11.1 W4 gate "MCP Server Wrapping" | Task 12 (end-to-end smoke) |

Gaps (deferred to later weeks):
- SSE transport (W4 does stdio only; SSE deferred to W4.5 or W5)
- skills.list / skills.route MCP tools (W7, when LLM Planner lands)
- External MCP client integration test with Claude Desktop / Cursor (manual, post-W4)
- YAML-loaded user-saved MCP servers (W7+)

**2. Placeholder scan:** No "TBD", "TODO", "fill in" found in implementation steps. The Task 8 Step 3 note about `clone_for_test` is resolved by switching to `Arc<TrustKernel>` (also in Step 3). The Task 11 Step 3 note about "exact locking primitive depends on existing field type" is a directive to the implementer to read the existing code first — not a placeholder, but a context-aware instruction.

**3. Type consistency:**
- `JsonRpcId` (Task 1) is used by `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError` (Task 1) and `McpServer::handle_request` (Task 6+).
- `IncomingMessage` (Task 2) is used by `parse_line` (Task 2) and `run_stdio` (Task 9).
- `OutgoingMessage` (Task 6) is returned by `handle_request` and consumed by `run_stdio`.
- `McpServerRecord` (Task 3) is used by `McpServerRepo::create/get/list/update/delete` (Task 3) + `seed_builtin_filesystem` (Task 10).
- `AllowedPaths` (W3b Task 6) is returned by `load_allowed_paths` (Task 4) and consumed by `replace_filesystem_with_allowed_paths` (Task 11).
- `McpCallResult` (W3b Task 8) is matched in `handle_tools_call` (Task 8).
- `JsonRpcErrorCode::MethodNotFound` / `ParseError` (Task 1) used in Tasks 6, 9.

No issues found.

---

## Spec Notes & Issues Found During W4 Planning

Per user preferences ("遇到不合理或可优化的规格 — 报告给用户"), additional observations from W4 planning:

37. **§6.1 spec mentions `rmcp` v1.6.1 as "生产可用" but VoicePilot W3a hard constraint says "no MCP SDK dependency".** W4 keeps the native Rust philosophy (hand-rolled JSON-RPC). **Suggestion**: spec should clarify whether `rmcp` is mandatory, optional, or discouraged. If optional, document the tradeoff (interop guarantees vs. dep tree size). W4 implements native; a future W9+ may revisit if interop issues arise with Claude Desktop / Cursor.

38. **§6.1 does not specify which MCP transport (stdio, SSE, HTTP) is the W4 baseline.** W4 implements stdio only (simplest, matches Claude Desktop config). **Suggestion**: spec should pin stdio as the W1-W8 baseline and add SSE/HTTP as W9+ optional. Without this, W4 cannot be tested against external clients deterministically.

39. **§6.1 `tools/call` response shape (`content` + `isError`) is from MCP spec but not mentioned in V1.1.** W4 follows the MCP 2025-11-25 convention (`{content: [{type: "text", text: "..."}], isError: bool}`) but V1.1 spec only describes `ToolResult` V2 (§6.3) which is the internal Rust type. **Suggestion**: spec should add a mapping table: ToolResult V2 → MCP tools/call response shape, so W4's translation layer is reproducible.

40. **§8.1 `mcp_servers.allowed_paths` is stored as JSON text but the spec doesn't specify whether paths are pre-canonicalized.** W4 stores raw strings and canonicalizes at `AllowedPaths::new()` time (per W3b `fs_paths::canonicalize`). **Suggestion**: spec should pin that `allowed_paths` entries are canonicalized at load time, not at write time — this allows users to write `C:\Users` or `C:/Users` interchangeably.

41. **§6.1 spec doesn't define audit logging requirements for MCP calls.** W4 logs `MCP_TOOLS_CALL` events for stateful calls (those with `task_id` in arguments) but skips stateless calls (`initialize`, `tools/list`). **Suggestion**: spec should specify which MCP methods require audit logging — at minimum `tools/call` for stateful tools, and whether `tools/list` (which reveals tool inventory) should be logged for threat detection.

42. **§11.1 W4 gate "MCP Server Wrapping" is ambiguous like the W3 gate was.** W4 implements: stdio MCP server + initialize handshake + tools/list + tools/call (search_files works, move_files rejected) + allowed_paths enforcement + audit trail. **Suggestion**: spec should pin the W4 gate to: "外部 MCP client 可经 stdio 连接 → initialize 握手 → tools/list 返回 3 个 filesystem tool → tools/call search_files 跑通 → tools/call move_files 被拒 → 审计链含 MCP_TOOLS_CALL 事件".

43. **§6.1 spec doesn't specify whether the MCP server should run in the same process as the Trust Kernel or as a separate process.** W4 runs in-process (CLI `mcp-serve` spawns the server in the same process, sharing the kernel). **Suggestion**: spec should clarify that the MCP server is an in-process module of the Trust Kernel, not a separate service — this matches the "single Rust trust kernel" hard constraint.

These are documented here for the user; no spec changes have been made.

---

## W4 Exit Criteria

W4 is complete when ALL of the following hold:
- [ ] All 12 tasks committed
- [ ] `cargo test` passes (W1 + W2 + W3a + W3b + W4, expected ~170+ tests total)
- [ ] `cargo build --manifest-path voicepilot\Cargo.toml -p cli` compiles with 0 warnings
- [ ] CLI `mcp-serve` command runs and accepts NDJSON on stdin
- [ ] `initialize` handshake returns `protocolVersion=2025-11-25` + `serverInfo`
- [ ] `tools/list` returns 3 filesystem tools with full inputSchema/outputSchema/annotations
- [ ] `tools/call filesystem.search_files` dispatches to `FilesystemTool::search_files` and returns matches
- [ ] `tools/call filesystem.move_files` returns `isError=true` (must go through Skill executor)
- [ ] `McpServerRepo` CRUD works against `mcp_servers` table
- [ ] `seed_builtin_filesystem` is idempotent (preserves user customizations)
- [ ] `load_allowed_paths` returns `AllowedPaths` instance ready for FilesystemTool injection
- [ ] `kernel.replace_filesystem_with_allowed_paths(allowed)` enforces whitelist on subsequent calls
- [ ] `kernel.audit_append_external(task_id, step_id, "MCP_TOOLS_CALL", details)` public method works
- [ ] `tools/call` with `task_id` in arguments logs `MCP_TOOLS_CALL` audit event
- [ ] `w4_e2e_smoke.rs` passes (full pipeline + allowed_paths enforcement)
- [ ] Spec issues 37-43 documented for user review
- [ ] No spec changes made (user decides whether to bump to V1.1.2)
