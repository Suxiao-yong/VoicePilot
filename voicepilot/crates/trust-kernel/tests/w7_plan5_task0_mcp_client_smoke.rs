//! W7 Plan 5 Task 0 — smoke tests for mcp_servers schema migration +
//! McpClient subprocess lifecycle.
//!
//! Tests:
//! 1. `migration_adds_command_args_env_columns` — schema has new columns
//! 2. `migration_is_idempotent` — re-running migrations doesn't fail
//! 3. `mcp_server_record_roundtrips_command_args_env` — repo round-trips new fields
//! 4. `mcp_client_spawn_nonexistent_command_returns_error` — spawn failure path
//! 5. `mcp_client_invoke_tool_via_python_mock` — end-to-end JSON-RPC over stdio
//!
//! Test 5 spawns a Python 3 mock MCP server (reads NDJSON on stdin, writes
//! canned responses on stdout). It is `#[ignore]`-by-default-when-python-
//! missing: at runtime we probe `python --version`; if absent, the test
//! short-circuits with a passing assertion (no Python ⇒ no mock ⇒ skip).
//! On CI/dev machines with Python 3 installed it exercises the full
//! spawn → initialize → tools/call → drop lifecycle.

use serde_json::json;
use std::process::Command;
use trust_kernel::db;
use trust_kernel::mcp::client::McpClient;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};

// ---- Test 1: schema migration adds columns ----

#[test]
fn migration_adds_command_args_env_columns() {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    // Verify columns exist by querying pragma_table_info.
    let mut stmt = conn.prepare("PRAGMA table_info(mcp_servers)").unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(
        cols.contains(&"command".to_string()),
        "command column missing: {:?}",
        cols
    );
    assert!(
        cols.contains(&"args".to_string()),
        "args column missing: {:?}",
        cols
    );
    assert!(
        cols.contains(&"env".to_string()),
        "env column missing: {:?}",
        cols
    );
}

// ---- Test 2: migration is idempotent ----

#[test]
fn migration_is_idempotent() {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    // Run again — should not fail (ALTER TABLE ADD COLUMN would error with
    // "duplicate column name", which the runner must swallow).
    db::run_migrations(&conn).unwrap();
}

// ---- Test 3: McpServerRecord round-trips new fields ----

#[test]
fn mcp_server_record_roundtrips_command_args_env() {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    let repo = McpServerRepo::new();
    let rec = McpServerRecord {
        server_id: "test".to_string(),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: false,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: None,
        command: Some("npx".to_string()),
        args: Some(r#"["-y","@playwright/mcp@latest"]"#.to_string()),
        env: Some("{}".to_string()),
    };
    repo.create(&conn, &rec).unwrap();
    let loaded = repo.get(&conn, "test").unwrap().unwrap();
    assert_eq!(loaded.command.as_deref(), Some("npx"));
    assert_eq!(
        loaded.args.as_deref(),
        Some(r#"["-y","@playwright/mcp@latest"]"#)
    );
    assert_eq!(loaded.env.as_deref(), Some("{}"));
}

// ---- Test 4: McpClient.spawn failure path ----

#[test]
fn mcp_client_spawn_nonexistent_command_returns_error() {
    let result = McpClient::spawn(
        "this-command-does-not-exist-12345",
        &[],
        &serde_json::json!({}),
    );
    let err = match result {
        Ok(_) => panic!("expected spawn to fail for non-existent command"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("failed to spawn"),
        "error must mention 'failed to spawn', got: {err}"
    );
}

// ---- Test 5: McpClient end-to-end via Python mock ----
//
// The mock is a single `python -c` invocation that:
//   1. Reads `initialize` request → responds with `{"protocolVersion":"2025-11-25","capabilities":{}}`
//   2. Reads `notifications/initialized` notification (no response)
//   3. Reads `tools/call` with name="echo" → responds with
//      `{"content":[{"type":"text","text":"{\"echo\":\"hello\"}"}],"isError":false}`
//
// The test then asserts `client.invoke_tool("echo", {"msg":"hello"})` returns
// `{"echo":"hello"}` (parsed from content[0].text).

#[test]
fn mcp_client_invoke_tool_via_python_mock() {
    // Probe Python availability. If absent, skip the test (not fail).
    let python_probe = Command::new("python").arg("--version").output();
    let python_available = match python_probe {
        Ok(out) => out.status.success(),
        Err(_) => false,
    };
    if !python_available {
        eprintln!("skipping mcp_client_invoke_tool_via_python_mock: python not on PATH");
        return;
    }

    // The mock server script. Reads NDJSON lines from stdin; for each line:
    //   - if it's initialize request → emit initialize response
    //   - if it's notifications/initialized → no response
    //   - if it's tools/call with name="echo" → emit canned content[0].text
    //   - any other request → emit a JSON-RPC error
    // Exits on EOF.
    let mock_script = r#"
import sys, json
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if msg.get("method") == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "serverInfo": {"name": "mock", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        # notification — no response
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "echo":
            text_payload = json.dumps({"echo": "hello"})
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "result": {
                    "content": [{"type": "text", "text": text_payload}],
                    "isError": False
                }
            })
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": f"unknown tool {name}"}
            })
    else:
        # Unknown method — emit error so the client surfaces it.
        emit({
            "jsonrpc": "2.0",
            "id": msg.get("id"),
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

    let mut client = McpClient::spawn(
        "python",
        &["-c".to_string(), mock_script.to_string()],
        &json!({}),
    )
    .expect("spawning python mock must succeed");

    client
        .initialize()
        .expect("initialize handshake must succeed");
    let result = client
        .invoke_tool("echo", json!({"msg": "hello"}))
        .expect("tools/call echo must succeed");

    // content[0].text was '{"echo":"hello"}' → parsed into JSON object.
    assert_eq!(result, json!({"echo": "hello"}));
}

// ---- Test 6 (Plan 5 Task 2): kernel boot seeds playwright row ----

#[test]
fn kernel_boot_seeds_playwright_mcp_server_row() {
    use trust_kernel::kernel::TrustKernel;
    use trust_kernel::mcp::repo::McpServerRepo;

    let kernel = TrustKernel::open_in_memory().expect("kernel must construct");
    let repo = McpServerRepo::new();
    let rec = repo
        .get(&kernel.conn(), "playwright")
        .expect("repo.get must not error")
        .expect("playwright row must exist after kernel boot");
    assert_eq!(rec.server_id, "playwright");
    assert_eq!(rec.command.as_deref(), Some("npx"));
    assert_eq!(
        rec.args.as_deref(),
        Some(r#"["-y","@playwright/mcp@latest"]"#)
    );
    assert_eq!(rec.allowed_paths.as_deref(), Some("[]"));
    assert!(rec.enabled, "playwright must be enabled by default at boot");
    // Idempotency on re-boot is covered by repo_insert_default_servers_is_idempotent_*
    // (file-based DB would be needed to verify cross-instance persistence).
}
