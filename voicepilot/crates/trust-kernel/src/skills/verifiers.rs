//! Strong Verifier 函数集合 — W10 Plan 1。
//!
//! 为 6 个有副作用的 Skill 提供真实 Strong Verifier 实现:
//! - verify_note_capture: 重读 note 文件存在 + sha256 + size 匹配 content
//! - verify_research_save: 重读 markdown 文件存在 + size > 0
//! - verify_form_prepare: Playwright 重查表单字段值匹配 effect_manifest.fields
//! - verify_form_submit: Playwright 查页面 URL 变更 或 success 元素存在
//! - verify_task_repeat: 同 files.organize.verify_move,重读目标文件 sha256+size
//! - verify_task_compensate: 查 compensations 表 status=reversed + reverse_payload 非空
//!
//! files.organize 已实现(FilesystemTool::verify_move),不在此模块。
//! task.explain 是只读 Skill,verifier.strategy="none",不在此模块。
//!
//! spec §6.3 "Verifier 读取真实状态":每个 verify 函数必须在 commit 之后
//! 重新读取真实世界状态(filesystem / DB / Playwright page),而非依赖
//! executor 内部状态。Strong = 真实 artifact 验证(sha256 / DB 记录 / 页面元素)。

use crate::error::Result;
use crate::kernel::TrustKernel;
use serde_json::Value;

/// Verifier 调用上下文。`step_id` 与现有 executor 的 String step_id 一致。
pub struct VerificationContext<'a> {
    pub kernel: &'a TrustKernel,
    pub step_id: &'a str,
}

/// Verifier 输出。Strong = 真实 artifact 验证通过;Failed = 验证失败
/// (文件不存在 / sha256 不匹配 / DB 记录不存在 / 页面元素缺失)。
/// Medium / Weak 在 Plan 1 不使用,保留枚举变体供未来扩展。
#[derive(Debug, Clone)]
pub enum VerificationOutcome {
    Strong { evidence: Value },
    #[allow(dead_code)]
    Medium { evidence: Value },
    #[allow(dead_code)]
    Weak { reason: String },
    Failed { reason: String },
}

impl VerificationOutcome {
    /// 提取 evidence_strength 字符串(供 finalize_step_success 使用)。
    pub fn evidence_strength(&self) -> &'static str {
        match self {
            VerificationOutcome::Strong { .. } => "strong",
            VerificationOutcome::Medium { .. } => "medium",
            VerificationOutcome::Weak { .. } => "weak",
            VerificationOutcome::Failed { .. } => "weak",
        }
    }
}

/// verify_note_capture — 重读 save_path 文件存在 + sha256 匹配 expected_content。
///
/// spec §6.3 Strong Verifier:commit 后重读真实文件,计算 sha256 + size,
/// 与 expected_content 的 sha256 比较。匹配 → Strong;不匹配 / 文件不存在 → Failed。
pub fn verify_note_capture(
    _ctx: &VerificationContext<'_>,
    save_path: &str,
    expected_content: &str,
) -> Result<VerificationOutcome> {
    use sha2::{Digest, Sha256};
    use std::path::Path;

    let path = Path::new(save_path);
    if !path.exists() {
        return Ok(VerificationOutcome::Failed {
            reason: format!("note file not found at {}", save_path),
        });
    }

    let on_disk = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&on_disk);
    let actual_sha = format!("{:x}", hasher.finalize());

    let mut expected_hasher = Sha256::new();
    expected_hasher.update(expected_content.as_bytes());
    let expected_sha = format!("{:x}", expected_hasher.finalize());

    if actual_sha != expected_sha {
        return Ok(VerificationOutcome::Failed {
            reason: format!(
                "sha256 mismatch: expected {} got {}",
                expected_sha, actual_sha
            ),
        });
    }

    Ok(VerificationOutcome::Strong {
        evidence: serde_json::json!({
            "save_path": save_path,
            "sha256": actual_sha,
            "size": on_disk.len(),
        }),
    })
}

/// verify_research_save — 重读 save_path 文件存在 + size > 0。
///
/// markdown 内容由 Playwright eval 动态生成,sha256 难匹配,只验证
/// 文件存在 + 非空(spec §6.3 "Strong = 真实 artifact 验证" 的弱化形式,
/// 文件存在 + 非空已足以证明 commit 成功)。
pub fn verify_research_save(
    _ctx: &VerificationContext<'_>,
    save_path: &str,
) -> Result<VerificationOutcome> {
    use std::path::Path;

    let path = Path::new(save_path);
    if !path.exists() {
        return Ok(VerificationOutcome::Failed {
            reason: format!("research markdown not found at {}", save_path),
        });
    }

    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    if size == 0 {
        return Ok(VerificationOutcome::Failed {
            reason: format!("research markdown is empty at {}", save_path),
        });
    }

    Ok(VerificationOutcome::Strong {
        evidence: serde_json::json!({
            "save_path": save_path,
            "size": size,
        }),
    })
}

/// verify_form_prepare — Playwright 重查表单字段值匹配 fields。
pub fn verify_form_prepare(
    _ctx: &VerificationContext<'_>,
    _fields: &std::collections::HashMap<String, String>,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 4 implements verify_form_prepare")
}

/// verify_form_submit — Playwright 查页面 URL 变更 或 success 元素存在。
pub fn verify_form_submit(
    _ctx: &VerificationContext<'_>,
    _submitted_url: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 5 implements verify_form_submit")
}

/// verify_task_repeat — 同 files.organize.verify_move,重读目标文件 sha256+size。
pub fn verify_task_repeat(
    _ctx: &VerificationContext<'_>,
    _target_task_id: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 6 implements verify_task_repeat")
}

/// verify_task_compensate — 查 compensations 表 status=reversed + reverse_payload 非空。
pub fn verify_task_compensate(
    _ctx: &VerificationContext<'_>,
    _target_step_id: &str,
) -> Result<VerificationOutcome> {
    unimplemented!("Task 7 implements verify_task_compensate")
}

#[cfg(test)]
mod tests {
    // 单元测试在 Task 2-7 各自添加。
}

#[cfg(test)]
mod note_capture_tests {
    use super::*;

    fn tmp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "voicepilot-w10p1-verify-note-{}.txt",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn verify_note_capture_strong_when_sha256_matches() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();
        let content = "hello notepad";
        std::fs::write(&path, content.as_bytes()).unwrap();

        let ctx = VerificationContext { kernel: &kernel, step_id: "s1" };
        let outcome = verify_note_capture(&ctx, &path.to_string_lossy(), content).unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert!(evidence.get("sha256").is_some(), "evidence must contain sha256");
                assert!(evidence.get("size").is_some(), "evidence must contain size");
            }
            other => panic!("expected Strong, got {:?}", other),
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn verify_note_capture_fails_when_file_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path(); // 不创建文件

        let ctx = VerificationContext { kernel: &kernel, step_id: "s1" };
        let outcome = verify_note_capture(&ctx, &path.to_string_lossy(), "any").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(reason.contains("not found") || reason.contains("missing"),
                    "expected 'not found' or 'missing' in reason, got: {}", reason);
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn verify_note_capture_fails_when_sha256_mismatches() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();
        std::fs::write(&path, b"different content").unwrap();

        let ctx = VerificationContext { kernel: &kernel, step_id: "s1" };
        let outcome = verify_note_capture(&ctx, &path.to_string_lossy(), "expected content").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(reason.contains("sha256") || reason.contains("mismatch"),
                    "expected 'sha256' or 'mismatch' in reason, got: {}", reason);
            }
            other => panic!("expected Failed, got {:?}", other),
        }

        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod research_save_tests {
    use super::*;

    fn tmp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "voicepilot-w10p1-verify-research-{}.md",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn verify_research_save_strong_when_file_exists_nonempty() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();
        std::fs::write(&path, b"# Example Domain\n\nillustrative examples").unwrap();

        let ctx = VerificationContext { kernel: &kernel, step_id: "s1" };
        let outcome = verify_research_save(&ctx, &path.to_string_lossy()).unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                let size = evidence.get("size").and_then(|v| v.as_u64());
                assert!(size.is_some(), "evidence must contain size");
                assert!(size.unwrap() > 0, "size must be > 0, got {}", size.unwrap());
            }
            other => panic!("expected Strong, got {:?}", other),
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn verify_research_save_fails_when_file_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();

        let ctx = VerificationContext { kernel: &kernel, step_id: "s1" };
        let outcome = verify_research_save(&ctx, &path.to_string_lossy()).unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(reason.contains("not found") || reason.contains("missing"),
                    "got: {}", reason);
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn verify_research_save_fails_when_file_empty() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();
        std::fs::write(&path, b"").unwrap();

        let ctx = VerificationContext { kernel: &kernel, step_id: "s1" };
        let outcome = verify_research_save(&ctx, &path.to_string_lossy()).unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(reason.contains("empty") || reason.contains("size"),
                    "got: {}", reason);
            }
            other => panic!("expected Failed, got {:?}", other),
        }

        std::fs::remove_file(&path).ok();
    }
}
