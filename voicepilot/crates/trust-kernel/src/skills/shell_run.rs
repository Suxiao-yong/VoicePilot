//! Daisy 移植：终端命令（shell.run）。
//!
//! 跑 `cmd /c` 命令：装软件、管文件、跑脚本——Daisy 里兜底一切的入口。
//! PerStep 审批 + 超时杀掉 + 输出截断 + 审计留痕。危险命令靠审批拦截，
//! 内核不做语义黑名单（黑名单永远漏，见 Daisy 的零设防教训——反向极端也
//! 不可取；审批人看得见完整命令）。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{
    SimpleInput, run_cmd, run_simple, simple_manifest, slot_text, truncate_chars,
};
use std::collections::HashMap;

pub fn shell_run_manifest() -> SkillManifest {
    simple_manifest(
        "shell.run",
        "跑命令",
        "执行终端命令（cmd），可装软件、管文件、跑脚本。60 秒超时自动杀掉。PerStep 审批，命令全文进审计。",
        &["命令", "终端", "cmd", "运行脚本"],
        &["运行这个命令", "帮我执行一条终端命令"],
        vec![
            SimpleInput {
                name: "command",
                input_type: SkillInputType::Text,
                required: true,
                max_length: Some(2000),
                allowed_roots: vec![],
            },
            SimpleInput {
                name: "timeout_secs",
                input_type: SkillInputType::Number,
                required: false,
                max_length: None,
                allowed_roots: vec![],
            },
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_shell_run(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let command = slot_text(inputs, "command")?;
    if command.trim().is_empty() {
        return Err(KernelError::Skill("command 不能为空".to_string()));
    }
    let timeout = inputs
        .get("timeout_secs")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(60)
        .clamp(5, 600) as u64;
    let mut map = HashMap::new();
    map.insert("command".to_string(), serde_json::json!(command));
    map.insert("timeout_secs".to_string(), serde_json::json!(timeout));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "shell.run",
        &format!("cmd:{command}"),
        true,
        &shell_run_manifest(),
        &map,
        || {
            // 经 cmd /c：内置命令（dir/echo）与 exe 一视同仁。
            let out = run_cmd("cmd", &["/c", command.trim()], timeout, 6000)?;
            if out.trim().is_empty() {
                return Ok("命令执行成功（无输出）".to_string());
            }
            Ok(format!("命令输出：\n{}", truncate_chars(&out, 6000)))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::{AutoApprover, AutoDenier};
    use crate::kernel::TrustKernel;

    #[test]
    fn shell_manifest_has_id() {
        let m = shell_run_manifest();
        assert_eq!(m.id, "shell.run");
        assert!(!m.keywords.is_empty());
    }

    #[test]
    fn shell_echo_roundtrip() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"command": "echo vp-hello-你好"});
        let out = execute_shell_run(&kernel, &AutoApprover, "t1", "s1", &inputs).unwrap();
        assert!(out.contains("vp-hello-你好"), "{out}");
    }

    #[test]
    fn shell_denied_runs_nothing() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let f = std::env::temp_dir().join(format!("vp-shell-denied-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let inputs = serde_json::json!({"command": format!("echo x > {}", f.display())});
        let err = execute_shell_run(&kernel, &AutoDenier, "t2", "s2", &inputs).unwrap_err();
        assert!(format!("{err:?}").contains("denied"), "{err:?}");
        assert!(!f.exists());
    }

    #[test]
    fn shell_timeout_kills() {
        // ping -n 30 ≈ 30 秒；timeout 5 秒必杀。Windows 专属，别的平台跳过。
        if !cfg!(windows) {
            return;
        }
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"command": "ping -n 30 127.0.0.1 >nul", "timeout_secs": 5});
        let err = execute_shell_run(&kernel, &AutoApprover, "t3", "s3", &inputs).unwrap_err();
        assert!(format!("{err:?}").contains("超时"), "{err:?}");
    }
}
