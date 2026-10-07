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

use crate::error::{KernelError, Result};
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
    Strong {
        evidence: Value,
    },
    #[allow(dead_code)]
    Medium {
        evidence: Value,
    },
    #[allow(dead_code)]
    Weak {
        reason: String,
    },
    Failed {
        reason: String,
    },
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

/// verify_form_prepare — Playwright eval 重查每个 selector 的值,与 fields 比较。
///
/// 通过 invoke_mcp_tool(playwright, eval, {script}) 查询所有字段当前值,
/// JSON 脚本返回 {selector: value} 映射。全部匹配 → Strong;任一不匹配 → Failed。
/// MCP 调用失败 → Err(KernelError::Mcp(...))。
pub fn verify_form_prepare(
    ctx: &VerificationContext<'_>,
    fields: &std::collections::HashMap<String, String>,
) -> Result<VerificationOutcome> {
    use crate::skills::common::invoke_mcp_tool;
    use std::collections::BTreeMap;

    // 构造 eval 脚本:对每个 selector,返回其当前 value。
    // 脚本返回 JSON 对象 {selector: value}。
    let sorted: BTreeMap<&String, &String> = fields.iter().collect();
    let selectors_vec: Vec<&str> = sorted.keys().map(|s| s.as_str()).collect();
    let selectors_json = serde_json::to_string(&selectors_vec)?;
    let script = format!(
        "Object.fromEntries({}.map(s => [s, document.querySelector(s)?.value || '']))",
        selectors_json
    );

    let result = invoke_mcp_tool(
        ctx.kernel,
        "playwright",
        "eval",
        serde_json::json!({"script": script}),
    )?;

    // eval 返回 {selector: value} 映射。
    let actual: std::collections::HashMap<String, String> = result
        .data
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut verified_count = 0u64;
    for (selector, expected_value) in &sorted {
        match actual.get(*selector) {
            Some(actual_value) if actual_value == expected_value.as_str() => {
                verified_count += 1;
            }
            Some(actual_value) => {
                return Ok(VerificationOutcome::Failed {
                    reason: format!(
                        "field {} mismatch: expected {:?} got {:?}",
                        selector, expected_value, actual_value
                    ),
                });
            }
            None => {
                return Ok(VerificationOutcome::Failed {
                    reason: format!("field {} not found in eval result", selector),
                });
            }
        }
    }

    Ok(VerificationOutcome::Strong {
        evidence: serde_json::json!({
            "verified_count": verified_count,
            "total_count": fields.len(),
        }),
    })
}

/// verify_form_submit — Playwright eval 查 document.URL 变更 或 success 元素存在。
///
/// 提交不可逆,verifier 必须验证提交确实发生。两种证据(任一满足即 Strong):
/// 1. document.URL != submitted_url(已跳转到 success/thank-you 页)
/// 2. document.querySelector('.success, [data-success="true"]') 存在
///
/// 都不满足 → Failed。
pub fn verify_form_submit(
    ctx: &VerificationContext<'_>,
    submitted_url: &str,
) -> Result<VerificationOutcome> {
    use crate::skills::common::invoke_mcp_tool;

    let script = "(function() { return {url: document.URL, success: !!(document.querySelector('.success, [data-success=\"true\"]'))}; })()";
    let result = invoke_mcp_tool(
        ctx.kernel,
        "playwright",
        "eval",
        serde_json::json!({"script": script}),
    )?;

    let current_url = result
        .data
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let has_success = result
        .data
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if current_url != submitted_url {
        return Ok(VerificationOutcome::Strong {
            evidence: serde_json::json!({
                "reason": "url_changed",
                "submitted_url": submitted_url,
                "current_url": current_url,
            }),
        });
    }

    if has_success {
        return Ok(VerificationOutcome::Strong {
            evidence: serde_json::json!({
                "reason": "success_element",
                "submitted_url": submitted_url,
            }),
        });
    }

    Ok(VerificationOutcome::Failed {
        reason: format!(
            "no evidence of submit success: url unchanged at {} and no success element",
            submitted_url
        ),
    })
}

/// verify_task_repeat — 查 target_task 的 effect_manifest,调 verify_move 重读目标文件。
///
/// 与 files.organize 的 verify_move 同源(spec §3.2 表格),重读目标文件
/// sha256+size,与 effect_manifest.sources 比较。通过 → Strong;失败 → Failed。
pub fn verify_task_repeat(
    ctx: &VerificationContext<'_>,
    target_task_id: &str,
) -> Result<VerificationOutcome> {
    use crate::policy::transaction::EffectManifest;

    let conn = ctx.kernel.conn();
    let manifest_str: Option<String> = conn
        .query_row(
            "SELECT effect_manifest FROM steps
             WHERE task_id = ?1 AND effect_manifest IS NOT NULL
             ORDER BY step_order DESC LIMIT 1",
            rusqlite::params![target_task_id],
            |r| r.get(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(KernelError::Db(other)),
        })?;

    let manifest_str = match manifest_str {
        Some(s) => s,
        None => {
            return Ok(VerificationOutcome::Failed {
                reason: format!("no effect_manifest found for task {}", target_task_id),
            });
        }
    };

    let manifest: EffectManifest = serde_json::from_str(&manifest_str)
        .map_err(|e| KernelError::Skill(format!("failed to parse effect_manifest: {}", e)))?;

    match ctx.kernel.filesystem().verify_move(&manifest) {
        Ok(_) => Ok(VerificationOutcome::Strong {
            evidence: serde_json::json!({
                "target_task_id": target_task_id,
                "verified_sources": manifest.sources.len(),
                "destination": manifest.destination,
            }),
        }),
        Err(e) => Ok(VerificationOutcome::Failed {
            reason: format!("verify_move failed: {}", e),
        }),
    }
}

/// verify_task_compensate — 查 compensations 表 status=reversed + reverse_payload 非空。
///
/// spec §6.3 Strong Verifier:commit(auto_reverse + mark_status "reversed")后,
/// 重新读取 compensations 表,确认:
/// 1. 存在 step_id = target_step_id 的记录
/// 2. status = "reversed"
/// 3. reverse_payload 非空(JSON 含 moves 数组,证明 reverse 操作已记录)
///
/// 三项全满足 → Strong;任一不满足 → Failed。
/// 注意:list_active_compensations 只返回 status='active' 的记录,不能用于验证 reversed,
/// 必须直接 SQL 查询 compensations 表(任何 status)。
///
/// **Stronghold 布局下 reverse_payload 列恒为空**(spec §2.2 明文不落库,payload
/// 只存在于 snapshot_encrypted 密文里)。此时验证对象是解出来的明文,所以这里不能
/// 拿 DB 明文列直接判空 —— 必须先经 vault 解密再校验。解密失败(vault 未注入 /
/// 未锁定 / 密文损坏)→ Failed,与 execute_compensate 的解密失败语义一致。
pub fn verify_task_compensate(
    ctx: &VerificationContext<'_>,
    target_step_id: &str,
) -> Result<VerificationOutcome> {
    let conn = ctx.kernel.conn();
    // snapshot_encrypted 一并取出:stronghold 布局下 reverse_payload 明文列为空,
    // 需要它走 vault 解密还原待验证的 payload。
    let row_result: rusqlite::Result<(String, String, Option<Vec<u8>>)> = conn.query_row(
        "SELECT status, reverse_payload, snapshot_encrypted FROM compensations
         WHERE step_id = ?1
         ORDER BY ttl_expires DESC LIMIT 1",
        rusqlite::params![target_step_id],
        |r| {
            let status: String = r.get(0)?;
            // reverse_payload 列在 migration 005 之前可能为 NULL,
            // 用 Option<String> 兜底再 unwrap_or_default。
            let reverse_payload: String = r.get::<_, Option<String>>(1)?.unwrap_or_default();
            let snapshot_encrypted: Option<Vec<u8>> = r.get(2)?;
            Ok((status, reverse_payload, snapshot_encrypted))
        },
    );

    let (status, reverse_payload, snapshot_encrypted) = match row_result {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Ok(VerificationOutcome::Failed {
                reason: format!("no compensation record found for step {}", target_step_id),
            });
        }
        Err(e) => return Err(KernelError::Db(e)),
    };

    if status != "reversed" {
        return Ok(VerificationOutcome::Failed {
            reason: format!(
                "compensation status for step {} is {:?}, expected \"reversed\"",
                target_step_id, status
            ),
        });
    }

    // 待验证的 payload:DB 明文列优先;为空且存在密文快照时经 vault 解密还原。
    let reverse_payload = if !reverse_payload.trim().is_empty() {
        reverse_payload
    } else {
        #[cfg(feature = "stronghold")]
        {
            match snapshot_encrypted.as_ref() {
                Some(blob) => {
                    let vault = ctx
                        .kernel
                        .stronghold_vault()
                        .filter(|v| v.is_unlocked())
                        .ok_or_else(|| {
                            KernelError::Compensation(
                                "stronghold vault not unlocked, cannot verify reverse_payload"
                                    .into(),
                            )
                        })?;
                    let payload: crate::crypto::stronghold::EncryptedPayload =
                        bincode::deserialize(blob).map_err(|e| {
                            KernelError::Compensation(format!(
                                "stronghold bincode decode failed while verifying: {}",
                                e
                            ))
                        })?;
                    let plaintext = vault.decrypt(&payload).map_err(|e| {
                        KernelError::Compensation(format!("stronghold decrypt failed: {}", e))
                    })?;
                    String::from_utf8(plaintext).map_err(|e| {
                        KernelError::Compensation(format!("plaintext not UTF-8: {}", e))
                    })?
                }
                // 无密文快照且明文列为空:payload 确实缺失。
                None => String::new(),
            }
        }
        #[cfg(not(feature = "stronghold"))]
        {
            let _ = &snapshot_encrypted;
            String::new()
        }
    };

    if reverse_payload.trim().is_empty() {
        return Ok(VerificationOutcome::Failed {
            reason: format!(
                "compensation reverse_payload for step {} is empty",
                target_step_id
            ),
        });
    }

    // 进一步验证 reverse_payload 是合法 JSON 且含 moves 数组(与 build_reverse_effect_manifest
    // 的解析逻辑一致),确保 payload 真实可执行(不只是非空字符串)。
    let payload: serde_json::Value = match serde_json::from_str(&reverse_payload) {
        Ok(v) => v,
        Err(e) => {
            return Ok(VerificationOutcome::Failed {
                reason: format!(
                    "compensation reverse_payload for step {} is not valid JSON: {}",
                    target_step_id, e
                ),
            });
        }
    };

    let moves_count = payload
        .get("moves")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    if moves_count == 0 {
        return Ok(VerificationOutcome::Failed {
            reason: format!(
                "compensation reverse_payload for step {} has empty or missing moves array",
                target_step_id
            ),
        });
    }

    Ok(VerificationOutcome::Strong {
        evidence: serde_json::json!({
            "target_step_id": target_step_id,
            "status": status,
            "moves_count": moves_count,
        }),
    })
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

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_note_capture(&ctx, &path.to_string_lossy(), content).unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert!(
                    evidence.get("sha256").is_some(),
                    "evidence must contain sha256"
                );
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

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_note_capture(&ctx, &path.to_string_lossy(), "any").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("not found") || reason.contains("missing"),
                    "expected 'not found' or 'missing' in reason, got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn verify_note_capture_fails_when_sha256_mismatches() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();
        std::fs::write(&path, b"different content").unwrap();

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome =
            verify_note_capture(&ctx, &path.to_string_lossy(), "expected content").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("sha256") || reason.contains("mismatch"),
                    "expected 'sha256' or 'mismatch' in reason, got: {}",
                    reason
                );
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

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
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

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_research_save(&ctx, &path.to_string_lossy()).unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("not found") || reason.contains("missing"),
                    "got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn verify_research_save_fails_when_file_empty() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let path = tmp_path();
        std::fs::write(&path, b"").unwrap();

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_research_save(&ctx, &path.to_string_lossy()).unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("empty") || reason.contains("size"),
                    "got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }

        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod form_prepare_tests {
    use super::*;
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

    /// Mock Playwright 脚本:对 eval 调用,返回 FORM_VALUES_PATH 中预存的
    /// 字段值 JSON(由测试预先写入)。
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
            values_path = os.environ.get("FORM_VALUES_PATH")
            values = {}
            if values_path and os.path.exists(values_path):
                try:
                    with open(values_path, "r", encoding="utf-8") as f:
                        values = json.load(f)
                except Exception:
                    values = {}
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "result": {"content": [{"type": "text", "text": json.dumps(values)}],
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

    fn set_values_env(
        temp: &std::path::Path,
        values: &HashMap<String, String>,
    ) -> std::path::PathBuf {
        let path = temp.join(format!("values-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, serde_json::to_string(values).unwrap()).unwrap();
        unsafe {
            std::env::set_var("FORM_VALUES_PATH", &path);
        }
        path
    }

    fn clear_values_env() {
        unsafe {
            std::env::remove_var("FORM_VALUES_PATH");
        }
    }

    #[test]
    fn verify_form_prepare_strong_when_all_fields_match() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);

        let mut fields = HashMap::new();
        fields.insert("#username".to_string(), "alice".to_string());
        fields.insert("#email".to_string(), "alice@example.com".to_string());
        set_values_env(temp.path(), &fields);

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_form_prepare(&ctx, &fields).unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert_eq!(
                    evidence.get("verified_count").and_then(|v| v.as_u64()),
                    Some(2)
                );
            }
            other => panic!("expected Strong, got {:?}", other),
        }

        clear_values_env();
    }

    #[test]
    fn verify_form_prepare_fails_when_field_value_mismatches() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);

        let mut expected = HashMap::new();
        expected.insert("#username".to_string(), "alice".to_string());
        let mut actual = HashMap::new();
        actual.insert("#username".to_string(), "bob".to_string());
        set_values_env(temp.path(), &actual);

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_form_prepare(&ctx, &expected).unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("#username") || reason.contains("mismatch"),
                    "got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }

        clear_values_env();
    }
}

#[cfg(test)]
mod form_submit_tests {
    use super::*;
    use crate::kernel::TrustKernel;
    use crate::mcp::repo::McpServerRepo;
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

    /// Mock:eval 返回 {url: MOCK_CURRENT_URL, success: MOCK_HAS_SUCCESS=="true"}。
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
            current_url = os.environ.get("MOCK_CURRENT_URL", "https://example.com/submit")
            has_success = os.environ.get("MOCK_HAS_SUCCESS", "false") == "true"
            payload = json.dumps({"url": current_url, "success": has_success})
            emit({"jsonrpc": "2.0", "id": msg.get("id"),
                  "result": {"content": [{"type": "text", "text": payload}],
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
    fn verify_form_submit_strong_when_url_changed() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);
        unsafe {
            std::env::set_var("MOCK_CURRENT_URL", "https://example.com/success");
        }
        unsafe {
            std::env::set_var("MOCK_HAS_SUCCESS", "false");
        }

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_form_submit(&ctx, "https://example.com/submit").unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert_eq!(
                    evidence.get("reason").and_then(|v| v.as_str()),
                    Some("url_changed")
                );
            }
            other => panic!("expected Strong, got {:?}", other),
        }

        unsafe {
            std::env::remove_var("MOCK_CURRENT_URL");
        }
        unsafe {
            std::env::remove_var("MOCK_HAS_SUCCESS");
        }
    }

    #[test]
    fn verify_form_submit_strong_when_success_element_present() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);
        unsafe {
            std::env::set_var("MOCK_CURRENT_URL", "https://example.com/submit");
        }
        unsafe {
            std::env::set_var("MOCK_HAS_SUCCESS", "true");
        }

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_form_submit(&ctx, "https://example.com/submit").unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert_eq!(
                    evidence.get("reason").and_then(|v| v.as_str()),
                    Some("success_element")
                );
            }
            other => panic!("expected Strong, got {:?}", other),
        }

        unsafe {
            std::env::remove_var("MOCK_CURRENT_URL");
        }
        unsafe {
            std::env::remove_var("MOCK_HAS_SUCCESS");
        }
    }

    #[test]
    fn verify_form_submit_fails_when_url_unchanged_and_no_success() {
        if !python_available() {
            eprintln!("skipping: python not on PATH");
            return;
        }
        let _guard = CWD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let kernel = TrustKernel::open_in_memory().unwrap();
        install_mock(&kernel);
        unsafe {
            std::env::set_var("MOCK_CURRENT_URL", "https://example.com/submit");
        }
        unsafe {
            std::env::set_var("MOCK_HAS_SUCCESS", "false");
        }

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "s1",
        };
        let outcome = verify_form_submit(&ctx, "https://example.com/submit").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(reason.contains("no evidence"), "got: {}", reason);
            }
            other => panic!("expected Failed, got {:?}", other),
        }

        unsafe {
            std::env::remove_var("MOCK_CURRENT_URL");
        }
        unsafe {
            std::env::remove_var("MOCK_HAS_SUCCESS");
        }
    }
}

#[cfg(test)]
mod task_repeat_tests {
    use super::*;
    use crate::kernel::TrustKernel;
    use crate::policy::transaction::EffectManifest;
    use crate::repo::step_repo::StepRecord;
    use crate::tools::fs_paths::canonicalize;
    use crate::tools::fs_snapshot::snapshot_file;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voicepilot-w10p1-verify-repeat-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_manifest(
        src_dir: &std::path::Path,
        dest_dir: &std::path::Path,
        names: &[&str],
    ) -> EffectManifest {
        let mut snapshots = Vec::new();
        let mut total = 0;
        for n in names {
            let snap = snapshot_file(&src_dir.join(n)).unwrap();
            total += snap.size;
            snapshots.push(snap);
        }
        EffectManifest {
            sources: snapshots,
            destination: canonicalize(&dest_dir.to_string_lossy()),
            conflicts: vec![],
            total_bytes: total,
        }
    }

    fn persist_previous_step(kernel: &TrustKernel, manifest: &EffectManifest) {
        kernel
            .create_task("prev-task", "previous organize")
            .unwrap();
        let mut s = StepRecord::new("prev-step", "prev-task", 1);
        s.effect_manifest = Some(serde_json::to_value(manifest).unwrap());
        kernel.create_step(&s).unwrap();
    }

    #[test]
    fn verify_task_repeat_strong_when_verify_move_passes() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let dir = tmp_dir();
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        let dest = dir.join("out");
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("a.pdf"), b"pdf1").unwrap();
        fs::write(dest.join("a.pdf"), b"pdf1").unwrap();

        let manifest = build_manifest(&src, &dest, &["a.pdf"]);
        persist_previous_step(&kernel, &manifest);

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_repeat(&ctx, "prev-task").unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert!(evidence.get("verified_sources").is_some());
            }
            other => panic!("expected Strong, got {:?}", other),
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_task_repeat_fails_when_destination_missing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let dir = tmp_dir();
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        let dest = dir.join("out");
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("a.pdf"), b"pdf1").unwrap();
        // 不在 dest 写文件 → verify_move 失败
        let manifest = build_manifest(&src, &dest, &["a.pdf"]);
        persist_previous_step(&kernel, &manifest);

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_repeat(&ctx, "prev-task").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("verify_move")
                        || reason.contains("missing")
                        || reason.contains("not found"),
                    "got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_task_repeat_fails_when_no_previous_manifest() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_repeat(&ctx, "nonexistent-task").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("no effect_manifest") || reason.contains("not found"),
                    "got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod task_compensate_tests {
    use super::*;
    use crate::compensation::types::{CompensationLevel, ConflictPolicy};
    use crate::kernel::TrustKernel;
    use crate::repo::step_repo::StepRecord;
    use crate::skills::common::create_post_commit_compensation;
    use std::path::PathBuf;

    /// Set up a previous task + step, then create a compensation record
    /// for `target_step_id` with the given moved paths. Returns the comp_id.
    /// The compensation record is initially "active".
    fn setup_compensation(
        kernel: &TrustKernel,
        target_step_id: &str,
        moved: &[(PathBuf, PathBuf)],
    ) -> String {
        kernel
            .create_task("prev-task", "previous organize")
            .unwrap();
        kernel
            .create_step(&StepRecord::new(target_step_id, "prev-task", 1))
            .unwrap();
        create_post_commit_compensation(
            kernel,
            target_step_id,
            moved,
            "filesystem.reverse_move",
            CompensationLevel::Strong,
            ConflictPolicy::AutoReverse,
            3600,
        )
        .unwrap()
    }

    #[test]
    fn verify_task_compensate_strong_when_status_reversed_and_payload_nonempty() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let moved: Vec<(PathBuf, PathBuf)> =
            vec![(PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf"))];
        let comp_id = setup_compensation(&kernel, "prev-step", &moved);

        // Mark the compensation as "reversed"(模拟 auto_reverse 已执行)。
        kernel
            .mark_compensation_status(&comp_id, "reversed")
            .unwrap();

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_compensate(&ctx, "prev-step").unwrap();

        match outcome {
            VerificationOutcome::Strong { evidence } => {
                assert_eq!(
                    evidence.get("status").and_then(|v| v.as_str()),
                    Some("reversed")
                );
                assert_eq!(
                    evidence.get("moves_count").and_then(|v| v.as_u64()),
                    Some(1)
                );
                assert_eq!(
                    evidence.get("target_step_id").and_then(|v| v.as_str()),
                    Some("prev-step")
                );
            }
            other => panic!("expected Strong, got {:?}", other),
        }
    }

    #[test]
    fn verify_task_compensate_fails_when_status_active() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let moved: Vec<(PathBuf, PathBuf)> =
            vec![(PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf"))];
        // 创建后不调用 mark_compensation_status,status 仍为 "active"。
        let _comp_id = setup_compensation(&kernel, "prev-step", &moved);

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_compensate(&ctx, "prev-step").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("reversed") || reason.contains("status"),
                    "expected 'reversed' or 'status' in reason, got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn verify_task_compensate_fails_when_no_record_exists() {
        let kernel = TrustKernel::open_in_memory().unwrap();

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_compensate(&ctx, "nonexistent-step").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("no compensation") || reason.contains("not found"),
                    "expected 'no compensation' or 'not found' in reason, got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn verify_task_compensate_fails_when_payload_empty() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // 创建带空 moves 的 compensation record(create_post_commit_compensation 允许空 moves)。
        let moved: Vec<(PathBuf, PathBuf)> = vec![];
        let comp_id = setup_compensation(&kernel, "prev-step", &moved);

        // 标记为 reversed,但 reverse_payload 的 moves 数组为空。
        kernel
            .mark_compensation_status(&comp_id, "reversed")
            .unwrap();

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        let outcome = verify_task_compensate(&ctx, "prev-step").unwrap();

        match outcome {
            VerificationOutcome::Failed { reason } => {
                assert!(
                    reason.contains("moves") || reason.contains("empty"),
                    "expected 'moves' or 'empty' in reason, got: {}",
                    reason
                );
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    /// W9 回归:stronghold 布局下 reverse_payload 明文列恒为空(spec §2.2),
    /// 验证器必须经 vault 解密密文快照后再校验,不能拿空明文列直接判空。
    /// 这条曾在 `--features stronghold` 下把 execute_compensate 整条链判失败。
    #[cfg(feature = "stronghold")]
    #[test]
    fn verify_task_compensate_strong_when_payload_only_in_encrypted_snapshot() {
        use crate::repo::config_repo::ConfigRepo;

        let kernel = TrustKernel::open_in_memory().unwrap();

        // 独立 vault_path,避免与其他并行用例抢默认 data_dir。
        let tmp = tempfile::tempdir().unwrap();
        let vault_path_str = tmp
            .path()
            .join("stronghold.bin")
            .to_string_lossy()
            .to_string();
        {
            let conn = kernel.conn();
            ConfigRepo::new()
                .set(&conn, "stronghold.vault_path", &vault_path_str)
                .unwrap();
        }
        // create() 返回的 vault 已是解锁态(key material 已派生)。
        let vault = {
            let conn = kernel.conn();
            crate::crypto::stronghold::StrongholdVault::create("test-pass", &conn)
                .expect("create vault")
        };
        assert!(
            vault.is_unlocked(),
            "precondition: created vault must be unlocked"
        );
        kernel.set_stronghold_vault(Some(std::sync::Arc::new(vault)));

        let moved: Vec<(PathBuf, PathBuf)> =
            vec![(PathBuf::from("src/a.pdf"), PathBuf::from("out/a.pdf"))];
        // 加密布局:明文列留空,payload 只在 snapshot_encrypted 里。
        let comp_id = setup_compensation(&kernel, "prev-step", &moved);
        {
            let rec = kernel.get_compensation(&comp_id).unwrap().unwrap();
            assert!(
                rec.reverse_payload.is_empty(),
                "precondition: stronghold 布局不应落明文"
            );
            assert!(
                rec.snapshot_encrypted.is_some(),
                "precondition: 应有密文快照"
            );
        }
        kernel
            .mark_compensation_status(&comp_id, "reversed")
            .unwrap();

        let ctx = VerificationContext {
            kernel: &kernel,
            step_id: "new-step",
        };
        match verify_task_compensate(&ctx, "prev-step").unwrap() {
            VerificationOutcome::Strong { evidence } => assert_eq!(
                evidence.get("moves_count").and_then(|v| v.as_u64()),
                Some(1),
            ),
            other => panic!(
                "expected Strong via vault-decrypted payload, got {:?}",
                other
            ),
        }
    }
}
