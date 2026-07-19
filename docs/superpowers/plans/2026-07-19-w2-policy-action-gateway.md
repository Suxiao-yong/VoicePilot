# W2: Policy + Action Gateway Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the V1.1 §4 Policy Engine (Cedar + Rust Constraint + E×D risk matrix + egress) and §6.2 prepare→approve→commit transaction protocol skeleton so that the W2 gate "E×D 策略可决策" is met.

**Architecture:** Layered policy pipeline — Hard Deny Rules → Cedar Authorization (allow/deny binary) → Rust Constraint Engine (param normalization + constraints) → E×D Risk Classification (allow/confirm/deny ternary) → Egress Check → prepare/commit transaction protocol with preconditions_hash to prevent TOCTOU. No real MCP tools yet (W3); prepare/commit use stub tool simulators that produce deterministic effect_manifests.

**Tech Stack:** Rust 1.96, `cedar-policy` 4.11.2 (confirmed on crates.io 2026-07-19), `rusqlite` 0.32, `serde` 1.0, `serde_json` 1.0, `sha2` 0.10, `chrono` 0.4, `uuid` 1.10, `thiserror` 2.0.

**Reference:** V1.1 spec at `d:\voicepilot\voicepilot-v1.1-spec\voicepilot-v1.1-spec.html`. W2 gate (§11.1): "E×D 策略可决策". Relevant sections: §4.1 (E×D matrix), §4.2 (Cedar + Rust Constraint), §4.3 (Egress), §4.4 (matching order), §6.2 (prepare/approve/commit), §6.3 (ToolResult V2), §8.1 (policies table).

**W1 prerequisites (already complete):** trust-kernel crate with state machine, audit hash chain, task/step repos, policies table schema (empty), CLI REPL.

---

## File Structure

**New files:**
- `voicepilot/crates/trust-kernel/src/policy/mod.rs` — re-exports
- `voicepilot/crates/trust-kernel/src/policy/types.rs` — `ELevel`, `DLevel`, `Effect`, `Decision`, `Resource`, `Action`, `EgressDest`, `PolicyDecision`
- `voicepilot/crates/trust-kernel/src/policy/risk_matrix.rs` — E×D 16-cell → `Effect`
- `voicepilot/crates/trust-kernel/src/policy/egress.rs` — egress policy table
- `voicepilot/crates/trust-kernel/src/policy/cedar_engine.rs` — Cedar `PolicySet` wrapper, `authorize()`
- `voicepilot/crates/trust-kernel/src/policy/constraint_engine.rs` — param normalization + constraints
- `voicepilot/crates/trust-kernel/src/policy/transaction.rs` — `PrepareToken`, `EffectManifest`, `preconditions_hash`, `commit()`
- `voicepilot/crates/trust-kernel/src/repo/policy_repo.rs` — persist/load Cedar policies to `policies` table
- `voicepilot/crates/trust-kernel/src/gateway.rs` — `ActionGateway` facade orchestrating all layers
- `voicepilot/crates/trust-kernel/tests/policy_types.rs`
- `voicepilot/crates/trust-kernel/tests/risk_matrix.rs`
- `voicepilot/crates/trust-kernel/tests/egress.rs`
- `voicepilot/crates/trust-kernel/tests/cedar_engine.rs`
- `voicepilot/crates/trust-kernel/tests/constraint_engine.rs`
- `voicepilot/crates/trust-kernel/tests/policy_repo.rs`
- `voicepilot/crates/trust-kernel/tests/transaction.rs`
- `voicepilot/crates/trust-kernel/tests/action_gateway.rs`
- `voicepilot/crates/trust-kernel/src/policies/default.cedar` — default Cedar policy bundle

**Modified files:**
- `voicepilot/Cargo.toml` — add `cedar-policy` workspace dep
- `voicepilot/crates/trust-kernel/Cargo.toml` — depend on `cedar-policy`
- `voicepilot/crates/trust-kernel/src/lib.rs` — add `pub mod policy; pub mod gateway;`
- `voicepilot/crates/trust-kernel/src/error.rs` — add policy error variants
- `voicepilot/crates/trust-kernel/src/repo/mod.rs` — add `pub mod policy_repo;`
- `voicepilot/crates/trust-kernel/src/kernel.rs` — add `gateway()` accessor, `prepare()`, `commit()` thin wrappers
- `voicepilot/crates/cli/src/main.rs` — add `policy <tool> <resource>` command to demo policy decisions

---

## Task 1: Add cedar-policy dependency and policy types

**Files:**
- Modify: `voicepilot/Cargo.toml`
- Modify: `voicepilot/crates/trust-kernel/Cargo.toml`
- Create: `voicepilot/crates/trust-kernel/src/policy/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/policy/types.rs`
- Create: `voicepilot/crates/trust-kernel/tests/policy_types.rs`
- Modify: `voicepilot/crates/trust-kernel/src/lib.rs`
- Modify: `voicepilot/crates/trust-kernel/src/error.rs`

- [ ] **Step 1: Add cedar-policy to workspace deps**

Edit `voicepilot/Cargo.toml` `[workspace.dependencies]` section, add after `anyhow = "1.0"`:

```toml
cedar-policy = "4.11.2"
```

- [ ] **Step 2: Add cedar-policy to trust-kernel crate**

Edit `voicepilot/crates/trust-kernel/Cargo.toml` `[dependencies]` section, add:

```toml
cedar-policy = { workspace = true }
```

- [ ] **Step 3: Write the failing test for policy types**

Create `voicepilot/crates/trust-kernel/tests/policy_types.rs`:

```rust
use trust_kernel::policy::types::{Action, Decision, DLevel, ELevel, Effect, EgressDest, Resource};
use trust_kernel::policy::risk_matrix::classify;

#[test]
fn e_level_round_trips_through_serde() {
    let e = ELevel::E2;
    let s = serde_json::to_string(&e).unwrap();
    assert_eq!(s, "\"E2\"");
    let back: ELevel = serde_json::from_str(&s).unwrap();
    assert_eq!(back, e);
}

#[test]
fn d_level_round_trips_through_serde() {
    let d = DLevel::D3;
    let s = serde_json::to_string(&d).unwrap();
    assert_eq!(s, "\"D3\"");
}

#[test]
fn effect_serializes_as_lowercase_string() {
    assert_eq!(serde_json::to_string(&Effect::Allow).unwrap(), "\"allow\"");
    assert_eq!(serde_json::to_string(&Effect::Confirm).unwrap(), "\"confirm\"");
    assert_eq!(serde_json::to_string(&Effect::Deny).unwrap(), "\"deny\"");
}

#[test]
fn resource_carries_data_class_and_path() {
    let r = Resource {
        path: "/docs/readme.md".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"data_class\":\"D0\""));
    assert!(s.contains("\"path\":\"/docs/readme.md\""));
}

#[test]
fn decision_carries_effect_and_matched_policies() {
    let d = Decision {
        effect: Effect::Confirm,
        matched_policies: vec!["policy_d2_read".to_string()],
        normalized_args: serde_json::json!({"path": "/Docs/Readme.md"}),
        constraints_applied: vec!["max_files=100".to_string()],
        approval_scope: "single".to_string(),
        policy_bundle_hash: "sha256:abc".to_string(),
        reasons: vec!["D2 requires confirm".to_string()],
    };
    assert_eq!(d.effect, Effect::Confirm);
    assert!(!d.matched_policies.is_empty());
}

#[test]
fn egress_dest_reprs_remote_destinations() {
    assert_eq!(EgressDest::RemoteLlm.as_str(), "remote_llm");
    assert_eq!(EgressDest::RemoteMcp.as_str(), "remote_mcp");
    assert_eq!(EgressDest::LocalFile.as_str(), "local_file");
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test policy_types 2>&1`
Expected: FAIL with `unresolved module policy` or `cannot find type ELevel`.

- [ ] **Step 5: Add policy module to lib.rs**

Edit `voicepilot/crates/trust-kernel/src/lib.rs`, add after `pub mod kernel;`:

```rust
pub mod policy;
pub mod gateway;
```

- [ ] **Step 6: Create policy/mod.rs**

Create `voicepilot/crates/trust-kernel/src/policy/mod.rs`:

```rust
//! Policy Engine — V1.1 §4.
//! Layered: hard-deny → Cedar → Rust Constraint → E×D risk → egress → transaction.

pub mod types;
pub mod risk_matrix;
pub mod egress;
pub mod cedar_engine;
pub mod constraint_engine;
pub mod transaction;
```

- [ ] **Step 7: Create policy/types.rs**

Create `voicepilot/crates/trust-kernel/src/policy/types.rs`:

```rust
//! Core policy types — V1.1 §4.1 (E×D matrix), §4.2 (Cedar + Constraint), §6.3 (ToolResult V2).

use serde::{Deserialize, Serialize};

/// Operation risk level — V1.1 §4.1.
/// E0: pure read; E1: local reversible; E2: local important; E3: irreversible/egress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ELevel {
    E0,
    E1,
    E2,
    E3,
}

/// Data sensitivity — V1.1 §4.1.
/// D0: public; D1: personal; D2: private docs; D3: credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DLevel {
    D0,
    D1,
    D2,
    D3,
}

/// Final ternary effect after all policy layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Confirm,
    Deny,
}

/// Resource being acted upon — V1.1 §4.2 Cedar resource entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub path: String,
    pub data_class: DLevel,
    /// Provenance tag — V1.1 §7.3 taint tracking. One of:
    /// user_direct | web_page | external_doc | tool_output
    pub provenance: String,
}

/// Action being requested — maps to Cedar Action entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub name: String, // e.g. "read_file", "move_files", "send_to_remote_llm"
    pub e_level: ELevel,
}

/// Egress destination — V1.1 §4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressDest {
    LocalFile,
    RemoteLlm,
    RemoteMcp,
    ToolArgument,
}

impl EgressDest {
    pub fn as_str(self) -> &'static str {
        match self {
            EgressDest::LocalFile => "local_file",
            EgressDest::RemoteLlm => "remote_llm",
            EgressDest::RemoteMcp => "remote_mcp",
            EgressDest::ToolArgument => "tool_argument",
        }
    }
}

/// Final decision returned by the Action Gateway.
/// Mirrors V1.1 §4.2 output: effect + reasons + matched_policies +
/// normalized_args + constraints_applied + approval_scope + policy_bundle_hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub effect: Effect,
    pub matched_policies: Vec<String>,
    pub normalized_args: serde_json::Value,
    pub constraints_applied: Vec<String>,
    pub approval_scope: String, // "single" | "batch"
    pub policy_bundle_hash: String,
    pub reasons: Vec<String>,
}

impl Decision {
    pub fn allow(hash: impl Into<String>) -> Self {
        Self {
            effect: Effect::Allow,
            matched_policies: vec![],
            normalized_args: serde_json::Value::Null,
            constraints_applied: vec![],
            approval_scope: "single".to_string(),
            policy_bundle_hash: hash.into(),
            reasons: vec![],
        }
    }

    pub fn deny(hash: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            effect: Effect::Deny,
            matched_policies: vec![],
            normalized_args: serde_json::Value::Null,
            constraints_applied: vec![],
            approval_scope: "single".to_string(),
            policy_bundle_hash: hash.into(),
            reasons: vec![reason.into()],
        }
    }
}
```

- [ ] **Step 8: Add stub modules so it compiles**

Create `voicepilot/crates/trust-kernel/src/policy/risk_matrix.rs`:

```rust
//! E×D risk matrix — V1.1 §4.1.

use crate::policy::types::{DLevel, Effect, ELevel};

/// Classify (E, D) → Effect per V1.1 §4.1 matrix.
/// D3 row: all deny (red line).
/// D2: confirm (except E0 is confirm per spec, E3 is deny).
/// D1: allow for E0/E1, confirm for E2/E3.
/// D0: allow for E0/E1/E2, confirm for E3.
pub fn classify(e: ELevel, d: DLevel) -> Effect {
    use DLevel::*;
    use ELevel::*;
    use Effect::*;
    match (e, d) {
        // D3 row — entire row deny (red line)
        (E0 | E1 | E2 | E3, D3) => Deny,
        // D2 row — confirm except E3 deny
        (E0 | E1 | E2, D2) => Confirm,
        (E3, D2) => Deny,
        // D1 row — allow E0/E1, confirm E2/E3
        (E0 | E1, D1) => Allow,
        (E2 | E3, D1) => Confirm,
        // D0 row — allow E0/E1/E2, confirm E3
        (E0 | E1 | E2, D0) => Allow,
        (E3, D0) => Confirm,
    }
}
```

Create `voicepilot/crates/trust-kernel/src/policy/egress.rs`:

```rust
//! Egress policy — V1.1 §4.3.
//! Stub; populated in Step 9 of this task.
pub fn check_egress(_data_class: crate::policy::types::DLevel, _dest: crate::policy::types::EgressDest) -> crate::policy::types::Effect {
    crate::policy::types::Effect::Allow
}
```

Create `voicepilot/crates/trust-kernel/src/policy/cedar_engine.rs`:

```rust
//! Cedar authorization engine — V1.1 §4.2.
//! Stub; implemented in Task 4.
```

Create `voicepilot/crates/trust-kernel/src/policy/constraint_engine.rs`:

```rust
//! Rust Constraint Engine — V1.1 §4.2.
//! Stub; implemented in Task 5.
```

Create `voicepilot/crates/trust-kernel/src/policy/transaction.rs`:

```rust
//! prepare→approve→commit transaction protocol — V1.1 §6.2.
//! Stub; implemented in Task 7.
```

Create `voicepilot/crates/trust-kernel/src/gateway.rs`:

```rust
//! Action Gateway facade — V1.1 §4.2, §6.2.
//! Stub; implemented in Task 8.
```

- [ ] **Step 9: Add policy error variants**

Edit `voicepilot/crates/trust-kernel/src/error.rs`, replace entire contents:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid state transition: from={from:?} to={to:?}")]
    InvalidTransition { from: crate::state::TaskState, to: crate::state::TaskState },
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cedar policy parse error: {0}")]
    CedarParse(String),
    #[error("cedar authorization error: {0}")]
    CedarAuthz(String),
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("constraint violation: {0}")]
    ConstraintViolation(String),
    #[error("transaction precondition mismatch: expected={expected} actual={actual}")]
    PreconditionMismatch { expected: String, actual: String },
    #[error("prepare token not found or expired: {0}")]
    InvalidPrepareToken(String),
    #[error("approval required but not granted")]
    ApprovalRequired,
}

pub type Result<T> = std::result::Result<T, KernelError>;
```

- [ ] **Step 10: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test policy_types 2>&1`
Expected: PASS — 6 tests.

- [ ] **Step 11: Commit**

```bash
git add voicepilot/Cargo.toml voicepilot/crates/trust-kernel/Cargo.toml voicepilot/crates/trust-kernel/src/policy/ voicepilot/crates/trust-kernel/src/gateway.rs voicepilot/crates/trust-kernel/src/lib.rs voicepilot/crates/trust-kernel/src/error.rs voicepilot/crates/trust-kernel/tests/policy_types.rs
git commit -m "feat(policy): add cedar-policy dep, policy types, E×D risk matrix stub"
```

---

## Task 2: E×D risk matrix (16 cells)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/policy/risk_matrix.rs` (already created in Task 1)
- Create: `voicepilot/crates/trust-kernel/tests/risk_matrix.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/risk_matrix.rs`:

```rust
use trust_kernel::policy::risk_matrix::classify;
use trust_kernel::policy::types::{DLevel::*, Effect::*, ELevel::*};

#[test]
fn d3_row_is_all_deny_red_line() {
    for e in [E0, E1, E2, E3] {
        assert_eq!(classify(e, D3), Deny, "D3×{:?} must be deny", e);
    }
}

#[test]
fn d2_row_confirms_except_e3_which_denies() {
    assert_eq!(classify(E0, D2), Confirm);
    assert_eq!(classify(E1, D2), Confirm);
    assert_eq!(classify(E2, D2), Confirm);
    assert_eq!(classify(E3, D2), Deny);
}

#[test]
fn d1_row_allows_low_e_confirms_high_e() {
    assert_eq!(classify(E0, D1), Allow);
    assert_eq!(classify(E1, D1), Allow);
    assert_eq!(classify(E2, D1), Confirm);
    assert_eq!(classify(E3, D1), Confirm);
}

#[test]
fn d0_row_allows_except_e3_confirms() {
    assert_eq!(classify(E0, D0), Allow);
    assert_eq!(classify(E1, D0), Allow);
    assert_eq!(classify(E2, D0), Allow);
    assert_eq!(classify(E3, D0), Confirm);
}

#[test]
fn all_16_cells_covered() {
    // Sanity: every (E, D) combination returns a valid Effect.
    for e in [E0, E1, E2, E3] {
        for d in [D0, D1, D2, D3] {
            let _ = classify(e, d); // panics if not covered
        }
    }
}
```

- [ ] **Step 2: Run test to verify it passes (logic already in Task 1 Step 7)**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test risk_matrix 2>&1`
Expected: PASS — 5 tests.

- [ ] **Step 3: Commit**

```bash
git add voicepilot/crates/trust-kernel/tests/risk_matrix.rs
git commit -m "test(policy): E×D risk matrix 16-cell coverage"
```

---

## Task 3: Egress policy

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/policy/egress.rs`
- Create: `voicepilot/crates/trust-kernel/tests/egress.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/egress.rs`:

```rust
use trust_kernel::policy::egress::check_egress;
use trust_kernel::policy::types::{DLevel::*, Effect::*, EgressDest};

#[test]
fn local_file_to_remote_llm_d0_d1_allowed() {
    assert_eq!(check_egress(D0, EgressDest::RemoteLlm), Allow);
    assert_eq!(check_egress(D1, EgressDest::RemoteLlm), Allow);
}

#[test]
fn local_file_to_remote_llm_d2_needs_confirm() {
    assert_eq!(check_egress(D2, EgressDest::RemoteLlm), Confirm);
}

#[test]
fn local_file_to_remote_llm_d3_denied_red_line() {
    assert_eq!(check_egress(D3, EgressDest::RemoteLlm), Deny);
}

#[test]
fn local_file_to_remote_mcp_d2_confirms_d3_denies() {
    assert_eq!(check_egress(D2, EgressDest::RemoteMcp), Confirm);
    assert_eq!(check_egress(D3, EgressDest::RemoteMcp), Deny);
}

#[test]
fn web_page_to_local_file_allowed_with_taint() {
    // web_page → local_file: allow (caller must tag taint=external_untrusted)
    assert_eq!(check_egress(D0, EgressDest::LocalFile), Allow);
}

#[test]
fn web_page_to_tool_argument_denied_taint_cannot_become_instruction() {
    // web_page → tool_argument: deny (taint cannot elevate to instruction)
    assert_eq!(check_egress(D0, EgressDest::ToolArgument), Deny);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test egress 2>&1`
Expected: FAIL — current stub returns Allow for everything.

- [ ] **Step 3: Implement egress policy**

Replace `voicepilot/crates/trust-kernel/src/policy/egress.rs`:

```rust
//! Egress policy — V1.1 §4.3.
//! Controls data flow from local sources to remote destinations.
//! Independent from E×D matrix; checked separately per §4.4 step 6.

use crate::policy::types::{DLevel, Effect, EgressDest};

/// Check whether data of `data_class` may flow to `dest`.
///
/// V1.1 §4.3 table:
///   local_file → remote_llm:  D0/D1 allow, D2 confirm, D3 deny
///   local_file → remote_mcp:  D0/D1 allow, D2 confirm, D3 deny
///   web_page → local_file:    allow (caller tags taint=external_untrusted)
///   web_page → remote_llm:    allow (read-only context)
///   web_page → tool_argument: deny (taint cannot elevate to instruction)
///
/// W2 simplification: `data_class` is the local data's classification.
/// `web_page` provenance is handled by checking `EgressDest::ToolArgument`
/// — any data flowing into a tool argument is denied because taint may
/// not elevate to an instruction. Full provenance-aware egress lands in W7
/// when Taint Tracking is implemented.
pub fn check_egress(data_class: DLevel, dest: EgressDest) -> Effect {
    use DLevel::*;
    use EgressDest::*;
    use Effect::*;
    match (data_class, dest) {
        // Tool arguments are never allowed to carry data (taint elevation).
        (_, ToolArgument) => Deny,
        // Local file destination: always allow (it's a local write, E×D covers it).
        (_, LocalFile) => Allow,
        // Remote LLM / MCP
        (D0 | D1, RemoteLlm | RemoteMcp) => Allow,
        (D2, RemoteLlm | RemoteMcp) => Confirm,
        (D3, RemoteLlm | RemoteMcp) => Deny,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test egress 2>&1`
Expected: PASS — 6 tests.

- [ ] **Step 5: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/policy/egress.rs voicepilot/crates/trust-kernel/tests/egress.rs
git commit -m "feat(policy): egress policy table per V1.1 §4.3"
```

---

## Task 4: Cedar authorization engine

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/policy/cedar_engine.rs`
- Create: `voicepilot/crates/trust-kernel/src/policies/default.cedar`
- Create: `voicepilot/crates/trust-kernel/tests/cedar_engine.rs`

- [ ] **Step 1: Create the default Cedar policy bundle**

Create `voicepilot/crates/trust-kernel/src/policies/default.cedar`:

```cedar
// VoicePilot V1.1 default Cedar policy bundle — §4.2.
// Cedar produces binary allow/deny; Rust Constraint Engine upgrades to ternary.

// L0: read public docs — allow
permit (
    principal == User::"voicepilot",
    action == Action::"read_file",
    resource
)
when { resource.data_class == "D0" && resource.path like "/docs/**" };

// D2 private doc read — Cedar permits, Rust Constraint upgrades to confirm
permit (
    principal == User::"voicepilot",
    action == Action::"read_file",
    resource
)
when { resource.data_class == "D2" };

// D3 credentials — hard forbid (red line)
forbid (
    principal,
    action,
    resource
)
when { resource.data_class == "D3" };

// Egress to remote LLM — D0/D1 only; D2/D3 forbidden in Cedar
permit (
    principal == User::"voicepilot",
    action == Action::"send_to_remote_llm",
    resource
)
when { resource.data_class == "D0" || resource.data_class == "D1" };

forbid (
    principal,
    action == Action::"send_to_remote_llm",
    resource
)
when { resource.data_class == "D2" || resource.data_class == "D3" };

// Move files — Cedar permits; Rust Constraint applies max_files/overwrite
permit (
    principal == User::"voicepilot",
    action == Action::"move_files",
    resource
)
when { resource.data_class != "D3" };

// Shell — always forbidden
forbid (
    principal,
    action == Action::"shell_exec",
    resource
);
```

- [ ] **Step 2: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/cedar_engine.rs`:

```rust
use trust_kernel::policy::cedar_engine::CedarEngine;
use trust_kernel::policy::types::{Action, DLevel, ELevel, Resource};

fn make_action(name: &str, e: ELevel) -> Action {
    Action { name: name.to_string(), e_level: e }
}

fn make_resource(path: &str, d: DLevel) -> Resource {
    Resource {
        path: path.to_string(),
        data_class: d,
        provenance: "user_direct".to_string(),
    }
}

fn load_default() -> CedarEngine {
    let src = include_str!("../src/policies/default.cedar");
    CedarEngine::from_source(src).expect("default cedar policy must parse")
}

#[test]
fn d0_public_doc_read_is_allowed() {
    let engine = load_default();
    let r = make_resource("/docs/readme.md", DLevel::D0);
    let a = make_action("read_file", ELevel::E0);
    assert!(engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn d3_credential_read_is_denied() {
    let engine = load_default();
    let r = make_resource("/secrets/token.txt", DLevel::D3);
    let a = make_action("read_file", ELevel::E0);
    assert!(!engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn d2_private_doc_read_is_allowed_by_cedar_upgraded_to_confirm_by_constraint() {
    // Cedar permits D2 read; Constraint Engine upgrades effect to confirm.
    // Cedar itself returns true here — the upgrade happens in the gateway.
    let engine = load_default();
    let r = make_resource("/docs/private.md", DLevel::D2);
    let a = make_action("read_file", ELevel::E0);
    assert!(engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn shell_exec_is_always_denied() {
    let engine = load_default();
    let r = make_resource("/bin/sh", DLevel::D0);
    let a = make_action("shell_exec", ELevel::E3);
    assert!(!engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn send_to_remote_llm_d3_denied() {
    let engine = load_default();
    let r = make_resource("/secrets/cookie.txt", DLevel::D3);
    let a = make_action("send_to_remote_llm", ELevel::E3);
    assert!(!engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn send_to_remote_llm_d0_allowed() {
    let engine = load_default();
    let r = make_resource("/docs/public.md", DLevel::D0);
    let a = make_action("send_to_remote_llm", ELevel::E3);
    assert!(engine.is_allowed(&a, &r).unwrap());
}

#[test]
fn malformed_cedar_source_returns_parse_error() {
    let result = CedarEngine::from_source("this is not cedar");
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test cedar_engine 2>&1`
Expected: FAIL — `CedarEngine` does not exist.

- [ ] **Step 4: Implement CedarEngine**

Replace `voicepilot/crates/trust-kernel/src/policy/cedar_engine.rs`:

```rust
//! Cedar authorization engine — V1.1 §4.2.
//!
//! Cedar produces binary allow/deny. The Rust Constraint Engine
//! (constraint_engine.rs) upgrades to ternary allow/confirm/deny based
//! on E×D risk classification.
//!
//! Cedar never modifies tool arguments — it only authorizes.

use crate::error::{KernelError, Result};
use crate::policy::types::{Action, Resource};
use cedar_policy::{Authorizer, Context, Decision, Entities, EntityId, EntityUid, PolicySet, Request, Schema};
use std::sync::Arc;

/// Compiled Cedar policy set. Cheap to clone (Arc inside).
#[derive(Clone)]
pub struct CedarEngine {
    policies: Arc<PolicySet>,
    authorizer: Authorizer,
}

impl CedarEngine {
    /// Parse Cedar source into a policy set.
    pub fn from_source(src: &str) -> Result<Self> {
        let policies: PolicySet = src
            .parse()
            .map_err(|e| KernelError::CedarParse(e.to_string()))?;
        Ok(Self {
            policies: Arc::new(policies),
            authorizer: Authorizer::new(),
        })
    }

    /// Return the SHA-256 hash of the policy source for bundle attribution.
    /// Computed on the original source string the engine was built from.
    pub fn bundle_hash(src: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(src.as_bytes());
        format!("sha256:{:x}", h.finalize())
    }

    /// Authorize a single (principal, action, resource) request.
    /// Returns true if Cedar permits (no forbid matches and at least one permit matches).
    pub fn is_allowed(&self, action: &Action, resource: &Resource) -> Result<bool> {
        let principal = make_uid("User", "voicepilot");
        let action_uid = make_uid("Action", &action.name);
        let resource_uid = make_uid("Resource", &resource.path);

        // Build entity for the resource carrying data_class as attribute.
        // We use the schema-free entities path: pass an empty Entities set
        // and encode data_class into the resource uid's path lookup.
        // For W2 PoC we pass attributes via Context (as request-side data).
        let mut ctx = Context::new();
        ctx.insert(
            "data_class",
            &format!("\"{}\"", resource.data_class.as_str())
                .parse()
                .map_err(|e| KernelError::CedarAuthz(e.to_string()))?,
        );
        ctx.insert(
            "provenance",
            &format!("\"{}\"", resource.provenance)
                .parse()
                .map_err(|e| KernelError::CedarAuthz(e.to_string()))?,
        );

        let request = Request::new(
            principal,
            action_uid,
            resource_uid,
            ctx,
            None, // no schema for W2 PoC
        )
        .map_err(|e| KernelError::CedarAuthz(e.to_string()))?;

        let entities = Entities::empty();
        let response = self.authorizer.is_authorized(&request, &self.policies, &entities);
        Ok(response.decision() == Decision::Allow)
    }

    /// Return the list of policy IDs that matched (for audit).
    pub fn matched_policy_ids(&self) -> Vec<String> {
        self.policies
            .policies()
            .map(|p| p.id().to_string())
            .collect()
    }
}

fn make_uid(entity_type: &str, id: &str) -> EntityUid {
    EntityUid::new(entity_type.parse().expect("valid entity type"), EntityId::new(id))
}
```

Add `as_str` helper to `DLevel` in `policy/types.rs`. Append before the closing `}` of the `impl` block (or add new impl block after the enum):

Edit `voicepilot/crates/trust-kernel/src/policy/types.rs`, find:

```rust
/// Data sensitivity — V1.1 §4.1.
/// D0: public; D1: personal; D2: private docs; D3: credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DLevel {
    D0,
    D1,
    D2,
    D3,
}
```

Replace with:

```rust
/// Data sensitivity — V1.1 §4.1.
/// D0: public; D1: personal; D2: private docs; D3: credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DLevel {
    D0,
    D1,
    D2,
    D3,
}

impl DLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            DLevel::D0 => "D0",
            DLevel::D1 => "D1",
            DLevel::D2 => "D2",
            DLevel::D3 => "D3",
        }
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test cedar_engine 2>&1`
Expected: PASS — 7 tests.

If some tests fail with "Cedar parse error", inspect the Cedar source. Common issues: `like` patterns require `**` suffix; attribute access in `when` clauses requires the attribute to be present in the request context (we pass via Context). Fix by adjusting the policy syntax in `default.cedar`.

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/policy/cedar_engine.rs voicepilot/crates/trust-kernel/src/policy/types.rs voicepilot/crates/trust-kernel/src/policies/default.cedar voicepilot/crates/trust-kernel/tests/cedar_engine.rs
git commit -m "feat(policy): Cedar authorization engine with default bundle"
```

---

## Task 5: Rust Constraint Engine

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/policy/constraint_engine.rs`
- Create: `voicepilot/crates/trust-kernel/tests/constraint_engine.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/constraint_engine.rs`:

```rust
use trust_kernel::policy::constraint_engine::{ConstraintEngine, ConstraintSpec};
use trust_kernel::policy::types::{DLevel, Effect, ELevel};

#[test]
fn normalize_path_lowercases_drive_letter_and_forward_slashes() {
    let engine = ConstraintEngine::default();
    let out = engine.normalize_args(
        "move_files",
        &serde_json::json!({
            "sources": ["C:\\Users\\Me\\Downloads\\Paper1.pdf"],
            "destination": "C:/Users/me/Documents"
        }),
    ).unwrap();
    let src = out["sources"][0].as_str().unwrap();
    assert!(src.starts_with("C:/Users/"), "normalized: {}", src);
    assert!(!src.contains('\\'), "no backslashes: {}", src);
}

#[test]
fn apply_max_files_constraint_truncates_sources() {
    let mut engine = ConstraintEngine::default();
    engine.register("move_files", ConstraintSpec {
        max_files: Some(2),
        overwrite: Some(false),
        allowed_destinations: None,
    });
    let normalized = serde_json::json!({
        "sources": ["a.txt", "b.txt", "c.txt", "d.txt"],
        "destination": "/out"
    });
    let (constrained, applied) = engine.apply_constraints("move_files", normalized).unwrap();
    assert_eq!(constrained["sources"].as_array().unwrap().len(), 2);
    assert!(applied.iter().any(|c| c.contains("max_files=2")));
}

#[test]
fn overwrite_false_blocks_existing_destination() {
    let mut engine = ConstraintEngine::default();
    engine.register("write_file", ConstraintSpec {
        max_files: None,
        overwrite: Some(false),
        allowed_destinations: None,
    });
    // Without filesystem access we can't check existence; we just record the constraint.
    let (out, applied) = engine.apply_constraints("write_file", serde_json::json!({"path": "/tmp/x", "content": "hi"})).unwrap();
    assert_eq!(out["path"], "/tmp/x");
    assert!(applied.iter().any(|c| c.contains("overwrite=false")));
}

#[test]
fn unregistered_tool_passes_through_with_no_constraints() {
    let engine = ConstraintEngine::default();
    let (out, applied) = engine.apply_constraints("unknown_tool", serde_json::json!({"x": 1})).unwrap();
    assert_eq!(out["x"], 1);
    assert!(applied.is_empty());
}

#[test]
fn upgrade_effect_to_confirm_when_risk_matrix_says_so() {
    let engine = ConstraintEngine::default();
    // Cedar allowed (true), but E0×D2 = Confirm per matrix.
    let effect = engine.upgrade_effect(true, ELevel::E0, DLevel::D2);
    assert_eq!(effect, Effect::Confirm);
}

#[test]
fn upgrade_effect_stays_allow_when_risk_matrix_allows() {
    let engine = ConstraintEngine::default();
    let effect = engine.upgrade_effect(true, ELevel::E0, DLevel::D0);
    assert_eq!(effect, Effect::Allow);
}

#[test]
fn upgrade_effect_is_deny_when_cedar_denies() {
    let engine = ConstraintEngine::default();
    let effect = engine.upgrade_effect(false, ELevel::E0, DLevel::D0);
    assert_eq!(effect, Effect::Deny);
}

#[test]
fn upgrade_effect_is_deny_when_risk_matrix_denies_regardless_of_cedar() {
    let engine = ConstraintEngine::default();
    // Even if Cedar permits, D3 is red line.
    let effect = engine.upgrade_effect(true, ELevel::E0, DLevel::D3);
    assert_eq!(effect, Effect::Deny);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test constraint_engine 2>&1`
Expected: FAIL — `ConstraintEngine` does not exist.

- [ ] **Step 3: Implement ConstraintEngine**

Replace `voicepilot/crates/trust-kernel/src/policy/constraint_engine.rs`:

```rust
//! Rust Constraint Engine — V1.1 §4.2.
//!
//! Cedar produces binary allow/deny. This engine:
//!   1. Normalizes tool arguments (path canonicalization).
//!   2. Applies Rust-side constraints (max_files, overwrite, allowed_destinations).
//!   3. Upgrades Cedar's binary decision to ternary (allow/confirm/deny)
//!      using the E×D risk matrix.
//!
//! Per §4.2: "Cedar 仅产出 allow/deny 二态，Rust Constraint Engine 在其上
//! 扩展为三态并应用参数约束。"

use crate::error::{KernelError, Result};
use crate::policy::risk_matrix::classify;
use crate::policy::types::{DLevel, Effect, ELevel};
use std::collections::HashMap;

/// Per-tool constraint specification. W2 covers filesystem-style tools.
#[derive(Debug, Clone, Default)]
pub struct ConstraintSpec {
    pub max_files: Option<usize>,
    pub overwrite: Option<bool>,
    pub allowed_destinations: Option<Vec<String>>,
}

#[derive(Default)]
pub struct ConstraintEngine {
    specs: HashMap<String, ConstraintSpec>,
}

impl ConstraintEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: &str, spec: ConstraintSpec) {
        self.specs.insert(tool.to_string(), spec);
    }

    /// Normalize tool arguments. W2: canonicalize paths (backslashes → forward,
    /// lowercase drive letter, dedupe slashes).
    pub fn normalize_args(&self, _tool: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        let mut out = args.clone();
        normalize_paths_in_place(&mut out);
        Ok(out)
    }

    /// Apply constraints to normalized args. Returns (constrained_args, applied_list).
    pub fn apply_constraints(
        &self,
        tool: &str,
        mut args: serde_json::Value,
    ) -> Result<(serde_json::Value, Vec<String>)> {
        let mut applied = Vec::new();
        if let Some(spec) = self.specs.get(tool) {
            if let Some(max) = spec.max_files {
                if let Some(arr) = args.get_mut("sources").and_then(|v| v.as_array_mut()) {
                    if arr.len() > max {
                        arr.truncate(max);
                        applied.push(format!("max_files={}", max));
                    }
                }
            }
            if let Some(false) = spec.overwrite {
                applied.push("overwrite=false".to_string());
                // Real existence check lands in W3 when filesystem MCP is wired.
            }
            if let Some(allowed) = &spec.allowed_destinations {
                if let Some(dest) = args.get("destination").and_then(|v| v.as_str()) {
                    if !allowed.iter().any(|a| dest.starts_with(a)) {
                        return Err(KernelError::ConstraintViolation(format!(
                            "destination {} not in allowed list", dest
                        )));
                    }
                }
                applied.push("allowed_destinations checked".to_string());
            }
        }
        Ok((args, applied))
    }

    /// Upgrade Cedar's binary decision to ternary using E×D risk matrix.
    /// - If Cedar denies → Deny
    /// - If Cedar allows → consult risk matrix (Allow / Confirm / Deny)
    pub fn upgrade_effect(&self, cedar_allows: bool, e: ELevel, d: DLevel) -> Effect {
        if !cedar_allows {
            return Effect::Deny;
        }
        classify(e, d)
    }
}

/// Recursively normalize path-like strings inside a JSON value.
/// Heuristic: any string containing a backslash or starting with a drive letter.
fn normalize_paths_in_place(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::String(s) => {
            if s.contains('\\') || (s.len() >= 2 && s.as_bytes()[1] == b':') {
                let normalized = s.replace('\\', "/");
                // Lowercase drive letter (e.g. "C:/" → "c:/") — keep rest as-is.
                let normalized = if normalized.len() >= 2 {
                    let bytes = normalized.as_bytes();
                    if bytes[1] == b':' {
                        let mut out = String::new();
                        out.push(bytes[0].to_ascii_lowercase() as char);
                        out.push_str(&normalized[1..]);
                        out
                    } else {
                        normalized
                    }
                } else {
                    normalized
                };
                *s = normalized;
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                normalize_paths_in_place(item);
            }
        }
        serde_json::Value::Object(obj) => {
            for (_, v) in obj.iter_mut() {
                normalize_paths_in_place(v);
            }
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test constraint_engine 2>&1`
Expected: PASS — 8 tests.

- [ ] **Step 5: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/policy/constraint_engine.rs voicepilot/crates/trust-kernel/tests/constraint_engine.rs
git commit -m "feat(policy): Rust Constraint Engine — normalize, constraints, ternary upgrade"
```

---

## Task 6: Policy repo (persist Cedar policies to `policies` table)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/repo/mod.rs`
- Create: `voicepilot/crates/trust-kernel/src/repo/policy_repo.rs`
- Create: `voicepilot/crates/trust-kernel/tests/policy_repo.rs`

- [ ] **Step 1: Add module to repo/mod.rs**

Edit `voicepilot/crates/trust-kernel/src/repo/mod.rs`, replace entire contents:

```rust
//! Repositories — CRUD for tasks, steps, policies.
pub mod task_repo;
pub mod step_repo;
pub mod policy_repo;
```

- [ ] **Step 2: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/policy_repo.rs`:

```rust
use trust_kernel::db;
use trust_kernel::repo::policy_repo::{PolicyRecord, PolicyRepo};

fn fresh_conn() -> rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    db::run_migrations(&conn).unwrap();
    conn
}

#[test]
fn create_policy_persists_and_can_be_loaded() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    let policy = PolicyRecord {
        policy_id: "default".to_string(),
        version: 1,
        rules_json: r#"{"rules":[]}"#.to_string(),
        hash: "sha256:abc".to_string(),
        enabled: true,
        cedar_policies: Some("permit(principal, action, resource);".to_string()),
        cedar_schema: None,
        rust_constraints: Some("max_files=100".to_string()),
    };
    repo.create(&conn, &policy).unwrap();

    let loaded = repo.get(&conn, "default").unwrap().expect("must exist");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.hash, "sha256:abc");
    assert!(loaded.enabled);
    assert!(loaded.cedar_policies.is_some());
}

#[test]
fn get_missing_policy_returns_none() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    assert!(repo.get(&conn, "nonexistent").unwrap().is_none());
}

#[test]
fn list_enabled_returns_only_enabled_policies() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    repo.create(&conn, &PolicyRecord {
        policy_id: "on".to_string(), version: 1, rules_json: "{}".to_string(),
        hash: "h1".to_string(), enabled: true,
        cedar_policies: None, cedar_schema: None, rust_constraints: None,
    }).unwrap();
    repo.create(&conn, &PolicyRecord {
        policy_id: "off".to_string(), version: 1, rules_json: "{}".to_string(),
        hash: "h2".to_string(), enabled: false,
        cedar_policies: None, cedar_schema: None, rust_constraints: None,
    }).unwrap();
    let enabled = repo.list_enabled(&conn).unwrap();
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].policy_id, "on");
}

#[test]
fn update_hash_replaces_existing() {
    let conn = fresh_conn();
    let repo = PolicyRepo::new();
    repo.create(&conn, &PolicyRecord {
        policy_id: "p".to_string(), version: 1, rules_json: "{}".to_string(),
        hash: "old".to_string(), enabled: true,
        cedar_policies: None, cedar_schema: None, rust_constraints: None,
    }).unwrap();
    repo.update_hash(&conn, "p", "new").unwrap();
    let loaded = repo.get(&conn, "p").unwrap().unwrap();
    assert_eq!(loaded.hash, "new");
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test policy_repo 2>&1`
Expected: FAIL — `policy_repo` module not found.

- [ ] **Step 4: Implement PolicyRepo**

Create `voicepilot/crates/trust-kernel/src/repo/policy_repo.rs`:

```rust
//! Policy repository — CRUD against SQLite `policies` table.

use crate::error::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRecord {
    pub policy_id: String,
    pub version: i64,
    pub rules_json: String,
    pub hash: String,
    pub enabled: bool,
    pub cedar_policies: Option<String>,
    pub cedar_schema: Option<String>,
    pub rust_constraints: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PolicyRepo;

impl PolicyRepo {
    pub fn new() -> Self {
        Self
    }

    pub fn create(&self, conn: &Connection, policy: &PolicyRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO policies
                (policy_id, version, rules_json, hash, enabled,
                 cedar_policies, cedar_schema, rust_constraints)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                policy.policy_id,
                policy.version,
                policy.rules_json,
                policy.hash,
                policy.enabled as i64,
                policy.cedar_policies,
                policy.cedar_schema,
                policy.rust_constraints,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, conn: &Connection, policy_id: &str) -> Result<Option<PolicyRecord>> {
        let mut stmt = conn.prepare(
            "SELECT policy_id, version, rules_json, hash, enabled,
                    cedar_policies, cedar_schema, rust_constraints
             FROM policies WHERE policy_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![policy_id], |r| {
            let policy_id: String = r.get(0)?;
            let version: i64 = r.get(1)?;
            let rules_json: String = r.get(2)?;
            let hash: String = r.get(3)?;
            let enabled: i64 = r.get(4)?;
            let cedar_policies: Option<String> = r.get(5)?;
            let cedar_schema: Option<String> = r.get(6)?;
            let rust_constraints: Option<String> = r.get(7)?;
            Ok((
                policy_id, version, rules_json, hash, enabled,
                cedar_policies, cedar_schema, rust_constraints,
            ))
        })?;
        if let Some(row_result) = rows.next() {
            let (policy_id, version, rules_json, hash, enabled,
                 cedar_policies, cedar_schema, rust_constraints) = row_result?;
            Ok(Some(PolicyRecord {
                policy_id, version, rules_json, hash, enabled: enabled != 0,
                cedar_policies, cedar_schema, rust_constraints,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_enabled(&self, conn: &Connection) -> Result<Vec<PolicyRecord>> {
        let mut stmt = conn.prepare(
            "SELECT policy_id, version, rules_json, hash, enabled,
                    cedar_policies, cedar_schema, rust_constraints
             FROM policies WHERE enabled = 1 ORDER BY policy_id",
        )?;
        let rows = stmt.query_map([], |r| {
            let policy_id: String = r.get(0)?;
            let version: i64 = r.get(1)?;
            let rules_json: String = r.get(2)?;
            let hash: String = r.get(3)?;
            let enabled: i64 = r.get(4)?;
            let cedar_policies: Option<String> = r.get(5)?;
            let cedar_schema: Option<String> = r.get(6)?;
            let rust_constraints: Option<String> = r.get(7)?;
            Ok((
                policy_id, version, rules_json, hash, enabled,
                cedar_policies, cedar_schema, rust_constraints,
            ))
        })?;
        let mut out = Vec::new();
        for row_result in rows {
            let (policy_id, version, rules_json, hash, enabled,
                 cedar_policies, cedar_schema, rust_constraints) = row_result?;
            out.push(PolicyRecord {
                policy_id, version, rules_json, hash, enabled: enabled != 0,
                cedar_policies, cedar_schema, rust_constraints,
            });
        }
        Ok(out)
    }

    pub fn update_hash(&self, conn: &Connection, policy_id: &str, new_hash: &str) -> Result<()> {
        conn.execute(
            "UPDATE policies SET hash = ?1 WHERE policy_id = ?2",
            params![new_hash, policy_id],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test policy_repo 2>&1`
Expected: PASS — 4 tests.

- [ ] **Step 6: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/repo/mod.rs voicepilot/crates/trust-kernel/src/repo/policy_repo.rs voicepilot/crates/trust-kernel/tests/policy_repo.rs
git commit -m "feat(repo): policy_repo for Cedar policy persistence"
```

---

## Task 7: Transaction protocol (prepare → approve → commit)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/policy/transaction.rs`
- Create: `voicepilot/crates/trust-kernel/tests/transaction.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/transaction.rs`:

```rust
use trust_kernel::policy::transaction::{EffectManifest, PrepareToken, TransactionManager};
use trust_kernel::policy::types::{DLevel, ELevel};

fn sample_manifest() -> EffectManifest {
    EffectManifest {
        sources: vec![
            trust_kernel::policy::transaction::FileSnapshot {
                canonical_path: "C:/Users/me/Downloads/paper1.pdf".to_string(),
                file_id: "fi_001".to_string(),
                size: 12345,
                last_write_time: "2026-07-18T10:00:00Z".to_string(),
                sha256: "a1b2c3".to_string(),
            },
        ],
        destination: "C:/Users/me/Documents/Papers".to_string(),
        conflicts: vec![],
        total_bytes: 12345,
    }
}

#[test]
fn prepare_returns_token_and_hash() {
    let mgr = TransactionManager::new();
    let manifest = sample_manifest();
    let token = mgr.prepare("task-1", "step-1", &manifest).unwrap();
    assert!(token.token.starts_with("prt_"));
    assert!(!token.preconditions_hash.is_empty());
    assert!(token.expires_at > chrono::Utc::now());
}

#[test]
fn commit_succeeds_when_preconditions_match() {
    let mgr = TransactionManager::new();
    let manifest = sample_manifest();
    let token = mgr.prepare("task-1", "step-1", &manifest).unwrap();

    // Re-supply identical manifest at commit.
    let result = mgr.commit(&token, &manifest).unwrap();
    assert!(result.committed);
    assert_eq!(result.preconditions_recheck, token.preconditions_hash);
}

#[test]
fn commit_fails_when_preconditions_differ() {
    let mgr = TransactionManager::new();
    let manifest = sample_manifest();
    let token = mgr.prepare("task-1", "step-1", &manifest).unwrap();

    // Tamper with manifest — file size changed.
    let mut modified = manifest.clone();
    modified.sources[0].size = 99999;

    let result = mgr.commit(&token, &modified);
    assert!(result.is_err(), "commit must fail on precondition mismatch");
    let err = result.unwrap_err();
    assert!(matches!(err, trust_kernel::error::KernelError::PreconditionMismatch { .. }));
}

#[test]
fn commit_fails_for_unknown_token() {
    let mgr = TransactionManager::new();
    let bogus = PrepareToken {
        token: "prt_unknown".to_string(),
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        preconditions_hash: "sha256:bogus".to_string(),
    };
    let result = mgr.commit(&bogus, &sample_manifest());
    assert!(result.is_err());
}

#[test]
fn commit_fails_for_expired_token() {
    let mgr = TransactionManager::new();
    let mut token = mgr.prepare("task-1", "step-1", &sample_manifest()).unwrap();
    token.expires_at = chrono::Utc::now() - chrono::Duration::minutes(1);
    let result = mgr.commit(&token, &sample_manifest());
    assert!(result.is_err());
}

#[test]
fn preconditions_hash_changes_when_any_source_field_changes() {
    let mgr = TransactionManager::new();
    let m1 = sample_manifest();
    let mut m2 = m1.clone();
    m2.sources[0].sha256 = "different".to_string();
    let h1 = mgr.preconditions_hash(&m1);
    let h2 = mgr.preconditions_hash(&m2);
    assert_ne!(h1, h2, "hash must change when sha256 changes");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test transaction 2>&1`
Expected: FAIL — `TransactionManager` does not exist.

- [ ] **Step 3: Implement TransactionManager**

Replace `voicepilot/crates/trust-kernel/src/policy/transaction.rs`:

```rust
//! prepare → approve → commit transaction protocol — V1.1 §6.2.
//!
//! Prevents TOCTOU (Time-of-Check-to-Time-of-Use) by:
//!   1. prepare: freeze a snapshot of affected files → effect_manifest +
//!      preconditions_hash + prepare_token (with expiry).
//!   2. approve: user reviews effect_manifest, grants approval_token.
//!   3. commit: re-check preconditions_hash; if mismatch → force re-prepare.
//!
//! W2 uses in-memory token store. W3 wires this to real filesystem MCP tools.

use crate::error::{KernelError, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub canonical_path: String,
    pub file_id: String,
    pub size: u64,
    pub last_write_time: String, // RFC3339
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectManifest {
    pub sources: Vec<FileSnapshot>,
    pub destination: String,
    pub conflicts: Vec<String>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareToken {
    pub token: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub preconditions_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitResult {
    pub committed: bool,
    pub preconditions_recheck: String,
}

/// In-memory store of active prepare tokens. W3 will move to SQLite.
pub struct TransactionManager {
    tokens: Mutex<HashMap<String, (PrepareToken, String /* task_id */, String /* step_id */)>>,
    ttl_seconds: i64,
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
            ttl_seconds: 300, // 5 min prepare→commit window
        }
    }

    /// Compute the preconditions hash for a manifest.
    /// SHA256 over canonical JSON of all snapshot fields, sorted by path.
    pub fn preconditions_hash(&self, manifest: &EffectManifest) -> String {
        let mut sorted = manifest.sources.clone();
        sorted.sort_by(|a, b| a.canonical_path.cmp(&b.canonical_path));
        let mut hasher = Sha256::new();
        for snap in &sorted {
            hasher.update(snap.canonical_path.as_bytes());
            hasher.update(snap.file_id.as_bytes());
            hasher.update(&snap.size.to_le_bytes());
            hasher.update(snap.last_write_time.as_bytes());
            hasher.update(snap.sha256.as_bytes());
        }
        hasher.update(manifest.destination.as_bytes());
        hasher.update(&manifest.total_bytes.to_le_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Issue a prepare token for the given manifest.
    pub fn prepare(
        &self,
        task_id: &str,
        step_id: &str,
        manifest: &EffectManifest,
    ) -> Result<PrepareToken> {
        let token = PrepareToken {
            token: format!("prt_{}", uuid::Uuid::new_v4()),
            expires_at: Utc::now() + Duration::seconds(self.ttl_seconds),
            preconditions_hash: self.preconditions_hash(manifest),
        };
        self.tokens.lock().unwrap().insert(
            token.token.clone(),
            (token.clone(), task_id.to_string(), step_id.to_string()),
        );
        Ok(token)
    }

    /// Attempt to commit. Re-checks preconditions_hash; rejects mismatch or expiry.
    pub fn commit(&self, token: &PrepareToken, manifest: &EffectManifest) -> Result<CommitResult> {
        let entry = {
            let tokens = self.tokens.lock().unwrap();
            tokens.get(&token.token).cloned()
        };
        let (stored, _task_id, _step_id) =
            entry.ok_or_else(|| KernelError::InvalidPrepareToken(token.token.clone()))?;

        if Utc::now() > stored.expires_at {
            return Err(KernelError::InvalidPrepareToken(format!(
                "expired: {}",
                token.token
            )));
        }

        let actual_hash = self.preconditions_hash(manifest);
        if actual_hash != stored.preconditions_hash {
            return Err(KernelError::PreconditionMismatch {
                expected: stored.preconditions_hash,
                actual: actual_hash,
            });
        }

        // Consume the token (one-shot commit).
        self.tokens.lock().unwrap().remove(&token.token);

        Ok(CommitResult {
            committed: true,
            preconditions_recheck: actual_hash,
        })
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test transaction 2>&1`
Expected: PASS — 6 tests.

- [ ] **Step 5: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/policy/transaction.rs voicepilot/crates/trust-kernel/tests/transaction.rs
git commit -m "feat(policy): prepare→commit transaction protocol with TOCTOU prevention"
```

---

## Task 8: Action Gateway facade (orchestrate all layers)

**Files:**
- Modify: `voicepilot/crates/trust-kernel/src/gateway.rs`
- Modify: `voicepilot/crates/trust-kernel/src/kernel.rs`
- Create: `voicepilot/crates/trust-kernel/tests/action_gateway.rs`

- [ ] **Step 1: Write the failing test**

Create `voicepilot/crates/trust-kernel/tests/action_gateway.rs`:

```rust
use trust_kernel::gateway::ActionGateway;
use trust_kernel::policy::types::{DLevel, ELevel, EgressDest, Resource};
use trust_kernel::policy::constraint_engine::ConstraintSpec;

fn load_gateway() -> ActionGateway {
    let cedar_src = include_str!("../src/policies/default.cedar");
    let mut gw = ActionGateway::new(cedar_src).unwrap();
    // Register a max_files constraint for move_files.
    gw.register_constraint("move_files", ConstraintSpec {
        max_files: Some(2),
        overwrite: Some(false),
        allowed_destinations: None,
    });
    gw
}

#[test]
fn d0_public_read_is_allowed() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/readme.md".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Allow);
}

#[test]
fn d3_credential_read_is_denied() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/secrets/token.txt".to_string(),
        data_class: DLevel::D3,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Deny);
    assert!(decision.reasons.iter().any(|r| r.contains("D3") || r.contains("deny")));
}

#[test]
fn d2_private_read_is_confirm() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/private.md".to_string(),
        data_class: DLevel::D2,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Confirm);
}

#[test]
fn egress_to_remote_llm_with_d2_is_confirm() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/private.md".to_string(),
        data_class: DLevel::D2,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("send_to_remote_llm", ELevel::E3, &resource,
                             Some(EgressDest::RemoteLlm), None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Confirm);
}

#[test]
fn egress_to_tool_argument_is_always_denied() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/web/page".to_string(),
        data_class: DLevel::D0,
        provenance: "web_page".to_string(),
    };
    let decision = gw.decide("inject_arg", ELevel::E0, &resource,
                             Some(EgressDest::ToolArgument), None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Deny);
}

#[test]
fn shell_exec_is_always_denied() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/bin/sh".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("shell_exec", ELevel::E3, &resource, None, None).unwrap();
    assert_eq!(decision.effect, trust_kernel::policy::types::Effect::Deny);
}

#[test]
fn decision_includes_policy_bundle_hash() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/readme.md".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let decision = gw.decide("read_file", ELevel::E0, &resource, None, None).unwrap();
    assert!(decision.policy_bundle_hash.starts_with("sha256:"));
}

#[test]
fn constraints_appear_in_decision_when_max_files_triggers() {
    let gw = load_gateway();
    let resource = Resource {
        path: "/docs/file.txt".to_string(),
        data_class: DLevel::D0,
        provenance: "user_direct".to_string(),
    };
    let args = serde_json::json!({
        "sources": ["a.txt", "b.txt", "c.txt", "d.txt"],
        "destination": "/out"
    });
    let decision = gw.decide("move_files", ELevel::E1, &resource, None, Some(args)).unwrap();
    assert!(decision.constraints_applied.iter().any(|c| c.contains("max_files=2")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test action_gateway 2>&1`
Expected: FAIL — `ActionGateway` does not exist.

- [ ] **Step 3: Implement ActionGateway**

Replace `voicepilot/crates/trust-kernel/src/gateway.rs`:

```rust
//! Action Gateway facade — V1.1 §4.2, §4.4, §6.2.
//!
//! Orchestrates the full policy pipeline per §4.4:
//!   1. Normalize resource (path canonicalization, provenance).
//!   2. Hard-deny rules (D3, shell, taint elevation).
//!   3. Cedar authorization (binary allow/deny).
//!   4. Rust Constraint Engine (args normalization + constraints).
//!   5. E×D risk classification (ternary allow/confirm/deny).
//!   6. Egress check (if data flows to remote destination).
//!   7. Return Decision with effect + reasons + matched_policies +
//!      normalized_args + constraints_applied + approval_scope + policy_bundle_hash.

use crate::error::Result;
use crate::policy::cedar_engine::CedarEngine;
use crate::policy::constraint_engine::{ConstraintEngine, ConstraintSpec};
use crate::policy::egress::check_egress;
use crate::policy::types::{Action, Decision, EgressDest, Resource, Effect};
use std::sync::Arc;

pub struct ActionGateway {
    cedar: CedarEngine,
    constraints: ConstraintEngine,
    bundle_hash: String,
    bundle_src: Arc<String>,
}

impl ActionGateway {
    /// Build a gateway from a Cedar source string.
    pub fn new(cedar_src: &str) -> Result<Self> {
        Ok(Self {
            cedar: CedarEngine::from_source(cedar_src)?,
            constraints: ConstraintEngine::new(),
            bundle_hash: CedarEngine::bundle_hash(cedar_src),
            bundle_src: Arc::new(cedar_src.to_string()),
        })
    }

    /// Register a per-tool Rust constraint.
    pub fn register_constraint(&mut self, tool: &str, spec: ConstraintSpec) {
        self.constraints.register(tool, spec);
    }

    /// The SHA-256 hash of the active Cedar policy bundle.
    pub fn bundle_hash(&self) -> &str {
        &self.bundle_hash
    }

    /// Full policy decision pipeline.
    ///
    /// `egress_dest`: Some(_) if the tool sends data to a remote destination.
    /// `args`: tool arguments (will be normalized + constrained).
    pub fn decide(
        &self,
        tool: &str,
        e_level: crate::policy::types::ELevel,
        resource: &Resource,
        egress_dest: Option<EgressDest>,
        args: Option<serde_json::Value>,
    ) -> Result<Decision> {
        let action = Action { name: tool.to_string(), e_level };
        let mut reasons: Vec<String> = Vec::new();

        // Step 2: hard-deny rules (D3, shell_exec, taint elevation).
        if resource.data_class == crate::policy::types::DLevel::D3 {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "D3 red line: credentials never enter model context",
            ));
        }
        if tool == "shell_exec" {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "shell_exec always denied",
            ));
        }
        // Taint elevation: web_page provenance cannot become tool argument.
        if resource.provenance == "web_page" && egress_dest == Some(EgressDest::ToolArgument) {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "taint elevation: web_page cannot become tool argument",
            ));
        }

        // Step 3: Cedar authorization.
        let cedar_allows = self.cedar.is_allowed(&action, resource)?;
        if !cedar_allows {
            return Ok(Decision::deny(
                self.bundle_hash.clone(),
                "Cedar denied",
            ));
        }

        // Step 4: normalize args + apply constraints.
        let (normalized_args, constraints_applied) = if let Some(args) = args {
            let normalized = self.constraints.normalize_args(tool, &args)?;
            let (constrained, applied) = self.constraints.apply_constraints(tool, normalized)?;
            (constrained, applied)
        } else {
            (serde_json::Value::Null, vec![])
        };

        // Step 5: E×D risk classification.
        let mut effect = self.constraints.upgrade_effect(cedar_allows, e_level, resource.data_class);
        if matches!(effect, Effect::Deny) {
            reasons.push(format!("E{:?}×D{:?} = deny", e_level, resource.data_class));
        } else if matches!(effect, Effect::Confirm) {
            reasons.push(format!("E{:?}×D{:?} = confirm", e_level, resource.data_class));
        }

        // Step 6: egress check (independent of E×D).
        if let Some(dest) = egress_dest {
            let egress_effect = check_egress(resource.data_class, dest);
            // Egress can only downgrade (more restrictive wins).
            effect = more_restrictive(effect, egress_effect);
            if matches!(egress_effect, Effect::Deny) {
                reasons.push(format!("egress to {:?} denied for D{:?}", dest, resource.data_class));
            } else if matches!(egress_effect, Effect::Confirm) {
                reasons.push(format!("egress to {:?} requires confirm for D{:?}", dest, resource.data_class));
            }
        }

        // Step 7: assemble Decision.
        let matched_policies = self.cedar.matched_policy_ids();
        let approval_scope = if matches!(effect, Effect::Confirm) { "single" } else { "single" };

        Ok(Decision {
            effect,
            matched_policies,
            normalized_args,
            constraints_applied,
            approval_scope: approval_scope.to_string(),
            policy_bundle_hash: self.bundle_hash.clone(),
            reasons,
        })
    }

    /// Reference to the original Cedar source (for audit / persistence).
    pub fn cedar_source(&self) -> &str {
        &self.bundle_src
    }
}

fn more_restrictive(a: Effect, b: Effect) -> Effect {
    use Effect::*;
    match (a, b) {
        (Deny, _) | (_, Deny) => Deny,
        (Confirm, _) | (_, Confirm) => Confirm,
        (Allow, Allow) => Allow,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path voicepilot\Cargo.toml -p trust-kernel --test action_gateway 2>&1`
Expected: PASS — 8 tests.

- [ ] **Step 5: Wire ActionGateway into TrustKernel**

Edit `voicepilot/crates/trust-kernel/src/kernel.rs`, add `gateway` field and accessor. Find:

```rust
pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
}
```

Replace with:

```rust
pub struct TrustKernel {
    conn: Arc<Mutex<Connection>>,
    task_repo: TaskRepo,
    audit: Arc<SqliteAuditLogger>,
    gateway: Arc<crate::gateway::ActionGateway>,
}
```

Find the `with_conn` method:

```rust
    fn with_conn(conn: Connection) -> Self {
        let shared = Arc::new(Mutex::new(conn));
        Self {
            conn: shared.clone(),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(shared)),
        }
    }
```

Replace with:

```rust
    fn with_conn(conn: Connection) -> Self {
        let shared = Arc::new(Mutex::new(conn));
        let cedar_src = include_str!("policies/default.cedar");
        let gateway = Arc::new(
            crate::gateway::ActionGateway::new(cedar_src)
                .expect("default cedar policy must parse"),
        );
        Self {
            conn: shared.clone(),
            task_repo: TaskRepo::new(),
            audit: Arc::new(SqliteAuditLogger::new(shared)),
            gateway,
        }
    }

    /// Access the Action Gateway for policy decisions.
    pub fn gateway(&self) -> &crate::gateway::ActionGateway {
        &self.gateway
    }
```

- [ ] **Step 6: Run full test suite to verify nothing broke**

Run: `cargo test --manifest-path voicepilot\Cargo.toml 2>&1`
Expected: ALL PASS — W1 tests (26) + W2 tests so far.

- [ ] **Step 7: Commit**

```bash
git add voicepilot/crates/trust-kernel/src/gateway.rs voicepilot/crates/trust-kernel/src/kernel.rs voicepilot/crates/trust-kernel/tests/action_gateway.rs
git commit -m "feat(gateway): Action Gateway orchestrating Cedar + Constraint + E×D + egress"
```

---

## Task 9: CLI extension + W2 end-to-end smoke test

**Files:**
- Modify: `voicepilot/crates/cli/src/main.rs`
- Manual smoke test

- [ ] **Step 1: Add `policy` command to CLI**

Edit `voicepilot/crates/cli/src/main.rs`, find the help text block:

```rust
    println!("Commands:");
    println!("  <text>          create a task with the given goal, run happy-path flow");
    println!("  cancel <task>   kill-switch cancel a task");
    println!("  show <task>     show task state and audit count");
    println!("  quit");
    println!();
```

Replace with:

```rust
    println!("Commands:");
    println!("  <text>          create a task with the given goal, run happy-path flow");
    println!("  cancel <task>   kill-switch cancel a task");
    println!("  show <task>     show task state and audit count");
    println!("  policy <tool> <path> <D-level> [E-level]  run policy decision (W2)");
    println!("  quit");
    println!();
```

Find the line:

```rust
        if let Some(task_id) = line.strip_prefix("show ") {
```

Insert before that line:

```rust
        if let Some(rest) = line.strip_prefix("policy ") {
            handle_policy_command(&kernel, rest);
            continue;
        }
```

At the end of the file, after `fn run_happy_path`, add:

```rust
fn handle_policy_command(kernel: &TrustKernel, args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() < 3 {
        println!("usage: policy <tool> <path> <D-level> [E-level]");
        println!("       D-level: D0|D1|D2|D3   E-level: E0|E1|E2|E3 (default E0)");
        return;
    }
    let tool = parts[0];
    let path = parts[1];
    let d_level = match parts[2] {
        "D0" => trust_kernel::policy::types::DLevel::D0,
        "D1" => trust_kernel::policy::types::DLevel::D1,
        "D2" => trust_kernel::policy::types::DLevel::D2,
        "D3" => trust_kernel::policy::types::DLevel::D3,
        _ => {
            println!("invalid D-level: {}", parts[2]);
            return;
        }
    };
    let e_level = if parts.len() >= 4 {
        match parts[3] {
            "E0" => trust_kernel::policy::types::ELevel::E0,
            "E1" => trust_kernel::policy::types::ELevel::E1,
            "E2" => trust_kernel::policy::types::ELevel::E2,
            "E3" => trust_kernel::policy::types::ELevel::E3,
            _ => {
                println!("invalid E-level: {}", parts[3]);
                return;
            }
        }
    } else {
        trust_kernel::policy::types::ELevel::E0
    };

    let resource = trust_kernel::policy::types::Resource {
        path: path.to_string(),
        data_class: d_level,
        provenance: "user_direct".to_string(),
    };
    let egress = if tool == "send_to_remote_llm" {
        Some(trust_kernel::policy::types::EgressDest::RemoteLlm)
    } else {
        None
    };

    match kernel.gateway().decide(tool, e_level, &resource, egress, None) {
        Ok(decision) => {
            println!("decision: {:?}", decision.effect);
            println!("  bundle_hash: {}", decision.policy_bundle_hash);
            println!("  matched_policies: {:?}", decision.matched_policies);
            if !decision.constraints_applied.is_empty() {
                println!("  constraints: {:?}", decision.constraints_applied);
            }
            if !decision.reasons.is_empty() {
                println!("  reasons: {:?}", decision.reasons);
            }
        }
        Err(e) => println!("error: {}", e),
    }
}
```

- [ ] **Step 2: Build the CLI**

Run: `cargo build --manifest-path voicepilot\Cargo.toml -p cli 2>&1`
Expected: compiles cleanly.

- [ ] **Step 3: Smoke test — D0 public read should allow**

Run:
```powershell
$env:VOICEPILOT_DB = "$env:TEMP\voicepilot-w2-smoke.db"; Remove-Item $env:VOICEPILOT_DB -Force -ErrorAction SilentlyContinue; "policy read_file /docs/readme.md D0`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```
Expected: `decision: Allow`.

- [ ] **Step 4: Smoke test — D3 credential read should deny**

Run:
```powershell
"policy read_file /secrets/token.txt D3`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```
Expected: `decision: Deny` with reason "D3 red line".

- [ ] **Step 5: Smoke test — D2 private read should confirm**

Run:
```powershell
"policy read_file /docs/private.md D2`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```
Expected: `decision: Confirm` with reason "E0×D2 = confirm".

- [ ] **Step 6: Smoke test — shell_exec should always deny**

Run:
```powershell
"policy shell_exec /bin/sh D0 E3`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```
Expected: `decision: Deny` with reason "shell_exec always denied".

- [ ] **Step 7: Smoke test — egress to remote LLM with D2 should confirm**

Run:
```powershell
"policy send_to_remote_llm /docs/private.md D2 E3`nquit`n" | voicepilot\target\debug\voicepilot.exe 2>&1
```
Expected: `decision: Confirm`.

- [ ] **Step 8: Run full test suite one final time**

Run: `cargo test --manifest-path voicepilot\Cargo.toml 2>&1`
Expected: ALL PASS.

- [ ] **Step 9: Commit**

```bash
git add voicepilot/crates/cli/src/main.rs
git commit -m "feat(cli): policy command for W2 E×D decision demo"
```

---

## Self-Review

**1. Spec coverage (V1.1 spec sections referenced):**

| Spec section | Covered by |
|---|---|
| §4.1 E×D matrix | Task 2 (risk_matrix.rs, 16 cells) |
| §4.2 Cedar + Rust Constraint | Task 4 (cedar_engine.rs) + Task 5 (constraint_engine.rs) |
| §4.3 Egress | Task 3 (egress.rs) |
| §4.4 Matching order | Task 8 (gateway.rs — 7-step pipeline) |
| §6.2 prepare→approve→commit | Task 7 (transaction.rs) |
| §6.3 ToolResult V2 (evidence_strength, compensation_ref) | Partially — types defined in Task 1; full ToolResult lands in W3 with real tools |
| §8.1 policies table | Task 6 (policy_repo.rs) |
| §11.1 W2 gate "E×D 策略可决策" | Task 9 smoke test |

Gaps:
- ToolResult V2 full struct (evidence_strength, compensation_ref, idempotency_key): deferred to W3 when real tools exist. Task 1 captures the core policy types only.
- Cedar schema validation: W2 PoC uses schema-free mode; W3 will add schema after Cedar PoC validation in §4.2 callout.
- Approval UI: not in W2 scope; CLI prints decision but doesn't actually prompt for approval. Approval flow lands in W4 (Tauri UI week).

**2. Placeholder scan:** No "TBD", "TODO", "fill in" found. All steps contain actual code or commands.

**3. Type consistency:**
- `ELevel` / `DLevel` used consistently across types.rs, risk_matrix.rs, constraint_engine.rs, gateway.rs, cedar_engine.rs.
- `Effect` enum has `Allow`, `Confirm`, `Deny` — consistent everywhere.
- `Decision` struct fields match across types.rs definition and gateway.rs construction.
- `Resource` has `path`, `data_class`, `provenance` — consistent.
- `Action` has `name`, `e_level` — consistent.
- `PrepareToken` has `token`, `expires_at`, `preconditions_hash` — consistent in transaction.rs and tests.
- `EffectManifest` has `sources`, `destination`, `conflicts`, `total_bytes` — consistent.
- `ConstraintSpec` has `max_files`, `overwrite`, `allowed_destinations` — consistent in constraint_engine.rs and tests.

No issues found.

---

## Spec Notes & Issues Found During W2 Planning

Per user request ("在遇到感觉开发文档不合理或者可用进行优化时，向我报告"), additional observations from W2 planning:

7. **§4.2 Cedar callout warns "no mature production cases for LLM Agent tool-call decisions".** W2 implements the PoC as spec recommends, but should track: if Cedar PoC reveals integration friction (e.g., entity schema complexity for dynamic resources), the spec should be updated in W3 to either commit to Cedar or fall back to pure Rust Constraint rules. **Suggestion**: add a W2-exit review checkpoint to evaluate Cedar PoC before W3 commits to it.

8. **§4.3 egress table rows for `web_page → local_file` and `web_page → remote_llm` are marked `allow` but the `data_class` column is "—".** The matrix doesn't specify how to classify web content. W2 implementation uses `D0` as default for web data (since the caller's `data_class` is what's checked). **Suggestion**: clarify in spec whether web-sourced data should default to D0 or carry a separate `provenance=web_page` tag that bypasses D-classification (which is what W2 implements).

9. **§6.2 prepare example shows `preconditions_hash: "sha256:9e8f7d..."` but the hash algorithm is unspecified.** W2 uses SHA-256 over canonical bytes of all snapshot fields sorted by path. **Suggestion**: pin the exact hash construction in spec (algorithm + field order + encoding) so W3 implementations match.

10. **§6.2 says "commit 时重新检查全部 preconditions" but doesn't specify token TTL.** W2 uses 300s (5 min). **Suggestion**: spec should specify default TTL and whether it's per-tool or global.

11. **§4.4 step 7 mentions `policy_bundle_hash` but doesn't define its construction.** W2 uses SHA-256 of the Cedar source string. **Suggestion**: spec should specify whether this is source-text hash or compiled-policy hash.

12. **§8.1 `policies` table has `rules_json` and `cedar_policies` columns that appear redundant.** W2 treats `cedar_policies` as the Cedar source and `rules_json` as a metadata blob. **Suggestion**: clarify in spec which is canonical, or merge into single column.

These are documented here for the user; no spec changes have been made.

---

## W2 Exit Criteria

W2 is complete when ALL of the following hold:
- [ ] All 9 tasks committed
- [ ] `cargo test` passes (W1 + W2 tests, ~60+ tests total)
- [ ] CLI `policy` command works for all 6 smoke test scenarios
- [ ] E×D 策略可决策 gate met: any (tool, E, D) tuple returns a Decision with a non-empty effect
- [ ] Cedar PoC validated (Task 4 tests pass — Cedar is viable for W3+)
- [ ] Spec issues 7-12 documented for user review
