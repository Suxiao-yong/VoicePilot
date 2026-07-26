//! W7 Plan 5 Task 7 — end-to-end smoke tests for the Playwright MCP Skill
//! pipeline (`research.save_markdown` + `form.prepare`).
//!
//! Three tests:
//! **Test A** `research_save_markdown_via_mock_mcp_writes_md_file`:
//!   Mock Python MCP server returns canned navigate/snapshot/eval
//!   responses → executor writes a Markdown file containing the eval
//!   text → step Succeeded with `evidence_strength: "strong"`.
//!
//! **Test B** `form_prepare_via_mock_mcp_no_click_submit`:
//!   Mock Python MCP server records every `tools/call` name to a JSON
//!   file → executor calls navigate + snapshot + fill × N → assertion
//!   gate: `click` MUST NOT appear in the recorded call list.
//!
//! **Test C** (`#[ignore]`) `research_save_markdown_via_real_playwright_mcp`:
//!   Uses the real `npx -y @playwright/mcp@latest` row seeded by
//!   `kernel_boot` (no mock override). Requires Node.js ≥ 18 + network
//!   access. Run with
//!   `cargo test --test w7_plan5_mcp_playwright_smoke -- --ignored`
//!   manually.
//!
//! Tests A and B short-circuit with a passing assertion when `python`
//! is not on PATH (mirrors `w7_plan5_task0_mcp_client_smoke.rs`).

use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::error::KernelError;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::McpServerRepo;
use trust_kernel::repo::step_repo::StepStatus;
use trust_kernel::skills::form_prepare::{execute_form_prepare, FormPrepareInput};
use trust_kernel::skills::research_save::{execute_research_save, ResearchSaveInput};

// Serialize tests that mutate CWD via a global mutex. CWD is
// process-global, so parallel test threads racing on
// set_current_dir would corrupt each other's file writes. Same
// pattern as `research_save.rs` and `form_prepare.rs` unit tests.
static CWD_MUTEX: Mutex<()> = Mutex::new(());

struct CwdGuard {
    prev: std::path::PathBuf,
}

impl CwdGuard {
    fn enter(temp: &std::path::Path) -> Self {
        let prev = std::env::current_dir().expect("getcwd");
        std::env::set_current_dir(temp).expect("setcwd");
        Self { prev }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev);
    }
}

fn python_available() -> bool {
    Command::new("python")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Override the default playwright MCP record (inserted by kernel boot)
/// with a Python mock that returns canned navigate/snapshot/eval/fill
/// responses and optionally records every `tools/call` name to a JSON
/// file at `$FORM_PREPARE_CALLS_PATH` (when set).
fn install_python_mock(kernel: &TrustKernel, mock_script: &str) {
    let args_json = serde_json::to_string(&vec!["-c".to_string(), mock_script.to_string()])
        .expect("serialize args");
    let mut rec = McpServerRepo::new()
        .get(&kernel.conn(), "playwright")
        .expect("playwright row must exist (kernel boot seeds it)")
        .expect("playwright row must exist");
    rec.command = Some("python".to_string());
    rec.args = Some(args_json);
    rec.env = Some("{}".to_string());
    McpServerRepo::new()
        .update(&kernel.conn(), &rec)
        .expect("update mock playwright record");
}

/// Combined mock MCP server script. Versatile enough for both Test A
/// (research_save — navigate/snapshot/eval) and Test B (form_prepare —
/// navigate/snapshot/fill with calls recording).
///
/// When the `FORM_PREPARE_CALLS_PATH` env var is set to a writable file
/// path, every `tools/call` name is appended to the JSON list at that
/// path. Test A leaves the env var unset so the recording branch is
/// skipped; Test B sets it to a temp file and asserts the recorded
/// sequence after the executor returns.
///
/// Canned responses:
/// - navigate → `{"ok": true}`
/// - snapshot → `{"tree": "Example Domain"}`
/// - eval     → `{"text": "Example Domain\n\nThis domain is for use in illustrative examples in documents."}`
/// - fill     → `{"filled": true}`
const MOCK_SCRIPT: &str = r#"
import sys, json, os
def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
calls_path = os.environ.get("FORM_PREPARE_CALLS_PATH")
calls = []
if calls_path and os.path.exists(calls_path):
    try:
        with open(calls_path, "r", encoding="utf-8") as f:
            calls = json.load(f)
    except Exception:
        calls = []
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
                "serverInfo": {"name": "mock-playwright", "version": "0.1.0"}
            }
        })
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if calls_path:
            calls.append(name)
            try:
                with open(calls_path, "w", encoding="utf-8") as f:
                    json.dump(calls, f)
            except Exception:
                pass
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "snapshot":
            text_payload = json.dumps({"tree": "Example Domain"})
        elif name == "eval":
            text_payload = json.dumps({"text": "Example Domain\n\nThis domain is for use in illustrative examples in documents."})
        elif name == "fill":
            text_payload = json.dumps({"filled": True})
        else:
            emit({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32601, "message": f"unknown tool {name}"}
            })
            continue
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
            "error": {"code": -32601, "message": "method not found"}
        })
"#;

// ---- Test A: research.save_markdown via mock MCP writes .md file ----

#[test]
fn research_save_markdown_via_mock_mcp_writes_md_file() {
    if !python_available() {
        eprintln!("skipping research_save_markdown_via_mock_mcp_writes_md_file: python not on PATH");
        return;
    }

    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().expect("tempdir");
    let temp_root = temp.path().to_path_buf();
    std::fs::create_dir_all(temp_root.join("Documents")).expect("create Documents dir");
    let _cwd = CwdGuard::enter(&temp_root);

    // TrustKernel::open_in_memory auto-seeds the `playwright` MCP row at
    // boot (Task 2). Override it with the Python mock.
    let kernel = TrustKernel::open_in_memory().expect("kernel must construct");
    install_python_mock(&kernel, MOCK_SCRIPT);

    let approver = AutoApprover;
    let save_path = format!("Documents/research-test-{}.md", uuid::Uuid::new_v4());
    let input = ResearchSaveInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com".to_string(),
        save_path: save_path.clone(),
    };
    let result = execute_research_save(&kernel, &input, &approver);

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    assert_eq!(result.unwrap(), "t1");

    // File exists on disk with the canned eval content.
    let file_path = temp_root.join(&save_path);
    let content = std::fs::read_to_string(&file_path)
        .expect("markdown file must exist after executor returns Ok");
    assert!(
        content.contains("Example Domain"),
        "expected 'Example Domain' in file, got: {}",
        content
    );

    // Step is Succeeded with strong evidence (file artifact on disk).
    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
}

// ---- Test B: form.prepare via mock MCP — no click submit ----

#[test]
fn form_prepare_via_mock_mcp_no_click_submit() {
    if !python_available() {
        eprintln!("skipping form_prepare_via_mock_mcp_no_click_submit: python not on PATH");
        return;
    }

    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().expect("tempdir");
    let temp_root = temp.path().to_path_buf();
    let calls_path = temp_root.join(format!("calls-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&calls_path, "[]").expect("seed calls file");
    // SAFETY: CWD_MUTEX serializes env mutations process-wide; no other
    // test thread is reading FORM_PREPARE_CALLS_PATH while this test
    // holds the lock.
    std::env::set_var("FORM_PREPARE_CALLS_PATH", &calls_path);

    let kernel = TrustKernel::open_in_memory().expect("kernel must construct");
    install_python_mock(&kernel, MOCK_SCRIPT);

    let approver = AutoApprover;
    let mut fields: HashMap<String, String> = HashMap::new();
    fields.insert("#name".to_string(), "Alice".to_string());
    fields.insert("#email".to_string(), "alice@example.com".to_string());
    let expected_fill_count = fields.len();
    let input = FormPrepareInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com".to_string(),
        fields,
    };
    let result = execute_form_prepare(&kernel, &input, &approver);

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    assert_eq!(result.unwrap(), "t1");

    // Step is Succeeded (form.prepare uses weak evidence — no file
    // artifact, only web state change).
    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);

    // CRITICAL acceptance gate: the recorded calls list contains
    // navigate + snapshot + fill × N, and MUST NOT contain click.
    let recorded: Vec<String> = std::fs::read_to_string(&calls_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    assert!(
        recorded.contains(&"navigate".to_string()),
        "expected navigate in recorded calls, got {:?}",
        recorded
    );
    assert!(
        recorded.contains(&"snapshot".to_string()),
        "expected snapshot in recorded calls, got {:?}",
        recorded
    );
    let fill_count = recorded.iter().filter(|n| *n == "fill").count();
    assert_eq!(
        fill_count,
        expected_fill_count,
        "expected {} fill calls, got {} (full list: {:?})",
        expected_fill_count,
        fill_count,
        recorded
    );
    // Acceptance gate: click MUST NOT be called.
    assert!(
        !recorded.iter().any(|n| n == "click"),
        "playwright.click was called — form.prepare MUST NOT click submit. recorded: {:?}",
        recorded
    );

    std::env::remove_var("FORM_PREPARE_CALLS_PATH");
}

// ---- Test C (#[ignore]): real Playwright MCP, manual verification ----
//
// Run with `cargo test --test w7_plan5_mcp_playwright_smoke -- --ignored`
// manually. Requires Node.js ≥ 18 and network access.

#[test]
#[ignore = "Requires real npx + @playwright/mcp + network. Run with `cargo test --test w7_plan5_mcp_playwright_smoke -- --ignored` manually. Requires Node.js ≥ 18 and network access."]
fn research_save_markdown_via_real_playwright_mcp() {
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().expect("tempdir");
    let temp_root = temp.path().to_path_buf();
    std::fs::create_dir_all(temp_root.join("Documents")).expect("create Documents dir");
    let _cwd = CwdGuard::enter(&temp_root);

    // Use real `npx -y @playwright/mcp@latest` (kernel boot seeds this
    // row by default — do NOT override with the Python mock).
    let kernel = TrustKernel::open_in_memory().expect("kernel must construct");
    let approver = AutoApprover;
    let save_path = "Documents/research-real.md".to_string();
    let input = ResearchSaveInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com".to_string(),
        save_path: save_path.clone(),
    };
    let result = execute_research_save(&kernel, &input, &approver);

    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

    let file_path = temp_root.join(&save_path);
    let content = std::fs::read_to_string(&file_path)
        .expect("markdown file must exist after executor returns Ok");
    assert!(
        content.contains("Example Domain"),
        "expected 'Example Domain' in file, got: {}",
        content
    );

    // Step is Succeeded with strong evidence.
    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(step.evidence_strength.as_deref(), Some("strong"));
}

// ---- Test D: MCP unavailable returns error and marks step Failed ----
//
// W7 Plan 6 Task 3 Step 4: when the playwright MCP server cannot be
// spawned (e.g. command not on PATH), `execute_research_save` must
// return `Err(KernelError::Mcp(_))` and the step must be marked
// `Failed`. This mirrors the unit-level
// `test_research_save_mcp_failure_marks_step_failed` in
// `research_save.rs` but at the integration level.
//
// TODO(follow-up): map to `error_code = "mcp_playwright_unavailable"` in
// the executor / SkillRouter. Current behavior returns `KernelError::Mcp(_)`
// and marks the step `Failed`; the source doc comment in `research_save.rs`
// already references this future mapping. Adding the mapping would change
// executor behavior and is out of scope for Task 3 (test-only changes).

#[test]
fn mcp_unavailable_returns_error_and_marks_step_failed() {
    if !python_available() {
        eprintln!(
            "skipping mcp_unavailable_returns_error_and_marks_step_failed: python not on PATH"
        );
        return;
    }

    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().expect("tempdir");
    let temp_root = temp.path().to_path_buf();
    std::fs::create_dir_all(temp_root.join("Documents")).expect("create Documents dir");
    let _cwd = CwdGuard::enter(&temp_root);

    let kernel = TrustKernel::open_in_memory().expect("kernel must construct");
    // Override playwright record with a guaranteed-to-fail command —
    // spawn will fail with KernelError::Mcp(...), executor marks step
    // Failed, no file written.
    let mut rec = McpServerRepo::new()
        .get(&kernel.conn(), "playwright")
        .expect("playwright row must exist (kernel boot seeds it)")
        .expect("playwright row must exist");
    rec.command = Some("this-command-does-not-exist-12345".to_string());
    rec.args = Some("[]".to_string());
    McpServerRepo::new()
        .update(&kernel.conn(), &rec)
        .expect("update broken playwright record");

    let approver = AutoApprover;
    let input = ResearchSaveInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com".to_string(),
        save_path: "Documents/research-unavailable.md".to_string(),
    };
    let result = execute_research_save(&kernel, &input, &approver);

    // MCP spawn failure maps to KernelError::Mcp.
    let err = result.unwrap_err();
    assert!(
        matches!(err, KernelError::Mcp(_)),
        "expected KernelError::Mcp, got {:?}",
        err
    );

    // Step is Failed.
    let step = kernel.get_step(&input.step_id).unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Failed);

    // No file written (spawn failure short-circuits before fs::write).
    assert!(
        !temp_root.join(&input.save_path).exists(),
        "no file should be written when MCP spawn fails"
    );
}
