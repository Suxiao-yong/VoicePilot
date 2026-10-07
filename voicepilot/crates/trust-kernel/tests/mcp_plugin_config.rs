//! Wave 2 Task 2.2 — real MCP plugin configuration entry points (RED→GREEN).
//!
//! Contract (plan Task 2.2 + design §4):
//! - `register_mcp_server` / `remove_mcp_server` / `toggle_mcp_server` are the
//!   validated entry points over the existing `mcp_servers` table
//!   (`McpServerRecord`, no new plugin table).
//! - Validation happens before any write: failure paths leave the DB and the
//!   runtime catalog untouched; success paths rebuild the extension catalog
//!   via `TrustKernel::reload_extensions()`.
//! - Removing a server still referenced by a User Skill execution target must
//!   fail and return the referencing Skill IDs.
//! - End-to-end: a server registered through the facade routes through the
//!   existing Task 2.1 MCP dispatch chain (deterministic "missing command"
//!   error — no real `npx` required).

use std::fs;
use std::path::Path;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::{McpServerRecord, McpServerRepo};
use trust_kernel::skills::dispatcher::dispatch_skill_executor;
use uuid::Uuid;

/// A structurally complete read-only stdio plugin record.
fn valid_record(server_id: &str) -> McpServerRecord {
    McpServerRecord {
        server_id: server_id.to_string(),
        name: format!("Read-only {server_id}"),
        version: "1.0.0".to_string(),
        transport: "stdio".to_string(),
        enabled: true,
        trusted: true,
        protocol_version: Some("2025-11-25".to_string()),
        allowed_origins: None,
        allowed_paths: Some(r#"["D:/readonly"]"#.to_string()),
        command: Some("npx".to_string()),
        args: Some(r#"["-y","@fixture/mcp-readonly"]"#.to_string()),
        env: Some(r#"{"RO_TOKEN":"env-secret-xyz"}"#.to_string()),
    }
}

/// A complete MCP tool contract Skill manifest file (same shape as the
/// `extensions_registry.rs` / `dispatch_snapshot_gate.rs` fixtures).
fn mcp_tool_skill_md(skill_id: &str, server_id: &str, tool_name: &str) -> String {
    format!(
        r#"---
name: "{skill_id}"
description: "A complete MCP tool contract fixture. Runs {tool_name} on {server_id}."
metadata:
  voicepilot:
    execution:
      type: mcp_tool
      server_id: "{server_id}"
      tool_name: "{tool_name}"
---

# MCP tool contract fixture
"#,
        skill_id = skill_id,
        server_id = server_id,
        tool_name = tool_name,
    )
}

/// Open an in-memory kernel whose user Skills directory is a fresh temp dir.
/// Returns the kernel and the temp guard (kept alive so later reads work).
fn open_kernel_with_skills_dir() -> (TrustKernel, tempfile::TempDir) {
    let temp_root = tempfile::tempdir().expect("create temporary user Skills root");
    let skills_dir = temp_root.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("create temporary user Skills directory");
    let kernel = TrustKernel::open_in_memory_with_user_skills_dir(skills_dir)
        .expect("open in-memory kernel with temporary user Skills directory");
    (kernel, temp_root)
}

fn write_bound_skill(
    kernel: &TrustKernel,
    skills_dir: &Path,
    skill_id: &str,
    server_id: &str,
    tool_name: &str,
) {
    let skill_path = skills_dir.join(format!("{skill_id}/SKILL.md"));
    std::fs::create_dir_all(skill_path.parent().expect("skill dir parent"))
        .expect("create skill dir");
    fs::write(
        skill_path,
        mcp_tool_skill_md(skill_id, server_id, tool_name),
    )
    .expect("write MCP-backed user Skill");
    kernel.load_user_skills().expect("load user Skills");
}

fn server_rows(kernel: &TrustKernel) -> Vec<McpServerRecord> {
    let conn = kernel.conn();
    McpServerRepo::new().list(&conn).expect("list mcp servers")
}

/// Count audit rows whose JSON details contain `needle` — used to prove env
/// values never leak into audit output.
fn count_audit_rows_containing(kernel: &TrustKernel, needle: &str) -> usize {
    let conn = kernel.conn();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE details LIKE ?1",
            rusqlite::params![format!("%{needle}%")],
            |row| row.get(0),
        )
        .expect("count audit rows");
    count as usize
}

// ===== register: invalid input must fail before any write =====

#[test]
fn register_rejects_blank_server_id_and_name_version_transport() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut blank_id = valid_record("srv-blank-id");
    blank_id.server_id = "   ".to_string();
    let err = kernel
        .register_mcp_server(blank_id)
        .expect_err("blank server_id must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("server_id"),
        "error should name the server_id field: {err}"
    );

    let mut blank_name = valid_record("srv-blank-name");
    blank_name.name = " ".to_string();
    let err = kernel
        .register_mcp_server(blank_name)
        .expect_err("blank name must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("name"),
        "error should name the name field: {err}"
    );

    let mut blank_version = valid_record("srv-blank-version");
    blank_version.version = "".to_string();
    let err = kernel
        .register_mcp_server(blank_version)
        .expect_err("blank version must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("version"),
        "error should name the version field: {err}"
    );

    let mut blank_transport = valid_record("srv-blank-transport");
    blank_transport.transport = "".to_string();
    let err = kernel
        .register_mcp_server(blank_transport)
        .expect_err("blank transport must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("transport"),
        "error should name the transport field: {err}"
    );

    assert_eq!(
        server_rows(&kernel).len(),
        before,
        "failed registers must not touch the DB"
    );
}

#[test]
fn register_rejects_transport_outside_allowlist() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut rec = valid_record("srv-http");
    rec.transport = "http".to_string();
    let err = kernel
        .register_mcp_server(rec)
        .expect_err("http transport must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("transport"),
        "error should mention the transport allowlist: {err}"
    );

    assert_eq!(server_rows(&kernel).len(), before);
}

#[test]
fn register_rejects_protocol_version_other_than_locked_value() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut rec = valid_record("srv-pv-bad");
    rec.protocol_version = Some("2024-11-05".to_string());
    let err = kernel
        .register_mcp_server(rec)
        .expect_err("protocol_version other than 2025-11-25 must be rejected");
    assert!(
        err.to_string().contains("2025-11-25"),
        "error should name the locked protocol version: {err}"
    );

    // protocol_version = None is allowed (unspecified).
    let mut rec = valid_record("srv-pv-none");
    rec.protocol_version = None;
    kernel
        .register_mcp_server(rec)
        .expect("None protocol_version must be accepted");

    assert_eq!(server_rows(&kernel).len(), before + 1);
}

#[test]
fn register_rejects_missing_command_and_blank_command() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut no_command = valid_record("srv-no-command");
    no_command.command = None;
    let err = kernel
        .register_mcp_server(no_command)
        .expect_err("stdio spawn server without command must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("command"),
        "error should mention the command field: {err}"
    );

    let mut blank_command = valid_record("srv-blank-command");
    blank_command.command = Some("  ".to_string());
    let err = kernel
        .register_mcp_server(blank_command)
        .expect_err("blank command must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("command"),
        "error should mention the command field: {err}"
    );

    assert_eq!(server_rows(&kernel).len(), before);
}

#[test]
fn register_rejects_malformed_and_non_array_args() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut broken = valid_record("srv-args-broken");
    broken.args = Some("{not-json".to_string());
    let err = kernel
        .register_mcp_server(broken)
        .expect_err("malformed args JSON must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("args"),
        "error should mention the args field: {err}"
    );

    // Valid JSON but not an array of strings (a bare string).
    let mut non_array = valid_record("srv-args-string");
    non_array.args = Some(r#""just-a-string""#.to_string());
    let err = kernel
        .register_mcp_server(non_array)
        .expect_err("non-array args must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("args"),
        "error should mention the args field: {err}"
    );

    // Valid JSON array but not all strings (numbers).
    let mut mixed = valid_record("srv-args-mixed");
    mixed.args = Some(r#"[1, 2, 3]"#.to_string());
    let err = kernel
        .register_mcp_server(mixed)
        .expect_err("args entries must be strings");
    assert!(
        err.to_string().to_ascii_lowercase().contains("args"),
        "error should mention the args field: {err}"
    );

    assert_eq!(server_rows(&kernel).len(), before);
}

#[test]
fn register_rejects_malformed_and_non_map_env() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut broken = valid_record("srv-env-broken");
    broken.env = Some("not-json".to_string());
    let err = kernel
        .register_mcp_server(broken)
        .expect_err("malformed env JSON must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("env"),
        "error should mention the env field: {err}"
    );

    // Valid JSON but not a string→string map (an array).
    let mut array = valid_record("srv-env-array");
    array.env = Some(r#"[1, 2, 3]"#.to_string());
    let err = kernel
        .register_mcp_server(array)
        .expect_err("non-map env must be rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("env"),
        "error should mention the env field: {err}"
    );

    // Valid JSON map but values are not strings.
    let mut non_string_values = valid_record("srv-env-values");
    non_string_values.env = Some(r#"{"PORT": 8080}"#.to_string());
    let err = kernel
        .register_mcp_server(non_string_values)
        .expect_err("env values must be strings");
    assert!(
        err.to_string().to_ascii_lowercase().contains("env"),
        "error should mention the env field: {err}"
    );

    assert_eq!(server_rows(&kernel).len(), before);
}

#[test]
fn register_rejects_non_array_allowed_paths_and_allowed_origins() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    let before = server_rows(&kernel).len();

    let mut bad_paths = valid_record("srv-paths-string");
    bad_paths.allowed_paths = Some(r#""D:/""#.to_string());
    let err = kernel
        .register_mcp_server(bad_paths)
        .expect_err("non-array allowed_paths must be rejected");
    assert!(
        err.to_string()
            .to_ascii_lowercase()
            .contains("allowed_paths"),
        "error should mention the allowed_paths field: {err}"
    );

    let mut bad_origins = valid_record("srv-origins-object");
    bad_origins.allowed_origins = Some(r#"{"x": 1}"#.to_string());
    let err = kernel
        .register_mcp_server(bad_origins)
        .expect_err("non-array allowed_origins must be rejected");
    assert!(
        err.to_string()
            .to_ascii_lowercase()
            .contains("allowed_origins"),
        "error should mention the allowed_origins field: {err}"
    );

    assert_eq!(server_rows(&kernel).len(), before);
}

// ===== register: duplicate id =====

#[test]
fn register_duplicate_server_id_is_rejected() {
    let kernel = TrustKernel::open_in_memory().expect("open_in_memory");
    kernel
        .register_mcp_server(valid_record("srv-dup"))
        .expect("first register must succeed");
    let err = kernel
        .register_mcp_server(valid_record("srv-dup"))
        .expect_err("duplicate server_id must be rejected (explicit safety, no upsert)");
    assert!(
        err.to_string()
            .to_ascii_lowercase()
            .contains("already exists"),
        "duplicate error should say the id already exists: {err}"
    );

    let matches = server_rows(&kernel)
        .into_iter()
        .filter(|row| row.server_id == "srv-dup")
        .count();
    assert_eq!(
        matches, 1,
        "exactly one row must remain for the duplicate id"
    );
}

// ===== register: valid input persists + reloads catalog =====

#[test]
fn register_valid_persists_row_and_publishes_bound_skill_candidate() {
    let server_id = format!("ro-server-{}", Uuid::new_v4());
    let skill_id = format!("ro.skill.{}", Uuid::new_v4());
    let tool_name = "read.query";
    let (kernel, temp_root) = open_kernel_with_skills_dir();
    let skills_dir = temp_root.path().join("skills");

    // Bound Skill exists before the server: display-only until registered.
    write_bound_skill(&kernel, &skills_dir, &skill_id, &server_id, tool_name);
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_none(),
        "bound Skill must not be a candidate before its server is registered"
    );

    kernel
        .register_mcp_server(valid_record(&server_id))
        .expect("register a valid read-only plugin");

    // Row persisted with every mapped field (full McpServerRecord roundtrip).
    let row = server_rows(&kernel)
        .into_iter()
        .find(|row| row.server_id == server_id)
        .expect("registered server row must exist");
    assert!(row.enabled && row.trusted);
    assert_eq!(row.transport, "stdio");
    assert_eq!(row.protocol_version.as_deref(), Some("2025-11-25"));
    assert_eq!(row.command.as_deref(), Some("npx"));
    assert_eq!(
        row.args.as_deref(),
        Some(r#"["-y","@fixture/mcp-readonly"]"#)
    );
    assert_eq!(
        row.env.as_deref(),
        Some(r#"{"RO_TOKEN":"env-secret-xyz"}"#),
        "env round-trips into the row"
    );

    // reload_extensions was triggered by the successful register: the bound
    // Skill is now an enabled candidate in the published snapshot.
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "registering the trusted+enabled server must publish the bound Skill candidate"
    );

    // env values must never appear in audit output.
    assert_eq!(
        count_audit_rows_containing(&kernel, "env-secret-xyz"),
        0,
        "env secret must never leak into audit_logs"
    );
}

// ===== toggle: runtime catalog follows the enabled flag =====

#[test]
fn toggle_off_removes_dependent_candidate_then_on_restores() {
    let server_id = format!("toggle-server-{}", Uuid::new_v4());
    let skill_id = format!("toggle.skill.{}", Uuid::new_v4());
    let tool_name = "read.query";
    let (kernel, temp_root) = open_kernel_with_skills_dir();
    let skills_dir = temp_root.path().join("skills");

    write_bound_skill(&kernel, &skills_dir, &skill_id, &server_id, tool_name);
    kernel
        .register_mcp_server(valid_record(&server_id))
        .expect("register plugin");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "candidate must exist before toggle-off"
    );

    kernel
        .toggle_mcp_server(&server_id, false)
        .expect("toggle off");
    let row = server_rows(&kernel)
        .into_iter()
        .find(|row| row.server_id == server_id)
        .expect("server row must still exist after toggle-off");
    assert!(
        !row.enabled,
        "toggle-off must persist enabled=false in the DB"
    );
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_none(),
        "toggle-off must remove the dependent candidate from the runtime snapshot"
    );

    kernel
        .toggle_mcp_server(&server_id, true)
        .expect("toggle on");
    let row = server_rows(&kernel)
        .into_iter()
        .find(|row| row.server_id == server_id)
        .expect("server row must still exist after toggle-on");
    assert!(row.enabled, "toggle-on must persist enabled=true in the DB");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "toggle-on must restore the dependent candidate"
    );
}

// ===== remove: referenced server is protected =====

#[test]
fn remove_referenced_server_fails_and_lists_referencing_skill_ids() {
    let server_id = format!("ref-server-{}", Uuid::new_v4());
    let skill_id = format!("ref.skill.{}", Uuid::new_v4());
    let tool_name = "read.query";
    let (kernel, temp_root) = open_kernel_with_skills_dir();
    let skills_dir = temp_root.path().join("skills");

    write_bound_skill(&kernel, &skills_dir, &skill_id, &server_id, tool_name);
    kernel
        .register_mcp_server(valid_record(&server_id))
        .expect("register plugin");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "candidate must exist before the protected removal attempt"
    );

    let err = kernel
        .remove_mcp_server(&server_id)
        .expect_err("removing a referenced server must fail");
    let msg = err.to_string();
    assert!(
        msg.contains(&skill_id),
        "error must list the referencing Skill ID, got: {msg}"
    );
    assert!(
        msg.to_ascii_lowercase().contains("referenced"),
        "error should say the server is referenced, got: {msg}"
    );

    assert!(
        server_rows(&kernel)
            .into_iter()
            .any(|row| row.server_id == server_id),
        "failed removal must leave the DB row present"
    );
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "failed removal must leave the runtime catalog unchanged"
    );
}

// ===== remove: unreferenced server is deleted =====

#[test]
fn remove_unreferenced_server_deletes_row_and_updates_catalog() {
    let server_id = format!("orphan-server-{}", Uuid::new_v4());
    let (kernel, temp_root) = open_kernel_with_skills_dir();
    kernel
        .register_mcp_server(valid_record(&server_id))
        .expect("register unreferenced plugin");
    assert!(
        server_rows(&kernel)
            .into_iter()
            .any(|row| row.server_id == server_id),
        "server must exist before removal"
    );

    kernel
        .remove_mcp_server(&server_id)
        .expect("unreferenced server must be removable");
    assert!(
        !server_rows(&kernel)
            .into_iter()
            .any(|row| row.server_id == server_id),
        "removed server row must be gone"
    );

    // Runtime catalog reflects the removal: a Skill bound to the removed
    // server id must not become a candidate on the next reload.
    let skill_id = format!("orphan.skill.{}", Uuid::new_v4());
    let tool_name = "read.query";
    {
        let skills_dir = temp_root.path().join("skills");
        write_bound_skill(&kernel, &skills_dir, &skill_id, &server_id, tool_name);
    }
    kernel.reload_extensions().expect("reload after removal");
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_none(),
        "catalog must not treat a removed server as executable"
    );
}

// ===== end-to-end: registered plugin routes through the MCP dispatch chain =====

#[test]
fn end_to_end_register_reload_dispatch_routes_through_mcp_chain() {
    let server_id = format!("e2e-ro-server-{}", Uuid::new_v4());
    let skill_id = format!("e2e.ro.{}", Uuid::new_v4());
    let tool_name = "read.query";
    let (kernel, temp_root) = open_kernel_with_skills_dir();
    let skills_dir = temp_root.path().join("skills");

    // Register a read-only plugin through the validated facade.
    kernel
        .register_mcp_server(valid_record(&server_id))
        .expect("register read-only plugin");

    // Clear the spawn command via the repo so the dispatch lands on the
    // deterministic MCP-chain "missing command" error (no real npx needed).
    {
        let conn = kernel.conn();
        let mut rec = McpServerRepo::new()
            .get(&conn, &server_id)
            .expect("read row")
            .expect("row exists");
        rec.command = None;
        McpServerRepo::new()
            .update(&conn, &rec)
            .expect("clear command for deterministic dispatch error");
    }
    kernel
        .reload_extensions()
        .expect("reload after test mutation");

    write_bound_skill(&kernel, &skills_dir, &skill_id, &server_id, tool_name);
    assert!(
        kernel
            .extension_snapshot()
            .resolve_candidate(&skill_id)
            .is_some(),
        "bound Skill must be a dispatch candidate"
    );

    let result = dispatch_skill_executor(
        &skill_id,
        &kernel,
        &serde_json::json!({"query": "test"}),
        &AutoApprover,
        "task-e2e-ro",
        "step-e2e-ro",
    );
    // The registered server must route through invoke_mcp_tool (Task 2.1
    // chain), NOT be rejected by the dispatch gate: the error is the MCP
    // chain's own "missing command" failure.
    let err = result.expect_err("dispatch must route through the MCP chain");
    let msg = err.to_string();
    assert!(
        msg.contains("missing command"),
        "expected the MCP-chain 'missing command' error, got: {msg}"
    );
    assert!(
        !msg.contains("was not found"),
        "must not be a catalog miss: {msg}"
    );
    assert!(
        !msg.contains("disabled"),
        "must not be a dispatch-gate rejection: {msg}"
    );
}
