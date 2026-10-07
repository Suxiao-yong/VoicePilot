//! W10 Plan 2 — Reverse function implementations for non-move Skills.
//!
//! 4 个新 reverse 函数:
//! - reverse_note_capture: 删除 note 文件(payload: {"save_path": "..."})
//! - reverse_research_save: 删除 markdown 文件(payload: {"save_path": "..."})
//! - reverse_form_prepare: Playwright eval clear 表单字段(payload: {"fields": {...}})
//! - reverse_media_clip_chorus: 删除副歌裁剪输出(payload: {"save_path": "..."})
//!
//! 所有 reverse 函数签名 `fn(&TrustKernel, &CompensationRecord) -> Result<()>`,
//! 与 `auto_reverse_move` 一致(通过 ReverseFnRegistry 注册)。
//!
//! **Idempotency:** 文件不存在 / 字段已清空视为成功(可能已被其他途径清理)。
//! **Stronghold:** 调用方(task_compensate.rs)负责解密 reverse_payload,
//! reverse 函数假设 reverse_payload 为明文 JSON。

use crate::compensation::types::CompensationRecord;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use std::path::Path;

/// reverse_note_capture — 删除 note.capture 写入的文件。
///
/// reverse_payload JSON 结构: `{"save_path": "<path>"}`
///
/// 文件不存在 → Ok(())(idempotent,可能已被其他途径删除)。
/// 文件存在但删除失败 → Err(KernelError::Compensation(...))。
/// payload 缺少 save_path / JSON 解析失败 → Err。
pub fn reverse_note_capture(_kernel: &TrustKernel, rec: &CompensationRecord) -> Result<()> {
    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_note_capture: invalid reverse_payload: {}",
            e
        ))
    })?;
    let save_path = payload
        .get("save_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            KernelError::Compensation(
                "reverse_note_capture: reverse_payload missing 'save_path'".to_string(),
            )
        })?;

    let path = Path::new(save_path);
    if !path.exists() {
        // Idempotent: file already deleted (possibly by user manually).
        return Ok(());
    }

    std::fs::remove_file(path).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_note_capture: failed to delete {}: {}",
            save_path, e
        ))
    })?;

    Ok(())
}

/// reverse_research_save — 删除 research.save_markdown 写入的文件。
///
/// reverse_payload JSON 结构: `{"save_path": "<path>"}`
///
/// 与 reverse_note_capture 结构相同,独立实现以便单独注册与测试。
/// 文件不存在 → Ok(())(idempotent)。
pub fn reverse_research_save(_kernel: &TrustKernel, rec: &CompensationRecord) -> Result<()> {
    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_research_save: invalid reverse_payload: {}",
            e
        ))
    })?;
    let save_path = payload
        .get("save_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            KernelError::Compensation(
                "reverse_research_save: reverse_payload missing 'save_path'".to_string(),
            )
        })?;

    let path = Path::new(save_path);
    if !path.exists() {
        return Ok(());
    }

    std::fs::remove_file(path).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_research_save: failed to delete {}: {}",
            save_path, e
        ))
    })?;

    Ok(())
}

/// reverse_form_prepare — 通过 Playwright eval 清空表单字段。
///
/// reverse_payload JSON 结构: `{"fields": {"<selector>": "<original_value>"}}`
///
/// 对每个 selector 调用 `invoke_mcp_tool(playwright, eval, {script})`,
/// script 将 `document.querySelector(selector).value = ''` 清空字段。
///
/// 空 fields map → no-op Ok(无 eval 调用)。
/// MCP 调用失败 → Err(KernelError::Mcp(...))(由调用方处理)。
/// payload 缺少 fields / JSON 解析失败 → Err。
pub fn reverse_form_prepare(kernel: &TrustKernel, rec: &CompensationRecord) -> Result<()> {
    use crate::skills::common::invoke_mcp_tool;
    use std::collections::BTreeMap;

    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_form_prepare: invalid reverse_payload: {}",
            e
        ))
    })?;

    let fields_obj = payload
        .get("fields")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            KernelError::Compensation(
                "reverse_form_prepare: reverse_payload missing 'fields'".to_string(),
            )
        })?;

    if fields_obj.is_empty() {
        // No fields to clear — no-op success.
        return Ok(());
    }

    // BTreeMap for deterministic iteration order (stable eval call sequence).
    let sorted: BTreeMap<&String, &serde_json::Value> = fields_obj.iter().collect();

    for selector in sorted.keys() {
        // Escape selector for safe embedding in JS string literal.
        let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");
        let script = format!(
            "(function() {{ var el = document.querySelector('{}'); if (el) {{ el.value = ''; }} return el != null; }})()",
            escaped
        );
        // 调用 Playwright eval 清空字段。失败 → 直接返回 Err(不继续清空后续字段)。
        invoke_mcp_tool(
            kernel,
            "playwright",
            "eval",
            serde_json::json!({"script": script}),
        )?;
    }

    Ok(())
}

/// reverse_media_clip_chorus — 删除 media.clip_chorus 裁出的副歌文件。
///
/// reverse_payload JSON 结构: `{"save_path": "<path>"}`
///
/// 与 reverse_note_capture 同形（删文件、幂等），独立实现以便单独注册与测试。
/// 文件不存在 → Ok(())(idempotent)。
pub fn reverse_media_clip_chorus(_kernel: &TrustKernel, rec: &CompensationRecord) -> Result<()> {
    let payload: serde_json::Value = serde_json::from_str(&rec.reverse_payload).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_media_clip_chorus: invalid reverse_payload: {}",
            e
        ))
    })?;
    let save_path = payload
        .get("save_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            KernelError::Compensation(
                "reverse_media_clip_chorus: reverse_payload missing 'save_path'".to_string(),
            )
        })?;

    let path = Path::new(save_path);
    if !path.exists() {
        return Ok(());
    }

    std::fs::remove_file(path).map_err(|e| {
        KernelError::Compensation(format!(
            "reverse_media_clip_chorus: failed to delete {}: {}",
            save_path, e
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod note_capture_reverse_tests {
    use super::*;
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;

    fn make_rec(payload: &str) -> CompensationRecord {
        CompensationRecord {
            comp_id: format!("comp-{}", uuid::Uuid::new_v4()),
            step_id: "s1".to_string(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None,
            ttl_expires: "2030-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "note.reverse_capture".to_string(),
            reverse_payload: payload.to_string(),
        }
    }

    fn tmp_note_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "voicepilot-w10p2-reverse-note-{}.txt",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn reverse_note_capture_deletes_existing_file() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_note_path();
        std::fs::write(&path, b"hello notepad").unwrap();

        let payload = serde_json::json!({"save_path": path.to_string_lossy()}).to_string();
        let rec = make_rec(&payload);
        reverse_note_capture(&kernel, &rec).unwrap();

        assert!(!path.exists(), "file must be deleted after reverse");
    }

    #[test]
    fn reverse_note_capture_idempotent_when_file_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_note_path(); // 不创建文件

        let payload = serde_json::json!({"save_path": path.to_string_lossy()}).to_string();
        let rec = make_rec(&payload);
        let result = reverse_note_capture(&kernel, &rec);

        assert!(
            result.is_ok(),
            "reverse must succeed when file already deleted"
        );
    }

    #[test]
    fn reverse_note_capture_fails_when_payload_missing_save_path() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec(r#"{"wrong_key": "value"}"#);
        let result = reverse_note_capture(&kernel, &rec);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("missing 'save_path'"),
            "expected 'missing save_path' in error, got: {}",
            err
        );
    }

    #[test]
    fn reverse_note_capture_fails_when_payload_invalid_json() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec("not valid json");
        let result = reverse_note_capture(&kernel, &rec);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("invalid reverse_payload"),
            "expected 'invalid reverse_payload' in error, got: {}",
            err
        );
    }
}

#[cfg(test)]
mod research_save_reverse_tests {
    use super::*;
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;

    fn make_rec(payload: &str) -> CompensationRecord {
        CompensationRecord {
            comp_id: format!("comp-{}", uuid::Uuid::new_v4()),
            step_id: "s1".to_string(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None,
            ttl_expires: "2030-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "research.reverse_save".to_string(),
            reverse_payload: payload.to_string(),
        }
    }

    fn tmp_md_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "voicepilot-w10p2-reverse-research-{}.md",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn reverse_research_save_deletes_existing_file() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_md_path();
        std::fs::write(&path, b"# Example\n\ncontent").unwrap();

        let payload = serde_json::json!({"save_path": path.to_string_lossy()}).to_string();
        let rec = make_rec(&payload);
        reverse_research_save(&kernel, &rec).unwrap();

        assert!(
            !path.exists(),
            "markdown file must be deleted after reverse"
        );
    }

    #[test]
    fn reverse_research_save_idempotent_when_file_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_md_path();

        let payload = serde_json::json!({"save_path": path.to_string_lossy()}).to_string();
        let rec = make_rec(&payload);
        let result = reverse_research_save(&kernel, &rec);
        assert!(
            result.is_ok(),
            "reverse must succeed when file already deleted"
        );
    }

    #[test]
    fn reverse_research_save_fails_when_payload_missing_save_path() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec(r#"{"other": "value"}"#);
        let result = reverse_research_save(&kernel, &rec);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing 'save_path'")
        );
    }
}

#[cfg(test)]
mod form_prepare_reverse_tests {
    use super::*;
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;
    use crate::mcp::repo::McpServerRepo;
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Mutex;

    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    fn python_available() -> bool {
        Command::new("python")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn make_rec(payload: &str) -> CompensationRecord {
        CompensationRecord {
            comp_id: format!("comp-{}", uuid::Uuid::new_v4()),
            step_id: "s1".to_string(),
            level: CompensationLevel::Strong,
            snapshot_encrypted: None,
            ttl_expires: "2030-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "form.reverse_prepare".to_string(),
            reverse_payload: payload.to_string(),
        }
    }

    /// Mock Playwright 脚本:记录 eval 调用的 script 到 FORM_REVERSE_CALLS_PATH,
    /// 返回空对象 `{}`(eval 结果不重要,reverse 只关心调用是否成功)。
    const MOCK_SCRIPT: &str = r#"
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
        if name == "eval":
            script = msg.get("params", {}).get("arguments", {}).get("script", "")
            calls_path = os.environ.get("FORM_REVERSE_CALLS_PATH")
            if calls_path:
                try:
                    with open(calls_path, "a", encoding="utf-8") as f:
                        f.write(script + "\n")
                except Exception:
                    pass
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "result": {"content": [{"type": "text", "text": "{}"}],
                             "isError": False}})
        else:
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "error": {"code": -32601, "message": f"unknown {name}"}})
    else:
        emit({"jsonrpc": "2.0", "id": msg.get("id"),
              "error": {"code": -32601, "message": "method not found"}})
"#;

    fn install_mock(kernel: &TrustKernel) {
        let args_json =
            serde_json::to_string(&vec!["-c".to_string(), MOCK_SCRIPT.to_string()]).unwrap();
        let mut rec = McpServerRepo::new()
            .get(&kernel.conn(), "playwright")
            .unwrap()
            .unwrap();
        rec.command = Some("python".to_string());
        rec.args = Some(args_json);
        rec.env = Some("{}".to_string());
        McpServerRepo::new().update(&kernel.conn(), &rec).unwrap();
    }

    #[test]
    fn reverse_form_prepare_clears_all_fields_via_eval() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let calls_path = temp.path().join("calls.log");
        unsafe {
            std::env::set_var("FORM_REVERSE_CALLS_PATH", &calls_path);
        }

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);

        let mut fields = HashMap::new();
        fields.insert("#username".to_string(), "alice".to_string());
        fields.insert("#email".to_string(), "alice@example.com".to_string());
        let payload = serde_json::json!({"fields": fields}).to_string();
        let rec = make_rec(&payload);

        let result = reverse_form_prepare(&kernel, &rec);
        unsafe {
            std::env::remove_var("FORM_REVERSE_CALLS_PATH");
        }

        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());

        // eval 调用次数 = 字段数(每个 selector 一次 eval 清空)。
        let calls = std::fs::read_to_string(&calls_path).unwrap_or_default();
        let eval_count = calls.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(
            eval_count, 2,
            "expected 2 eval calls (one per field), got {}",
            eval_count
        );
        // 每个 eval script 应包含 selector + 空字符串赋值。
        assert!(
            calls.contains("#username"),
            "calls must contain #username: {}",
            calls
        );
        assert!(
            calls.contains("#email"),
            "calls must contain #email: {}",
            calls
        );
    }

    #[test]
    fn reverse_form_prepare_fails_when_payload_missing_fields() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec(r#"{"other": "value"}"#);
        let result = reverse_form_prepare(&kernel, &rec);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing 'fields'"));
    }

    #[test]
    fn reverse_form_prepare_no_fields_is_noop() {
        // 空 fields map → no-op Ok(无 eval 调用)。
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let calls_path = temp.path().join("calls.log");
        unsafe {
            std::env::set_var("FORM_REVERSE_CALLS_PATH", &calls_path);
        }

        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);

        let fields: HashMap<String, String> = HashMap::new();
        let payload = serde_json::json!({"fields": fields}).to_string();
        let rec = make_rec(&payload);

        let result = reverse_form_prepare(&kernel, &rec);
        unsafe {
            std::env::remove_var("FORM_REVERSE_CALLS_PATH");
        }

        assert!(
            result.is_ok(),
            "expected Ok for empty fields, got {:?}",
            result.err()
        );
        let calls = std::fs::read_to_string(&calls_path).unwrap_or_default();
        assert!(
            calls.trim().is_empty(),
            "expected no eval calls for empty fields, got: {}",
            calls
        );
    }
}

#[cfg(test)]
mod clip_chorus_reverse_tests {
    use super::*;
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;

    fn make_rec(payload: &str) -> CompensationRecord {
        CompensationRecord {
            comp_id: format!("comp-{}", uuid::Uuid::new_v4()),
            step_id: "s1".to_string(),
            level: CompensationLevel::BestEffort,
            snapshot_encrypted: None,
            ttl_expires: "2030-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            snapshot_vault_ref: None,
            conflict_policy: ConflictPolicy::AutoReverse,
            compensate_fn: "media.reverse_clip_chorus".to_string(),
            reverse_payload: payload.to_string(),
        }
    }

    #[test]
    fn reverse_clip_chorus_deletes_output_and_is_idempotent() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = std::env::temp_dir().join(format!(
            "voicepilot-reverse-chorus-{}.mp3",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"fake chorus").unwrap();
        let payload = serde_json::json!({"save_path": path.to_string_lossy()}).to_string();
        let rec = make_rec(&payload);
        reverse_media_clip_chorus(&kernel, &rec).unwrap();
        assert!(!path.exists());
        // 二次调用幂等成功。
        reverse_media_clip_chorus(&kernel, &rec).unwrap();
    }

    #[test]
    fn reverse_clip_chorus_fails_when_payload_missing_save_path() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec(r#"{"other": "value"}"#);
        let err = reverse_media_clip_chorus(&kernel, &rec).unwrap_err();
        assert!(err.to_string().contains("missing 'save_path'"), "{err}");
    }
}
