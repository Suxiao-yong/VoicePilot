//! W10 Plan 2 — Reverse function implementations for non-move Skills.
//!
//! 3 个新 reverse 函数:
//! - reverse_note_capture: 删除 note 文件(payload: {"save_path": "..."})
//! - reverse_research_save: 删除 markdown 文件(payload: {"save_path": "..."})
//! - reverse_form_prepare: Playwright eval clear 表单字段(payload: {"fields": {...}})
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
        KernelError::Compensation(format!("reverse_note_capture: invalid reverse_payload: {}", e))
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
        KernelError::Compensation(format!("reverse_research_save: invalid reverse_payload: {}", e))
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

        assert!(result.is_ok(), "reverse must succeed when file already deleted");
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

        assert!(!path.exists(), "markdown file must be deleted after reverse");
    }

    #[test]
    fn reverse_research_save_idempotent_when_file_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_md_path();

        let payload = serde_json::json!({"save_path": path.to_string_lossy()}).to_string();
        let rec = make_rec(&payload);
        let result = reverse_research_save(&kernel, &rec);
        assert!(result.is_ok(), "reverse must succeed when file already deleted");
    }

    #[test]
    fn reverse_research_save_fails_when_payload_missing_save_path() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let rec = make_rec(r#"{"other": "value"}"#);
        let result = reverse_research_save(&kernel, &rec);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing 'save_path'"));
    }
}
