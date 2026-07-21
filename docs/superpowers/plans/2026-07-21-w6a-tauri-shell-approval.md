# VoicePilot W6a: Tauri UI Shell + Approval Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Tauri 2 desktop UI shell to VoicePilot — bridge `trust-kernel` via Tauri commands, implement `TauriApprover` (IPC-based Approver trait impl), build the Approval window (React + TypeScript) that displays `EffectManifest` and collects y/N decisions, and validate end-to-end with a smoke test. W6b will add Main Chat, Settings, Audit Viewer, and Trust Center windows.

**Architecture:** New `voicepilot/crates/ui` crate (Tauri v2 application, opt-in via `tauri` workspace feature). Tauri Rust backend depends on `trust-kernel` and exposes typed Tauri commands. Frontend lives in `crates/ui/web/` (Vite + React + TypeScript). The Approval window uses Tauri events (`tauri::Emitter`/`tauri::Listener`) to bridge the synchronous `Approver::prompt` call (Rust side) with the async React approval modal (webview side). Channel-based synchronization (`tokio::sync::oneshot`) ensures the prepare→approve→commit pipeline stays blocked until the user clicks y/N.

**Spec alignment (V1.1.2 §8.2 + §8.3):**
- §8.2 window split: W6a implements **Main** (minimal — just an "organize" button + status display) + **Approval** (full effect_manifest + Diff Preview stub + y/N buttons). Audit/Settings/Trust Center deferred to W6b.
- §8.2 IPC hardening red lines enforced:
  - WebView never directly accesses filesystem (all FS ops go through `FilesystemTool` Rust adapter)
  - UI cannot invoke MCP directly (all calls route through `ActionGateway`/`FilesOrganizeSkill`)
  - Approval Window accepts only one-time `approval_request_id` (UUID v4 per prompt, consumed on decision)
- §8.3 Approval Modal features (W6a scope): effect_manifest display, risk level (E×D), compensability, y/N buttons. **Diff Preview deferred to W6b** (needs file content reader; not in W6a minimal viable shell).

**Feature gating (opt-in, consistent with W5 voice decision):**
- `default = []` — pure Rust, no Tauri dependency, W1-W5 tests + CLI still work (196 + voice opt-in tests)
- `tauri = ["dep:tauri", "dep:tauri-build", "dep:tokio", "trust-kernel/voice"]` — opt-in, enables UI crate
- The `ui` crate is **excluded from the default workspace members** via `default-members = ["crates/trust-kernel", "crates/cli"]` so `cargo test` (default) never touches Tauri
- All Tauri commands gate voice-dependent operations behind `#[cfg(feature = "voice")]` (voice listen command etc.); pure-IPC commands (route_text, organize_files) work without voice feature

**Tech Stack:**
- Tauri 2.x (stable, Oct 2024 release)
- React 18 + TypeScript 5
- Vite 5 (build tooling, HMR)
- `@tauri-apps/api` 2.x (frontend IPC bindings)
- `@tauri-apps/plugin-shell` (optional, deferred — not in W6a)
- `tokio` 1.x (Rust async runtime for oneshot channels)
- `tauri::Manager` + `tauri::Emitter` + `tauri::Listener` (event system)

**Build prerequisites (only when `--features tauri` is used):**
- Node 22+ (verified: v22.16.0)
- npm 10+ (verified: 10.9.4)
- Rust 1.96+ (verified: cargo 1.96.1)
- WebView2 Runtime (pre-installed on Windows 11; Windows 10 22H2 may need manual install)
- **If Node unavailable:** Tauri code can still be written and committed; verify with `cargo check` (default, no tauri) for W1-W5 no-regression. Tauri-specific compilation/tests require Node + npm install in `crates/ui/web/`.

**Out of scope (deferred to W6b):**
- Main Chat window (voice input button, real-time transcription, route outcome feedback)
- Settings panel (Whisper model path config, allowed_paths editor, mic device picker, VAD threshold)
- Audit Viewer (read-only audit_logs query + display)
- Trust Center (MCP server list, egress policy, one-click disable)
- Skills Manager (saved Skills list, success rate, latency)
- Diff Preview in Approval Modal (needs file content reader)
- Kill Switch Bar (always-on top bar)
- VAD-based auto-stop (replaces W5 PoC 5s timeout, issue #45)
- Model auto-download (issue #46)
- Tauri packaging (NSIS installer, code signing — W7+)
- Streaming partial transcripts (issue #47)
- Wake word detection (issue #48)

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `voicepilot/crates/ui/Cargo.toml` | Tauri app crate manifest, `tauri` feature gate, deps on `trust-kernel` + `tauri` + `tokio` |
| `voicepilot/crates/ui/build.rs` | Tauri build script (calls `tauri_build::build()`) |
| `voicepilot/crates/ui/tauri.conf.json` | Tauri config (productName, windows list, security CSP, bundle settings) |
| `voicepilot/crates/ui/src/main.rs` | Tauri app entry (`tauri::Builder` + command registration + plugin setup) |
| `voicepilot/crates/ui/src/lib.rs` | Module declarations + re-exports for tests |
| `voicepilot/crates/ui/src/approver.rs` | `TauriApprover` — impl `Approver` trait, bridges to webview via events + oneshot channel |
| `voicepilot/crates/ui/src/commands.rs` | Tauri `#[command]` functions: `route_text`, `organize_files`, `list_voice_models`, `submit_approval` |
| `voicepilot/crates/ui/src/state.rs` | `AppState` — holds `Arc<TrustKernel>` + pending approval request registry |
| `voicepilot/crates/ui/src/error.rs` | `UiError` enum → Tauri `Result<T, String>` serialization |
| `voicepilot/crates/ui/tests/approver_unit.rs` | Unit tests for `TauriApprover` (mock event emitter, no real webview) |
| `voicepilot/crates/ui/tests/commands_unit.rs` | Unit tests for Tauri commands (route_text, organize_files) |
| `voicepilot/crates/ui/tests/w6a_e2e_smoke.rs` | End-to-end smoke: invoke `organize_files` command with `AutoApprover` injection, verify audit chain |
| `voicepilot/crates/ui/web/package.json` | npm deps: React, TypeScript, Vite, @tauri-apps/api |
| `voicepilot/crates/ui/web/vite.config.ts` | Vite config (Tauri-friendly base + port) |
| `voicepilot/crates/ui/web/tsconfig.json` | TypeScript strict mode config |
| `voicepilot/crates/ui/web/index.html` | Vite entry HTML |
| `voicepilot/crates/ui/web/src/main.tsx` | React app entry |
| `voicepilot/crates/ui/web/src/App.tsx` | Root component (tab switch: Main / Approval) |
| `voicepilot/crates/ui/web/src/components/MainView.tsx` | Minimal Main window (organize form + status display) |
| `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx` | Approval window (EffectManifest display + y/N buttons + submit) |
| `voicepilot/crates/ui/web/src/api.ts` | Tauri `invoke` wrappers + event listeners |
| `voicepilot/crates/ui/web/src/types.ts` | TypeScript types mirroring Rust DTOs (EffectManifest, RouteOutcome, ApprovalDecision) |

### Modified files

| File | Change |
|---|---|
| `voicepilot/Cargo.toml` | Add `tauri`, `tauri-build`, `tokio` to workspace deps; add `default-members` excluding `crates/ui`; add `ui` to members list |
| `voicepilot/crates/trust-kernel/Cargo.toml` | No change (already exposes needed APIs via `kernel.rs`) |
| `voicepilot/crates/trust-kernel/src/lib.rs` | No change |
| `voicepilot/crates/cli/Cargo.toml` | No change (CLI remains a separate headless entry point) |
| `voicepilot/crates/trust-kernel/src/approval/approver.rs` | Add `Send + Sync` bound clarification in doc comment (already present, just document why) |

---

## Task 1: Add `ui` crate to workspace + Tauri feature gate

**Files:**
- Modify: `voicepilot/Cargo.toml`
- Create: `voicepilot/crates/ui/Cargo.toml`
- Create: `voicepilot/crates/ui/build.rs`
- Create: `voicepilot/crates/ui/src/lib.rs`

- [ ] **Step 1: Update root Cargo.toml — add `default-members` + new workspace deps + `ui` member**

Edit `voicepilot/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/trust-kernel",
    "crates/cli",
    "crates/ui",
]
default-members = ["crates/trust-kernel", "crates/cli"]

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
cedar-policy = "4.11.2"
walkdir = "2.5"
whisper-rs = { version = "0.13" }
cpal = { version = "0.15" }
hound = { version = "3.5" }
tauri = { version = "2", features = ["wry"] }
tauri-build = { version = "2" }
tokio = { version = "1", features = ["sync", "rt", "macros"] }
```

- [ ] **Step 2: Create `voicepilot/crates/ui/Cargo.toml`**

```toml
[package]
name = "voicepilot-ui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true

[lib]
name = "voicepilot_ui"
path = "src/lib.rs"

[[bin]]
name = "voicepilot-ui"
path = "src/main.rs"
required-features = ["tauri"]

[build-dependencies]
tauri-build = { workspace = true, optional = true }

[dependencies]
trust-kernel = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true, optional = true }
tauri = { workspace = true, optional = true }

[features]
default = []
tauri = ["dep:tauri", "dep:tauri-build", "dep:tokio", "trust-kernel/voice"]
```

- [ ] **Step 3: Create `voicepilot/crates/ui/build.rs`**

```rust
fn main() {
    #[cfg(feature = "tauri")]
    tauri_build::build()
}
```

- [ ] **Step 4: Create `voicepilot/crates/ui/src/lib.rs` (skeleton)**

```rust
//! VoicePilot UI crate — Tauri 2 desktop application.
//!
//! W6a scope:
//! - Tauri command bridge to `trust-kernel`
//! - `TauriApprover` impl (IPC-based Approver)
//! - Approval window (React + TypeScript)
//! - End-to-end smoke test
//!
//! Feature gating: `default = []` keeps the crate pure-Rust (compiles without
//! Tauri). `tauri` feature enables the desktop app binary + voice feature.

pub mod error;
pub mod state;

#[cfg(feature = "tauri")]
pub mod approver;

#[cfg(feature = "tauri")]
pub mod commands;

#[cfg(feature = "tauri")]
pub mod app;

pub use error::UiError;
pub use state::AppState;
```

- [ ] **Step 5: Create empty `voicepilot/crates/ui/src/error.rs` + `state.rs` skeletons**

`error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UiError {
    #[error("kernel error: {0}")]
    Kernel(#[from] trust_kernel::error::KernelError),
    #[error("tauri error: {0}")]
    Tauri(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("approval timed out")]
    ApprovalTimeout,
    #[error("approval request not found: {0}")]
    ApprovalNotFound(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl From<UiError> for String {
    fn from(e: UiError) -> String {
        e.to_string()
    }
}

pub type UiResult<T> = Result<T, UiError>;
```

`state.rs`:
```rust
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
}

impl AppState {
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }
}
```

- [ ] **Step 6: Verify default workspace still compiles**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml
# Expect: no errors, no warnings; ui crate is in members but has only stub lib.rs
cargo test --manifest-path voicepilot\Cargo.toml
# Expect: 196 passing (W1-W4) + voice opt-in tests still skipped
```

- [ ] **Step 7: Verify Tauri feature compiles**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# Expect: compiles (Tauri deps pulled, but no commands yet — only state + error modules)
```

- [ ] **Step 8: Commit**

```powershell
git add voicepilot/Cargo.toml voicepilot/crates/ui/
git commit -m "Task 1: ui crate scaffolding with tauri feature gate (V1.1 §8.2)"
```

---

## Task 2: `TauriApprover` skeleton — channel-based Approver impl

**Files:**
- Create: `voicepilot/crates/ui/src/approver.rs`
- Create: `voicepilot/crates/ui/tests/approver_unit.rs`

**Design:**
The `Approver` trait is synchronous (`fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision`). Tauri events are async. To bridge:
1. `TauriApprover` holds an `AppHandle` (cloned, cheap — it's an `Arc` internally)
2. On `prompt()`:
   - Generate `approval_request_id` (UUID v4)
   - Create a `tokio::sync::oneshot::channel::<ApprovalDecision>()`
   - Store the `Sender` in a `Mutex<HashMap<String, Sender>>` on `AppState`
   - Emit `approval-request` event to the webview with `{approval_request_id, manifest}`
   - Block on `Receiver::blocking_recv()` with a timeout (default 300s, configurable)
   - If timeout → return `Deny` (safer default per CliApprover convention)
   - If received → return the decision
3. The webview's "Approve"/"Deny" button calls `submit_approval` Tauri command, which looks up the `Sender` and sends the decision.

- [ ] **Step 1: Write the test first (red)**

`voicepilot/crates/ui/tests/approver_unit.rs`:

```rust
#![cfg(feature = "tauri")]

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use trust_kernel::approval::approver::Approver;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use voicepilot_ui::approver::{ApprovalRegistry, TauriApprover};
use voicepilot_ui::state::AppState;

fn dummy_manifest() -> EffectManifest {
    EffectManifest {
        sources: vec![],
        destination: "D:/test/dest".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    }
}

#[test]
fn approval_registry_resolves_submitted_decision() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let approval_id = registry.create_request(&manifest);
    
    // Simulate webview submit_approval command
    let handle = registry.get_sender(&approval_id).expect("sender exists");
    
    // Spawn a thread that sends the decision after 50ms
    let sender = handle;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = sender.send(ApprovalDecision::Allow);
    });
    
    let decision = registry.wait_for_decision(&approval_id, Duration::from_secs(5));
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn approval_registry_times_out_to_deny() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let approval_id = registry.create_request(&manifest);
    
    // Never send a decision — should time out
    let decision = registry.wait_for_decision(&approval_id, Duration::from_millis(100));
    assert_eq!(decision, ApprovalDecision::Deny);
}

#[test]
fn approval_registry_consumes_request_after_decision() {
    let registry = ApprovalRegistry::new();
    let manifest = dummy_manifest();
    let approval_id = registry.create_request(&manifest);
    
    let sender = registry.get_sender(&approval_id).expect("exists");
    let _ = sender.send(ApprovalDecision::Deny);
    
    // Wait for decision
    let _ = registry.wait_for_decision(&approval_id, Duration::from_secs(1));
    
    // Second lookup should fail (one-shot)
    assert!(registry.get_sender(&approval_id).is_none());
}
```

- [ ] **Step 2: Run tests (red)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test approver_unit
# Expect: compile error — ApprovalRegistry does not exist yet
```

- [ ] **Step 3: Implement `ApprovalRegistry` + `TauriApprover` (green)**

Design notes:
- `ApprovalRegistry` stores `oneshot::Sender<ApprovalDecision>` keyed by `approval_request_id`.
- `create_request` returns `(approval_id, receiver)` so the caller (TauriApprover::prompt) holds the receiver and blocks on it. The `take_sender` method (called by `submit_approval` Tauri command) removes the sender from the map and returns it for delivery — one-shot per §8.2.
- `wait_for_decision` blocks on the receiver with timeout using a dedicated current-thread tokio runtime (avoids deadlock if called inside an existing Tauri async context). On timeout or sender-dropped: returns `Deny` (safer default per CliApprover convention).

`voicepilot/crates/ui/src/approver.rs`:

```rust
//! TauriApprover — bridges synchronous `Approver::prompt` to async Tauri events.
//!
//! V1.1 §8.2 + §6.2: Approval window must accept a one-time approval_request_id
//! and consume it on decision. This module implements the registry that holds
//! pending approval senders + the Approver trait impl that blocks on recv.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use trust_kernel::approval::approver::Approver;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::policy::transaction::EffectManifest;
use uuid::Uuid;

const DEFAULT_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct ApprovalRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new pending approval request.
    /// Returns (approval_request_id, receiver) — caller blocks on receiver.
    /// The manifest is passed so the production TauriApprover can emit it
    /// to the webview alongside the approval_request_id.
    pub fn create_request(
        &self,
        _manifest: &EffectManifest,
    ) -> (String, oneshot::Receiver<ApprovalDecision>) {
        let approval_id = format!("apr_{}", Uuid::new_v4());
        let (tx, rx) = oneshot::channel::<ApprovalDecision>();
        self.senders
            .lock()
            .unwrap()
            .insert(approval_id.clone(), tx);
        (approval_id, rx)
    }

    /// Look up + remove the sender for the given approval_request_id.
    /// Called by the `submit_approval` Tauri command.
    /// Returns None if the request was already consumed or expired.
    pub fn take_sender(&self, approval_id: &str) -> Option<oneshot::Sender<ApprovalDecision>> {
        self.senders.lock().unwrap().remove(approval_id)
    }

    /// Block until a decision arrives or timeout expires.
    /// On timeout or sender-dropped: returns Deny (safer default).
    pub fn wait_for_decision(
        &self,
        rx: oneshot::Receiver<ApprovalDecision>,
        timeout: Duration,
    ) -> ApprovalDecision {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(decision)) => decision,
                Ok(Err(_)) => ApprovalDecision::Deny, // sender dropped
                Err(_) => ApprovalDecision::Deny,     // timeout
            }
        })
    }
}

impl Default for ApprovalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TauriApprover {
    registry: ApprovalRegistry,
}

impl TauriApprover {
    pub fn new(registry: ApprovalRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }
}

impl Approver for TauriApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        let (approval_id, rx) = self.registry.create_request(manifest);
        // In production (Task 6): emit "approval-request" event to webview here.
        // For unit tests: caller directly invokes `registry.take_sender(id).send(decision)`.
        let _ = approval_id; // emitted by TauriApprover::prompt in Task 6
        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }
}
```

- [ ] **Step 4: Run tests (green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test approver_unit
# Expect: 4 passing
```

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/src/approver.rs voicepilot/crates/ui/tests/approver_unit.rs
git commit -m "Task 2: TauriApprover with oneshot channel + 5min timeout (V1.1 §8.2 one-shot approval_request_id)"
```

---

## Task 3: Tauri command `route_text` — bridge to SkillRouter

**Files:**
- Create: `voicepilot/crates/ui/src/commands.rs`
- Create: `voicepilot/crates/ui/tests/commands_unit.rs`

- [ ] **Step 1: Write the test first (red)**

`voicepilot/crates/ui/tests/commands_unit.rs`:

```rust
#![cfg(feature = "tauri")]

use serde_json::json;
use trust_kernel::kernel::TrustKernel;
use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::state::AppState;

#[test]
fn route_text_returns_routed_when_skill_keyword_matches() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "organize my downloads").unwrap();
    assert!(matches!(result, RouteTextResult::Routed { ref skill_id } if skill_id == "files.organize"));
}

#[test]
fn route_text_returns_unmatched_when_no_keyword() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "hello world").unwrap();
    assert!(matches!(result, RouteTextResult::Unmatched { .. }));
}

#[test]
fn route_text_returns_empty_for_whitespace() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "   ").unwrap();
    assert!(matches!(result, RouteTextResult::Empty));
}
```

- [ ] **Step 2: Run tests (red)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# Expect: compile error — commands module + route_text don't exist
```

- [ ] **Step 3: Implement `commands.rs` (green)**

```rust
//! Tauri commands — V1.1 §8.2 IPC bridge between webview and trust-kernel.
//!
//! All commands are `#[cfg(feature = "tauri")]`-gated. They take `&AppState`
//! (managed by Tauri) and return `Result<T, String>` for webview consumption.

use serde::{Deserialize, Serialize};
use trust_kernel::skills::router::RouteDecision;
use trust_kernel::skills::manifest::files_organize_manifest;
use crate::error::{UiError, UiResult};
use crate::state::AppState;

/// Mirrors `trust_kernel::voice::router_bridge::RouteOutcome` but with
/// Serialize for Tauri command return type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteTextResult {
    Routed { skill_id: String },
    Unmatched { text: String },
    Empty,
}

/// Route transcribed text (or arbitrary text input) through SkillRouter.
/// V1.1 §5.1 — pure keyword matching (W7 will add LLM Planner fallback).
pub fn route_text(state: &AppState, text: &str) -> UiResult<RouteTextResult> {
    use trust_kernel::skills::router::SkillRouter;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(RouteTextResult::Empty);
    }
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    match router.route(trimmed) {
        RouteDecision::Skill(manifest) => Ok(RouteTextResult::Routed {
            skill_id: manifest.id,
        }),
        RouteDecision::Planner => Ok(RouteTextResult::Unmatched {
            text: trimmed.to_string(),
        }),
    }
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn route_text_command(
    state: tauri::State<'_, AppState>,
    text: String,
) -> Result<RouteTextResult, String> {
    route_text(&state, &text).map_err(Into::into)
}
```

- [ ] **Step 4: Update `lib.rs` to export commands module**

Already exported via `pub mod commands;` in Task 1.

- [ ] **Step 5: Run tests (green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# Expect: 3 passing
```

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "Task 3: route_text Tauri command bridges SkillRouter (V1.1 §5.1, §8.2)"
```

---

## Task 4: Tauri command `organize_files` — full Skill pipeline with TauriApprover

**Files:**
- Modify: `voicepilot/crates/ui/src/commands.rs`
- Modify: `voicepilot/crates/ui/src/state.rs`
- Modify: `voicepilot/crates/ui/tests/commands_unit.rs`

**Design:**
- `AppState` gains `approval_registry: ApprovalRegistry` field
- `organize_files` command:
  1. Create task + step in kernel
  2. Build `FilesOrganizeInput`
  3. Construct `TauriApprover` from the registry
  4. Call `FilesOrganizeSkill::execute(kernel, input, approver)`
  5. Return `OrganizeResult { tool_result, moved_paths }` to webview

- [ ] **Step 1: Write the test first (red)**

Append to `commands_unit.rs`:

```rust
use std::path::PathBuf;
use tempfile::TempDir;
use trust_kernel::approval::approver::AutoApprover;
use voicepilot_ui::commands::{organize_files, OrganizeInput, OrganizeResult};

#[test]
fn organize_files_with_auto_approver_commits_move() {
    let tmp = TempDir::new().unwrap();
    let src_dir = tmp.path().join("src");
    let dest_dir = tmp.path().join("dest");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&dest_dir).unwrap();
    std::fs::write(src_dir.join("a.txt"), "hello").unwrap();

    let state = AppState::new_in_memory().unwrap();
    let result = organize_files(
        &state,
        &AutoApprover,
        &OrganizeInput {
            task_id: "t-test".to_string(),
            step_id: "s-test".to_string(),
            source: src_dir.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest_dir.to_string_lossy().into_owned(),
        },
    ).unwrap();

    assert!(result.committed);
    assert_eq!(result.moved_paths.len(), 1);
    assert!(dest_dir.join("a.txt").exists());
}
```

- [ ] **Step 2: Run tests (red)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# Expect: compile error — organize_files doesn't exist
```

- [ ] **Step 3: Implement `organize_files` (green)**

Add to `commands.rs`:

```rust
use std::path::PathBuf;
use trust_kernel::approval::approver::Approver;
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::repo::task_repo::TaskRecord;
use trust_kernel::repo::step_repo::StepRecord;
use trust_kernel::state::TaskState;

#[derive(Debug, Clone, Deserialize)]
pub struct OrganizeInput {
    pub task_id: String,
    pub step_id: String,
    pub source: String,
    pub filter: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganizeResult {
    pub committed: bool,
    pub moved_paths: Vec<[String; 2]>, // [[from, to], ...]
    pub evidence_strength: String,
    pub compensation_ref: Option<String>,
    pub error: Option<String>,
}

pub fn organize_files(
    state: &AppState,
    approver: &dyn Approver,
    input: &OrganizeInput,
) -> UiResult<OrganizeResult> {
    // Create task + step if not exists (idempotent for retries)
    if state.kernel.get_task(&input.task_id)?.is_none() {
        state.kernel.create_task(&input.task_id, "organize files")?;
    }
    if state.kernel.get_step(&input.step_id)?.is_none() {
        state.kernel.create_step(&StepRecord::new(
            &input.step_id,
            &input.task_id,
            1,
        ))?;
    }
    
    let skill_input = FilesOrganizeInput {
        task_id: input.task_id.clone(),
        step_id: input.step_id.clone(),
        source: PathBuf::from(&input.source),
        filter: input.filter.clone(),
        destination: PathBuf::from(&input.destination),
    };
    
    let skill = FilesOrganizeSkill::new();
    let execution = skill.execute(&state.kernel, &skill_input, approver)?;
    
    let moved_paths = execution
        .moved_paths
        .into_iter()
        .map(|(from, to)| [from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned()])
        .collect();
    
    Ok(OrganizeResult {
        committed: execution.tool_result.status == trust_kernel::toolresult::ToolStatus::Succeeded,
        moved_paths,
        evidence_strength: execution.tool_result.evidence_strength.as_str().to_string(),
        compensation_ref: execution.tool_result.compensation_ref.clone(),
        error: None,
    })
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn organize_files_command(
    state: tauri::State<'_, AppState>,
    input: OrganizeInput,
) -> Result<OrganizeResult, String> {
    // Construct TauriApprover from the registry on AppState
    let approver = crate::approver::TauriApprover::new(state.approval_registry.clone());
    organize_files(&state, &approver, &input).map_err(Into::into)
}
```

Update `state.rs`:

```rust
use std::sync::Arc;
use trust_kernel::kernel::TrustKernel;
use crate::approver::ApprovalRegistry;

pub struct AppState {
    pub kernel: Arc<TrustKernel>,
    #[cfg(feature = "tauri")]
    pub approval_registry: ApprovalRegistry,
}

impl AppState {
    #[cfg(feature = "tauri")]
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
            approval_registry: ApprovalRegistry::new(),
        }
    }

    #[cfg(not(feature = "tauri"))]
    pub fn new(kernel: TrustKernel) -> Self {
        Self {
            kernel: Arc::new(kernel),
        }
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_in_memory()?;
        Ok(Self::new(kernel))
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let kernel = TrustKernel::open_file(path)?;
        Ok(Self::new(kernel))
    }
}
```

- [ ] **Step 4: Add `tempfile` dev-dependency to ui crate**

Edit `voicepilot/crates/ui/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
trust-kernel = { workspace = true }
```

- [ ] **Step 5: Run tests (green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# Expect: 4 passing (3 route_text + 1 organize_files)
```

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/ui/Cargo.toml voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/src/state.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "Task 4: organize_files Tauri command wires FilesOrganizeSkill + TauriApprover (V1.1 §5.2, §6.2, §8.2)"
```

---

## Task 5: `submit_approval` command — webview decision delivery

**Files:**
- Modify: `voicepilot/crates/ui/src/commands.rs`
- Modify: `voicepilot/crates/ui/tests/commands_unit.rs`

- [ ] **Step 1: Write the test first (red)**

Append to `commands_unit.rs`:

```rust
use trust_kernel::approval::types::ApprovalDecision;
use voicepilot_ui::commands::submit_approval;
use voicepilot_ui::approver::ApprovalRegistry;
use trust_kernel::policy::transaction::EffectManifest;

#[test]
fn submit_approval_delivers_decision_to_waiting_approver() {
    let mut state = AppState::new_in_memory().unwrap();
    let manifest = EffectManifest {
        sources: vec![],
        destination: "D:/test".to_string(),
        conflicts: vec![],
        total_bytes: 0,
    };
    let (approval_id, rx) = state.approval_registry.create_request(&manifest);
    
    // Spawn a thread that waits for the decision
    let registry = state.approval_registry.clone();
    let handle = std::thread::spawn(move || {
        registry.wait_for_decision(rx, std::time::Duration::from_secs(5))
    });
    
    // Give the thread a moment to start waiting
    std::thread::sleep(std::time::Duration::from_millis(100));
    
    // Submit the approval decision
    let result = submit_approval(&state, &approval_id, ApprovalDecision::Allow).unwrap();
    assert!(result);
    
    let decision = handle.join().unwrap();
    assert_eq!(decision, ApprovalDecision::Allow);
}

#[test]
fn submit_approval_returns_false_for_unknown_id() {
    let state = AppState::new_in_memory().unwrap();
    let result = submit_approval(&state, "apr_nonexistent", ApprovalDecision::Deny).unwrap();
    assert!(!result);
}
```

- [ ] **Step 2: Run tests (red)**

- [ ] **Step 3: Implement `submit_approval` (green)**

Add to `commands.rs`:

```rust
use trust_kernel::approval::types::ApprovalDecision;

/// Submit the user's approval decision for a pending request.
/// Returns true if the decision was delivered, false if the request
/// was already consumed or never existed (one-shot per §8.2).
pub fn submit_approval(
    state: &AppState,
    approval_id: &str,
    decision: ApprovalDecision,
) -> UiResult<bool> {
    let sender = match state.approval_registry.take_sender(approval_id) {
        Some(s) => s,
        None => return Ok(false),
    };
    let _ = sender.send(decision);
    Ok(true)
}

#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn submit_approval_command(
    state: tauri::State<'_, AppState>,
    approval_id: String,
    decision: ApprovalDecision,
) -> Result<bool, String> {
    submit_approval(&state, &approval_id, decision).map_err(Into::into)
}
```

- [ ] **Step 4: Run tests (green)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test commands_unit
# Expect: 6 passing
```

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tests/commands_unit.rs
git commit -m "Task 5: submit_approval Tauri command delivers webview decision (V1.1 §8.2 one-shot)"
```

---

## Task 6: Tauri app entry + event emission for approval requests

**Files:**
- Create: `voicepilot/crates/ui/src/main.rs`
- Create: `voicepilot/crates/ui/src/app.rs`
- Create: `voicepilot/crates/ui/tauri.conf.json`

**Design:**
- `TauriApprover::prompt` emits `approval-request` event to all webviews with `{approval_request_id, manifest}`
- The webview's ApprovalModal listens via `@tauri-apps/api/event`
- On submit, webview calls `submit_approval_command`

- [ ] **Step 1: Create `tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "VoicePilot",
  "version": "0.1.0",
  "identifier": "com.voicepilot.app",
  "build": {
    "frontendDist": "web/dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "VoicePilot",
        "width": 1024,
        "height": 768,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.png"]
  }
}
```

- [ ] **Step 2: Create `app.rs` with Tauri builder**

```rust
//! Tauri app builder + command registration.

use tauri::Manager;
use crate::commands::{
    route_text_command, organize_files_command, submit_approval_command,
};
use crate::state::AppState;
use crate::error::UiResult;

pub fn run(kernel: trust_kernel::kernel::TrustKernel) -> UiResult<()> {
    let state = AppState::new(kernel);
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            route_text_command,
            organize_files_command,
            submit_approval_command,
        ])
        .setup(|_app| {
            // W6b: open Approval window on demand via app.get_webview_window("approval")
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| crate::error::UiError::Tauri(e.to_string()))?;
    Ok(())
}
```

- [ ] **Step 3: Create `main.rs`**

```rust
use voicepilot_ui::app;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();
    
    let db_path = std::env::var("VOICEPILOT_DB")
        .unwrap_or_else(|_| "voicepilot.db".to_string());
    let kernel = trust_kernel::kernel::TrustKernel::open_file(&db_path)
        .expect("failed to open kernel");
    
    app::run(kernel).expect("failed to run Tauri app");
}
```

- [ ] **Step 4: Update `TauriApprover` to emit events**

Modify `approver.rs` — `TauriApprover` needs an `AppHandle` to emit events. Add a new constructor and update `prompt`:

```rust
#[cfg(feature = "tauri")]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(feature = "tauri")]
pub struct TauriApprover {
    registry: ApprovalRegistry,
    app: AppHandle,
}

#[cfg(feature = "tauri")]
impl TauriApprover {
    pub fn new(registry: ApprovalRegistry, app: AppHandle) -> Self {
        Self { registry, app }
    }

    pub fn registry(&self) -> &ApprovalRegistry {
        &self.registry
    }
}

#[cfg(feature = "tauri")]
#[derive(serde::Serialize)]
struct ApprovalRequestPayload {
    approval_request_id: String,
    manifest: trust_kernel::policy::transaction::EffectManifest,
}

#[cfg(feature = "tauri")]
impl Approver for TauriApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        let (approval_id, rx) = self.registry.create_request(manifest);
        let payload = ApprovalRequestPayload {
            approval_request_id: approval_id.clone(),
            manifest: manifest.clone(),
        };
        // Emit to all webviews; the ApprovalModal listens via @tauri-apps/api/event
        let _ = self.app.emit("approval-request", payload);
        self.registry.wait_for_decision(rx, DEFAULT_APPROVAL_TIMEOUT)
    }
}
```

Update `organize_files_command` to construct TauriApprover with `AppHandle`:

```rust
#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn organize_files_command(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    input: OrganizeInput,
) -> Result<OrganizeResult, String> {
    let approver = crate::approver::TauriApprover::new(state.approval_registry.clone(), app);
    organize_files(&state, &approver, &input).map_err(Into::into)
}
```

- [ ] **Step 5: Verify Tauri compiles (without webview — main.rs uses `tauri::generate_context!` which needs `tauri.conf.json`)**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# Expect: compiles. (Tauri context will fail at runtime without web/dist, but check passes.)
```

- [ ] **Step 6: Commit**

```powershell
git add voicepilot/crates/ui/src/main.rs voicepilot/crates/ui/src/app.rs voicepilot/crates/ui/src/approver.rs voicepilot/crates/ui/src/commands.rs voicepilot/crates/ui/tauri.conf.json
git commit -m "Task 6: Tauri app entry + approval-request event emission (V1.1 §8.2)"
```

---

## Task 7: React frontend — Approval modal + Main view

**Files:**
- Create: `voicepilot/crates/ui/web/package.json`
- Create: `voicepilot/crates/ui/web/vite.config.ts`
- Create: `voicepilot/crates/ui/web/tsconfig.json`
- Create: `voicepilot/crates/ui/web/index.html`
- Create: `voicepilot/crates/ui/web/src/main.tsx`
- Create: `voicepilot/crates/ui/web/src/App.tsx`
- Create: `voicepilot/crates/ui/web/src/components/MainView.tsx`
- Create: `voicepilot/crates/ui/web/src/components/ApprovalModal.tsx`
- Create: `voicepilot/crates/ui/web/src/api.ts`
- Create: `voicepilot/crates/ui/web/src/types.ts`

**Design:**
The frontend uses **Trae frontend-design aesthetic guidelines** (per the loaded skill): bold typography, distinctive color palette, not generic AI slop. For VoicePilot — a local-first privacy tool — the aesthetic should be **trustworthy, technical, slightly editorial**. Think: dark theme, monospace accents, sharp typography, "engineering console" feel.

Aesthetic direction: **"Engineering Console"** — dark navy + warm amber accents, IBM Plex Mono for code/data, IBM Plex Sans for body, generous whitespace, sharp 4px corners (not rounded), subtle grid backgrounds.

- [ ] **Step 1: Create `web/package.json`**

```json
{
  "name": "voicepilot-ui-web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "react": "^18.3.1",
    "react-dom": "^18.3.1"
  },
  "devDependencies": {
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0"
  }
}
```

- [ ] **Step 2: Create `web/vite.config.ts`**

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
});
```

- [ ] **Step 3: Create `web/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2021",
    "useDefineForClassFields": true,
    "lib": ["ES2021", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src"]
}
```

- [ ] **Step 4: Create `web/index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>VoicePilot</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&family=IBM+Plex+Sans:wght@400;500;600;700&display=swap" rel="stylesheet" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 5: Create `web/src/types.ts`** (mirrors Rust DTOs)

```typescript
export interface EffectManifest {
  sources: FileSnapshot[];
  destination: string;
  conflicts: string[];
  total_bytes: number;
}

export interface FileSnapshot {
  canonical_path: string;
  file_id: string;
  size: number;
  last_write_time: string;
  sha256: string;
}

export type ApprovalDecision = "allow" | "deny" | "modify";

export interface ApprovalRequestPayload {
  approval_request_id: string;
  manifest: EffectManifest;
}

export type RouteTextResult =
  | { kind: "routed"; skill_id: string }
  | { kind: "unmatched"; text: string }
  | { kind: "empty" };

export interface OrganizeInput {
  task_id: string;
  step_id: string;
  source: string;
  filter: string;
  destination: string;
}

export interface OrganizeResult {
  committed: boolean;
  moved_paths: [string, string][];
  evidence_strength: string;
  compensation_ref: string | null;
  error: string | null;
}
```

- [ ] **Step 6: Create `web/src/api.ts`**

```typescript
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApprovalRequestPayload,
  ApprovalDecision,
  OrganizeInput,
  OrganizeResult,
  RouteTextResult,
} from "./types";

export async function routeText(text: string): Promise<RouteTextResult> {
  return invoke<RouteTextResult>("route_text_command", { text });
}

export async function organizeFiles(
  input: OrganizeInput
): Promise<OrganizeResult> {
  return invoke<OrganizeResult>("organize_files_command", { input });
}

export async function submitApproval(
  approvalRequestId: string,
  decision: ApprovalDecision
): Promise<boolean> {
  return invoke<boolean>("submit_approval_command", {
    approvalId: approvalRequestId,
    decision,
  });
}

export function onApprovalRequest(
  handler: (payload: ApprovalRequestPayload) => void
): Promise<UnlistenFn> {
  return listen<ApprovalRequestPayload>("approval-request", (event) => {
    handler(event.payload);
  });
}
```

- [ ] **Step 7: Create `web/src/main.tsx`**

```typescript
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

- [ ] **Step 8: Create `web/src/styles.css`** (Engineering Console aesthetic)

```css
:root {
  --bg-deep: #0a0e1a;
  --bg-base: #111827;
  --bg-elev: #1a2234;
  --bg-elev2: #232d44;
  --border: #2d3650;
  --border-bright: #3d4866;
  --text-primary: #e6edf7;
  --text-secondary: #94a3b8;
  --text-muted: #64748b;
  --accent: #f59e0b;
  --accent-bright: #fbbf24;
  --danger: #ef4444;
  --success: #10b981;
  --mono: "IBM Plex Mono", "SF Mono", "Monaco", monospace;
  --sans: "IBM Plex Sans", "Inter", system-ui, sans-serif;
}

* {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html, body, #root {
  height: 100%;
  background: var(--bg-deep);
  color: var(--text-primary);
  font-family: var(--sans);
  font-size: 14px;
  line-height: 1.55;
  -webkit-font-smoothing: antialiased;
  overflow: hidden;
}

.app-shell {
  display: grid;
  grid-template-rows: 48px 1fr;
  height: 100vh;
}

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 24px;
  background: var(--bg-base);
  border-bottom: 1px solid var(--border);
  font-family: var(--mono);
  font-size: 12px;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--text-secondary);
}

.topbar .brand {
  color: var(--accent);
  font-weight: 600;
}

.topbar .brand::before {
  content: "◆ ";
  color: var(--accent-bright);
}

.main {
  display: grid;
  grid-template-columns: 1fr 320px;
  gap: 1px;
  background: var(--border);
  overflow: hidden;
}

.panel {
  background: var(--bg-base);
  padding: 32px 40px;
  overflow-y: auto;
}

.panel-header {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--text-muted);
  margin-bottom: 12px;
}

.panel-title {
  font-family: var(--sans);
  font-size: 28px;
  font-weight: 600;
  letter-spacing: -0.02em;
  margin-bottom: 32px;
  color: var(--text-primary);
}

.panel-title em {
  font-style: normal;
  color: var(--accent);
}

.form-row {
  display: grid;
  grid-template-columns: 120px 1fr;
  gap: 16px;
  align-items: center;
  margin-bottom: 16px;
}

.form-row label {
  font-family: var(--mono);
  font-size: 11px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-secondary);
}

.form-row input {
  background: var(--bg-deep);
  border: 1px solid var(--border);
  color: var(--text-primary);
  padding: 8px 12px;
  font-family: var(--mono);
  font-size: 13px;
  border-radius: 0;
  outline: none;
  transition: border-color 0.15s;
}

.form-row input:focus {
  border-color: var(--accent);
}

.btn {
  font-family: var(--mono);
  font-size: 12px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  padding: 10px 20px;
  border: 1px solid var(--border-bright);
  background: var(--bg-elev);
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 0;
  transition: all 0.15s;
}

.btn:hover {
  background: var(--bg-elev2);
  border-color: var(--accent);
}

.btn-primary {
  background: var(--accent);
  color: var(--bg-deep);
  border-color: var(--accent);
  font-weight: 600;
}

.btn-primary:hover {
  background: var(--accent-bright);
  border-color: var(--accent-bright);
}

.btn-danger {
  border-color: var(--danger);
  color: var(--danger);
}

.btn-danger:hover {
  background: var(--danger);
  color: var(--bg-deep);
}

.status-panel {
  background: var(--bg-deep);
  padding: 24px;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--text-secondary);
}

.status-line {
  margin-bottom: 8px;
  display: flex;
  gap: 12px;
}

.status-line .key {
  color: var(--text-muted);
  min-width: 100px;
}

.status-line .val {
  color: var(--text-primary);
}

/* Approval Modal */
.modal-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(10, 14, 26, 0.85);
  backdrop-filter: blur(8px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  animation: fadeIn 0.15s ease-out;
}

@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

.modal {
  background: var(--bg-base);
  border: 1px solid var(--border-bright);
  width: min(680px, 90vw);
  max-height: 85vh;
  overflow-y: auto;
  display: grid;
  grid-template-rows: auto 1fr auto;
  animation: slideUp 0.2s ease-out;
}

@keyframes slideUp {
  from { transform: translateY(20px); opacity: 0; }
  to { transform: translateY(0); opacity: 1; }
}

.modal-header {
  padding: 20px 28px;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.modal-header h2 {
  font-family: var(--sans);
  font-size: 18px;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.modal-header .badge {
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--accent);
  border: 1px solid var(--accent);
  padding: 4px 8px;
}

.modal-body {
  padding: 24px 28px;
}

.manifest-table {
  width: 100%;
  border-collapse: collapse;
  font-family: var(--mono);
  font-size: 12px;
}

.manifest-table th,
.manifest-table td {
  text-align: left;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}

.manifest-table th {
  color: var(--text-muted);
  font-weight: 500;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  font-size: 10px;
}

.manifest-table td {
  color: var(--text-primary);
}

.manifest-table .path {
  color: var(--accent);
}

.manifest-summary {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  margin-bottom: 24px;
  padding: 16px;
  background: var(--bg-deep);
  border: 1px solid var(--border);
}

.summary-stat {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.summary-stat .label {
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--text-muted);
}

.summary-stat .value {
  font-family: var(--mono);
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
}

.summary-stat .value.danger {
  color: var(--danger);
}

.modal-footer {
  padding: 16px 28px;
  border-top: 1px solid var(--border);
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  background: var(--bg-deep);
}

.conflicts-list {
  margin-top: 12px;
  padding: 12px 16px;
  background: rgba(239, 68, 68, 0.08);
  border-left: 3px solid var(--danger);
  font-family: var(--mono);
  font-size: 12px;
  color: var(--danger);
}

.conflicts-list ul {
  list-style: none;
  margin-top: 8px;
}

.conflicts-list li {
  padding: 2px 0;
}

.route-result {
  padding: 16px;
  background: var(--bg-deep);
  border: 1px solid var(--border);
  margin-top: 16px;
  font-family: var(--mono);
  font-size: 13px;
}

.route-result.routed {
  border-left: 3px solid var(--success);
}

.route-result.unmatched {
  border-left: 3px solid var(--accent);
}
```

- [ ] **Step 9: Create `web/src/components/MainView.tsx`**

```typescript
import { useState } from "react";
import { routeText, organizeFiles } from "../api";
import type { RouteTextResult, OrganizeResult } from "../types";

export function MainView() {
  const [text, setText] = useState("");
  const [routeResult, setRouteResult] = useState<RouteTextResult | null>(null);
  const [source, setSource] = useState("");
  const [filter, setFilter] = useState("*.txt");
  const [destination, setDestination] = useState("");
  const [organizeResult, setOrganizeResult] = useState<OrganizeResult | null>(null);
  const [busy, setBusy] = useState(false);

  async function onRoute() {
    setBusy(true);
    try {
      const r = await routeText(text);
      setRouteResult(r);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  async function onOrganize() {
    setBusy(true);
    try {
      const r = await organizeFiles({
        task_id: `t-${Date.now()}`,
        step_id: `s-${Date.now()}`,
        source,
        filter,
        destination,
      });
      setOrganizeResult(r);
    } catch (e) {
      console.error(e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="panel">
      <div className="panel-header">§ 5.1 Skill Router</div>
      <h1 className="panel-title">
        Route <em>intent</em> → Skill
      </h1>

      <div className="form-row">
        <label>Text</label>
        <input
          type="text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="organize my downloads"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onRoute} disabled={busy}>
          Route
        </button>
      </div>

      {routeResult && (
        <div
          className={`route-result ${
            routeResult.kind === "routed" ? "routed" : "unmatched"
          }`}
        >
          {routeResult.kind === "routed" && (
            <>✓ Routed to skill: <strong>{routeResult.skill_id}</strong></>
          )}
          {routeResult.kind === "unmatched" && (
            <>? No skill matched: <strong>{routeResult.text}</strong></>
          )}
          {routeResult.kind === "empty" && <>∅ Empty input</>}
        </div>
      )}

      <div className="panel-header" style={{ marginTop: 48 }}>§ 5.2 Files Organize</div>
      <h1 className="panel-title">
        Run <em>files.organize</em>
      </h1>

      <div className="form-row">
        <label>Source</label>
        <input
          type="text"
          value={source}
          onChange={(e) => setSource(e.target.value)}
          placeholder="D:/Downloads"
        />
      </div>
      <div className="form-row">
        <label>Filter</label>
        <input
          type="text"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="*.pdf"
        />
      </div>
      <div className="form-row">
        <label>Destination</label>
        <input
          type="text"
          value={destination}
          onChange={(e) => setDestination(e.target.value)}
          placeholder="D:/Documents/Papers"
        />
      </div>
      <div style={{ marginBottom: 32 }}>
        <button className="btn btn-primary" onClick={onOrganize} disabled={busy}>
          Organize
        </button>
      </div>

      {organizeResult && (
        <div className="route-result routed">
          <div>
            committed: <strong>{String(organizeResult.committed)}</strong>
          </div>
          <div>
            moved: <strong>{organizeResult.moved_paths.length}</strong> file(s)
          </div>
          <div>
            evidence: <strong>{organizeResult.evidence_strength}</strong>
          </div>
          {organizeResult.compensation_ref && (
            <div>
              compensation_ref: <strong>{organizeResult.compensation_ref}</strong>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 10: Create `web/src/components/ApprovalModal.tsx`**

```typescript
import { useEffect, useState } from "react";
import { submitApproval } from "../api";
import type { ApprovalRequestPayload } from "../types";

interface Props {
  payload: ApprovalRequestPayload;
  onDismiss: () => void;
}

export function ApprovalModal({ payload, onDismiss }: Props) {
  const [submitting, setSubmitting] = useState(false);
  const { approval_request_id, manifest } = payload;

  async function decide(decision: "allow" | "deny") {
    setSubmitting(true);
    try {
      await submitApproval(approval_request_id, decision);
      onDismiss();
    } catch (e) {
      console.error(e);
    } finally {
      setSubmitting(false);
    }
  }

  // Auto-deny on unmount (e.g., user closes window)
  useEffect(() => {
    return () => {
      // Best-effort deny on close — but only if not already submitted
      // The Rust side will return false if already consumed (one-shot)
      submitApproval(approval_request_id, "deny").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-backdrop">
      <div className="modal">
        <div className="modal-header">
          <h2>Approve File Operation</h2>
          <span className="badge">E2 · D2 · Local</span>
        </div>
        <div className="modal-body">
          <div className="manifest-summary">
            <div className="summary-stat">
              <span className="label">Sources</span>
              <span className="value">{manifest.sources.length}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Total Bytes</span>
              <span className="value">{manifest.total_bytes.toLocaleString()}</span>
            </div>
            <div className="summary-stat">
              <span className="label">Conflicts</span>
              <span className={`value ${manifest.conflicts.length > 0 ? "danger" : ""}`}>
                {manifest.conflicts.length}
              </span>
            </div>
          </div>

          <table className="manifest-table">
            <thead>
              <tr>
                <th>Path</th>
                <th>Size</th>
                <th>SHA-256</th>
              </tr>
            </thead>
            <tbody>
              {manifest.sources.map((s) => (
                <tr key={s.canonical_path}>
                  <td className="path">{s.canonical_path}</td>
                  <td>{s.size}</td>
                  <td>{s.sha256.slice(0, 16)}…</td>
                </tr>
              ))}
            </tbody>
          </table>

          <div className="form-row" style={{ marginTop: 24 }}>
            <label>Destination</label>
            <input type="text" value={manifest.destination} readOnly />
          </div>

          {manifest.conflicts.length > 0 && (
            <div className="conflicts-list">
              ⚠ {manifest.conflicts.length} conflict(s) detected:
              <ul>
                {manifest.conflicts.map((c, i) => (
                  <li key={i}>{c}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
        <div className="modal-footer">
          <button
            className="btn btn-danger"
            onClick={() => decide("deny")}
            disabled={submitting}
          >
            Deny
          </button>
          <button
            className="btn btn-primary"
            onClick={() => decide("allow")}
            disabled={submitting}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 11: Create `web/src/App.tsx`**

```typescript
import { useEffect, useState } from "react";
import { MainView } from "./components/MainView";
import { ApprovalModal } from "./components/ApprovalModal";
import { onApprovalRequest } from "./api";
import type { ApprovalRequestPayload } from "./types";

export function App() {
  const [approval, setApproval] = useState<ApprovalRequestPayload | null>(null);

  useEffect(() => {
    const unlisten = onApprovalRequest((payload) => {
      setApproval(payload);
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  return (
    <div className="app-shell">
      <div className="topbar">
        <span className="brand">VoicePilot</span>
        <span>W6a · Trust Kernel · Single Rust Architecture</span>
      </div>
      <div className="main">
        <MainView />
        <div className="status-panel">
          <div className="panel-header">Kernel Status</div>
          <div className="status-line">
            <span className="key">Spec:</span>
            <span className="val">V1.1.2</span>
          </div>
          <div className="status-line">
            <span className="key">Architecture:</span>
            <span className="val">Single Rust Kernel</span>
          </div>
          <div className="status-line">
            <span className="key">Voice:</span>
            <span className="val">opt-in (W5)</span>
          </div>
          <div className="status-line">
            <span className="key">Approval TTL:</span>
            <span className="val">300s</span>
          </div>
        </div>
      </div>
      {approval && (
        <ApprovalModal
          payload={approval}
          onDismiss={() => setApproval(null)}
        />
      )}
    </div>
  );
}
```

- [ ] **Step 12: Install npm deps + build frontend**

```powershell
cd d:\voicepilot\voicepilot\crates\ui\web
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force
npm install
npm run build
# Expect: web/dist/ contains index.html + assets/
```

- [ ] **Step 13: Verify Tauri app compiles with frontend**

```powershell
cd d:\voicepilot
cargo check --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# Expect: compiles. tauri::generate_context! picks up web/dist.
```

- [ ] **Step 14: Commit**

```powershell
git add voicepilot/crates/ui/web/ voicepilot/crates/ui/src/
git commit -m "Task 7: React frontend — Main view + Approval modal (Engineering Console aesthetic)"
```

---

## Task 8: End-to-end smoke test — Tauri command pipeline

**Files:**
- Create: `voicepilot/crates/ui/tests/w6a_e2e_smoke.rs`

**Design:**
Test the full pipeline WITHOUT launching a real Tauri webview (which requires GUI). Instead:
1. Create `AppState` with in-memory kernel
2. Create temp dir with test files
3. Call `organize_files` with `AutoApprover` (bypasses Tauri event system)
4. Verify files moved + audit chain complete + compensation recorded

This validates the Rust-side pipeline. Full webview E2E (with actual button clicks) is a manual test in W6b.

- [ ] **Step 1: Write the smoke test**

`voicepilot/crates/ui/tests/w6a_e2e_smoke.rs`:

```rust
#![cfg(feature = "tauri")]

use tempfile::TempDir;
use trust_kernel::approval::approver::AutoApprover;
use voicepilot_ui::commands::{organize_files, OrganizeInput};
use voicepilot_ui::state::AppState;

/// W6a §11.1 gate: end-to-end organize_files with the full pipeline.
/// Uses AutoApprover (bypasses the Tauri event system) to validate the
/// Rust-side Skill executor + audit chain + compensation recording.
/// TauriApprover-specific event emission is covered in approver_unit.rs.
#[test]
fn end_to_end_organize_files_with_auto_approver_full_pipeline() {
    // Setup: temp dir with 2 .txt files + 1 .log file + a dest dir
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();
    std::fs::write(src.join("b.txt"), "beta").unwrap();
    std::fs::write(src.join("c.log"), "gamma").unwrap(); // not matched

    // AppState with in-memory kernel (TauriApprover's registry is on AppState
    // but unused here since AutoApprover is injected directly)
    let state = AppState::new_in_memory().unwrap();

    let result = organize_files(
        &state,
        &AutoApprover,
        &OrganizeInput {
            task_id: "t-w6a-smoke".to_string(),
            step_id: "s-w6a-smoke".to_string(),
            source: src.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest.to_string_lossy().into_owned(),
        },
    ).unwrap();

    // Verify: 2 files moved (a.txt + b.txt), c.log untouched
    assert!(result.committed, "tool_result should be committed");
    assert_eq!(result.moved_paths.len(), 2);
    assert!(dest.join("a.txt").exists());
    assert!(dest.join("b.txt").exists());
    assert!(src.join("c.log").exists(), "non-matching file untouched");

    // Verify: audit chain has expected events (TASK_CREATED + STEP_CREATED +
    // STEP_PREPARED + APPROVAL_RECORDED + STEP_COMMITTED + COMPENSATION_CREATED)
    let audit_count = state
        .kernel
        .audit_count_for_task("t-w6a-smoke")
        .unwrap();
    assert!(audit_count >= 4, "expected ≥4 audit events, got {}", audit_count);

    // Verify: step is in Committed state
    let step = state.kernel.get_step("s-w6a-smoke").unwrap().unwrap();
    assert_eq!(
        step.status,
        trust_kernel::repo::step_repo::StepStatus::Committed
    );

    // Verify: compensation recorded (strong + auto_reverse ready)
    assert!(
        result.compensation_ref.is_some(),
        "compensation_ref must be set after successful commit"
    );
}

/// Test that denial cancels the operation without committing.
#[test]
fn end_to_end_deny_cancels_commit() {
    use trust_kernel::approval::approver::AutoDenier;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(src.join("a.txt"), "alpha").unwrap();

    let state = AppState::new_in_memory().unwrap();
    let result = organize_files(
        &state,
        &AutoDenier,
        &OrganizeInput {
            task_id: "t-w6a-deny".to_string(),
            step_id: "s-w6a-deny".to_string(),
            source: src.to_string_lossy().into_owned(),
            filter: "*.txt".to_string(),
            destination: dest.to_string_lossy().into_owned(),
        },
    );

    // Deny → Skill executor returns error (commit skipped)
    assert!(result.is_err(), "deny should produce an error");

    // Verify: source file untouched
    assert!(src.join("a.txt").exists(), "source file must not be moved on deny");
    assert!(!dest.join("a.txt").exists(), "dest must not contain the file on deny");
}
```

- [ ] **Step 2: Run the smoke test**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri --test w6a_e2e_smoke
# Expect: 2 passing
```

- [ ] **Step 3: Run the full ui test suite**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri
# Expect: approver_unit (4) + commands_unit (6) + w6a_e2e_smoke (2) = 12 passing
```

- [ ] **Step 4: Run default workspace tests (no-regression check)**

```powershell
cargo test --manifest-path voicepilot\Cargo.toml
# Expect: 196 passing (W1-W4) + ui crate skipped (default-members = trust-kernel + cli only)
```

- [ ] **Step 5: Commit**

```powershell
git add voicepilot/crates/ui/tests/w6a_e2e_smoke.rs
git commit -m "Task 8: W6a end-to-end smoke test — organize_files + audit chain + compensation (V1.1 §11.1 W6a gate)"
```

---

## Final Review Checklist

After all 8 tasks complete, verify:

- [ ] `cargo test --manifest-path voicepilot\Cargo.toml` — 196 passing (W1-W4 no regression)
- [ ] `cargo test --manifest-path voicepilot\Cargo.toml --features voice` — voice opt-in tests still pass (W5 no regression)
- [ ] `cargo test --manifest-path voicepilot\Cargo.toml -p voicepilot-ui --features tauri` — 12 passing (W6a)
- [ ] `cargo check --manifest-path voicepilot\Cargo.toml` — 0 warnings (default)
- [ ] `cargo check --manifest-path voicepilot\Cargo.toml --features tauri` — 0 warnings
- [ ] `cargo clippy --manifest-path voicepilot\Cargo.toml --features tauri -- -D warnings` — 0 warnings (W6a code only; pre-existing W1-W5 nits are out of scope)
- [ ] `cd voicepilot/crates/ui/web && npm run build` — produces `web/dist/` with `index.html`
- [ ] Git log shows 8 commits + 1 plan commit (9 total for W6a)
- [ ] PROGRESS.md updated with W6a section

---

## Known Spec Issues (likely to surface during W6a)

Anticipated spec issues to log if encountered (per user instruction "遇到spec issue直接修复"):

| # | Topic | Likely Trigger |
|---|---|---|
| #50 | Tauri capability/permission schema for per-window IPC hardening not specified | Task 6 (tauri.conf.json lacks capabilities section) |
| #51 | Approval window lifecycle (open/close/timeout) not specified | Task 6 (when to open Approval window vs. inline modal) |
| #52 | `approval_request_id` format not specified (UUID v4 vs. sequential) | Task 2 (chose `apr_<uuid>` prefix) |
| #53 | Approval timeout default (300s vs. configurable) not specified | Task 2 (chose 300s matching prepare_token TTL) |
| #54 | Tauri command error → webview error mapping not specified | Task 3-5 (using `String` error, may need structured error) |
| #55 | `web/dist` build artifacts gitignore policy not specified | Task 7 (add to .gitignore) |
| #56 | Frontend bundle signing/integrity check not specified | Task 7 (Tauri CSP is set, but no SRI) |

If any of these surface, fix the spec inline (per user instruction) and document the fix in PROGRESS.md §4.2.
