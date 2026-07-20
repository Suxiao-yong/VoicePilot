# W3b: files.organize Skill + End-to-End Approval Flow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the W3a `FilesystemTool` into the V1.1 §5.2 `files.organize` Skill with a full prepare → approve → commit → verify → compensate pipeline, expose `kernel.create_compensation()` and friends to replace the W3a CLI placeholder, add a CLI approval prompt, enforce `allowed_paths` whitelist, and scaffold an MCP handler with `inputSchema`/`outputSchema`/`annotations`. End state: `organize <root> <filter> <dest>` CLI command runs the entire Skill flow with audit trail + persisted compensation record.

**Architecture:** Three new modules under `trust-kernel/src/`:
- `skills/` — `SkillManifest` struct (§5.3 + Appendix C), `SkillRouter` (intent + slot keyword match), `FilesOrganizeSkill` executor that orchestrates `FilesystemTool` calls around an `Approver` trait callback.
- `approval/` — `ApprovalDecision` / `ApprovalScope` / `ApprovalRecord` types matching §8.1 `approvals` table, `ApprovalRepo` CRUD, `Approver` trait (UI-agnostic).
- `mcp/` — `McpToolSchema` (name + inputSchema + outputSchema + annotations per §6.1 / Appendix B), `McpHandler` with `list_tools()` + `call_tool()` dispatch. No real MCP protocol wiring (that's W4); W3b delivers the schema + dispatch skeleton that W4 wraps in JSON-RPC.

Plus a single-file `allowed_paths.rs` module for path whitelist enforcement, plugged into `FilesystemTool::new_with_allowed_paths()`.

The CLI gains an `organize` command that constructs the skill, routes via `SkillRouter`, invokes the executor with a stdin-based `CliApprover`, and prints the resulting `ToolResult` V2.

**Tech Stack:** Rust 1.96, `rusqlite` 0.32, `serde` 1.0, `serde_json` 1.0, `chrono` 0.4, `uuid` 1.10, `thiserror` 2.0, `walkdir` 2.5 (already in W3a). No new deps — we reuse the W3a stack. Built-in Skill manifests are Rust struct literals (no `serde_yaml` dependency for W3b; W7 may add YAML loading when user-saved Skills land).

**Reference:** V1.1.1 spec at `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`. Relevant sections: §5.1 (Skills vs MCP vs Planner), §5.2 (8 Skill table), §5.3 (Skill Manifest example), §6.1 (MCP server annotations), §6.2 (prepare→approve→commit), §6.3 (ToolResult V2), §7.1 (Verifier evidence_strength), §7.2 (Compensation three-level), §8.1 (approvals/skills/mcp_servers tables + approval_scope batch semantics), Appendix B (Tool Schema V2 full fields), Appendix C (Skill Manifest full schema).

**W3a prerequisites (already complete):** `FilesystemTool` (prepare_move/commit_move/verify_move/search_files), `CompensationRepo` + `auto_reverse_move`, `ToolResult` V2, `StepRepo::update_prepare_state`/`update_post_commit`, kernel accessors `filesystem()` / `compensation_repo()` / `transaction_manager()`, `compensations` / `steps` / `approvals` / `skills` / `mcp_servers` tables (created by `001_init.sql`).

---

## File Structure

**New files:**
- `voicepilot/crates/trust-kernel/src/skills/mod.rs` — re-exports
- `voicepilot/crates/trust-kernel/src/skills/manifest.rs` — `SkillManifest` struct + `FilesOrganizeManifest::builtin()` constructor
- `voicepilot/crates/trust-kernel/src/skills/router.rs` — `SkillRouter` with `register()` + `route(intent)`
- `voicepilot/crates/trust-kernel/src/skills/executor.rs` — `FilesOrganizeSkill` orchestrator + `SkillExecution` result
- `voicepilot/crates/trust-kernel/src/approval/mod.rs` — re-exports
- `voicepilot/crates/trust-kernel/src/approval/types.rs` — `ApprovalDecision`, `ApprovalScope`, `ApprovalRecord`
- `voicepilot/crates/trust-kernel/src/approval/repo.rs` — `ApprovalRepo` CRUD against `approvals` table
- `voicepilot/crates/trust-kernel/src/approval/approver.rs` — `Approver` trait
- `voicepilot/crates/trust-kernel/src/mcp/mod.rs` — re-exports
- `voicepilot/crates/trust-kernel/src/mcp/schema.rs` — `McpToolSchema`, `McpAnnotations`
- `voicepilot/crates/trust-kernel/src/mcp/handler.rs` — `McpHandler` with `list_tools()` + `call_tool()` dispatch
- `voicepilot/crates/trust-kernel/src/allowed_paths.rs` — `AllowedPaths` whitelist
- `voicepilot/crates/trust-kernel/tests/skills_manifest.rs`
- `voicepilot/crates/trust-kernel/tests/skills_router.rs`
- `voicepilot/crates/trust-kernel/tests/skills_executor.rs`
- `voicepilot/crates/trust-kernel/tests/approval_repo.rs`
- `voicepilot/crates/trust-kernel/tests/mcp_handler.rs`
- `voicepilot/crates/trust-kernel/tests/allowed_paths.rs`
- `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`
- `voicepilot/crates/trust-kernel/tests/w3b_e2e_smoke.rs`

**Modified files:**
- `voicepilot/crates/trust-kernel/src/lib.rs` — add `pub mod skills; pub mod approval; pub mod mcp; pub mod allowed_paths;`
- `voicepilot/crates/trust-kernel/src/error.rs` — add `Skill`, `Mcp`, `Approval` error variants
- `voicepilot/crates/trust-kernel/src/kernel.rs` — add `create_compensation()`, `list_active_compensations()`, `get_compensation()`, `mark_compensation_status()`, `create_step()`, `update_step_prepare_state()`, `update_step_post_commit()`, `update_step_status()`, `get_step()`, `list_steps_for_task()`, `record_approval()`, `get_approval()`, `list_approvals_for_task()`, `audit_append_step()` (public wrapper around private `audit_append`)
- `voicepilot/crates/trust-kernel/src/tools/fs.rs` — add `FilesystemTool::new_with_allowed_paths(allowed_paths)` + enforce in `prepare_move` and `search_files`
- `voicepilot/crates/cli/src/main.rs` — add `organize <root> <filter> <dest>` command + `CliApprover` struct implementing `Approver` trait

---

## Task 1: Kernel public methods for compensation + step lifecycle

**Goal:** Replace W3a CLI placeholder ("compensation record creation deferred to W3b") with real kernel methods that persist compensation records and step state through the kernel's private `conn`. Same pattern for step lifecycle (prepare state, post-commit state, status) — currently `StepRepo` has the methods but the CLI cannot call them because `conn` is private to the kernel.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Create: `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`

- [ ] **Step 1: Write the failing test for compensation + step accessors**

Create `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`:

```rust
use chrono::Utc;
use trust_kernel::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};

fn fresh_kernel() -> TrustKernel {
    TrustKernel::open_in_memory().unwrap()
}

fn sample_comp(comp_id: &str, step_id: &str) -> CompensationRecord {
    CompensationRecord {
        comp_id: comp_id.to_string(),
        step_id: step_id.to_string(),
        level: CompensationLevel::Strong,
        snapshot_encrypted: None,
        ttl_expires: (Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339(),
        status: "active".to_string(),
        snapshot_vault_ref: None,
        conflict_policy: ConflictPolicy::AutoReverse,
        compensate_fn: "filesystem.reverse_move".to_string(),
        reverse_payload: r#"{"moves":[]}"#.to_string(),
    }
}

#[test]
fn kernel_create_compensation_persists() {
    let k = fresh_kernel();
    k.create_task("t1", "test goal").unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    k.create_compensation(&sample_comp("c1", "s1")).unwrap();
    let loaded = k.get_compensation("c1").unwrap().expect("must exist");
    assert_eq!(loaded.level, CompensationLevel::Strong);
    assert_eq!(loaded.status, "active");
}

#[test]
fn kernel_list_active_compensations() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    k.create_compensation(&sample_comp("c1", "s1")).unwrap();
    k.create_compensation(&sample_comp("c2", "s1")).unwrap();
    k.mark_compensation_status("c2", "consumed").unwrap();
    let active = k.list_active_compensations().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].comp_id, "c1");
}

#[test]
fn kernel_step_lifecycle_persists_prepare_and_post_commit() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    let step = StepRecord::new("s1", "t1", 1);
    k.create_step(&step).unwrap();

    // Update prepare state.
    let manifest_json = serde_json::json!({"sources": [], "destination": "c:/out"});
    k.update_step_prepare_state("s1", "prt_token", "sha256:abc", &manifest_json)
        .unwrap();
    let loaded = k.get_step("s1").unwrap().expect("must exist");
    assert_eq!(loaded.prepare_token.as_deref(), Some("prt_token"));
    assert_eq!(loaded.preconditions_hash.as_deref(), Some("sha256:abc"));
    assert!(loaded.effect_manifest.is_some());

    // Update post-commit state.
    k.update_step_post_commit("s1", "strong", Some("c1")).unwrap();
    let loaded = k.get_step("s1").unwrap().unwrap();
    assert_eq!(loaded.evidence_strength.as_deref(), Some("strong"));
    assert_eq!(loaded.compensation_ref.as_deref(), Some("c1"));

    // Status transition.
    k.update_step_status("s1", StepStatus::Succeeded).unwrap();
    let loaded = k.get_step("s1").unwrap().unwrap();
    assert_eq!(loaded.status, StepStatus::Succeeded);
}

#[test]
fn kernel_list_steps_for_task_returns_in_order() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    k.create_step(&StepRecord::new("s2", "t1", 2)).unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();
    let steps = k.list_steps_for_task("t1").unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].step_id, "s1");
    assert_eq!(steps[1].step_id, "s2");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: FAIL — `create_compensation`, `create_step`, `update_step_prepare_state`, etc. do not exist on `TrustKernel`.

- [ ] **Step 3: Add the kernel accessor methods**

Edit `voicepilot/crates/trust-kernel/src/kernel.rs`. First, add these imports near the top (after the existing `use` lines):

```rust
use crate::compensation::types::CompensationRecord;
use crate::repo::step_repo::{StepRecord, StepStatus};
```

Then append the following methods inside `impl TrustKernel` (after the existing `audit_count_for_task` method, before the private `audit_append` helper):

```rust
    // ===== Compensation accessors (W3b) =====

    pub fn create_compensation(&self, rec: &CompensationRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        self.comp_repo.create(&conn, rec)?;
        self.audit_append(
            &rec.step_id,
            Some(&rec.step_id),
            "COMPENSATION_CREATED",
            serde_json::json!({
                "comp_id": rec.comp_id,
                "level": rec.level.as_str(),
                "conflict_policy": rec.conflict_policy.as_str(),
                "ttl_expires": rec.ttl_expires,
            }),
        )?;
        Ok(())
    }

    pub fn get_compensation(&self, comp_id: &str) -> Result<Option<CompensationRecord>> {
        let conn = self.conn.lock().unwrap();
        self.comp_repo.get(&conn, comp_id)
    }

    pub fn list_active_compensations(&self) -> Result<Vec<CompensationRecord>> {
        let conn = self.conn.lock().unwrap();
        self.comp_repo.list_active(&conn)
    }

    pub fn mark_compensation_status(&self, comp_id: &str, new_status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        self.comp_repo.mark_status(&conn, comp_id, new_status)?;
        self.audit_append(
            "unknown-task",
            None,
            "COMPENSATION_STATUS_CHANGED",
            serde_json::json!({
                "comp_id": comp_id,
                "new_status": new_status,
            }),
        )?;
        Ok(())
    }

    // ===== Step accessors (W3b) =====

    pub fn create_step(&self, step: &StepRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.create(&conn, step)?;
        drop(conn);
        self.audit_append(
            &step.task_id,
            Some(&step.step_id),
            "STEP_CREATED",
            serde_json::json!({
                "step_id": step.step_id,
                "step_order": step.step_order,
                "tool_name": step.tool_name,
            }),
        )?;
        Ok(())
    }

    pub fn get_step(&self, step_id: &str) -> Result<Option<StepRecord>> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.get(&conn, step_id)
    }

    pub fn list_steps_for_task(&self, task_id: &str) -> Result<Vec<StepRecord>> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.list_for_task(&conn, task_id)
    }

    pub fn update_step_status(&self, step_id: &str, new_status: StepStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.update_status(&conn, step_id, new_status)?;
        drop(conn);
        self.audit_append(
            "unknown-task",
            Some(step_id),
            "STEP_STATUS_CHANGED",
            serde_json::json!({
                "step_id": step_id,
                "new_status": new_status.as_str(),
            }),
        )?;
        Ok(())
    }

    pub fn update_step_prepare_state(
        &self,
        step_id: &str,
        prepare_token: &str,
        preconditions_hash: &str,
        effect_manifest: &serde_json::Value,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.update_prepare_state(
            &conn, step_id, prepare_token, preconditions_hash, effect_manifest,
        )?;
        drop(conn);
        self.audit_append(
            "unknown-task",
            Some(step_id),
            "STEP_PREPARED",
            serde_json::json!({
                "step_id": step_id,
                "prepare_token": prepare_token,
                "preconditions_hash": preconditions_hash,
            }),
        )?;
        Ok(())
    }

    pub fn update_step_post_commit(
        &self,
        step_id: &str,
        evidence_strength: &str,
        compensation_ref: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let repo = crate::repo::step_repo::StepRepo::new();
        repo.update_post_commit(&conn, step_id, evidence_strength, compensation_ref)?;
        drop(conn);
        self.audit_append(
            "unknown-task",
            Some(step_id),
            "STEP_COMMITTED",
            serde_json::json!({
                "step_id": step_id,
                "evidence_strength": evidence_strength,
                "compensation_ref": compensation_ref,
            }),
        )?;
        Ok(())
    }
```

**Note on `audit_append` first arg:** The W3a `audit_append` takes `task_id` as first arg. For step-scoped events where we don't want to do an extra DB lookup, we pass `"unknown-task"` as a placeholder. This is a known PoC limitation — W7 will refactor `audit_append` to look up `task_id` from `steps.task_id` when only `step_id` is available. Document this in the W3b spec issues section.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/kernel_accessors.rs
git commit -m "feat(kernel): public compensation + step lifecycle accessors (V1.1 §8.1, W3a placeholder unblocked)"
```

---

## Task 2: Approval types + repo

**Goal:** Define V1.1 §8.1 `approvals` table row as a Rust struct + CRUD repo. The struct must carry all V1.1 columns: `approval_id`, `task_id`, `step_id`, `risk_level`, `args_hash`, `user_decision`, `decided_at`, `E_level`, `D_level`, `destination`, `egress_approved`, `approval_scope`, `policy_bundle_hash`.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`
- Create: `voicepilot/crates/trust-kernel/src/approval/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/approval/types.rs`
- Create: `voicepilot/crates/trust-kernel/src/approval/repo.rs`
- Create: `voicepilot/crates/trust-kernel/src/approval/approver.rs`
- Create: `voicepilot/crates/trust-kernel/tests/approval_repo.rs`

- [ ] **Step 1: Add approval module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add after `pub mod toolresult;`:

```rust
pub mod approval;
```

- [ ] **Step 2: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/approval_repo.rs`:

```rust
use trust_kernel::approval::repo::ApprovalRepo;
use trust_kernel::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use trust_kernel::db;
use trust_kernel::policy::types::{DLevel, ELevel};

fn fresh_conn() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

fn sample_approval(approval_id: &str, task_id: &str, step_id: &str) -> ApprovalRecord {
    ApprovalRecord {
        approval_id: approval_id.to_string(),
        task_id: task_id.to_string(),
        step_id: Some(step_id.to_string()),
        risk_level: "E2".to_string(),
        args_hash: "sha256:args".to_string(),
        user_decision: ApprovalDecision::Allow,
        decided_at: "2026-07-20T10:00:00Z".to_string(),
        e_level: ELevel::E2,
        d_level: DLevel::D2,
        destination: "local_file".to_string(),
        egress_approved: false,
        approval_scope: ApprovalScope::Single,
        policy_bundle_hash: "sha256:bundle".to_string(),
    }
}

#[test]
fn create_approval_persists_and_loads() {
    let conn = fresh_conn();
    let repo = ApprovalRepo::new();
    // Parent rows required for FK constraints.
    conn.execute(
        "INSERT INTO tasks (task_id, user_goal, status, created_at, updated_at) VALUES ('t1', 'g', 'IDLE', '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO steps (step_id, task_id, step_order, status) VALUES ('s1', 't1', 1, 'PENDING')",
        [],
    ).unwrap();

    let rec = sample_approval("a1", "t1", "s1");
    repo.create(&conn, &rec).unwrap();

    let loaded = repo.get(&conn, "a1").unwrap().expect("must exist");
    assert_eq!(loaded.user_decision, ApprovalDecision::Allow);
    assert_eq!(loaded.approval_scope, ApprovalScope::Single);
    assert_eq!(loaded.e_level, ELevel::E2);
    assert_eq!(loaded.d_level, DLevel::D2);
    assert_eq!(loaded.policy_bundle_hash, "sha256:bundle");
}

#[test]
fn list_for_task_returns_all_approvals() {
    let conn = fresh_conn();
    let repo = ApprovalRepo::new();
    conn.execute(
        "INSERT INTO tasks (task_id, user_goal, status, created_at, updated_at) VALUES ('t1', 'g', 'IDLE', '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO steps (step_id, task_id, step_order, status) VALUES ('s1', 't1', 1, 'PENDING')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO steps (step_id, task_id, step_order, status) VALUES ('s2', 't1', 2, 'PENDING')",
        [],
    ).unwrap();

    repo.create(&conn, &sample_approval("a1", "t1", "s1")).unwrap();
    repo.create(&conn, &sample_approval("a2", "t1", "s2")).unwrap();

    let list = repo.list_for_task(&conn, "t1").unwrap();
    assert_eq!(list.len(), 2);
}

#[test]
fn approval_decision_round_trips() {
    for d in [ApprovalDecision::Allow, ApprovalDecision::Deny, ApprovalDecision::Modify] {
        let s = d.as_str();
        let back = ApprovalDecision::parse(s).expect("must round-trip");
        assert_eq!(d, back);
    }
}

#[test]
fn approval_scope_round_trips() {
    for s in [ApprovalScope::Single, ApprovalScope::Batch] {
        let str = s.as_str();
        let back = ApprovalScope::parse(str).expect("must round-trip");
        assert_eq!(s, back);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test approval_repo 2>&1`
Expected: FAIL — `approval` module does not exist.

- [ ] **Step 4: Create approval/mod.rs**

Create `voicepilot/crates/trust-kernel/src/approval/mod.rs`:

```rust
//! Approval flow — V1.1 §6.2 approve phase + §8.1 approvals table.
//!
//! W3b: types + repo + Approver trait. CLI/Tauri implement the trait.

pub mod approver;
pub mod repo;
pub mod types;
```

- [ ] **Step 5: Create approval/types.rs**

Create `voicepilot/crates/trust-kernel/src/approval/types.rs`:

```rust
//! Approval types — V1.1 §8.1 approvals table + §6.2 approve phase.

use crate::policy::types::{DLevel, ELevel};
use serde::{Deserialize, Serialize};

/// User's decision on a prepare→approve→commit prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Allow,
    Deny,
    Modify,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalDecision::Allow => "allow",
            ApprovalDecision::Deny => "deny",
            ApprovalDecision::Modify => "modify",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "allow" => Some(ApprovalDecision::Allow),
            "deny" => Some(ApprovalDecision::Deny),
            "modify" => Some(ApprovalDecision::Modify),
            _ => None,
        }
    }
}

/// Whether this approval covers a single step or a batch of N steps.
/// V1.1 §8.1: batch requires Skill manifest `approval.mode: batch_once`
/// + same args_hash + same policy_bundle_hash + within max_approval_scope.
/// W3b always returns Single; W7 enables batch logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalScope {
    Single,
    Batch,
}

impl ApprovalScope {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalScope::Single => "single",
            ApprovalScope::Batch => "batch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "single" => Some(ApprovalScope::Single),
            "batch" => Some(ApprovalScope::Batch),
            _ => None,
        }
    }
}

/// Persisted approval record. Mirrors V1.1 §8.1 `approvals` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub task_id: String,
    pub step_id: Option<String>,
    /// V1.0 legacy column — kept for backward compat. Typically "E{e_level}".
    pub risk_level: String,
    pub args_hash: String,
    pub user_decision: ApprovalDecision,
    pub decided_at: String, // RFC3339
    pub e_level: ELevel,
    pub d_level: DLevel,
    pub destination: String,
    pub egress_approved: bool,
    pub approval_scope: ApprovalScope,
    pub policy_bundle_hash: String,
}
```

- [ ] **Step 6: Create approval/repo.rs**

Create `voicepilot/crates/trust-kernel/src/approval/repo.rs`:

```rust
//! Approval repository — V1.1 §8.1 `approvals` table.

use crate::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use crate::error::Result;
use crate::policy::types::{DLevel, ELevel};
use rusqlite::{params, Connection};

#[derive(Debug, Clone, Default)]
pub struct ApprovalRepo;

impl ApprovalRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, rec: &ApprovalRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO approvals
                (approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                 decided_at, E_level, D_level, destination, egress_approved,
                 approval_scope, policy_bundle_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rec.approval_id,
                rec.task_id,
                rec.step_id,
                rec.risk_level,
                rec.args_hash,
                rec.user_decision.as_str(),
                rec.decided_at,
                format!("{:?}", rec.e_level),
                rec.d_level.as_str(),
                rec.destination,
                rec.egress_approved as i64,
                rec.approval_scope.as_str(),
                rec.policy_bundle_hash,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, approval_id: &str) -> Result<Option<ApprovalRecord>> {
        let mut stmt = conn.prepare(
            "SELECT approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                    decided_at, E_level, D_level, destination, egress_approved,
                    approval_scope, policy_bundle_hash
             FROM approvals WHERE approval_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![approval_id], |r| {
            let approval_id: String = r.get(0)?;
            let task_id: String = r.get(1)?;
            let step_id: Option<String> = r.get(2)?;
            let risk_level: String = r.get(3)?;
            let args_hash: String = r.get(4)?;
            let user_decision: String = r.get(5)?;
            let decided_at: String = r.get(6)?;
            let e_level_str: String = r.get(7)?;
            let d_level_str: String = r.get(8)?;
            let destination: String = r.get(9)?;
            let egress_approved: i64 = r.get(10)?;
            let approval_scope_str: String = r.get(11)?;
            let policy_bundle_hash: String = r.get(12)?;
            Ok((
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (
                approval_id, task_id, step_id, risk_level, args_hash, user_decision_str,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ) = row_result?;
            let user_decision = ApprovalDecision::parse(&user_decision_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid user_decision: {}", user_decision_str
                )))?;
            let e_level = parse_e_level(&e_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid E_level: {}", e_level_str
                )))?;
            let d_level = DLevel::as_enum_from_str(&d_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid D_level: {}", d_level_str
                )))?;
            let approval_scope = ApprovalScope::parse(&approval_scope_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid approval_scope: {}", approval_scope_str
                )))?;
            Ok(Some(ApprovalRecord {
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level, d_level, destination,
                egress_approved: egress_approved != 0,
                approval_scope, policy_bundle_hash,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_for_task(&self, conn: &Connection, task_id: &str) -> Result<Vec<ApprovalRecord>> {
        let mut stmt = conn.prepare(
            "SELECT approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                    decided_at, E_level, D_level, destination, egress_approved,
                    approval_scope, policy_bundle_hash
             FROM approvals WHERE task_id = ?1 ORDER BY decided_at",
        )?;
        let rows = stmt.query_map(params![task_id], |r| {
            let approval_id: String = r.get(0)?;
            let task_id: String = r.get(1)?;
            let step_id: Option<String> = r.get(2)?;
            let risk_level: String = r.get(3)?;
            let args_hash: String = r.get(4)?;
            let user_decision: String = r.get(5)?;
            let decided_at: String = r.get(6)?;
            let e_level_str: String = r.get(7)?;
            let d_level_str: String = r.get(8)?;
            let destination: String = r.get(9)?;
            let egress_approved: i64 = r.get(10)?;
            let approval_scope_str: String = r.get(11)?;
            let policy_bundle_hash: String = r.get(12)?;
            Ok((
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (
                approval_id, task_id, step_id, risk_level, args_hash, user_decision_str,
                decided_at, e_level_str, d_level_str, destination, egress_approved,
                approval_scope_str, policy_bundle_hash,
            ) = row_result?;
            let user_decision = ApprovalDecision::parse(&user_decision_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid user_decision: {}", user_decision_str
                )))?;
            let e_level = parse_e_level(&e_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid E_level: {}", e_level_str
                )))?;
            let d_level = DLevel::as_enum_from_str(&d_level_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid D_level: {}", d_level_str
                )))?;
            let approval_scope = ApprovalScope::parse(&approval_scope_str)
                .ok_or_else(|| crate::error::KernelError::Approval(format!(
                    "invalid approval_scope: {}", approval_scope_str
                )))?;
            out.push(ApprovalRecord {
                approval_id, task_id, step_id, risk_level, args_hash, user_decision,
                decided_at, e_level, d_level, destination,
                egress_approved: egress_approved != 0,
                approval_scope, policy_bundle_hash,
            });
        }
        Ok(out)
    }
}

/// Parse "E0".."E3" string into ELevel. Used because rusqlite stores E_level as TEXT.
fn parse_e_level(s: &str) -> Option<ELevel> {
    match s {
        "E0" => Some(ELevel::E0),
        "E1" => Some(ELevel::E1),
        "E2" => Some(ELevel::E2),
        "E3" => Some(ELevel::E3),
        _ => None,
    }
}
```

- [ ] **Step 7: Add `DLevel::as_enum_from_str` helper + `Approval` error variant**

Edit `voicepilot/crates/trust-kernel/src/policy/types.rs`. Find the `impl DLevel` block (currently has `as_str` only). Add a parser method after `as_str`:

```rust
    pub fn as_enum_from_str(s: &str) -> Option<Self> {
        match s {
            "D0" => Some(DLevel::D0),
            "D1" => Some(DLevel::D1),
            "D2" => Some(DLevel::D2),
            "D3" => Some(DLevel::D3),
            _ => None,
        }
    }
```

Edit `voicepilot/crates/trust-kernel/src/error.rs`. Add before the closing `}` of `KernelError`:

```rust
    #[error("approval error: {0}")]
    Approval(String),
    #[error("skill error: {0}")]
    Skill(String),
    #[error("mcp error: {0}")]
    Mcp(String),
    #[error("path not allowed: {0}")]
    PathNotAllowed(String),
```

- [ ] **Step 8: Create approval/approver.rs**

Create `voicepilot/crates/trust-kernel/src/approval/approver.rs`:

```rust
//! Approver trait — V1.1 §6.2 approve phase.
//!
//! UI-agnostic: CLI implements it with stdin; Tauri implements it with
//! an IPC call to the approval window. The Skill executor calls
//! `approver.prompt(manifest)` between prepare and commit.

use crate::approval::types::ApprovalDecision;
use crate::policy::transaction::EffectManifest;

/// Callback the Skill executor invokes between prepare and commit.
pub trait Approver: Send + Sync {
    /// Show the effect_manifest to the user and return their decision.
    /// May block (CLI stdin) or return immediately (auto-approve / auto-deny).
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision;
}

/// Auto-approver for tests and headless runs. Always returns Allow.
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Allow
    }
}

/// Auto-denier for negative-path tests.
pub struct AutoDenier;

impl Approver for AutoDenier {
    fn prompt(&self, _manifest: &EffectManifest) -> ApprovalDecision {
        ApprovalDecision::Deny
    }
}
```

- [ ] **Step 9: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test approval_repo 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 10: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/approval/ voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/src/policy/types.rs voicepilot/crates/trust-kernel/src/error.rs voicepilot/crates/trust-kernel/tests/approval_repo.rs
git commit -m "feat(approval): types + repo + Approver trait (V1.1 §6.2 approve, §8.1 approvals table)"
```

---

## Task 3: kernel.record_approval() + ApprovalRepo accessor

**Goal:** Expose `ApprovalRepo` through the kernel so the Skill executor (Task 7) can persist approval decisions without touching `conn` directly. Includes an `APPROVAL_RECORDED` audit event.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Modify: `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`

- [ ] **Step 1: Append the failing test**

Add to the end of `voicepilot/crates/trust-kernel/tests/kernel_accessors.rs`:

```rust
use trust_kernel::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use trust_kernel::policy::types::{DLevel, ELevel};

#[test]
fn kernel_record_approval_persists_and_audits() {
    let k = fresh_kernel();
    k.create_task("t1", "g").unwrap();
    k.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let rec = ApprovalRecord {
        approval_id: "a1".to_string(),
        task_id: "t1".to_string(),
        step_id: Some("s1".to_string()),
        risk_level: "E2".to_string(),
        args_hash: "sha256:args".to_string(),
        user_decision: ApprovalDecision::Allow,
        decided_at: "2026-07-20T10:00:00Z".to_string(),
        e_level: ELevel::E2,
        d_level: DLevel::D2,
        destination: "local_file".to_string(),
        egress_approved: false,
        approval_scope: ApprovalScope::Single,
        policy_bundle_hash: "sha256:bundle".to_string(),
    };
    k.record_approval(&rec).unwrap();

    let loaded = k.get_approval("a1").unwrap().expect("must exist");
    assert_eq!(loaded.user_decision, ApprovalDecision::Allow);

    let list = k.list_approvals_for_task("t1").unwrap();
    assert_eq!(list.len(), 1);

    // Audit trail must include APPROVAL_RECORDED.
    let audit_count = k.audit_count_for_task("t1").unwrap();
    assert!(audit_count >= 2, "task + approval events expected");
}

#[test]
fn kernel_check_approval_scope_returns_single_in_w3b() {
    // W3b always returns Single per V1.1 §8.1 — batch lands in W7 with Skill context.
    let k = fresh_kernel();
    let scope = k.check_approval_scope("t1", "files.organize", "sha256:args", "sha256:bundle", 3);
    assert_eq!(scope, ApprovalScope::Single);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: FAIL — `record_approval`, `get_approval`, `list_approvals_for_task`, `check_approval_scope` do not exist.

- [ ] **Step 3: Add approval accessors to kernel**

Edit `voicepilot/crates/trust-kernel/src/kernel.rs`. Add to the imports at the top:

```rust
use crate::approval::repo::ApprovalRepo;
use crate::approval::types::{ApprovalRecord, ApprovalScope};
```

Add `approval_repo: Arc<ApprovalRepo>` field to the `TrustKernel` struct (after `comp_repo`):

```rust
pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
    gateway: Arc<crate::gateway::ActionGateway>,
    fs: Arc<crate::tools::fs::FilesystemTool>,
    comp_repo: Arc<crate::compensation::repo::CompensationRepo>,
    approval_repo: Arc<ApprovalRepo>,
    txn_mgr: Arc<crate::policy::transaction::TransactionManager>,
}
```

In `with_conn`, add initialization of the new field (after `comp_repo`):

```rust
            comp_repo: Arc::new(crate::compensation::repo::CompensationRepo::new()),
            approval_repo: Arc::new(ApprovalRepo::new()),
```

Add the accessor methods inside `impl TrustKernel` (after `mark_compensation_status`, before `create_step`):

```rust
    // ===== Approval accessors (W3b) =====

    pub fn record_approval(&self, rec: &ApprovalRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        self.approval_repo.create(&conn, rec)?;
        drop(conn);
        self.audit_append(
            &rec.task_id,
            rec.step_id.as_deref(),
            "APPROVAL_RECORDED",
            serde_json::json!({
                "approval_id": rec.approval_id,
                "user_decision": rec.user_decision.as_str(),
                "approval_scope": rec.approval_scope.as_str(),
                "e_level": format!("{:?}", rec.e_level),
                "d_level": rec.d_level.as_str(),
                "policy_bundle_hash": rec.policy_bundle_hash,
            }),
        )?;
        Ok(())
    }

    pub fn get_approval(&self, approval_id: &str) -> Result<Option<ApprovalRecord>> {
        let conn = self.conn.lock().unwrap();
        self.approval_repo.get(&conn, approval_id)
    }

    pub fn list_approvals_for_task(&self, task_id: &str) -> Result<Vec<ApprovalRecord>> {
        let conn = self.conn.lock().unwrap();
        self.approval_repo.list_for_task(&conn, task_id)
    }

    /// Determine whether this step qualifies for batch approval.
    /// V1.1 §8.1: batch requires (1) Skill manifest mode=batch_once,
    /// (2) same task_id + skill_id, (3) same args_hash, (4) same policy_bundle_hash,
    /// (5) count < max_approval_scope.
    /// W3b: always returns Single. W7 enables batch when Skill context is wired.
    pub fn check_approval_scope(
        &self,
        _task_id: &str,
        _skill_id: &str,
        _args_hash: &str,
        _policy_bundle_hash: &str,
        _max_scope: u32,
    ) -> ApprovalScope {
        ApprovalScope::Single
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test kernel_accessors 2>&1`
Expected: PASS — 6 tests (4 original + 2 new).

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/kernel_accessors.rs
git commit -m "feat(kernel): record_approval + audit chain (V1.1 §6.2 approve, §8.1 approval_scope=single for W3b)"
```

---

## Task 4: SkillManifest schema + files.organize builtin

**Goal:** Define the `SkillManifest` Rust struct matching V1.1 §5.3 + Appendix C. Provide a `files_organize_manifest()` constructor that returns the manifest for the `files.organize` Skill. No YAML parsing in W3b — builtins are Rust struct literals. (User-saved Skill loading from YAML lands in W7.)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`
- Create: `voicepilot/crates/trust-kernel/src/skills/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/skills/manifest.rs`
- Create: `voicepilot/crates/trust-kernel/tests/skills_manifest.rs`

- [ ] **Step 1: Add skills module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add after `pub mod toolresult;`:

```rust
pub mod skills;
```

- [ ] **Step 2: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/skills_manifest.rs`:

```rust
use trust_kernel::policy::types::{DLevel, ELevel};
use trust_kernel::skills::manifest::{
    files_organize_manifest, ApprovalMode, CompensationConfig, FailurePolicy,
    SkillInput, SkillInputType, SkillManifest, VerifierConfig,
};

#[test]
fn files_organize_manifest_has_correct_metadata() {
    let m = files_organize_manifest();
    assert_eq!(m.id, "files.organize");
    assert_eq!(m.version, "1.0.0");
    assert_eq!(m.title, "整理文件");
    assert!(!m.intent_examples.is_empty());
    assert_eq!(m.risk_ceiling, ELevel::E2);
    assert_eq!(m.data_class_ceiling, DLevel::D2);
    assert_eq!(m.egress.as_str(), "local_only");
    assert_eq!(m.max_steps, 4);
}

#[test]
fn files_organize_manifest_declares_tool_whitelist() {
    let m = files_organize_manifest();
    assert!(m.tools.contains(&"filesystem.search_files".to_string()));
    assert!(m.tools.contains(&"filesystem.prepare_move".to_string()));
    assert!(m.tools.contains(&"filesystem.commit_move".to_string()));
    assert!(m.tools.contains(&"filesystem.verify_move".to_string()));
    // No shell_exec, no send_to_remote_llm — must be a closed whitelist.
    assert!(!m.tools.iter().any(|t| t == "shell_exec"));
}

#[test]
fn files_organize_manifest_uses_batch_once_approval() {
    let m = files_organize_manifest();
    assert_eq!(m.approval.mode, ApprovalMode::BatchOnce);
    assert_eq!(m.approval.required_for, "commit");
    assert!(m.approval.show_effect_manifest);
    assert_eq!(m.approval.max_approval_scope, 3);
}

#[test]
fn files_organize_manifest_declares_strong_compensation() {
    let m = files_organize_manifest();
    assert_eq!(m.compensation.level, trust_kernel::compensation::types::CompensationLevel::Strong);
    assert_eq!(m.compensation.ttl_seconds, 3600);
    assert_eq!(
        m.compensation.conflict_policy,
        trust_kernel::compensation::types::ConflictPolicy::RequireConfirmation
    );
}

#[test]
fn files_organize_manifest_declares_inputs_with_allowed_roots() {
    let m = files_organize_manifest();
    let source = m.inputs.get("source").expect("source input required");
    assert_eq!(source.input_type, SkillInputType::Directory);
    assert!(!source.allowed_roots.is_empty());

    let dest = m.inputs.get("destination").expect("destination input required");
    assert_eq!(dest.input_type, SkillInputType::Directory);
    assert!(!dest.allowed_roots.is_empty());

    let filter = m.inputs.get("filter").expect("filter input required");
    assert_eq!(filter.input_type, SkillInputType::FileFilter);
}

#[test]
fn files_organize_manifest_disallows_replan() {
    let m = files_organize_manifest();
    assert!(!m.failure_policy.allow_replan);
    assert_eq!(m.failure_policy.max_retries, 1);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skills_manifest 2>&1`
Expected: FAIL — `skills` module does not exist.

- [ ] **Step 4: Create skills/mod.rs**

Create `voicepilot/crates/trust-kernel/src/skills/mod.rs`:

```rust
//! Skills layer — V1.1 §5.
//!
//! W3b: SkillManifest struct + SkillRouter + files.organize executor.

pub mod executor;
pub mod manifest;
pub mod router;
```

- [ ] **Step 5: Create skills/manifest.rs**

Create `voicepilot/crates/trust-kernel/src/skills/manifest.rs`:

```rust
//! SkillManifest — V1.1 §5.3 + Appendix C.
//!
//! Each Skill fixes: usable tools, max risk, param schema, max steps,
//! approval mode, verifier, compensation, egress. Built-in Skills are
//! Rust struct literals; user-saved Skills load from YAML in W7.

use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::policy::types::{DLevel, ELevel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub intent_examples: Vec<String>,
    pub inputs: HashMap<String, SkillInput>,
    pub risk_ceiling: ELevel,
    pub data_class_ceiling: DLevel,
    pub egress: EgressKind,
    pub max_steps: u32,
    pub tools: Vec<String>,
    pub approval: ApprovalConfig,
    pub compensation: CompensationConfig,
    pub verifier: VerifierConfig,
    pub failure_policy: FailurePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInput {
    pub input_type: SkillInputType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    #[serde(default)]
    pub allowed_values: Vec<String>,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInputType {
    Directory,
    File,
    FileFilter,
    Text,
    Number,
    Enum,
    Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressKind {
    LocalOnly,
    WebToLocal,
    LocalToWebDraft,
    LocalToWebSubmit,
}

impl EgressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EgressKind::LocalOnly => "local_only",
            EgressKind::WebToLocal => "web_to_local",
            EgressKind::LocalToWebDraft => "local_to_web_draft",
            EgressKind::LocalToWebSubmit => "local_to_web_submit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    None,
    PerStep,
    BatchOnce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    pub mode: ApprovalMode,
    /// "prepare" | "commit" | "both"
    pub required_for: String,
    pub show_effect_manifest: bool,
    pub max_approval_scope: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompensationConfig {
    pub level: CompensationLevel,
    pub ttl_seconds: u32,
    pub conflict_policy: ConflictPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VerifierConfig {
    /// "strong" | "medium" | "weak"
    pub strategy: String,
    pub recheck_after_seconds: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FailurePolicy {
    pub max_retries: u32,
    pub allow_replan: bool,
    /// "stop" | "ask_user" | "compensate"
    pub on_fail: String,
}

/// The built-in files.organize Skill manifest — V1.1 §5.2 + §5.3 example.
pub fn files_organize_manifest() -> SkillManifest {
    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "source".to_string(),
        SkillInput {
            input_type: SkillInputType::Directory,
            required: true,
            allowed_roots: vec![
                "Downloads".to_string(),
                "Desktop".to_string(),
                "Workspace".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "filter".to_string(),
        SkillInput {
            input_type: SkillInputType::FileFilter,
            required: true,
            allowed_roots: vec![],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );
    inputs.insert(
        "destination".to_string(),
        SkillInput {
            input_type: SkillInputType::Directory,
            required: true,
            allowed_roots: vec![
                "Workspace".to_string(),
                "Documents".to_string(),
                "Desktop".to_string(),
            ],
            allowed_values: vec![],
            max_length: None,
            default: None,
        },
    );

    SkillManifest {
        id: "files.organize".to_string(),
        version: "1.0.0".to_string(),
        title: "整理文件".to_string(),
        description: "搜索文件 → 生成变更清单 → 一次性批次批准 → 移动并验证 → 生成 strong Compensation".to_string(),
        intent_examples: vec![
            "把下载目录里的 PDF 移到论文文件夹".to_string(),
            "整理今天下载的文档".to_string(),
            "把下载的 PDF 整理到项目文件夹".to_string(),
        ],
        inputs,
        risk_ceiling: ELevel::E2,
        data_class_ceiling: DLevel::D2,
        egress: EgressKind::LocalOnly,
        max_steps: 4,
        tools: vec![
            "filesystem.search_files".to_string(),
            "filesystem.prepare_move".to_string(),
            "filesystem.commit_move".to_string(),
            "filesystem.verify_move".to_string(),
        ],
        approval: ApprovalConfig {
            mode: ApprovalMode::BatchOnce,
            required_for: "commit".to_string(),
            show_effect_manifest: true,
            max_approval_scope: 3,
        },
        compensation: CompensationConfig {
            level: CompensationLevel::Strong,
            ttl_seconds: 3600,
            conflict_policy: ConflictPolicy::RequireConfirmation,
        },
        verifier: VerifierConfig {
            strategy: "strong".to_string(),
            recheck_after_seconds: 0,
        },
        failure_policy: FailurePolicy {
            max_retries: 1,
            allow_replan: false,
            on_fail: "stop".to_string(),
        },
    }
}
```

- [ ] **Step 6: Create stub router.rs and executor.rs**

Create `voicepilot/crates/trust-kernel/src/skills/router.rs`:

```rust
//! SkillRouter — V1.1 §5.1.
//! Stub; implemented in Task 5.
```

Create `voicepilot/crates/trust-kernel/src/skills/executor.rs`:

```rust
//! Skill executor — V1.1 §5.2 + §6.2.
//! Stub; implemented in Task 7.
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skills_manifest 2>&1`
Expected: PASS — 6 tests.

- [ ] **Step 8: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/skills/ voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/tests/skills_manifest.rs
git commit -m "feat(skills): SkillManifest schema + files.organize builtin (V1.1 §5.2, §5.3, Appendix C)"
```

---

## Task 5: SkillRouter (intent keyword matching)

**Goal:** V1.1 §5.1 Skill Router. W3b uses deterministic keyword matching against `intent_examples` (no LLM). If any intent_example substring matches the user's goal text, return the Skill. Future W7 will add a lightweight classifier.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/router.rs`
- Create: `voicepilot/crates/trust-kernel/tests/skills_router.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/skills_router.rs`:

```rust
use trust_kernel::skills::manifest::files_organize_manifest;
use trust_kernel::skills::router::{RouteDecision, SkillRouter};

#[test]
fn router_returns_skill_when_intent_matches() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    let decision = router.route("把下载目录里的 PDF 移到论文文件夹");
    assert!(matches!(decision, RouteDecision::Skill(ref s) if s.id == "files.organize"));
}

#[test]
fn router_returns_skill_for_partial_keyword_match() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    // "整理" appears in intent_example "整理今天下载的文档".
    let decision = router.route("请整理一下我的下载文件夹");
    assert!(matches!(decision, RouteDecision::Skill(_)));
}

#[test]
fn router_returns_planner_when_no_skill_matches() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    let decision = router.route("帮我研究 Rust async runtime 并写一份对比报告");
    assert!(matches!(decision, RouteDecision::Planner));
}

#[test]
fn router_empty_returns_planner() {
    let router = SkillRouter::new();
    let decision = router.route("anything");
    assert!(matches!(decision, RouteDecision::Planner));
}

#[test]
fn router_first_match_wins_when_multiple_skills_match() {
    let mut router = SkillRouter::new();
    let mut skill_a = files_organize_manifest();
    skill_a.id = "files.organize.a".to_string();
    let mut skill_b = files_organize_manifest();
    skill_b.id = "files.organize.b".to_string();
    // Register in order A, B — A should win on first-match-wins.
    router.register(skill_a);
    router.register(skill_b);

    let decision = router.route("整理下载目录");
    if let RouteDecision::Skill(s) = decision {
        assert_eq!(s.id, "files.organize.a");
    } else {
        panic!("expected Skill decision");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skills_router 2>&1`
Expected: FAIL — `SkillRouter` / `RouteDecision` do not exist.

- [ ] **Step 3: Implement SkillRouter**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/skills/router.rs`:

```rust
//! SkillRouter — V1.1 §5.1.
//!
//! W3b: deterministic keyword matching against `intent_examples`.
//! Future W7 will add a lightweight intent classifier (BERT mini or
//! similar), but V1.1 §5.2 explicitly says Skill routing must NOT
//! call an LLM — so even W7's classifier runs locally.
//!
//! Matching rule: case-insensitive substring match. First registered
//! Skill whose intent_examples contains a substring of the user_goal
//! wins (first-match-wins).

use crate::skills::manifest::SkillManifest;

#[derive(Debug, Clone)]
pub enum RouteDecision {
    Skill(SkillManifest),
    Planner,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRouter {
    skills: Vec<SkillManifest>,
}

impl SkillRouter {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    pub fn register(&mut self, manifest: SkillManifest) {
        self.skills.push(manifest);
    }

    /// Route a user goal to a Skill or to the Planner.
    /// Matching is case-insensitive substring over `intent_examples`.
    pub fn route(&self, user_goal: &str) -> RouteDecision {
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            for example in &skill.intent_examples {
                let example_lower = example.to_lowercase();
                // Two-way substring match: goal contains example OR example contains goal.
                // The two-way handles both "short goal matches long example" and
                // "long goal contains short example keyword".
                if goal_lower.contains(&example_lower) || example_lower.contains(&goal_lower) {
                    return RouteDecision::Skill(skill.clone());
                }
            }
        }
        RouteDecision::Planner
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skills_router 2>&1`
Expected: PASS — 5 tests.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/skills/router.rs voicepilot/crates/trust-kernel/tests/skills_router.rs
git commit -m "feat(skills): SkillRouter with intent keyword matching (V1.1 §5.1, no LLM)"
```

---

## Task 6: AllowedPaths whitelist enforcement

**Goal:** V1.1 §4.4 step 1 + §8.1 `mcp_servers.allowed_paths`. W3b enforces at the `FilesystemTool` level — sources and destinations must be under an allowed root. `AllowedPaths::check(path)` canonicalizes the path (via existing `fs_paths::canonicalize`) and checks if it has any allowed root as a prefix.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`
- Create: `voicepilot/crates/trust-kernel/src/allowed_paths.rs`
- Modify: `voicepilot/crates/trust-kernel/src/tools/fs.rs`
- Create: `voicepilot/crates/trust-kernel/tests/allowed_paths.rs`

- [ ] **Step 1: Add allowed_paths module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add after `pub mod approval;`:

```rust
pub mod allowed_paths;
```

- [ ] **Step 2: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/allowed_paths.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use trust_kernel::allowed_paths::AllowedPaths;
use trust_kernel::tools::fs::FilesystemTool;
use trust_kernel::policy::transaction::TransactionManager;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-paths-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn allowed_paths_check_passes_for_path_under_root() {
    let root = tmp_dir();
    let allowed = AllowedPaths::new(vec![root.to_string_lossy().to_string()]);
    let file = root.join("a.txt");
    fs::write(&file, b"hi").unwrap();
    assert!(allowed.check(&file).is_ok());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn allowed_paths_check_fails_for_path_outside_root() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![root_a.to_string_lossy().to_string()]);
    let file = root_b.join("a.txt");
    fs::write(&file, b"hi").unwrap();
    assert!(allowed.check(&file).is_err());
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn allowed_paths_check_accepts_multiple_roots() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![
        root_a.to_string_lossy().to_string(),
        root_b.to_string_lossy().to_string(),
    ]);
    let f_a = root_a.join("a.txt"); fs::write(&f_a, b"a").unwrap();
    let f_b = root_b.join("b.txt"); fs::write(&f_b, b"b").unwrap();
    assert!(allowed.check(&f_a).is_ok());
    assert!(allowed.check(&f_b).is_ok());
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn filesystem_tool_with_allowed_paths_rejects_outside_source() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![root_a.to_string_lossy().to_string()]);
    let tool = FilesystemTool::new_with_allowed_paths(allowed);
    let dest = root_a.join("out"); fs::create_dir_all(&dest).unwrap();
    let src_outside = root_b.join("a.txt"); fs::write(&src_outside, b"hi").unwrap();

    let mgr = TransactionManager::new();
    let result = tool.prepare_move("t1", "s1", &[&src_outside], &dest, &mgr);
    assert!(result.is_err(), "prepare must reject source outside allowed roots");
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn filesystem_tool_with_allowed_paths_rejects_outside_destination() {
    let root_a = tmp_dir();
    let root_b = tmp_dir();
    let allowed = AllowedPaths::new(vec![root_a.to_string_lossy().to_string()]);
    let tool = FilesystemTool::new_with_allowed_paths(allowed);
    let src = root_a.join("a.txt"); fs::write(&src, b"hi").unwrap();
    let dest_outside = root_b.join("out"); fs::create_dir_all(&dest_outside).unwrap();

    let mgr = TransactionManager::new();
    let result = tool.prepare_move("t1", "s1", &[&src], &dest_outside, &mgr);
    assert!(result.is_err(), "prepare must reject destination outside allowed roots");
    fs::remove_dir_all(&root_a).ok();
    fs::remove_dir_all(&root_b).ok();
}

#[test]
fn filesystem_tool_without_allowed_paths_allows_any_path() {
    // Backward compat: new() has no allowed_paths, so any path is allowed.
    let root = tmp_dir();
    let tool = FilesystemTool::new();
    let src = root.join("a.txt"); fs::write(&src, b"hi").unwrap();
    let dest = root.join("out"); fs::create_dir_all(&dest).unwrap();

    let mgr = TransactionManager::new();
    let result = tool.prepare_move("t1", "s1", &[&src], &dest, &mgr);
    assert!(result.is_ok(), "default FilesystemTool must allow any path");
    fs::remove_dir_all(&root).ok();
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test allowed_paths 2>&1`
Expected: FAIL — `AllowedPaths` does not exist; `FilesystemTool::new_with_allowed_paths` does not exist.

- [ ] **Step 4: Create allowed_paths.rs**

Create `voicepilot/crates/trust-kernel/src/allowed_paths.rs`:

```rust
//! AllowedPaths whitelist — V1.1 §4.4 step 1 + §8.1 mcp_servers.allowed_paths.
//!
//! Canonicalizes paths via `fs_paths::canonicalize` (pure string) and
//! checks prefix match against any allowed root. Backward slashes are
//! normalized so Windows paths match regardless of separator.

use crate::error::{KernelError, Result};
use crate::tools::fs_paths::canonicalize;

#[derive(Debug, Clone, Default)]
pub struct AllowedPaths {
    roots: Vec<String>, // each is canonicalized
}

impl AllowedPaths {
    pub fn new(roots: Vec<String>) -> Self {
        let canonical: Vec<String> = roots.iter().map(|r| canonicalize(r)).collect();
        Self { roots: canonical }
    }

    /// Check if `path` is under one of the allowed roots.
    pub fn check(&self, path: &std::path::Path) -> Result<()> {
        let canon = canonicalize(&path.to_string_lossy());
        for root in &self.roots {
            // Match if canon == root OR canon starts with root + "/".
            if canon == *root || canon.starts_with(&format!("{}/", root)) {
                return Ok(());
            }
        }
        Err(KernelError::PathNotAllowed(format!(
            "path {} not under any allowed root: {:?}",
            canon, self.roots
        )))
    }

    /// Returns true if no allowed_paths are configured (open access).
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}
```

- [ ] **Step 5: Extend FilesystemTool with allowed_paths config**

Edit `voicepilot/crates/trust-kernel/src/tools/fs.rs`. Replace the `FilesystemTool` struct + `Default` + `new` block (lines ~20-33 in current file):

```rust
pub struct FilesystemTool {
    allowed_paths: Option<crate::allowed_paths::AllowedPaths>,
}

impl Default for FilesystemTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemTool {
    pub fn new() -> Self {
        Self { allowed_paths: None }
    }

    /// Construct a FilesystemTool that enforces `allowed_paths` on every
    /// source and destination. V1.1 §4.4 step 1 + §8.1 mcp_servers.allowed_paths.
    pub fn new_with_allowed_paths(allowed: crate::allowed_paths::AllowedPaths) -> Self {
        Self { allowed_paths: Some(allowed) }
    }
```

Then, inside `prepare_move`, insert this enforcement block immediately after the `if sources.is_empty()` check (before the `let dest_meta = std::fs::metadata(destination)` line):

```rust
        // V1.1 §4.4 step 1 + §8.1 allowed_paths enforcement.
        if let Some(allowed) = &self.allowed_paths {
            for src in sources {
                allowed.check(src)?;
            }
            allowed.check(destination)?;
        }
```

Then, inside `search_files`, insert this enforcement block immediately after `pub fn search_files(...)` opening brace (before `let meta = std::fs::metadata(root)`):

```rust
        if let Some(allowed) = &self.allowed_paths {
            allowed.check(root)?;
        }
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test allowed_paths 2>&1`
Expected: PASS — 6 tests.

- [ ] **Step 7: Run the W3a filesystem tests to confirm no regressions**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test fs_prepare_commit 2>&1`
Expected: PASS — all 8 W3a tests still pass (they use `FilesystemTool::new()` which has no allowed_paths).

- [ ] **Step 8: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/allowed_paths.rs voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/src/tools/fs.rs voicepilot/crates/trust-kernel/tests/allowed_paths.rs
git commit -m "feat(tools): AllowedPaths whitelist enforcement on FilesystemTool (V1.1 §4.4, §8.1 mcp_servers.allowed_paths)"
```

---

## Task 7: files.organize Skill executor (orchestrator)

**Goal:** The Skill executor wires the W3a `FilesystemTool` calls around the `Approver` callback to implement the V1.1 §6.2 prepare → approve → commit → verify → compensate pipeline. Returns a `ToolResult` V2.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/skills/executor.rs`
- Create: `voicepilot/crates/trust-kernel/tests/skills_executor.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/skills_executor.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use trust_kernel::approval::approver::{AutoApprover, AutoDenier};
use trust_kernel::compensation::types::CompensationLevel;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill, SkillExecution};
use trust_kernel::toolresult::{EvidenceStrength, ToolStatus};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-exec-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn files_organize_skill_executes_full_pipeline_on_allow() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "把下载目录里的 PDF 移到论文文件夹").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("src"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf1").unwrap();
    fs::write(src_dir.join("b.pdf"), b"pdf2").unwrap();
    fs::write(src_dir.join("c.txt"), b"txt").unwrap();

    let input = FilesOrganizeInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };

    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let result: SkillExecution = skill.execute(&kernel, &input, approver.as_ref()).unwrap();

    // ToolResult V2 fields.
    assert_eq!(result.tool_result.status, ToolStatus::Succeeded);
    assert_eq!(result.tool_result.evidence_strength, EvidenceStrength::Strong);
    assert_eq!(result.tool_result.compensation_level, CompensationLevel::Strong);
    assert!(result.tool_result.compensation_ref.is_some());

    // Filesystem state.
    assert!(!src_dir.join("a.pdf").exists());
    assert!(!src_dir.join("b.pdf").exists());
    assert!(src_dir.join("c.txt").exists(), "non-matching file must stay");
    assert!(dest_dir.join("a.pdf").exists());
    assert!(dest_dir.join("b.pdf").exists());

    // Kernel persistence.
    let comp_id = result.tool_result.compensation_ref.as_ref().unwrap();
    let comp = kernel.get_compensation(comp_id).unwrap().expect("compensation must exist");
    assert_eq!(comp.level, CompensationLevel::Strong);
    assert_eq!(comp.status, "active");

    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert!(step.prepare_token.is_some());
    assert!(step.preconditions_hash.is_some());
    assert!(step.effect_manifest.is_some());
    assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
    assert!(step.compensation_ref.is_some());

    let approvals = kernel.list_approvals_for_task("t1").unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].user_decision, trust_kernel::approval::types::ApprovalDecision::Allow);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn files_organize_skill_aborts_on_deny_without_commit() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "整理").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("src"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf1").unwrap();

    let input = FilesOrganizeInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };

    let approver = Arc::new(AutoDenier);
    let skill = FilesOrganizeSkill::new();
    let result = skill.execute(&kernel, &input, approver.as_ref()).unwrap();

    assert_eq!(result.tool_result.status, ToolStatus::Cancelled);
    assert!(result.tool_result.compensation_ref.is_none());

    // Source must still exist (no commit happened).
    assert!(src_dir.join("a.pdf").exists());

    let approvals = kernel.list_approvals_for_task("t1").unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].user_decision, trust_kernel::approval::types::ApprovalDecision::Deny);

    // No compensation should have been created.
    let active_comps = kernel.list_active_compensations().unwrap();
    assert!(active_comps.is_empty());

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn files_organize_skill_fails_when_no_files_match_filter() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "整理").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("src"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.txt"), b"txt").unwrap();

    let input = FilesOrganizeInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };

    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let result = skill.execute(&kernel, &input, approver.as_ref());

    // Empty search → executor returns Err (Skill failure_policy.on_fail = "stop").
    assert!(result.is_err());

    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skills_executor 2>&1`
Expected: FAIL — `FilesOrganizeSkill`, `FilesOrganizeInput`, `SkillExecution` do not exist.

- [ ] **Step 3: Implement the executor**

Replace the ENTIRE contents of `voicepilot/crates/trust-kernel/src/skills/executor.rs`:

```rust
//! files.organize Skill executor — V1.1 §5.2 + §6.2.
//!
//! Orchestrates the V1.1 §6.2 prepare → approve → commit → verify → compensate
//! pipeline for the files.organize Skill (V1.1 §5.2). The executor is UI-agnostic:
//! it calls the `Approver` trait between prepare and commit.
//!
//! Steps:
//!   1. Search sources via filesystem.search_files (V1.1 §6.1).
//!   2. Prepare move via filesystem.prepare_move (V1.1 §6.2 prepare).
//!      Persist prepare_token + preconditions_hash + effect_manifest on the step.
//!   3. Prompt the Approver with the effect_manifest (V1.1 §6.2 approve).
//!      Persist the approval decision. If Deny → cancel without commit.
//!   4. Commit move via filesystem.commit_move (V1.1 §6.2 commit).
//!   5. Verify via filesystem.verify_move (V1.1 §7.1 Strong Verifier).
//!   6. Create CompensationRecord (V1.1 §7.2 strong + auto_reverse ready).
//!      Persist compensation_ref on the step.
//!   7. Return ToolResult V2 (V1.1 §6.3) with status + evidence_strength +
//!      compensation_ref + idempotency_key.

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalRecord, ApprovalScope};
use crate::compensation::types::{CompensationLevel, CompensationRecord, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::StepStatus;
use crate::toolresult::{EvidenceStrength, ToolResult, ToolStatus};
use crate::tools::fs_paths::canonicalize;
use chrono::Utc;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FilesOrganizeInput {
    pub task_id: String,
    pub step_id: String,
    pub source: PathBuf,
    pub filter: String,
    pub destination: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SkillExecution {
    pub tool_result: ToolResult,
    pub moved_paths: Vec<(PathBuf, PathBuf)>,
}

pub struct FilesOrganizeSkill;

impl Default for FilesOrganizeSkill {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesOrganizeSkill {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(
        &self,
        kernel: &TrustKernel,
        input: &FilesOrganizeInput,
        approver: &dyn Approver,
    ) -> Result<SkillExecution> {
        let started_at = Utc::now();
        let idempotency_key = format!("idem-{}", uuid::Uuid::new_v4());

        // Step 1: search.
        kernel.update_step_status(&input.step_id, StepStatus::Running)?;
        let found = kernel
            .filesystem()
            .search_files(&input.source, &input.filter)?;
        if found.is_empty() {
            kernel.update_step_status(&input.step_id, StepStatus::Failed)?;
            return Err(KernelError::Skill(format!(
                "no files matching '{}' found under {}",
                input.filter,
                input.source.display()
            )));
        }
        let src_refs: Vec<&std::path::Path> = found.iter().map(|p| p.as_path()).collect();

        // Step 2: prepare.
        let prepared = kernel.filesystem().prepare_move(
            &input.task_id,
            &input.step_id,
            &src_refs,
            &input.destination,
            kernel.transaction_manager(),
        )?;
        let manifest_json = serde_json::to_value(&prepared.manifest)?;
        kernel.update_step_prepare_state(
            &input.step_id,
            &prepared.token.token,
            &prepared.preconditions_hash,
            &manifest_json,
        )?;

        // Step 3: prompt + record approval.
        let decision = approver.prompt(&prepared.manifest);
        let approval_id = format!("appr-{}", uuid::Uuid::new_v4());
        let approval_rec = ApprovalRecord {
            approval_id: approval_id.clone(),
            task_id: input.task_id.clone(),
            step_id: Some(input.step_id.clone()),
            risk_level: "E2".to_string(),
            args_hash: prepared.preconditions_hash.clone(),
            user_decision: decision,
            decided_at: Utc::now().to_rfc3339(),
            e_level: ELevel::E2,
            d_level: DLevel::D2,
            destination: canonicalize(&input.destination.to_string_lossy()),
            egress_approved: false,
            approval_scope: ApprovalScope::Single, // W3b always Single; W7 enables batch.
            policy_bundle_hash: kernel.gateway().bundle_hash().to_string(),
        };
        kernel.record_approval(&approval_rec)?;

        if decision == ApprovalDecision::Deny {
            // User denied — cancel step, return Cancelled ToolResult.
            kernel.update_step_status(&input.step_id, StepStatus::Cancelled)?;
            let finished_at = Utc::now();
            return Ok(SkillExecution {
                tool_result: ToolResult {
                    status: ToolStatus::Cancelled,
                    data: serde_json::json!({
                        "reason": "user_denied",
                        "approval_id": approval_id,
                    }),
                    evidence_strength: EvidenceStrength::Weak,
                    compensation_ref: None,
                    compensation_level: CompensationLevel::None,
                    preconditions_hash: Some(prepared.preconditions_hash.clone()),
                    idempotency_key,
                    egress_performed: false,
                    data_classification: DLevel::D2,
                    error_code: None,
                    retryable: false,
                    safe_to_retry: true,
                    started_at,
                    finished_at,
                },
                moved_paths: vec![],
            });
        }

        // Step 4: commit.
        let committed = kernel.filesystem().commit_move(
            &prepared.token,
            &prepared.manifest,
            kernel.transaction_manager(),
        )?;

        // Step 5: verify (Strong Verifier).
        let verify_result = kernel.filesystem().verify_move(&prepared.manifest)?;

        // Step 6: create CompensationRecord (strong + auto_reverse ready).
        let comp_id = format!("comp-{}", uuid::Uuid::new_v4());
        let reverse_payload = serde_json::json!({
            "moves": committed.moved_paths.iter().map(|(orig, curr)| {
                serde_json::json!({
                    "from": orig.to_string_lossy().replace('\\', "/"),
                    "to": curr.to_string_lossy().replace('\\', "/"),
                })
            }).collect::<Vec<_>>()
        })
        .to_string();
        let comp_rec = CompensationRecord {
            comp_id: comp_id.clone(),
            step_id: input.step_id.clone(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None,
            ttl_expires: (Utc::now() + chrono::Duration::seconds(3600)).to_rfc3339(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "filesystem.reverse_move".to_string(),
            reverse_payload,
        };
        kernel.create_compensation(&comp_rec)?;

        // Step 7: persist post-commit state + assemble ToolResult.
        kernel.update_step_post_commit(
            &input.step_id,
            "strong",
            Some(&comp_id),
        )?;
        kernel.update_step_status(&input.step_id, StepStatus::Succeeded)?;
        let finished_at = Utc::now();

        let tool_result = ToolResult {
            status: ToolStatus::Succeeded,
            data: serde_json::json!({
                "moved_count": committed.moved_paths.len(),
                "destination": prepared.manifest.destination,
                "approval_id": approval_id,
            }),
            evidence_strength: verify_result.evidence_strength,
            compensation_ref: Some(comp_id),
            compensation_level: CompensationLevel::Strong,
            preconditions_hash: Some(prepared.preconditions_hash.clone()),
            idempotency_key,
            egress_performed: false,
            data_classification: DLevel::D2,
            error_code: None,
            retryable: false,
            safe_to_retry: false,
            started_at,
            finished_at,
        };

        Ok(SkillExecution {
            tool_result,
            moved_paths: committed.moved_paths,
        })
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test skills_executor 2>&1`
Expected: PASS — 3 tests.

- [ ] **Step 5: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/skills/executor.rs voicepilot/crates/trust-kernel/tests/skills_executor.rs
git commit -m "feat(skills): files.organize executor with prepare→approve→commit→verify→compensate (V1.1 §5.2, §6.2, §7.1, §7.2)"
```

---

## Task 8: MCP server handler skeleton (inputSchema/outputSchema/annotations)

**Goal:** V1.1 §6.1 + Appendix B. `McpHandler` exposes `list_tools()` (returns schemas with `inputSchema`/`outputSchema`/`annotations`) and `call_tool()` (dispatches by name to `FilesystemTool`). W3b does not wire real JSON-RPC — that's W4. W3b delivers the schema + dispatch layer that W4 wraps in a transport.

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`
- Create: `voicepilot/crates/trust-kernel/src/mcp/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/mcp/schema.rs`
- Create: `voicepilot/crates/trust-kernel/src/mcp/handler.rs`
- Create: `voicepilot/crates/trust-kernel/tests/mcp_handler.rs`

- [ ] **Step 1: Add mcp module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add after `pub mod allowed_paths;`:

```rust
pub mod mcp;
```

- [ ] **Step 2: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/mcp_handler.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::handler::{McpCallResult, McpHandler};
use trust_kernel::mcp::schema::McpAnnotations;
use trust_kernel::repo::step_repo::StepRecord;

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-mcp-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn mcp_handler_lists_filesystem_tools_with_full_schema() {
    let handler = McpHandler::new();
    let tools = handler.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"filesystem.search_files"));
    assert!(names.contains(&"filesystem.move_files"));
    assert!(names.contains(&"filesystem.verify_move"));

    // Each tool must have inputSchema, outputSchema, and annotations.
    for t in &tools {
        assert!(t.input_schema.is_object(), "{} missing inputSchema", t.name);
        assert!(t.output_schema.is_object(), "{} missing outputSchema", t.name);
        // annotations must be present (even if all hints default to false).
        let _ann: &McpAnnotations = &t.annotations;
    }
}

#[test]
fn mcp_handler_move_files_annotations_match_v1_1_appendix_b() {
    let handler = McpHandler::new();
    let tools = handler.list_tools();
    let move_tool = tools
        .iter()
        .find(|t| t.name == "filesystem.move_files")
        .expect("filesystem.move_files must be registered");

    // V1.1 Appendix B: move_files is non-readOnly, non-destructive (it's reversible),
    // idempotent (in the prepare→commit sense), closed-world.
    assert!(!move_tool.annotations.read_only_hint);
    assert!(!move_tool.annotations.destructive_hint);
    assert!(move_tool.annotations.idempotent_hint);
    assert!(!move_tool.annotations.open_world_hint);
}

#[test]
fn mcp_handler_call_search_files_dispatches_to_filesystem() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let handler = McpHandler::new();
    let dir = tmp_dir();
    fs::write(dir.join("a.pdf"), b"pdf").unwrap();
    fs::write(dir.join("b.txt"), b"txt").unwrap();

    let args = serde_json::json!({
        "root": dir.to_string_lossy(),
        "pattern": "*.pdf"
    });
    let result = handler.call_tool(&kernel, "filesystem.search_files", &args).unwrap();
    if let McpCallResult::Ok(value) = result {
        let matches = value.get("matches").and_then(|v| v.as_array()).unwrap();
        assert_eq!(matches.len(), 1);
    } else {
        panic!("expected Ok, got {:?}", result);
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn mcp_handler_call_unknown_tool_returns_error() {
    let kernel = TrustKernel::open_in_memory().unwrap();
    let handler = McpHandler::new();
    let result = handler.call_tool(&kernel, "nonexistent.tool", &serde_json::json!({}));
    assert!(result.is_err());
}

#[test]
fn mcp_handler_call_move_files_requires_task_id_and_step_id() {
    // The MCP handler wraps the W3a FilesystemTool, but move_files is a
    // two-phase operation. W3b MCP handler rejects direct move_files calls
    // because the proper entry is through the Skill executor (Task 7).
    // W4 will add prepare_move + commit_move as separate MCP tools.
    let kernel = TrustKernel::open_in_memory().unwrap();
    kernel.create_task("t1", "g").unwrap();
    kernel.create_step(&StepRecord::new("s1", "t1", 1)).unwrap();

    let handler = McpHandler::new();
    let args = serde_json::json!({
        "task_id": "t1",
        "step_id": "s1",
        "sources": [],
        "destination": "/tmp/out"
    });
    let result = handler.call_tool(&kernel, "filesystem.move_files", &args);
    // W3b: returns Err because the handler defers move_files to the Skill executor.
    // Direct calls would bypass approval — handler refuses.
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_handler 2>&1`
Expected: FAIL — `mcp` module does not exist.

- [ ] **Step 4: Create mcp/mod.rs**

Create `voicepilot/crates/trust-kernel/src/mcp/mod.rs`:

```rust
//! MCP server handler skeleton — V1.1 §6.1 + Appendix B.
//!
//! W3b: schema + dispatch. No JSON-RPC transport — that's W4.

pub mod handler;
pub mod schema;
```

- [ ] **Step 5: Create mcp/schema.rs**

Create `voicepilot/crates/trust-kernel/src/mcp/schema.rs`:

```rust
//! MCP tool schema — V1.1 §6.1 + Appendix B.
//!
//! Mirrors the MCP Tool schema: name, description, inputSchema (JSON Schema),
//! outputSchema (JSON Schema), annotations (readOnlyHint etc.).
//! Annotations are treated as untrusted hints per V1.1 §6.1 — the Rust-side
//! Action Gateway is the authoritative source of risk classification.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub annotations: McpAnnotations,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct McpAnnotations {
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}
```

- [ ] **Step 6: Create mcp/handler.rs**

Create `voicepilot/crates/trust-kernel/src/mcp/handler.rs`:

```rust
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

        let matches = kernel.filesystem().search_files(
            std::path::Path::new(root),
            pattern,
        )?;
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
            "evidence_strength": format!("{:?}", result.evidence_strength),
        })))
    }
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test mcp_handler 2>&1`
Expected: PASS — 5 tests.

- [ ] **Step 8: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/src/mcp/ voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/tests/mcp_handler.rs
git commit -m "feat(mcp): handler skeleton with inputSchema/outputSchema/annotations (V1.1 §6.1, Appendix B)"
```

---

## Task 9: CLI `organize` command + CliApprover

**Goal:** Add the `organize <root> <filter> <dest>` CLI command. It constructs a `FilesOrganizeInput`, invokes `FilesOrganizeSkill::execute` with a `CliApprover` that prints the effect_manifest and reads `y/n` from stdin, and prints the resulting `ToolResult` V2 summary.

**Files:**
- Modify: `voicepilot/crates/cli/src/main.rs`
- Modify: `voicepilot/crates/trust-kernel/src/approval/approver.rs` (export for CLI use)

- [ ] **Step 1: Add CliApprover to CLI main.rs**

Edit `voicepilot/crates/cli/src/main.rs`. Add this struct + impl near the top of the file (after the `use` statements, before `fn main`):

```rust
use trust_kernel::approval::approver::Approver;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};

/// CLI Approver that prints the effect_manifest and prompts y/n on stdin.
struct CliApprover;

impl Approver for CliApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        println!("\n=== Effect Manifest ===");
        println!("  sources: {} file(s)", manifest.sources.len());
        println!("  total_bytes: {}", manifest.total_bytes);
        println!("  destination: {}", manifest.destination);
        if !manifest.conflicts.is_empty() {
            println!("  conflicts: {:?}", manifest.conflicts);
        }
        println!("========================\n");
        print!("approve commit? [y/N] ");
        let _ = io::stdout().flush();
        let mut buf = String::new();
        let n = io::stdin().read_line(&mut buf).unwrap_or(0);
        if n == 0 {
            // EOF — treat as deny (safer default).
            return ApprovalDecision::Deny;
        }
        let trimmed = buf.trim().to_lowercase();
        if trimmed == "y" || trimmed == "yes" {
            ApprovalDecision::Allow
        } else {
            ApprovalDecision::Deny
        }
    }
}
```

- [ ] **Step 2: Add organize command to CLI dispatch**

In the same file `voicepilot/crates/cli/src/main.rs`, find:

```rust
    println!("  move <src1> [src2...] <dest>  move files via prepare→commit (W3a)");
    println!("  quit");
```

Insert before the `quit` line:

```rust
    println!("  organize <root> <filter> <dest>  run files.organize Skill (W3b)");
```

Find:

```rust
        if let Some(rest) = line.strip_prefix("move ") {
            handle_move_command(&kernel, rest);
            continue;
        }
```

Insert before it:

```rust
        if let Some(rest) = line.strip_prefix("organize ") {
            handle_organize_command(&kernel, rest);
            continue;
        }
```

- [ ] **Step 3: Add handle_organize_command**

Append at the end of `voicepilot/crates/cli/src/main.rs` (after `handle_move_command`):

```rust
fn handle_organize_command(kernel: &TrustKernel, args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() != 3 {
        println!("usage: organize <root> <filter> <dest>");
        println!("  e.g. organize {} *.pdf {}", "%TEMP%\\dl", "%TEMP%\\papers");
        return;
    }
    let root = std::path::PathBuf::from(parts[0]);
    let filter = parts[1].to_string();
    let dest = std::path::PathBuf::from(parts[2]);

    // Create a task + step for this organize run.
    let task_id = format!("task-{}", Uuid::new_v4());
    let step_id = format!("step-{}", Uuid::new_v4());
    let goal = format!("organize {} ({}) -> {}", parts[0], parts[1], parts[2]);
    if let Err(e) = kernel.create_task(&task_id, &goal) {
        println!("create_task failed: {}", e);
        return;
    }
    if let Err(e) = kernel.create_step(&trust_kernel::repo::step_repo::StepRecord::new(
        &step_id, &task_id, 1,
    )) {
        println!("create_step failed: {}", e);
        return;
    }

    let input = FilesOrganizeInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        source: root,
        filter,
        destination: dest,
    };

    let approver = CliApprover;
    let skill = FilesOrganizeSkill::new();
    match skill.execute(kernel, &input, &approver) {
        Ok(execution) => {
            println!("\n--- ToolResult V2 ---");
            println!("  status: {:?}", execution.tool_result.status);
            println!("  evidence_strength: {:?}", execution.tool_result.evidence_strength);
            println!("  compensation_ref: {:?}", execution.tool_result.compensation_ref);
            println!("  compensation_level: {}", execution.tool_result.compensation_level.as_str());
            println!("  idempotency_key: {}", execution.tool_result.idempotency_key);
            println!("  moved: {} file(s)", execution.moved_paths.len());
            println!("  task_id: {}", task_id);
            println!("  step_id: {}", step_id);
        }
        Err(e) => {
            println!("skill execution failed: {}", e);
        }
    }
}
```

- [ ] **Step 4: Build the CLI**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli 2>&1`
Expected: compiles cleanly, 0 warnings.

- [ ] **Step 5: Smoke test the organize command**

Run:

```powershell
$env:VOICEPILOT_DB = "$env:TEMP\voicepilot-w3b-smoke.db"; Remove-Item $env:VOICEPILOT_DB -Force -ErrorAction SilentlyContinue
$root = "$env:TEMP\w3b_dl"; $dest = "$env:TEMP\w3b_papers"
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $root | Out-Null
New-Item -ItemType Directory -Path $dest | Out-Null
Set-Content "$root\a.pdf" "pdf1"; Set-Content "$root\b.pdf" "pdf2"; Set-Content "$root\c.txt" "txt"
"organize $root *.pdf $dest`ny`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```

Expected output contains:
- `=== Effect Manifest ===`
- `sources: 2 file(s)`
- `approve commit? [y/N]`
- `--- ToolResult V2 ---`
- `status: Succeeded`
- `evidence_strength: Strong`
- `compensation_ref: Some("comp-...")`
- `moved: 2 file(s)`

- [ ] **Step 6: Run full test suite**

Run: `cargo test --manifest-path voicepilot\Cargo.toml 2>&1`
Expected: ALL PASS — W1 (26) + W2 (53) + W3a (38) + W3b tasks so far.

- [ ] **Step 7: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/cli/src/main.rs voicepilot/crates/trust-kernel/src/approval/approver.rs
git commit -m "feat(cli): organize command + CliApprover for files.organize Skill (V1.1 §5.2, §6.2)"
```

---

## Task 10: End-to-end smoke test (Rust integration)

**Goal:** Verify the entire V1.1 §6.2 prepare → approve → commit → verify → compensate chain leaves a complete audit trail in SQLite. This is the §11.1 W3 gate "files.organize Skill 可跑" — proven via Rust integration test, not just CLI smoke.

**Files:**
- Create: `voicepilot/crates/trust-kernel/tests/w3b_e2e_smoke.rs`

- [ ] **Step 1: Write the end-to-end test**

Create `voicepilot/crates/trust-kernel/tests/w3b_e2e_smoke.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::compensation::types::CompensationLevel;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::skills::manifest::files_organize_manifest;
use trust_kernel::skills::router::SkillRouter;
use trust_kernel::toolresult::{EvidenceStrength, ToolStatus};

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("voicepilot-w3b-e2e-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn end_to_end_files_organize_skill_smoke() {
    // ===== Setup: kernel + task + step =====
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "e2e-task-1".to_string();
    let step_id = "e2e-step-1".to_string();
    let goal = "把下载目录里的 PDF 移到论文文件夹";
    kernel.create_task(&task_id, goal).unwrap();
    kernel.create_step(&StepRecord::new(&step_id, &task_id, 1)).unwrap();

    // ===== Setup: temp filesystem with PDFs + a non-matching .txt =====
    let dir = tmp_dir();
    let src_dir = dir.join("downloads"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("papers"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("a.pdf"), b"pdf-a-content").unwrap();
    fs::write(src_dir.join("b.pdf"), b"pdf-b-content").unwrap();
    fs::write(src_dir.join("c.txt"), b"not-a-pdf").unwrap();

    // ===== Route: Skill Router must pick files.organize =====
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    let decision = router.route(goal);
    let skill_manifest = match decision {
        trust_kernel::skills::router::RouteDecision::Skill(m) => m,
        trust_kernel::skills::router::RouteDecision::Planner => {
            panic!("router should have picked files.organize Skill")
        }
    };
    assert_eq!(skill_manifest.id, "files.organize");

    // ===== Execute: full pipeline with AutoApprover =====
    let input = FilesOrganizeInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };
    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(&kernel, &input, approver.as_ref())
        .expect("skill must succeed with AutoApprover");

    // ===== Assert: ToolResult V2 =====
    let tr = &execution.tool_result;
    assert_eq!(tr.status, ToolStatus::Succeeded);
    assert_eq!(tr.evidence_strength, EvidenceStrength::Strong);
    assert_eq!(tr.compensation_level, CompensationLevel::Strong);
    let comp_id = tr.compensation_ref.as_ref().expect("compensation_ref must be set");
    assert!(!tr.idempotency_key.is_empty());

    // ===== Assert: filesystem state =====
    assert!(!src_dir.join("a.pdf").exists());
    assert!(!src_dir.join("b.pdf").exists());
    assert!(src_dir.join("c.txt").exists(), "non-matching file must stay in place");
    assert!(dest_dir.join("a.pdf").exists());
    assert!(dest_dir.join("b.pdf").exists());
    assert_eq!(fs::read(dest_dir.join("a.pdf")).unwrap(), b"pdf-a-content");

    // ===== Assert: step record fully populated =====
    let step = kernel.get_step(&step_id).unwrap().expect("step must exist");
    assert_eq!(step.status, StepStatus::Succeeded);
    assert!(step.prepare_token.is_some(), "prepare_token must be persisted");
    assert!(step.preconditions_hash.is_some(), "preconditions_hash must be persisted");
    assert!(step.effect_manifest.is_some(), "effect_manifest must be persisted");
    assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
    assert_eq!(step.compensation_ref.as_deref(), Some(comp_id));

    // ===== Assert: approval record persisted =====
    let approvals = kernel.list_approvals_for_task(&task_id).unwrap();
    assert_eq!(approvals.len(), 1);
    assert!(approvals[0].user_decision == trust_kernel::approval::types::ApprovalDecision::Allow);
    assert_eq!(approvals[0].approval_scope, trust_kernel::approval::types::ApprovalScope::Single);
    assert_eq!(approvals[0].e_level, trust_kernel::policy::types::ELevel::E2);
    assert_eq!(approvals[0].d_level, trust_kernel::policy::types::DLevel::D2);

    // ===== Assert: compensation record persisted =====
    let comp = kernel.get_compensation(comp_id).unwrap().expect("compensation must exist");
    assert_eq!(comp.level, CompensationLevel::Strong);
    assert_eq!(comp.status, "active");
    assert_eq!(comp.compensate_fn, "filesystem.reverse_move");
    assert!(!comp.reverse_payload.is_empty());

    // ===== Assert: audit chain (hash-chained events) =====
    let audit_count = kernel.audit_count_for_task(&task_id).unwrap();
    assert!(
        audit_count >= 4,
        "expected at least 4 audit events for task (TASK_CREATED, STEP_CREATED, STEP_PREPARED, APPROVAL_RECORDED, STEP_COMMITTED, COMPENSATION_CREATED, STEP_STATUS_CHANGED), got {}",
        audit_count
    );

    // ===== Assert: compensation can be loaded + reverse_payload parses =====
    let payload: serde_json::Value = serde_json::from_str(&comp.reverse_payload).unwrap();
    let moves = payload.get("moves").and_then(|m| m.as_array()).unwrap();
    assert_eq!(moves.len(), 2, "reverse_payload must list 2 moves");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn end_to_end_compensation_can_be_reversed_via_auto_reverse() {
    // After the skill runs, the user invokes task.compensate (V1.1 §5.2)
    // to roll back. W3b verifies the auto_reverse_move function works against
    // the persisted CompensationRecord.
    use trust_kernel::compensation::executor::auto_reverse_move;

    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = "e2e-task-2".to_string();
    let step_id = "e2e-step-2".to_string();
    kernel.create_task(&task_id, "整理").unwrap();
    kernel.create_step(&StepRecord::new(&step_id, &task_id, 1)).unwrap();

    let dir = tmp_dir();
    let src_dir = dir.join("dl"); fs::create_dir_all(&src_dir).unwrap();
    let dest_dir = dir.join("out"); fs::create_dir_all(&dest_dir).unwrap();
    fs::write(src_dir.join("x.pdf"), b"pdf-x").unwrap();

    let input = FilesOrganizeInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        source: src_dir.clone(),
        filter: "*.pdf".to_string(),
        destination: dest_dir.clone(),
    };
    let approver = Arc::new(AutoApprover);
    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(&kernel, &input, approver.as_ref()).unwrap();
    let comp_id = execution.tool_result.compensation_ref.as_ref().unwrap();

    // File is now at dest_dir/x.pdf, not at src_dir/x.pdf.
    assert!(!src_dir.join("x.pdf").exists());
    assert!(dest_dir.join("x.pdf").exists());

    // Load compensation + reverse it.
    let comp = kernel.get_compensation(comp_id).unwrap().unwrap();
    auto_reverse_move(&comp).unwrap();

    // After reverse: file is back at src_dir/x.pdf.
    assert!(src_dir.join("x.pdf").exists(), "file must be back at original location");
    assert!(!dest_dir.join("x.pdf").exists(), "file must be removed from destination");

    // Mark compensation as consumed.
    kernel.mark_compensation_status(comp_id, "consumed").unwrap();
    let active = kernel.list_active_compensations().unwrap();
    assert!(active.is_empty(), "compensation must no longer be active after consume");

    fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run the end-to-end test**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test w3b_e2e_smoke 2>&1`
Expected: PASS — 2 tests.

- [ ] **Step 3: Run the full test suite one more time**

Run: `cargo test --manifest-path voicepilot\Cargo.toml 2>&1`
Expected: ALL PASS — W1 + W2 + W3a + W3b.

- [ ] **Step 4: Commit**

```powershell
cd d:\voicepilot
git add voicepilot/crates/trust-kernel/tests/w3b_e2e_smoke.rs
git commit -m "test(w3b): end-to-end smoke test for files.organize Skill (V1.1 §11.1 W3 gate)"
```

---

## Self-Review

**1. Spec coverage (V1.1.1 spec sections referenced):**

| Spec section | Covered by |
|---|---|
| §5.1 Skills vs MCP vs Planner (router determinism, no LLM) | Task 5 (SkillRouter) |
| §5.2 files.organize Skill (E2/D2/local_only/4 steps) | Tasks 4, 7 (manifest + executor) |
| §5.3 Skill Manifest (id/version/inputs/tools/approval/compensation/verifier/failure_policy) | Task 4 (SkillManifest struct) |
| §6.1 MCP Tool schema (inputSchema/outputSchema/annotations) | Task 8 (McpHandler + McpToolSchema) |
| §6.2 prepare → approve → commit (approve phase) | Tasks 2, 3, 7 (Approver trait + ApprovalRepo + executor) |
| §6.3 ToolResult V2 | Task 7 (executor returns full ToolResult) |
| §7.1 Strong Verifier (filesystem re-read) | Task 7 (verify_move wired into executor) |
| §7.2 Compensation three-level + auto_reverse | Task 7 (create_compensation) + Task 10 (auto_reverse_move round-trip) |
| §8.1 approvals table + approval_scope=single (W3b) | Tasks 2, 3 (ApprovalRepo + kernel methods) |
| §8.1 skills table (created by 001_init.sql; W7 populates from user saves) | Not populated in W3b (builtins are Rust struct literals) |
| §8.1 mcp_servers.allowed_paths | Task 6 (AllowedPaths) |
| §4.4 step 1 resource.path canonicalization enforcement | Task 6 (AllowedPaths.check uses canonicalize) |
| §11.1 W3 gate "files.organize Skill 可跑" | Task 10 (end-to-end smoke) |
| Appendix B (Tool Schema V2 full fields) | Task 8 (McpToolSchema + annotations) |
| Appendix C (Skill Manifest full schema) | Task 4 (SkillManifest struct + fields) |

Gaps (deferred to later weeks):
- Tauri UI approval window — W6
- Real MCP JSON-RPC transport (stdio/SSE) — W4
- LLM Planner fallback when SkillRouter returns Planner — W7
- Batch approval_scope logic (always Single in W3b) — W7
- YAML-loaded user-saved Skills (builtins are Rust struct literals in W3b) — W7
- Stronghold encryption for `snapshot_encrypted` — W8
- Taint tracking propagation through filesystem ops — W8

**2. Placeholder scan:** No "TBD", "TODO", "fill in" found in implementation steps. The W4 reference in Task 8 ("W4 will expose prepare_move + commit_move as separate MCP tools") is a documented scope boundary, not a placeholder — the W3b handler intentionally rejects direct move_files calls to enforce the approve phase.

**3. Type consistency:**
- `ApprovalRecord` (Task 2) matches `approvals` table schema in `001_init.sql` — all 13 columns covered.
- `ApprovalDecision` / `ApprovalScope` enums (Task 2) round-trip through DB via `as_str`/`parse`.
- `SkillManifest` (Task 4) fields match V1.1 §5.3 example + Appendix C: id, version, title, intent_examples, inputs, risk_ceiling, data_class_ceiling, egress, max_steps, tools, approval, compensation, verifier, failure_policy.
- `Approver` trait (Task 2) is consumed by `FilesOrganizeSkill::execute` (Task 7) — signature matches: `fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision`.
- `FilesOrganizeInput` (Task 7) fields (task_id, step_id, source, filter, destination) match the SkillManifest inputs (source, filter, destination) + task/step context.
- `ToolResult` V2 fields filled by the executor (Task 7) match the struct defined in W3a `toolresult.rs`: status, data, evidence_strength, compensation_ref, compensation_level, preconditions_hash, idempotency_key, egress_performed, data_classification, error_code, retryable, safe_to_retry, started_at, finished_at.
- `McpAnnotations` (Task 8) fields match V1.1 Appendix B + MCP spec: read_only_hint, destructive_hint, idempotent_hint, open_world_hint.
- `AllowedPaths` (Task 6) uses existing `fs_paths::canonicalize` from W3a — consistent path normalization.
- `FilesystemTool::new()` (W3a) and `FilesystemTool::new_with_allowed_paths()` (Task 6) both produce the same struct type — no type drift.

No issues found.

---

## Spec Notes & Issues Found During W3b Planning

Per user preferences ("遇到不合理或可优化的规格 — 报告给用户"), additional observations from W3b planning:

27. **§5.3 Skill Manifest uses YAML, but no `serde_yaml` is mentioned in the V1.1 tech stack.** W3b implements built-in Skills as Rust struct literals to avoid adding `serde_yaml`. W7 will need `serde_yaml` (or `serde_yml`) when user-saved Skills land. **Suggestion**: spec should pin the YAML library version (e.g., `serde_yml = "0.0.12"` — the maintained fork of `serde_yaml`) in §10 tech stack, and clarify that builtins are Rust struct literals while user-saved Skills are YAML.

28. **§6.2 mentions `approval_token` (line 1011) but never defines its format or construction.** W3b's `ApprovalRecord` row serves the same purpose (proof that the user approved), but there's no separate `approval_token` string the way there's a `prepare_token`. **Suggestion**: spec should either (a) define `approval_token` as `approval_id` (the UUID), or (b) drop the `approval_token` concept and use `approval_id` consistently.

29. **§8.1 `approvals` table has both `risk_level` (V1.0 legacy) and `E_level`/`D_level` (V1.1).** W3b populates both for backward compat (`risk_level = "E2"`, `E_level = "E2"`, `D_level = "D2"`), but this is redundant. **Suggestion**: spec should mark `risk_level` as deprecated in V1.2 and drop it in V2.0; V1.1 should require both columns populated for migration safety.

30. **§5.3 `inputs.<param>.type: file_filter` is mentioned but its schema is undefined.** W3b treats `filter` as a glob string (e.g., `*.pdf`), matching `FilesystemTool::search_files`'s pattern parameter. **Suggestion**: spec should add a `FileFilter` schema definition (e.g., `{ kind: "glob" | "extension" | "name_pattern" | "date_range", value: string }`) so Skills can declare filter kinds beyond glob.

31. **§8.1 `mcp_servers.allowed_paths` is on the MCP server row, but the spec doesn't define how it's enforced.** W3b enforces at the `FilesystemTool` level (per-tool `AllowedPaths` config), not the MCP server level. This means the same FilesystemTool instance can be configured differently per caller (CLI vs MCP server). **Suggestion**: spec should clarify whether `allowed_paths` is (a) a per-FilesystemTool config (W3b's approach), (b) a per-MCP-server config that wraps the FilesystemTool, or (c) both. V1.1 §6.1 implies (b), but (a) is more flexible.

32. **§5.1 Skill Router says "Intent + Slot Parser — 确定性规则 + 轻量模型" but doesn't specify the lightweight model.** W3b implements pure keyword substring matching (deterministic, no model). W7 is supposed to add "Skills Router + 4 Skill" but the spec doesn't say which model. **Suggestion**: spec should name a specific on-device model (e.g., MiniLM-L6-v2 quantized, or a fasttext classifier) so W7's implementation is reproducible. Without this, "Skills Router 命中率 ≥ 40%" (§1.4) is unverifiable.

33. **§11.1 W3 gate "files.organize Skill 可跑" is ambiguous about scope.** Does "可跑" mean (a) just the Skill runs end-to-end with a fixed input, or (b) the full prepare→approve→commit→verify→compensate chain leaves a complete audit trail? W3b's Task 10 implements (b), which is stricter. **Suggestion**: spec should pin the W3 gate to (b) — "files.organize Skill 端到端可跑: prepare → 用户 approve → commit → verify(Strong) → compensation 持久化,审计链完整".

34. **§6.2 `effect_manifest` is persisted as JSON in `steps.effect_manifest` (TEXT column), but the spec doesn't specify whether it should be redacted for D2/D3 data.** W3b stores the full manifest including sha256 + size + canonical_path. For D3 (credentials) this would leak metadata. **Suggestion**: spec should add a `effect_manifest_redacted` column or specify that D3 steps never reach prepare (consistent with §4.4 step 1 hard-deny), so redaction is moot. Currently W3b relies on the Action Gateway's D3 hard-deny to prevent this case.

35. **§7.2 conflict_policy `require_confirmation` is mentioned for files.organize, but the spec doesn't define what "confirmation" means in the auto-reverse context.** W3b's `ConflictPolicy::RequireConfirmation` is stored on the CompensationRecord but the `auto_reverse_move` executor doesn't check it — auto_reverse always runs if invoked. **Suggestion**: spec should clarify that `conflict_policy` governs the *rollback trigger*, not the rollback execution: `auto_reverse` = rollback automatically on verify failure; `require_confirmation` = prompt user before rollback; `fail` = never rollback, just fail. W3b implements `auto_reverse` behavior; W7 will add the prompt path.

These are documented here for the user; no spec changes have been made.

---

## W3b Exit Criteria

W3b is complete when ALL of the following hold:
- [ ] All 10 tasks committed
- [ ] `cargo test` passes (W1 + W2 + W3a + W3b, expected ~150+ tests total)
- [ ] `cargo build --manifest-path voicepilot\Cargo.toml -p cli` compiles with 0 warnings
- [ ] CLI `organize <root> <filter> <dest>` runs end-to-end: search → prepare → CLI approval prompt → commit → verify → compensation record persisted
- [ ] Rust integration test `w3b_e2e_smoke.rs` passes (full pipeline + auto_reverse round-trip)
- [ ] `kernel.create_compensation(rec)` public method replaces W3a CLI placeholder
- [ ] `kernel.record_approval(rec)` persists approval + emits APPROVAL_RECORDED audit event
- [ ] `AllowedPaths` enforcement rejects sources/destinations outside whitelist
- [ ] `McpHandler::list_tools()` returns 3 filesystem tools with full inputSchema/outputSchema/annotations
- [ ] `McpHandler::call_tool("filesystem.move_files", ...)` returns Err (must go through Skill executor)
- [ ] `SkillRouter::route("把下载目录里的 PDF ...")` returns `RouteDecision::Skill(files.organize)`
- [ ] Spec issues 27-35 documented for user review
- [ ] No spec changes made (user decides whether to bump to V1.1.2)
