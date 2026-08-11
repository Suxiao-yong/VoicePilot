//! W11 Plan 1 Task 2: voicepilot eval 子命令 JSON I/O 烟雾测试。
//!
//! 验证 `voicepilot eval --input <json> --mode auto` 能正确:
//! - 解析 --input JSON
//! - 路由到 SkillRouter
//! - 输出 JSON 结果(含 task_id / skill_id / risk_level / approval_decision /
//!   commit_status / blocked / block_reason / audit_trace / error)
//!
//! 不测真实 Skill 执行(避免文件系统副作用),只测 JSON I/O 契约。

use std::process::Command;
use std::str;

/// voicepilot 二进制路径(target/debug/voicepilot.exe)
///
/// CARGO_MANIFEST_DIR = voicepilot/crates/cli
/// target 目录在 workspace root = voicepilot/
/// 所以路径 = cli/../../target/debug/voicepilot.exe(上溯两级)
fn voicepilot_bin() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    std::path::Path::new(&manifest_dir)
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join("voicepilot.exe")
}

#[test]
fn eval_subcommand_returns_valid_json_for_unknown_intent() {
    let input = r#"{"transcript":"totally unknown intent","mode":"auto"}"#;
    let output = Command::new(voicepilot_bin())
        .args(["eval", "--input", input])
        .env("VOICEPILOT_DB", ":memory:")
        .output()
        .expect("failed to run voicepilot eval");

    assert!(
        output.status.success(),
        "voicepilot eval failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = str::from_utf8(&output.stdout).expect("stdout not utf8");
    let v: serde_json::Value = serde_json::from_str(stdout).expect("stdout not valid JSON");

    // 契约字段
    assert!(v.get("task_id").is_some(), "missing task_id");
    assert_eq!(v["transcript"], "totally unknown intent");
    assert!(v.get("skill_id").is_some(), "missing skill_id");
    assert!(v.get("risk_level").is_some(), "missing risk_level");
    assert!(v.get("approval_decision").is_some(), "missing approval_decision");
    assert!(v.get("commit_status").is_some(), "missing commit_status");
    assert!(v.get("blocked").is_some(), "missing blocked");
    assert!(v.get("block_reason").is_some(), "missing block_reason");
    assert!(v.get("audit_trace").is_some(), "missing audit_trace");
    assert!(v.get("error").is_some(), "missing error");

    // 未知 intent → Planner → skill_id=null, commit_status=skipped
    assert_eq!(v["skill_id"], serde_json::Value::Null);
    assert_eq!(v["commit_status"], "skipped");
}

#[test]
fn eval_subcommand_routes_files_organize_keyword() {
    let input = r#"{"transcript":"整理下载目录","mode":"auto"}"#;
    let output = Command::new(voicepilot_bin())
        .args(["eval", "--input", input])
        .env("VOICEPILOT_DB", ":memory:")
        .output()
        .expect("failed to run voicepilot eval");

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = str::from_utf8(&output.stdout).expect("stdout not utf8");
    let v: serde_json::Value = serde_json::from_str(stdout).expect("stdout not valid JSON");

    // 关键词命中 files.organize
    assert_eq!(v["skill_id"], "files.organize");
    // auto mode → approval_decision=auto
    assert_eq!(v["approval_decision"], "auto");
}

#[test]
fn eval_subcommand_invalid_json_returns_error() {
    let output = Command::new(voicepilot_bin())
        .args(["eval", "--input", "not-json"])
        .env("VOICEPILOT_DB", ":memory:")
        .output()
        .expect("failed to run voicepilot eval");

    // 非 0 退出码
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid JSON") || stderr.contains("parse"), "stderr: {}", stderr);
}
