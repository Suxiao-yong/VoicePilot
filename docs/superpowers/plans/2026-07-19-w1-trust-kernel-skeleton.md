# VoicePilot W1 Implementation Plan — Rust Trust Kernel Skeleton

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust Trust Kernel skeleton with SQLite persistence, audit logging, task state machine, and a text-only CLI entry — enough that a text command can flow through `IDLE → PLANNING → AWAITING_APPROVAL → EXECUTING → DONE` and produce complete audit records.

**Architecture:** Cargo workspace at `d:\voicepilot\voicepilot\` with two crates: `trust-kernel` (library: state machine, audit, db, repositories) and `cli` (binary: text REPL). SQLite via `rusqlite` (no Tauri yet — Tauri lands in W6+). All V1.1 schema fields are included even if unused in W1 (forward-compat). No voice, no MCP, no tools yet — those land in W2+.

**Tech Stack:** Rust 1.96, `rusqlite` 0.32 (bundled SQLite), `serde` 1.0, `serde_json` 1.0, `thiserror` 2.0, `uuid` 1.10, `sha2` 0.10, `chrono` 0.4, `tracing` 0.1.

**Reference:** V1.1 spec at `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`. W1 gate (§11.1): "文本命令可执行".

---

## File Structure

```
d:\voicepilot\
├── .git\                                  # already initialized
├── .gitignore                            # NEW
├── voicepilot\                            # NEW — code workspace root
│   ├── Cargo.toml                         # workspace
│   ├── README.md                          # short
│   ├── crates\
│   │   ├── trust-kernel\
│   │   │   ├── Cargo.toml
│   │   │   ├── src\
│   │   │   │   ├── lib.rs                 # crate root, re-exports
│   │   │   │   ├── error.rs               # KernelError
│   │   │   │   ├── state.rs               # TaskState + transitions
│   │   │   │   ├── db.rs                  # Connection, migrations
│   │   │   │   ├── migrations\
│   │   │   │   │   └── 001_init.sql       # SQLite schema
│   │   │   │   ├── audit.rs               # AuditEvent + AuditLogger
│   │   │   │   ├── repo\
│   │   │   │   │   ├── mod.rs             # repo module
│   │   │   │   │   ├── task_repo.rs       # tasks CRUD
│   │   │   │   │   └── step_repo.rs       # steps CRUD
│   │   │   │   └── kernel.rs              # TrustKernel facade
│   │   │   └── tests\
│   │   │       ├── state_machine.rs       # integration tests
│   │   │       ├── audit.rs
│   │   │       └── end_to_end.rs
│   │   └── cli\
│   │       ├── Cargo.toml
│   │       └── src\
│   │           └── main.rs                # text REPL
└── docs\
    └── superpowers\
        └── plans\
            └── 2026-07-19-w1-trust-kernel-skeleton.md  # THIS FILE
```

Each crate has one clear responsibility. `trust-kernel` owns all persistence and state; `cli` only does I/O. Files split by responsibility (state / db / audit / repo) so each stays small.

---

## Task 1: Initialize Cargo workspace

**Files:**
- Create: `d:\voicepilot\.gitignore`
- Create: `d:\voicepilot\voicepilot\Cargo.toml`
- Create: `d:\voicepilot\voicepilot\README.md`
- Create: `d:\voicepilot\voicepilot\crates\trust-kernel\Cargo.toml`
- Create: `d:\voicepilot\voicepilot\crates\trust-kernel\src\lib.rs`
- Create: `d:\voicepilot\voicepilot\crates\cli\Cargo.toml`
- Create: `d:\voicepilot\voicepilot\crates\cli\src\main.rs`

- [ ] **Step 1: Create `.gitignore`**

```gitignore
# Rust
target/
**/*.rs.bk
Cargo.lock

# IDE
.idea/
.vscode/
*.swp

# OS
Thumbs.db
.DS_Store

# VoicePilot runtime data
*.db
*.db-journal
*.sqlite
*.sqlite-journal
voicepilot/data/

# Logs
*.log
```

- [ ] **Step 2: Create workspace `Cargo.toml`**

```toml
# voicepilot/Cargo.toml
[workspace]
resolver = "2"
members = [
    "crates/trust-kernel",
    "crates/cli",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.96"
authors = ["VoicePilot Team"]
license = "MIT"

[workspace.dependencies]
trust-kernel = { path = "crates/trust-kernel" }
rusqlite = { version = "0.32", features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
uuid = { version = "1.10", features = ["v4", "serde"] }
sha2 = "0.10"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1.0"
```

- [ ] **Step 3: Create `voicepilot/README.md`**

```markdown
# VoicePilot

权限感知语音桌面 Agent — 单一 Rust 信任内核实现。

## 当前状态：W1（Rust Trust Kernel 骨架）

W1 范围：文本入口 + SQLite + 审计 + 状态机。无语音、无 MCP、无工具调用。

## 开发

```bash
cargo build
cargo test
cargo run -p cli
```

参考规格：`../voicepilot-v1.1-spec/voicepilot-v1.1-spec.html`
```

- [ ] **Step 4: Create `trust-kernel` crate manifest**

```toml
# voicepilot/crates/trust-kernel/Cargo.toml
[package]
name = "trust-kernel"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true

[dependencies]
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
sha2 = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 5: Create `trust-kernel/src/lib.rs` stub**

```rust
//! VoicePilot Trust Kernel — the single trust boundary.
//!
//! W1 scope: SQLite persistence, audit logging, task state machine.
//! Voice / MCP / Policy / Action Gateway land in W2+.

pub mod error;
pub mod state;
pub mod db;
pub mod audit;
pub mod repo;
pub mod kernel;
```

- [ ] **Step 6: Create stub modules so `cargo build` compiles**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid state transition: from={from:?} to={to:?}")]
    InvalidTransition { from: super::state::TaskState, to: super::state::TaskState },
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, KernelError>;
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\state.rs`:

```rust
//! Task state machine — see V1.1 spec §3.1 (Trust Kernel components).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    Idle,
    Listening,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Compensating,
    Done,
    Failed,
    Cancelled,
}

impl TaskState {
    /// Returns allowed next states from the current state.
    /// Spec §3.1: IDLE→LISTENING→PLANNING→…; Kill Switch can cancel from most states.
    pub fn allowed_next(self) -> &'static [TaskState] {
        use TaskState::*;
        match self {
            Idle => &[Listening, Cancelled],
            Listening => &[Planning, Cancelled],
            Planning => &[AwaitingApproval, Failed, Cancelled],
            AwaitingApproval => &[Executing, Cancelled],
            Executing => &[Verifying, Compensating, Failed, Cancelled],
            Verifying => &[Done, Compensating, Failed, Cancelled],
            Compensating => &[Done, Failed, Cancelled],
            Done => &[],
            Failed => &[],
            Cancelled => &[],
        }
    }

    pub fn can_transition_to(self, target: TaskState) -> bool {
        self.allowed_next().contains(&target)
    }
}
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\db.rs` (stub — real impl in Task 2):

```rust
//! SQLite connection + migration runner.

use crate::error::Result;
use rusqlite::Connection;

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    Ok(conn)
}

pub fn open_file(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    Ok(conn)
}
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\audit.rs` (stub):

```rust
//! Audit log writer — every tool call must be recorded (V1.1 §1.4: 100% coverage).

use crate::error::Result;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub log_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub details: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub prev_hash: Option<String>,
    pub hash: String,
}

pub trait AuditLogger: Send + Sync {
    fn append(&self, event: &AuditEvent) -> Result<()>;
}
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\mod.rs`:

```rust
//! Repositories — CRUD for tasks, steps.
pub mod task_repo;
pub mod step_repo;
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\task_repo.rs` (stub):

```rust
//! Task repository — CRUD against SQLite `tasks` table.
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\step_repo.rs` (stub):

```rust
//! Step repository — CRUD against SQLite `steps` table.
```

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\kernel.rs` (stub):

```rust
//! TrustKernel facade — entry point for the CLI and (later) Tauri commands.

use crate::error::Result;

pub struct TrustKernel {
    pub db: rusqlite::Connection,
}

impl TrustKernel {
    pub fn open_in_memory() -> Result<Self> {
        let db = crate::db::open_in_memory()?;
        Ok(Self { db })
    }
}
```

- [ ] **Step 7: Create `cli` crate manifest**

```toml
# voicepilot/crates/cli/Cargo.toml
[package]
name = "cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true

[[bin]]
name = "voicepilot"
path = "src/main.rs"

[dependencies]
trust-kernel = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 8: Create `cli/src/main.rs` stub**

```rust
use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();
    let kernel = trust_kernel::kernel::TrustKernel::open_in_memory()?;
    println!("VoicePilot W1 stub. Kernel opened (in-memory). State: Idle.");
    let _ = kernel;
    Ok(())
}
```

- [ ] **Step 9: Verify `cargo build` succeeds**

Run: `cargo build` (cwd: `d:\voicepilot\voicepilot`)
Expected: "Compiling … Finished" with no errors. May show unused-code warnings — acceptable for stubs.

- [ ] **Step 10: Commit**

```bash
git add .gitignore voicepilot/ docs/
git commit -m "feat(w1): initialize cargo workspace with trust-kernel and cli crates"
```

---

## Task 2: SQLite schema migrations

**Files:**
- Create: `d:\voicepilot\voicepilot\crates\trust-kernel\src\migrations\001_init.sql`
- Modify: `d:\voicepilot\voicepilot\crates\trust-kernel\src\db.rs` (replace stub with real migration runner)
- Test: `d:\voicepilot\voicepilot\crates\trust-kernel\tests\migrations.rs`

The V1.1 spec §8.1 lists 10 tables. W1 only uses `tasks`, `steps`, `audit_logs`, `policies`, `approvals`, but we create all tables with all V1.1 fields (forward-compat — saves a migration later).

- [ ] **Step 1: Create the migration SQL**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\src\migrations\001_init.sql`:

```sql
-- VoicePilot V1.1 initial schema (spec §8.1)
-- All tables created up-front; W1 only populates tasks/steps/audit_logs.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tasks (
    task_id            TEXT PRIMARY KEY,
    user_goal          TEXT NOT NULL,
    status             TEXT NOT NULL,           -- TaskState as SCREAMING_SNAKE_CASE
    skill_id           TEXT,                    -- V1.1: route metadata
    route_path         TEXT,                    -- V1.1: 'skill' | 'planner'
    transcript_hash    TEXT,
    created_at         TEXT NOT NULL,           -- RFC3339
    updated_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS steps (
    step_id            TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    step_order         INTEGER NOT NULL,
    tool_name          TEXT,
    args               TEXT,                    -- JSON
    args_hash          TEXT,
    status             TEXT NOT NULL,
    prepare_token      TEXT,                    -- V1.1
    preconditions_hash TEXT,                    -- V1.1
    effect_manifest    TEXT,                    -- V1.1 JSON
    evidence_strength  TEXT,                    -- V1.1: 'weak'|'medium'|'strong'
    compensation_ref   TEXT,                    -- V1.1
    egress_performed   INTEGER DEFAULT 0,       -- V1.1 bool
    started_at         TEXT,
    finished_at        TEXT
);

CREATE TABLE IF NOT EXISTS policies (
    policy_id          TEXT PRIMARY KEY,
    version            INTEGER NOT NULL,
    rules_json         TEXT NOT NULL,
    hash               TEXT NOT NULL,
    enabled            INTEGER NOT NULL DEFAULT 1,
    cedar_policies     TEXT,                    -- V1.1
    cedar_schema       TEXT,                    -- V1.1
    rust_constraints   TEXT                     -- V1.1
);

CREATE TABLE IF NOT EXISTS approvals (
    approval_id        TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    step_id            TEXT REFERENCES steps(step_id) ON DELETE SET NULL,
    risk_level         TEXT,
    args_hash          TEXT,
    user_decision      TEXT,                    -- 'allow'|'deny'|'modify'
    decided_at         TEXT,
    E_level            TEXT,                    -- V1.1: 'E0'..'E3'
    D_level            TEXT,                    -- V1.1: 'D0'..'D3'
    destination        TEXT,                    -- V1.1
    egress_approved    INTEGER DEFAULT 0,       -- V1.1 bool
    approval_scope     TEXT,                    -- V1.1: 'single'|'batch'
    policy_bundle_hash TEXT                     -- V1.1
);

CREATE TABLE IF NOT EXISTS compensations (
    comp_id            TEXT PRIMARY KEY,
    step_id            TEXT NOT NULL REFERENCES steps(step_id) ON DELETE CASCADE,
    level              TEXT NOT NULL,           -- 'strong'|'best_effort'|'none'
    snapshot_encrypted BLOB,
    ttl_expires        TEXT,
    status             TEXT NOT NULL,
    compensation_level TEXT,                    -- V1.1 alias
    snapshot_vault_ref TEXT,                    -- V1.1
    conflict_policy    TEXT                     -- V1.1
);

CREATE TABLE IF NOT EXISTS audit_logs (
    log_id             TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    step_id            TEXT REFERENCES steps(step_id) ON DELETE SET NULL,
    event_type         TEXT NOT NULL,
    details            TEXT NOT NULL,           -- JSON
    timestamp          TEXT NOT NULL,
    prev_hash          TEXT,
    hash               TEXT NOT NULL,
    otlp_trace_id      TEXT,                    -- V1.1
    otlp_span_id       TEXT,                    -- V1.1
    data_classification_redacted INTEGER DEFAULT 1  -- V1.1 bool
);

CREATE TABLE IF NOT EXISTS skills (
    skill_id           TEXT PRIMARY KEY,
    version            INTEGER NOT NULL,
    manifest_json      TEXT NOT NULL,
    enabled            INTEGER NOT NULL DEFAULT 1,
    success_count      INTEGER DEFAULT 0,
    avg_latency_ms     REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS taints (
    taint_id           TEXT PRIMARY KEY,
    value_hash         TEXT NOT NULL,
    provenance         TEXT NOT NULL,
    taints_json        TEXT NOT NULL,
    collected_at       TEXT NOT NULL,
    source_ref         TEXT
);

CREATE TABLE IF NOT EXISTS mcp_servers (
    server_id          TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    version            TEXT NOT NULL,
    transport          TEXT NOT NULL,
    enabled            INTEGER NOT NULL DEFAULT 1,
    trusted            INTEGER NOT NULL DEFAULT 0,
    protocol_version   TEXT,                    -- V1.1: locked to '2025-11-25'
    allowed_origins    TEXT,                    -- V1.1 JSON
    allowed_paths      TEXT                     -- V1.1 JSON
);

CREATE TABLE IF NOT EXISTS egress_log (
    egress_id          TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES tasks(task_id) ON DELETE CASCADE,
    data_class         TEXT NOT NULL,           -- 'D0'..'D3'
    source             TEXT NOT NULL,
    destination        TEXT NOT NULL,
    approved           INTEGER NOT NULL DEFAULT 0,
    bytes              INTEGER DEFAULT 0,
    timestamp          TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_steps_task_id ON steps(task_id);
CREATE INDEX IF NOT EXISTS idx_audit_task_id ON audit_logs(task_id);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_approvals_task_id ON approvals(task_id);
```

- [ ] **Step 2: Write the failing migration test**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\tests\migrations.rs`:

```rust
use trust_kernel::db;

#[test]
fn migrations_apply_cleanly_on_fresh_db() {
    let conn = db::open_in_memory().expect("open in-memory db");
    db::run_migrations(&conn).expect("migrations must apply");
    // Verify all 10 tables exist (V1.1 spec §8.1)
    let expected_tables = [
        "tasks", "steps", "policies", "approvals", "compensations",
        "audit_logs", "skills", "taints", "mcp_servers", "egress_log",
    ];
    for t in &expected_tables {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![t],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("table {} should exist", t));
        assert_eq!(count, 1, "table {} missing after migration", t);
    }
}

#[test]
fn migrations_are_idempotent() {
    let conn = db::open_in_memory().expect("open db");
    db::run_migrations(&conn).expect("first migration run");
    db::run_migrations(&conn).expect("second migration run (idempotent)");
}

#[test]
fn foreign_keys_are_enforced() {
    let conn = db::open_in_memory().expect("open db");
    db::run_migrations(&conn).expect("migrations");
    let fk_enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .expect("PRAGMA foreign_keys");
    assert_eq!(fk_enabled, 1, "foreign_keys pragma must be ON");
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p trust-kernel --test migrations` (cwd: `d:\voicepilot\voicepilot`)
Expected: FAIL — `run_migrations` is undefined (stub doesn't have it).

- [ ] **Step 4: Implement `db.rs` with migration runner**

Replace the entire contents of `d:\voicepilot\voicepilot\crates\trust-kernel\src\db.rs` with:

```rust
//! SQLite connection + migration runner.
//!
//! Migrations are embedded SQL files under `src/migrations/`.
//! Each migration is idempotent (`CREATE TABLE IF NOT EXISTS`).

use crate::error::Result;
use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("migrations/001_init.sql");

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

pub fn open_file(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Apply all migrations. Idempotent — safe to call on every startup.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(MIGRATION_001)?;
    tracing::info!("migrations applied");
    Ok(())
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p trust-kernel --test migrations` (cwd: `d:\voicepilot\voicepilot`)
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/trust-kernel/
git commit -m "feat(w1): add SQLite schema with all V1.1 tables + migration runner"
```

---

## Task 3: Audit log writer with hash chain

**Files:**
- Modify: `d:\voicepilot\voicepilot\crates\trust-kernel\src\audit.rs` (replace stub)
- Test: `d:\voicepilot\voicepilot\crates\trust-kernel\tests\audit.rs`

The audit log is **tamper-evident** via a hash chain (each event's `hash` includes the previous event's `hash`). This blocks trivial DB edits. V1.1 §1.4: "审计日志覆盖率 = 100%".

- [ ] **Step 1: Write the failing audit test**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\tests\audit.rs`:

```rust
use chrono::Utc;
use trust_kernel::audit::{AuditEvent, AuditLogger, SqliteAuditLogger};
use trust_kernel::db;

fn make_event(task_id: &str, event_type: &str, prev_hash: Option<String>) -> AuditEvent {
    AuditEvent {
        log_id: uuid::Uuid::new_v4().to_string(),
        task_id: task_id.to_string(),
        step_id: None,
        event_type: event_type.to_string(),
        details: serde_json::json!({"note": "test"}),
        timestamp: Utc::now(),
        prev_hash,
        hash: String::new(), // computed by logger
    }
}

#[test]
fn append_writes_event_with_correct_hash() {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    let logger = SqliteAuditLogger::new(conn);
    let event = make_event("task-1", "TASK_CREATED", None);
    logger.append(&event).unwrap();

    let rows: Vec<(String, Option<String>)> = logger
        .query("SELECT hash, prev_hash FROM audit_logs WHERE task_id=?1", "task-1")
        .unwrap();
    assert_eq!(rows.len(), 1);
    let (hash, prev) = &rows[0];
    assert!(!hash.is_empty(), "hash must be non-empty");
    assert!(prev.is_none(), "first event has no prev_hash");
}

#[test]
fn hash_chain_links_consecutive_events() {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    let logger = SqliteAuditLogger::new(conn);

    let e1 = make_event("task-2", "TASK_CREATED", None);
    logger.append(&e1).unwrap();
    let first_hash: String = logger
        .query(
            "SELECT hash FROM audit_logs WHERE task_id=?1 ORDER BY timestamp LIMIT 1",
            "task-2",
        )
        .unwrap()[0]
        .0
        .clone();

    let e2 = make_event("task-2", "STATE_TRANSITION", Some(first_hash.clone()));
    logger.append(&e2).unwrap();

    let rows: Vec<(String, Option<String>)> = logger
        .query("SELECT hash, prev_hash FROM audit_logs WHERE task_id=?1", "task-2")
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].1.as_deref(), Some(first_hash.as_str()), "second event must link to first hash");
    assert_ne!(rows[0].0, rows[1].0, "hashes must differ");
}

#[test]
fn audit_logger_records_step_id_when_provided() {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    let logger = SqliteAuditLogger::new(conn);

    let mut event = make_event("task-3", "STEP_STARTED", None);
    event.step_id = Some("step-1".to_string());
    logger.append(&event).unwrap();

    let step_id: Option<String> = logger
        .conn()
        .query_row(
            "SELECT step_id FROM audit_logs WHERE task_id=?1",
            rusqlite::params!["task-3"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(step_id.as_deref(), Some("step-1"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trust-kernel --test audit` (cwd: `d:\voicepilot\voicepilot`)
Expected: FAIL — `SqliteAuditLogger` not found.

- [ ] **Step 3: Implement the audit module**

Replace the entire contents of `d:\voicepilot\voicepilot\crates\trust-kernel\src\audit.rs` with:

```rust
//! Audit log writer with hash chain — tamper-evident event record.
//!
//! Each event's hash = SHA256(prev_hash || canonical_json(event_fields)).
//! V1.1 §1.4 requires 100% audit coverage for every tool call.

use crate::error::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEvent {
    pub log_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub details: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Hash of the previous event in the same task (None for the first event).
    pub prev_hash: Option<String>,
    /// Computed by the logger on append. Empty when caller constructs the event.
    pub hash: String,
}

pub trait AuditLogger: Send + Sync {
    fn append(&self, event: &AuditEvent) -> Result<()>;
}

/// SQLite-backed audit logger. Uses an internal Mutex because `rusqlite::Connection`
/// is not `Sync`. Sufficient for W1 single-process use; W6+ may move to a writer task.
pub struct SqliteAuditLogger {
    conn: Mutex<Connection>,
}

impl SqliteAuditLogger {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
    }

    /// Look up the most recent hash for a given task, used to chain the next event.
    fn last_hash_for_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
        let hash: Option<String> = conn
            .query_row(
                "SELECT hash FROM audit_logs WHERE task_id=?1 ORDER BY timestamp DESC, log_id DESC LIMIT 1",
                params![task_id],
                |r| r.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(hash)
    }

    /// Compute SHA256(prev_hash || canonical_json(payload)).
    fn compute_hash(event: &AuditEvent) -> String {
        let mut hasher = Sha256::new();
        if let Some(prev) = &event.prev_hash {
            hasher.update(prev.as_bytes());
        }
        // Canonical JSON: stable key order, no whitespace.
        let payload = serde_json::json!({
            "log_id": event.log_id,
            "task_id": event.task_id,
            "step_id": event.step_id,
            "event_type": event.event_type,
            "details": event.details,
            "timestamp": event.timestamp.to_rfc3339(),
        });
        let canonical = serde_json::to_string(&payload).unwrap_or_default();
        hasher.update(canonical.as_bytes());
        let digest = hasher.finalize();
        format!("{:x}", digest)
    }

    /// Test helper: query rows back. Not part of the trait.
    pub fn query(
        &self,
        sql: &str,
        task_id: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Test helper: expose a reference to the connection (single-threaded tests only).
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
}

impl AuditLogger for SqliteAuditLogger {
    fn append(&self, event: &AuditEvent) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Resolve prev_hash if caller didn't supply one (auto-chain).
        let prev_hash = match &event.prev_hash {
            Some(h) => Some(h.clone()),
            None => Self::last_hash_for_task(&conn, &event.task_id)?,
        };
        let mut to_write = event.clone();
        to_write.prev_hash = prev_hash;
        to_write.hash = Self::compute_hash(&to_write);

        conn.execute(
            "INSERT INTO audit_logs
                (log_id, task_id, step_id, event_type, details, timestamp, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                to_write.log_id,
                to_write.task_id,
                to_write.step_id,
                to_write.event_type,
                serde_json::to_string(&to_write.details)?,
                to_write.timestamp.to_rfc3339(),
                to_write.prev_hash,
                to_write.hash,
            ],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 4: Add `uuid` to test dependencies**

Modify `d:\voicepilot\voicepilot\crates\trust-kernel\Cargo.toml` — add `[dev-dependencies]`:

```toml
[dev-dependencies]
uuid = { workspace = true }
chrono = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p trust-kernel --test audit` (cwd: `d:\voicepilot\voicepilot`)
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/trust-kernel/
git commit -m "feat(w1): audit log writer with SHA256 hash chain"
```

---

## Task 4: Task state machine — exhaustive transition tests

**Files:**
- Test: `d:\voicepilot\voicepilot\crates\trust-kernel\tests\state_machine.rs`

State machine was added in Task 1; this task hardens it with exhaustive transition tests. V1.1 §3.1 diagram shows `IDLE→LISTENING→PLANNING→…`. Kill Switch (§VP-FR-009) must allow `→CANCELLED` from any non-terminal state.

- [ ] **Step 1: Write the failing state machine test**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\tests\state_machine.rs`:

```rust
use trust_kernel::state::TaskState::*;

#[test]
fn idle_to_listening_is_allowed() {
    assert!(Idle.can_transition_to(Listening));
}

#[test]
fn listening_to_planning_is_allowed() {
    assert!(Listening.can_transition_to(Planning));
}

#[test]
fn planning_to_awaiting_approval_is_allowed() {
    assert!(Planning.can_transition_to(AwaitingApproval));
}

#[test]
fn awaiting_approval_to_executing_is_allowed() {
    assert!(AwaitingApproval.can_transition_to(Executing));
}

#[test]
fn executing_to_verifying_is_allowed() {
    assert!(Executing.can_transition_to(Verifying));
}

#[test]
fn verifying_to_done_is_allowed() {
    assert!(Verifying.can_transition_to(Done));
}

#[test]
fn kill_switch_can_cancel_from_any_non_terminal_state() {
    for s in [Idle, Listening, Planning, AwaitingApproval, Executing, Verifying, Compensating] {
        assert!(s.can_transition_to(Cancelled), "must allow Cancelled from {:?}", s);
    }
}

#[test]
fn terminal_states_have_no_outgoing_transitions() {
    for s in [Done, Failed, Cancelled] {
        assert!(s.allowed_next().is_empty(), "{:?} must be terminal", s);
    }
}

#[test]
fn backward_transitions_are_forbidden() {
    // No state can go backward in the happy path.
    assert!(!Listening.can_transition_to(Idle));
    assert!(!Planning.can_transition_to(Listening));
    assert!(!Executing.can_transition_to(Planning));
    assert!(!Done.can_transition_to(Verifying));
}

#[test]
fn executing_to_failed_is_allowed_for_unrecoverable_errors() {
    assert!(Executing.can_transition_to(Failed));
}

#[test]
fn verifying_to_compensating_is_allowed_when_verification_fails() {
    assert!(Verifying.can_transition_to(Compensating));
}

#[test]
fn compensating_to_done_is_allowed_when_compensation_succeeds() {
    assert!(Compensating.can_transition_to(Done));
}
```

- [ ] **Step 2: Run tests — they should pass (state.rs already implemented in Task 1)**

Run: `cargo test -p trust-kernel --test state_machine` (cwd: `d:\voicepilot\voicepilot`)
Expected: 11 tests pass.

If any fail, fix `state.rs` `allowed_next()` to match the test expectations. The spec diagram implies the happy path `IDLE→LISTENING→PLANNING→AWAITING_APPROVAL→EXECUTING→VERIFYING→DONE`, plus `Compensating` from `Executing`/`Verifying`, plus `Failed`/`Cancelled` from any non-terminal.

- [ ] **Step 3: Commit**

```bash
git add voicepilot/crates/trust-kernel/tests/state_machine.rs
git commit -m "test(w1): exhaustive task state machine transition tests"
```

---

## Task 5: Task & Step repositories (CRUD)

**Files:**
- Modify: `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\task_repo.rs`
- Modify: `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\step_repo.rs`
- Test: `d:\voicepilot\voicepilot\crates\trust-kernel\tests\repos.rs`

- [ ] **Step 1: Write the failing repo test**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\tests\repos.rs`:

```rust
use chrono::Utc;
use trust_kernel::db;
use trust_kernel::repo::step_repo::{StepRecord, StepRepo, StepStatus};
use trust_kernel::repo::task_repo::{TaskRecord, TaskRepo};
use trust_kernel::state::TaskState;

fn fresh_db() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

#[test]
fn create_task_persists_and_can_be_loaded() {
    let conn = fresh_db();
    let repo = TaskRepo::new();
    let task = TaskRecord::new("task-1", "open notepad and write hello");
    repo.create(&conn, &task).unwrap();

    let loaded = repo.get(&conn, "task-1").unwrap().expect("task must exist");
    assert_eq!(loaded.task_id, "task-1");
    assert_eq!(loaded.user_goal, "open notepad and write hello");
    assert_eq!(loaded.status, TaskState::Idle);
}

#[test]
fn update_status_transitions_state() {
    let conn = fresh_db();
    let repo = TaskRepo::new();
    let task = TaskRecord::new("task-2", "goal");
    repo.create(&conn, &task).unwrap();

    repo.update_status(&conn, "task-2", TaskState::Planning).unwrap();
    let loaded = repo.get(&conn, "task-2").unwrap().unwrap();
    assert_eq!(loaded.status, TaskState::Planning);
}

#[test]
fn create_step_persists_with_task_link() {
    let conn = fresh_db();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    let task = TaskRecord::new("task-3", "goal");
    task_repo.create(&conn, &task).unwrap();

    let step = StepRecord {
        step_id: "step-1".to_string(),
        task_id: "task-3".to_string(),
        step_order: 0,
        tool_name: Some("filesystem.read".to_string()),
        args: Some(serde_json::json!({"path": "/tmp"})),
        args_hash: Some("abc123".to_string()),
        status: StepStatus::Pending,
        prepare_token: None,
        preconditions_hash: None,
        effect_manifest: None,
        evidence_strength: None,
        compensation_ref: None,
        egress_performed: false,
        started_at: None,
        finished_at: None,
    };
    step_repo.create(&conn, &step).unwrap();

    let steps = step_repo.list_for_task(&conn, "task-3").unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].step_id, "step-1");
    assert_eq!(steps[0].tool_name.as_deref(), Some("filesystem.read"));
}

#[test]
fn update_step_status_advances_lifecycle() {
    let conn = fresh_db();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    task_repo.create(&conn, &TaskRecord::new("task-4", "goal")).unwrap();

    let mut step = StepRecord::new("step-1", "task-4", 0);
    step_repo.create(&conn, &step).unwrap();

    step_repo.update_status(&conn, "step-1", StepStatus::Running).unwrap();
    step_repo.update_status(&conn, "step-1", StepStatus::Succeeded).unwrap();

    let loaded = step_repo.get(&conn, "step-1").unwrap().unwrap();
    assert_eq!(loaded.status, StepStatus::Succeeded);
}

#[test]
fn deleting_task_cascades_to_steps() {
    let conn = fresh_db();
    let task_repo = TaskRepo::new();
    let step_repo = StepRepo::new();
    task_repo.create(&conn, &TaskRecord::new("task-5", "goal")).unwrap();
    let mut step = StepRecord::new("step-1", "task-5", 0);
    step_repo.create(&conn, &step).unwrap();

    task_repo.delete(&conn, "task-5").unwrap();
    assert!(task_repo.get(&conn, "task-5").unwrap().is_none());
    // Steps must be gone (FK ON DELETE CASCADE).
    let steps = step_repo.list_for_task(&conn, "task-5").unwrap();
    assert!(steps.is_empty(), "steps must cascade-delete with task");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trust-kernel --test repos` (cwd: `d:\voicepilot\voicepilot`)
Expected: FAIL — `TaskRepo`, `StepRepo`, `TaskRecord`, `StepRecord` undefined.

- [ ] **Step 3: Implement `task_repo.rs`**

Replace the entire contents of `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\task_repo.rs` with:

```rust
//! Task repository — CRUD against SQLite `tasks` table.

use crate::error::Result;
use crate::state::TaskState;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub task_id: String,
    pub user_goal: String,
    pub status: TaskState,
    pub skill_id: Option<String>,
    pub route_path: Option<String>,
    pub transcript_hash: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl TaskRecord {
    pub fn new(task_id: impl Into<String>, user_goal: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            task_id: task_id.into(),
            user_goal: user_goal.into(),
            status: TaskState::Idle,
            skill_id: None,
            route_path: None,
            transcript_hash: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TaskRepo;

impl TaskRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, task: &TaskRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO tasks
                (task_id, user_goal, status, skill_id, route_path, transcript_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task.task_id,
                task.user_goal,
                serde_json::to_string(&task.status)?,
                task.skill_id,
                task.route_path,
                task.transcript_hash,
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, task_id: &str) -> Result<Option<TaskRecord>> {
        let mut stmt = conn.prepare(
            "SELECT task_id, user_goal, status, skill_id, route_path, transcript_hash,
                    created_at, updated_at
             FROM tasks WHERE task_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![task_id], |r| {
            let status_str: String = r.get(2)?;
            Ok((r, status_str))
        })?;
        if let Some(row_result) = rows.next() {
            let (r, status_str) = row_result?;
            let status: TaskState = serde_json::from_str(&status_str)?;
            Ok(Some(TaskRecord {
                task_id: r.get(0)?,
                user_goal: r.get(1)?,
                status,
                skill_id: r.get(3)?,
                route_path: r.get(4)?,
                transcript_hash: r.get(5)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&r.get::<_, String>(6)?)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&r.get::<_, String>(7)?)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn update_status(
        &self,
        conn: &Connection,
        task_id: &str,
        new_status: TaskState,
    ) -> Result<()> {
        conn.execute(
            "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE task_id = ?3",
            params![
                serde_json::to_string(&new_status)?,
                Utc::now().to_rfc3339(),
                task_id,
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, conn: &Connection, task_id: &str) -> Result<()> {
        conn.execute("DELETE FROM tasks WHERE task_id = ?1", params![task_id])?;
        Ok(())
    }
}
```

- [ ] **Step 4: Implement `step_repo.rs`**

Replace the entire contents of `d:\voicepilot\voicepilot\crates\trust-kernel\src\repo\step_repo.rs` with:

```rust
//! Step repository — CRUD against SQLite `steps` table.

use crate::error::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        use StepStatus::*;
        match self {
            Pending => "PENDING",
            Running => "RUNNING",
            Succeeded => "SUCCEEDED",
            Failed => "FAILED",
            Skipped => "SKIPPED",
            Cancelled => "CANCELLED",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        use StepStatus::*;
        Some(match s {
            "PENDING" => Pending,
            "RUNNING" => Running,
            "SUCCEEDED" => Succeeded,
            "FAILED" => Failed,
            "SKIPPED" => Skipped,
            "CANCELLED" => Cancelled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub step_id: String,
    pub task_id: String,
    pub step_order: i64,
    pub tool_name: Option<String>,
    pub args: Option<serde_json::Value>,
    pub args_hash: Option<String>,
    pub status: StepStatus,
    pub prepare_token: Option<String>,
    pub preconditions_hash: Option<String>,
    pub effect_manifest: Option<serde_json::Value>,
    pub evidence_strength: Option<String>,
    pub compensation_ref: Option<String>,
    pub egress_performed: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl StepRecord {
    pub fn new(step_id: impl Into<String>, task_id: impl Into<String>, step_order: i64) -> Self {
        Self {
            step_id: step_id.into(),
            task_id: task_id.into(),
            step_order,
            tool_name: None,
            args: None,
            args_hash: None,
            status: StepStatus::Pending,
            prepare_token: None,
            preconditions_hash: None,
            effect_manifest: None,
            evidence_strength: None,
            compensation_ref: None,
            egress_performed: false,
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StepRepo;

impl StepRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, step: &StepRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO steps
                (step_id, task_id, step_order, tool_name, args, args_hash, status,
                 prepare_token, preconditions_hash, effect_manifest, evidence_strength,
                 compensation_ref, egress_performed, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                step.step_id,
                step.task_id,
                step.step_order,
                step.tool_name,
                step.args.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                step.args_hash,
                step.status.as_str(),
                step.prepare_token,
                step.preconditions_hash,
                step.effect_manifest.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                step.evidence_strength,
                step.compensation_ref,
                step.egress_performed as i64,
                step.started_at,
                step.finished_at,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, step_id: &str) -> Result<Option<StepRecord>> {
        let mut stmt = conn.prepare(
            "SELECT step_id, task_id, step_order, tool_name, args, args_hash, status,
                    prepare_token, preconditions_hash, effect_manifest, evidence_strength,
                    compensation_ref, egress_performed, started_at, finished_at
             FROM steps WHERE step_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![step_id], |r| {
            let status_str: String = r.get(6)?;
            let args_str: Option<String> = r.get(4)?;
            let manifest_str: Option<String> = r.get(9)?;
            Ok((r, status_str, args_str, manifest_str))
        })?;
        if let Some(row_result) = rows.next() {
            let (r, status_str, args_str, manifest_str) = row_result?;
            let status = StepStatus::parse(&status_str)
                .ok_or_else(|| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(std::fmt::Error)))?;
            let args = args_str
                .and_then(|s| serde_json::from_str(&s).ok());
            let manifest = manifest_str
                .and_then(|s| serde_json::from_str(&s).ok());
            Ok(Some(StepRecord {
                step_id: r.get(0)?,
                task_id: r.get(1)?,
                step_order: r.get(2)?,
                tool_name: r.get(3)?,
                args,
                args_hash: r.get(5)?,
                status,
                prepare_token: r.get(7)?,
                preconditions_hash: r.get(8)?,
                effect_manifest: manifest,
                evidence_strength: r.get(10)?,
                compensation_ref: r.get(11)?,
                egress_performed: r.get::<_, i64>(12)? != 0,
                started_at: r.get(13)?,
                finished_at: r.get(14)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_for_task(&self, conn: &Connection, task_id: &str) -> Result<Vec<StepRecord>> {
        let mut stmt = conn.prepare(
            "SELECT step_id, task_id, step_order, tool_name, args, args_hash, status,
                    prepare_token, preconditions_hash, effect_manifest, evidence_strength,
                    compensation_ref, egress_performed, started_at, finished_at
             FROM steps WHERE task_id = ?1 ORDER BY step_order ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |r| {
            let status_str: String = r.get(6)?;
            let args_str: Option<String> = r.get(4)?;
            let manifest_str: Option<String> = r.get(9)?;
            Ok((r, status_str, args_str, manifest_str))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (r, status_str, args_str, manifest_str) = row_result?;
            let status = StepStatus::parse(&status_str)
                .ok_or_else(|| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(std::fmt::Error)))?;
            let args = args_str.and_then(|s| serde_json::from_str(&s).ok());
            let manifest = manifest_str.and_then(|s| serde_json::from_str(&s).ok());
            out.push(StepRecord {
                step_id: r.get(0)?,
                task_id: r.get(1)?,
                step_order: r.get(2)?,
                tool_name: r.get(3)?,
                args,
                args_hash: r.get(5)?,
                status,
                prepare_token: r.get(7)?,
                preconditions_hash: r.get(8)?,
                effect_manifest: manifest,
                evidence_strength: r.get(10)?,
                compensation_ref: r.get(11)?,
                egress_performed: r.get::<_, i64>(12)? != 0,
                started_at: r.get(13)?,
                finished_at: r.get(14)?,
            });
        }
        Ok(out)
    }

    pub fn update_status(
        &self,
        conn: &Connection,
        step_id: &str,
        new_status: StepStatus,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        match new_status {
            StepStatus::Running => {
                conn.execute(
                    "UPDATE steps SET status = ?1, started_at = COALESCE(started_at, ?2) WHERE step_id = ?3",
                    params![new_status.as_str(), now, step_id],
                )?;
            }
            StepStatus::Succeeded | StepStatus::Failed | StepStatus::Cancelled | StepStatus::Skipped => {
                conn.execute(
                    "UPDATE steps SET status = ?1, finished_at = ?2 WHERE step_id = ?3",
                    params![new_status.as_str(), now, step_id],
                )?;
            }
            StepStatus::Pending => {
                conn.execute(
                    "UPDATE steps SET status = ?1 WHERE step_id = ?2",
                    params![new_status.as_str(), step_id],
                )?;
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p trust-kernel --test repos` (cwd: `d:\voicepilot\voicepilot`)
Expected: 5 tests pass.

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/trust-kernel/
git commit -m "feat(w1): task & step repositories with CRUD and cascade delete"
```

---

## Task 6: TrustKernel facade + text CLI entry

**Files:**
- Modify: `d:\voicepilot\voicepilot\crates\trust-kernel\src\kernel.rs`
- Modify: `d:\voicepilot\voicepilot\crates\cli\src\main.rs`
- Test: `d:\voicepilot\voicepilot\crates\trust-kernel\tests\end_to_end.rs`

The `TrustKernel` facade wraps DB + repos + audit logger and exposes high-level operations. The CLI is a text REPL that uses the facade. End-to-end test verifies a text command flows through the full state machine and produces audit records.

- [ ] **Step 1: Write the failing end-to-end test**

Create `d:\voicepilot\voicepilot\crates\trust-kernel\tests\end_to_end.rs`:

```rust
use trust_kernel::audit::AuditEvent;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::state::TaskState;
use uuid::Uuid;

#[test]
fn text_command_flows_through_state_machine_with_audit() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = Uuid::new_v4().to_string();
    let goal = "open notepad and write hello";

    // 1. Create task in IDLE
    let task = kernel.create_task(&task_id, goal).unwrap();
    assert_eq!(task.status, TaskState::Idle);
    assert_audit_event_count(&kernel, &task_id, 1, "TASK_CREATED should be audited");

    // 2. Listen → Plan → Await approval (text input simulates voice transcript)
    kernel.transition(&task_id, TaskState::Listening).unwrap();
    kernel.transition(&task_id, TaskState::Planning).unwrap();
    kernel.transition(&task_id, TaskState::AwaitingApproval).unwrap();

    // 3. User approves (in W1: auto-approve for text commands; real approval UI in W2+)
    kernel.transition(&task_id, TaskState::Executing).unwrap();

    // 4. Execute → Verify → Done. W1 has no real tools; executor is a stub.
    kernel.transition(&task_id, TaskState::Verifying).unwrap();
    kernel.transition(&task_id, TaskState::Done).unwrap();

    // 5. Final state and audit chain
    let final_task = kernel.get_task(&task_id).unwrap().unwrap();
    assert_eq!(final_task.status, TaskState::Done);

    let audit_count = kernel.audit_count_for_task(&task_id).unwrap();
    assert_eq!(
        audit_count, 7,
        "expected 7 audit events (1 create + 6 transitions), got {}",
        audit_count
    );
}

#[test]
fn illegal_transition_returns_error() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = Uuid::new_v4().to_string();
    kernel.create_task(&task_id, "goal").unwrap();

    // IDLE → EXECUTING is illegal (must go through PLANNING → AWAITING_APPROVAL)
    let result = kernel.transition(&task_id, TaskState::Executing);
    assert!(result.is_err(), "transition IDLE → EXECUTING must be rejected");
}

#[test]
fn kill_switch_cancels_from_executing() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = Uuid::new_v4().to_string();
    kernel.create_task(&task_id, "goal").unwrap();
    kernel.transition(&task_id, TaskState::Listening).unwrap();
    kernel.transition(&task_id, TaskState::Planning).unwrap();
    kernel.transition(&task_id, TaskState::AwaitingApproval).unwrap();
    kernel.transition(&task_id, TaskState::Executing).unwrap();

    // Kill switch
    kernel.transition(&task_id, TaskState::Cancelled).unwrap();
    let final_task = kernel.get_task(&task_id).unwrap().unwrap();
    assert_eq!(final_task.status, TaskState::Cancelled);
}

fn assert_audit_event_count(kernel: &TrustKernel, task_id: &str, expected: usize, msg: &str) {
    let count = kernel.audit_count_for_task(task_id).unwrap();
    assert_eq!(count, expected, "{} (got {})", msg, count);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trust-kernel --test end_to_end` (cwd: `d:\voicepilot\voicepilot`)
Expected: FAIL — `TrustKernel::create_task`, `transition`, `get_task`, `audit_count_for_task` don't exist.

- [ ] **Step 3: Implement `TrustKernel` facade**

Replace the entire contents of `d:\voicepilot\voicepilot\crates\trust-kernel\src\kernel.rs` with:

```rust
//! TrustKernel facade — the single entry point for CLI/Tauri/UI.
//!
//! W1: text entry → state machine + audit. No real tools yet.

use crate::audit::{AuditEvent, AuditLogger, SqliteAuditLogger};
use crate::db;
use crate::error::{KernelError, Result};
use crate::repo::task_repo::{TaskRecord, TaskRepo};
use crate::state::TaskState;
use chrono::Utc;
use rusqlite::Connection;
use std::sync::Arc;
use uuid::Uuid;

pub struct TrustKernel {
    conn: Arc<std::sync::Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
}

impl TrustKernel {
    pub fn open_in_memory() -> Result<Self> {
        let conn = db::open_in_memory()?;
        db::run_migrations(&conn)?;
        Ok(Self::with_conn(conn))
    }

    pub fn open_file(path: &str) -> Result<Self> {
        let conn = db::open_file(path)?;
        db::run_migrations(&conn)?;
        Ok(Self::with_conn(conn))
    }

    fn with_conn(conn: Connection) -> Self {
        Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(conn)),
        }
    }

    pub fn create_task(&self, task_id: &str, user_goal: &str) -> Result<TaskRecord> {
        let task = TaskRecord::new(task_id, user_goal);
        {
            let conn = self.conn.lock().unwrap();
            self.task_repo.create(&conn, &task)?;
        }
        self.audit_append(&task.task_id, None, "TASK_CREATED", serde_json::json!({
            "user_goal": task.user_goal,
        }))?;
        Ok(task)
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRecord>> {
        let conn = self.conn.lock().unwrap();
        self.task_repo.get(&conn, task_id)
    }

    /// Transition a task to a new state. Rejects illegal transitions.
    /// Emits a `STATE_TRANSITION` audit event on success.
    pub fn transition(&self, task_id: &str, target: TaskState) -> Result<()> {
        let current = self
            .get_task(task_id)?
            .ok_or_else(|| KernelError::TaskNotFound(task_id.to_string()))?;
        if !current.status.can_transition_to(target) {
            return Err(KernelError::InvalidTransition {
                from: current.status,
                to: target,
            });
        }
        {
            let conn = self.conn.lock().unwrap();
            self.task_repo.update_status(&conn, task_id, target)?;
        }
        self.audit_append(task_id, None, "STATE_TRANSITION", serde_json::json!({
            "from": current.status,
            "to": target,
        }))?;
        Ok(())
    }

    pub fn audit_count_for_task(&self, task_id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM audit_logs WHERE task_id = ?1",
            rusqlite::params![task_id],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    fn audit_append(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        details: serde_json::Value,
    ) -> Result<()> {
        let event = AuditEvent {
            log_id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            step_id: step_id.map(String::from),
            event_type: event_type.to_string(),
            details,
            timestamp: Utc::now(),
            prev_hash: None, // auto-chained by logger
            hash: String::new(), // computed by logger
        };
        self.audit.append(&event)
    }
}
```

- [ ] **Step 4: Add `uuid` to non-dev dependencies**

Modify `d:\voicepilot\voicepilot\crates\trust-kernel\Cargo.toml` — move `uuid` from `[dev-dependencies]` to `[dependencies]`:

```toml
[dependencies]
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
sha2 = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
chrono = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p trust-kernel --test end_to_end` (cwd: `d:\voicepilot\voicepilot`)
Expected: 3 tests pass.

If `audit_count_for_task` returns 0 events, the `SqliteAuditLogger` is sharing a separate `Connection` from the one used by `task_repo`. Fix by ensuring `TrustKernel::with_conn` clones the same underlying connection — the current `SqliteAuditLogger::new(conn)` takes ownership and the same `Connection` is also passed to `Mutex`. The fix is to **clone the connection** before passing to the logger, OR have the logger share the same Mutex. Use `Connection::try_clone` and wrap both in their own Mutexes.

If the test reveals this issue, apply this fix to `with_conn`:

```rust
fn with_conn(conn: Connection) -> Self {
    let audit_conn = conn.try_clone().expect("clone sqlite connection");
    Self {
        conn: Arc::new(std::sync::Mutex::new(conn)),
        task_repo: TaskRepo::new(),
        audit: Arc::new(SqliteAuditLogger::new(audit_conn)),
    }
}
```

SQLite allows multiple connections to the same in-memory DB only when shared via `OpenFlags::SHARED_CACHE`. For W1, use a **file-backed temp DB** in tests if needed. Simpler alternative: have `SqliteAuditLogger` accept `Arc<Mutex<Connection>>` instead of owning it. Use that approach.

Replace `SqliteAuditLogger::new` signature and field to use `Arc<Mutex<Connection>>`, and update the audit module accordingly. Then `TrustKernel::with_conn` becomes:

```rust
fn with_conn(conn: Connection) -> Self {
    let shared = Arc::new(std::sync::Mutex::new(conn));
    Self {
        conn: shared.clone(),
        task_repo: TaskRepo::new(),
        audit: Arc::new(SqliteAuditLogger::new(shared)),
    }
}
```

Update `audit.rs` `SqliteAuditLogger`:

```rust
pub struct SqliteAuditLogger {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteAuditLogger {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
    // … replace all self.conn.lock() usages; the existing code already uses self.conn.lock()
}
```

And the test helper `query` and `conn` methods need updating: `query` uses `self.conn.lock()` (already does), `conn()` returns `MutexGuard` (already does). The only change is the constructor and field type.

Apply this refactor before re-running the test.

- [ ] **Step 6: Re-run all trust-kernel tests**

Run: `cargo test -p trust-kernel` (cwd: `d:\voicepilot\voicepilot`)
Expected: all tests pass (migrations + audit + state_machine + repos + end_to_end).

If audit tests now fail because they constructed `SqliteAuditLogger::new(conn)` with a plain `Connection`, update those tests to wrap in `Arc::new(Mutex::new(conn))`. Specifically in `tests/audit.rs`:

```rust
let logger = SqliteAuditLogger::new(std::sync::Arc::new(std::sync::Mutex::new(conn)));
```

And update the test helper `query` and `conn` methods to match.

- [ ] **Step 7: Implement the CLI REPL**

Replace the entire contents of `d:\voicepilot\voicepilot\crates\cli\src\main.rs` with:

```rust
use anyhow::{anyhow, Result};
use std::io::{self, Write};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::state::TaskState;
use uuid::Uuid;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?),
        )
        .init();

    let db_path = std::env::var("VOICEPILOT_DB")
        .unwrap_or_else(|_| "voicepilot.db".to_string());
    let kernel = TrustKernel::open_file(&db_path)
        .map_err(|e| anyhow!("failed to open kernel: {}", e))?;

    println!("VoicePilot W1 — text command entry (no voice yet)");
    println!("DB: {}", db_path);
    println!("Commands:");
    println!("  <text>          create a task with the given goal, run happy-path flow");
    println!("  cancel <task>   kill-switch cancel a task");
    println!("  show <task>     show task state and audit count");
    println!("  quit");
    println!();

    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }
        if let Some(task_id) = line.strip_prefix("cancel ") {
            match kernel.transition(task_id.trim(), TaskState::Cancelled) {
                Ok(_) => println!("cancelled task {}", task_id.trim()),
                Err(e) => println!("error: {}", e),
            }
            continue;
        }
        if let Some(task_id) = line.strip_prefix("show ") {
            match kernel.get_task(task_id.trim()) {
                Ok(Some(t)) => {
                    let audit = kernel.audit_count_for_task(task_id.trim()).unwrap_or(0);
                    println!("task {} | status={:?} | audit_events={}", t.task_id, t.status, audit);
                    println!("  goal: {}", t.user_goal);
                }
                Ok(None) => println!("task not found"),
                Err(e) => println!("error: {}", e),
            }
            continue;
        }
        // Treat the entire line as a goal — run the W1 happy path.
        let task_id = Uuid::new_v4().to_string();
        match run_happy_path(&kernel, &task_id, line) {
            Ok(_) => println!("done: task_id={}", task_id),
            Err(e) => println!("failed: {}", e),
        }
    }
    Ok(())
}

/// Run the W1 happy-path stub: IDLE → LISTENING → PLANNING → AWAITING_APPROVAL
/// → EXECUTING → VERIFYING → DONE. No real tools — just state + audit.
fn run_happy_path(kernel: &TrustKernel, task_id: &str, goal: &str) -> Result<()> {
    kernel
        .create_task(task_id, goal)
        .map_err(|e| anyhow!("create_task: {}", e))?;
    for next in [
        TaskState::Listening,
        TaskState::Planning,
        TaskState::AwaitingApproval,
        TaskState::Executing,
        TaskState::Verifying,
        TaskState::Done,
    ] {
        kernel
            .transition(task_id, next)
            .map_err(|e| anyhow!("transition {:?}: {}", next, e))?;
    }
    Ok(())
}
```

- [ ] **Step 8: Build the CLI**

Run: `cargo build -p cli` (cwd: `d:\voicepilot\voicepilot`)
Expected: Compiles successfully.

- [ ] **Step 9: Commit**

```bash
git add voicepilot/crates/trust-kernel/ voicepilot/crates/cli/
git commit -m "feat(w1): TrustKernel facade + text CLI with happy-path flow"
```

---

## Task 7: End-to-end smoke test (manual)

This task verifies the W1 gate from spec §11.1: "文本命令可执行". It's a manual verification step — no automated test added.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test` (cwd: `d:\voicepilot\voicepilot`)
Expected: all tests across all crates pass. Note the test count.

- [ ] **Step 2: Run the CLI and execute a text command**

Run: `cargo run -p cli` (cwd: `d:\voicepilot\voicepilot`)
At the `> ` prompt, type: `open notepad and write hello`
Expected output:
```
done: task_id=<some-uuid>
```

Then type: `show <the-uuid-from-above>`
Expected output:
```
task <uuid> | status=Done | audit_events=7
  goal: open notepad and write hello
```

Then type: `quit`

- [ ] **Step 3: Verify the DB file was created and contains the task**

Run: `cargo run -p cli` then `show <uuid>` again to confirm persistence across runs.

- [ ] **Step 4: Test the kill switch path**

Run: `cargo run -p cli`
Type: `long-running task that should be cancelled`
Note the task_id, then immediately type: `cancel <task_id>`
Expected: "cancelled task <task_id>"
Then: `show <task_id>` — should show `status=Cancelled`.

- [ ] **Step 5: Verify audit log hash chain integrity**

Run a quick SQL check (use `sqlite3` CLI or a Rust one-off) against `voicepilot.db`:

```sql
SELECT log_id, event_type, prev_hash, hash
FROM audit_logs
ORDER BY timestamp
LIMIT 10;
```

Verify each row's `prev_hash` matches the previous row's `hash` (except the first, which has `prev_hash = NULL`).

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit --allow-empty -m "chore(w1): smoke test passed — text commands execute end-to-end"
```

- [ ] **Step 7: Mark W1 gate as met**

W1 gate (§11.1): "文本命令可执行" — **MET**. The CLI accepts a text goal, transitions through the full state machine, persists to SQLite, and writes a tamper-evident audit chain.

---

## Spec Notes & Issues Found During W1 Planning

These are observations from reading the V1.1 spec while writing this plan. Flagging per user request ("在遇到感觉开发文档不合理或者可用进行优化时，向我报告"):

1. **§3.1 State diagram is incomplete.** The architecture SVG shows only `IDLE→LISTENING→PLANNING` and lists components, but doesn't enumerate the full state machine. This plan infers the happy path as `IDLE→LISTENING→PLANNING→AWAITING_APPROVAL→EXECUTING→VERIFYING→DONE` with `Compensating`/`Failed`/`Cancelled` branches. **Suggestion**: add an explicit §3.1.x state transition table to the spec for W2 review.

2. **§3.3 mentions `tauri-plugin-sql` for SQLite.** But W1 has no Tauri yet (Tauri lands in W6 per §11.1). This plan uses plain `rusqlite` for W1. **Suggestion**: clarify in spec whether `tauri-plugin-sql` is required from W6, or whether `rusqlite` can persist throughout. Using `rusqlite` directly is simpler and avoids a Tauri dependency in the kernel crate.

3. **§8.1 `audit_logs` table has `data_classification_redacted` boolean but the redaction logic is unspecified.** W1 writes raw `details` JSON. **Suggestion**: spec should describe redaction rules (which fields, by what data_class) before W8 (data security week).

4. **§8.1 `compensations` table has both `level` and `compensation_level` columns that look like duplicates.** W1 schema creates both for forward-compat. **Suggestion**: spec should clarify which is canonical, or remove one.

5. **§11.1 W1 gate "文本命令可执行" is ambiguous.** Does it mean (a) a text command can trigger any state transition, or (b) a text command can actually invoke a tool? This plan interprets it as (a) — tool invocation lands in W3 with filesystem MCP. **Suggestion**: rephrase gate as "文本命令可触发完整状态机转移并产生审计记录".

6. **§3.3 specifies `RMCP` (official Rust MCP SDK v1.6.1) but the latest RMCP crate is at a different version** (as of 2026-07-19). **Suggestion**: pin the exact `cargo add` version in W2 plan after checking crates.io.

These are documented here for the user; no spec changes have been made.
