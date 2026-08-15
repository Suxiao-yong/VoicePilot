//! W10 Plan 1 集成测试 — 7 个有副作用 Skill 的 Strong Verifier 覆盖率。
//!
//! spec §3.4 验收:每个有副作用 Skill(排除只读 task.explain)各跑一次
//! happy path,断言 step.evidence_strength = "strong"。task.explain 单独
//! 断言 verifier.strategy = "none"(不计入分母)。
//!
//! 覆盖矩阵:
//! - files.organize:manifest 断言 "strong"(executor 已在 fs_verify.rs 测试)
//! - note.capture:manifest 断言 "strong"(executor 需 UIA feature,verifier
//!   单元测试在 verifiers::note_capture_tests 已覆盖)
//! - research.save_markdown:executor happy path → "strong"(需 python mock)
//! - form.prepare:executor happy path → "strong"(需 python mock + values env)
//! - form.submit:executor happy path → "strong"(需 python mock)
//! - task.repeat_verified:executor happy path → "strong"(纯文件系统,无 mock)
//! - task.compensate:executor happy path → "strong"(纯文件系统,无 mock)
//! - task.explain:manifest 断言 "none"(只读 Skill,spec §6.3 不适用)
//!
//! 所有需要 python mock 的测试用 CWD_MUTEX 序列化,避免 env var 竞争。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use trust_kernel::approval::approver::AutoApprover;
use trust_kernel::compensation::types::{CompensationLevel, ConflictPolicy};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::mcp::repo::McpServerRepo;
use trust_kernel::policy::transaction::EffectManifest;
use trust_kernel::repo::step_repo::{StepRecord, StepStatus};
use trust_kernel::skills::common::create_post_commit_compensation;
use trust_kernel::skills::manifest::{
    files_organize_manifest, form_prepare_manifest, form_submit_manifest,
    note_capture_manifest, research_save_manifest, task_compensate_manifest,
    task_explain_manifest, task_repeat_verified_manifest,
};
use trust_kernel::skills::task_compensate::{execute_compensate, TaskCompensateInput};
use trust_kernel::skills::task_repeat::{execute_repeat_verified, TaskRepeatVerifiedInput};
use trust_kernel::tools::fs_paths::canonicalize;
use trust_kernel::tools::fs_snapshot::snapshot_file;

// 单一全局 mutex 序列化所有改 CWD / env var 的测试,避免并行测试线程竞争。
static CWD_MUTEX: Mutex<()> = Mutex::new(());

fn python_available() -> bool {
    Command::new("python")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// CWD guard — 进入 temp 目录,离开时恢复原 CWD。
struct CwdGuard {
    prev: PathBuf,
}

impl CwdGuard {
    fn enter(dir: &std::path::Path) -> Self {
        let prev = std::env::current_dir().expect("get current_dir");
        std::env::set_current_dir(dir).expect("set_current_dir");
        CwdGuard { prev }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev);
    }
}

/// 安装 python mock 为 playwright MCP server。
fn install_python_mock(kernel: &TrustKernel, mock_script: &str) {
    let args_json =
        serde_json::to_string(&vec!["-c".to_string(), mock_script.to_string()]).unwrap();
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

// ===== Manifest 断言(无 feature flag,无 python) =====

/// W10 Plan 1 验收门禁 §1:7 个有副作用 Skill 的 verifier.strategy 都是 "strong"。
#[test]
fn verifier_strategy_manifest_all_strong_for_side_effect_skills() {
    assert_eq!(files_organize_manifest().verifier.strategy, "strong");
    assert_eq!(note_capture_manifest().verifier.strategy, "strong");
    assert_eq!(research_save_manifest().verifier.strategy, "strong");
    assert_eq!(form_prepare_manifest().verifier.strategy, "strong");
    assert_eq!(form_submit_manifest().verifier.strategy, "strong");
    assert_eq!(task_repeat_verified_manifest().verifier.strategy, "strong");
    assert_eq!(task_compensate_manifest().verifier.strategy, "strong");
}

/// W10 Plan 1 验收门禁 §2:task.explain 是只读 Skill,verifier.strategy = "none"。
/// 不计入 Strong Verifier 分母(分母 = 7)。
#[test]
fn task_explain_verifier_strategy_is_none() {
    let m = task_explain_manifest();
    assert_eq!(
        m.verifier.strategy,
        "none",
        "task.explain must be 'none' (read-only skill, spec §6.3)"
    );
}

// ===== Executor happy path 断言(无 python mock) =====

/// task.repeat_verified happy path → evidence_strength = "strong"。
#[test]
fn task_repeat_executor_returns_strong_evidence() {
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "w10p1-smoke-repeat-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("out")).unwrap();
    fs::write(dir.join("src").join("a.pdf"), b"pdf1").unwrap();
    fs::write(dir.join("out").join("a.pdf"), b"pdf1").unwrap();

    let snap = snapshot_file(&dir.join("src").join("a.pdf")).unwrap();
    let manifest = EffectManifest {
        sources: vec![snap],
        destination: canonicalize(&dir.join("out").to_string_lossy()),
        conflicts: vec![],
        total_bytes: 4,
    };
    kernel.create_task("prev-task", "previous").unwrap();
    let mut prev_step = StepRecord::new("prev-step", "prev-task", 1);
    prev_step.effect_manifest = Some(serde_json::to_value(&manifest).unwrap());
    kernel.create_step(&prev_step).unwrap();

    let input = TaskRepeatVerifiedInput {
        task_id: "new-task".to_string(),
        step_id: "new-step".to_string(),
        target_task_id: "prev-task".to_string(),
        source_filter: "*.pdf".to_string(),
    };
    let approver = AutoApprover;
    let result = execute_repeat_verified(&kernel, &input, &approver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

    let step = kernel.get_step("new-step").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(
        step.evidence_strength.as_deref(),
        Some("strong"),
        "task.repeat_verified must return strong evidence (W10 Plan 1)"
    );

    fs::remove_dir_all(&dir).ok();
}

/// task.compensate happy path → evidence_strength = "strong"。
#[test]
fn task_compensate_executor_returns_strong_evidence() {
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let kernel = TrustKernel::open_in_memory().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "w10p1-smoke-comp-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(dir.join("orig")).unwrap();
    fs::create_dir_all(dir.join("curr")).unwrap();
    let orig = dir.join("orig").join("f.txt");
    let curr = dir.join("curr").join("f.txt");
    fs::write(&curr, b"hello").unwrap();

    kernel.create_task("prev-task", "previous").unwrap();
    kernel
        .create_step(&StepRecord::new("prev-step", "prev-task", 1))
        .unwrap();
    let moved: Vec<(PathBuf, PathBuf)> = vec![(orig.clone(), curr.clone())];
    create_post_commit_compensation(
        &kernel,
        "prev-step",
        &moved,
        "filesystem.reverse_move",
        CompensationLevel::Strong,
        ConflictPolicy::AutoReverse,
        3600,
    )
    .unwrap();

    let input = TaskCompensateInput {
        task_id: "new-task".to_string(),
        step_id: "new-step".to_string(),
        target_step_id: "prev-step".to_string(),
    };
    let approver = AutoApprover;
    let result = execute_compensate(&kernel, &input, &approver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

    let step = kernel.get_step("new-step").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(
        step.evidence_strength.as_deref(),
        Some("strong"),
        "task.compensate must return strong evidence (W10 Plan 1)"
    );

    fs::remove_dir_all(&dir).ok();
}

// ===== Executor happy path 断言(需 python mock) =====

/// research.save_markdown happy path → evidence_strength = "strong"。
#[test]
fn research_save_executor_returns_strong_evidence() {
    if !python_available() {
        eprintln!("skipping: python not on PATH");
        return;
    }
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("Documents")).unwrap();
    let _cwd = CwdGuard::enter(temp.path());

    let kernel = TrustKernel::open_in_memory().unwrap();
    install_python_mock(&kernel, RESEARCH_MOCK_SCRIPT);

    use trust_kernel::skills::research_save::{execute_research_save, ResearchSaveInput};
    let approver = AutoApprover;
    let save_path = format!("Documents/research-{}.md", uuid::Uuid::new_v4());
    let input = ResearchSaveInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com".to_string(),
        save_path: save_path.clone(),
    };
    let result = execute_research_save(&kernel, &input, &approver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(
        step.evidence_strength.as_deref(),
        Some("strong"),
        "research.save_markdown must return strong evidence (W10 Plan 1)"
    );
}

/// form.prepare happy path → evidence_strength = "strong"。
#[test]
fn form_prepare_executor_returns_strong_evidence() {
    if !python_available() {
        eprintln!("skipping: python not on PATH");
        return;
    }
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let temp_root = temp.path().to_path_buf();

    let kernel = TrustKernel::open_in_memory().unwrap();
    install_python_mock(&kernel, FORM_PREPARE_MOCK_SCRIPT);

    use trust_kernel::skills::form_prepare::{execute_form_prepare, FormPrepareInput};
    let approver = AutoApprover;
    let mut fields = HashMap::new();
    fields.insert("#username".to_string(), "alice".to_string());
    fields.insert("#email".to_string(), "alice@example.com".to_string());

    // 写入与 input.fields 一致的 values JSON,供 mock eval 读取返回。
    let values_path = temp_root.join(format!("values-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&values_path, serde_json::to_string(&fields).unwrap()).unwrap();
    unsafe { std::env::set_var("FORM_PREPARE_VALUES_PATH", &values_path); }

    let input = FormPrepareInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com".to_string(),
        fields,
    };
    let result = execute_form_prepare(&kernel, &input, &approver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(
        step.evidence_strength.as_deref(),
        Some("strong"),
        "form.prepare must return strong evidence (W10 Plan 1)"
    );

    unsafe { std::env::remove_var("FORM_PREPARE_VALUES_PATH"); }
}

/// form.submit happy path → evidence_strength = "strong"。
#[test]
fn form_submit_executor_returns_strong_evidence() {
    if !python_available() {
        eprintln!("skipping: python not on PATH");
        return;
    }
    let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let kernel = TrustKernel::open_in_memory().unwrap();
    install_python_mock(&kernel, FORM_SUBMIT_MOCK_SCRIPT);
    // mock 默认返回 current_url = .../success(与 input.url 不同)→ url_changed → Strong。
    unsafe { std::env::remove_var("MOCK_CURRENT_URL"); }
    unsafe { std::env::remove_var("MOCK_HAS_SUCCESS"); }

    use trust_kernel::skills::form_submit::{execute_form_submit, FormSubmitInput};
    let approver = AutoApprover;
    let input = FormSubmitInput {
        task_id: "t1".to_string(),
        step_id: "s1".to_string(),
        url: "https://example.com/submit".to_string(),
        submit_selector: "button[type=submit]".to_string(),
    };
    let result = execute_form_submit(&kernel, &input, &approver);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

    let step = kernel.get_step("s1").unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Succeeded);
    assert_eq!(
        step.evidence_strength.as_deref(),
        Some("strong"),
        "form.submit must return strong evidence (W10 Plan 1)"
    );
}

// ===== Python mock scripts =====

/// research.save_markdown mock:navigate → {ok}, snapshot → {tree}, eval → {text}。
/// eval 返回固定 markdown 文本,executor 写入文件后 verify_research_save 重读验证。
const RESEARCH_MOCK_SCRIPT: &str = r#"
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
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "result": {"protocolVersion": "2025-11-25", "capabilities": {},
                         "serverInfo": {"name": "mock", "version": "0.1"}}})
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "snapshot":
            text_payload = json.dumps({"tree": "Example Domain"})
        elif name == "eval":
            text_payload = json.dumps({"text": "Example Domain\n\nThis domain is for use in illustrative examples in documents."})
        else:
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "error": {"code": -32601, "message": f"unknown {name}"}})
            continue
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "result": {"content": [{"type": "text", "text": text_payload}],
                         "isError": False}})
    else:
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "error": {"code": -32601, "message": "method not found"}})
"#;

/// form.prepare mock:navigate/snapshot/fill/eval。
/// eval 从 FORM_PREPARE_VALUES_PATH 读取预存 JSON 返回(与 input.fields 一致 → Strong)。
const FORM_PREPARE_MOCK_SCRIPT: &str = r#"
import sys, json, os
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
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "result": {"protocolVersion": "2025-11-25", "capabilities": {},
                         "serverInfo": {"name": "mock", "version": "0.1"}}})
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "snapshot":
            text_payload = json.dumps({"tree": "form"})
        elif name == "fill":
            text_payload = json.dumps({"filled": True})
        elif name == "eval":
            values_path = os.environ.get("FORM_PREPARE_VALUES_PATH")
            values = {}
            if values_path and os.path.exists(values_path):
                try:
                    with open(values_path, "r", encoding="utf-8") as f:
                        values = json.load(f)
                except Exception:
                    values = {}
            text_payload = json.dumps(values)
        else:
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "error": {"code": -32601, "message": f"unknown {name}"}})
            continue
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "result": {"content": [{"type": "text", "text": text_payload}],
                         "isError": False}})
    else:
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "error": {"code": -32601, "message": "method not found"}})
"#;

/// form.submit mock:navigate/click/eval。
/// eval 默认返回 current_url = .../success(与 input.url .../submit 不同)→ url_changed → Strong。
const FORM_SUBMIT_MOCK_SCRIPT: &str = r#"
import sys, json, os
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
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "result": {"protocolVersion": "2025-11-25", "capabilities": {},
                         "serverInfo": {"name": "mock", "version": "0.1"}}})
    elif msg.get("method") == "notifications/initialized":
        pass
    elif msg.get("method") == "tools/call":
        name = msg.get("params", {}).get("name")
        if name == "navigate":
            text_payload = json.dumps({"ok": True})
        elif name == "click":
            text_payload = json.dumps({"clicked": True})
        elif name == "eval":
            current_url = os.environ.get("MOCK_CURRENT_URL", "https://example.com/success")
            has_success = os.environ.get("MOCK_HAS_SUCCESS", "false") == "true"
            text_payload = json.dumps({"url": current_url, "success": has_success})
        else:
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "error": {"code": -32601, "message": f"unknown {name}"}})
            continue
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "result": {"content": [{"type": "text", "text": text_payload}],
                         "isError": False}})
    else:
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "error": {"code": -32601, "message": "method not found"}})
"#;
