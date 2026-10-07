//! Daisy 能力移植共用骨架：manifest 构造 + 执行骨架 + 本机调用工具。
//!
//! 审批姿态矩阵（只读先例见 task.explain：ApprovalMode::None）：
//! - 只读本地 Skill（时间/前台应用/诊断/读剪贴板/读文件/列目录/纯 HTTP
//!   查询）：ApprovalMode::None + verifier "none"，不弹审批打扰用户；
//!   任务与步骤照常落库审计（可见、可追溯）。
//! - 一切外部发射与写入（开网址/打字按键/写文件/删文件/shell/定时任务/
//!   关应用/下载转码）：ApprovalMode::PerStep，用户逐次确认。

use crate::approval::approver::Approver;
use crate::approval::types::{ApprovalDecision, ApprovalScope};
use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::policy::transaction::EffectManifest;
use crate::policy::types::{DLevel, ELevel};
use crate::repo::step_repo::{StepRecord, StepStatus};
use crate::skills::common::{
    ApprovalContext, finalize_step_success, record_approval_decision,
    validate_input_against_manifest,
};
use crate::skills::manifest::{
    ApprovalConfig, ApprovalMode, CompensationConfig, EgressKind, FailurePolicy, SkillInput,
    SkillInputType, SkillManifest, VerifierConfig,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 审批白名单（单一事实来源：2026 用户决定）。
///
/// “任何操作都审批”已取消，仅保留文件类 + shell 的逐次审批：
/// fs.write_file / fs.create_file / fs.delete_file（改/删文件）与 shell.run（可删任意文件）。
/// files.organize（BatchOnce）与 form.submit（E3，PerStep）在 manifest.rs 单独保留。
/// 名单之外（sys/clip/web/media/pim/doc/app_control/note_capture/…）：
/// manifest.mode=None + run_simple 不弹审批，仅审计留痕。
pub fn approval_required(skill_id: &str) -> bool {
    matches!(
        skill_id,
        "fs.write_file" | "fs.create_file" | "fs.delete_file" | "shell.run"
    )
}

/// manifest 输入槽位精简描述。
pub struct SimpleInput {
    pub name: &'static str,
    pub input_type: SkillInputType,
    pub required: bool,
    pub max_length: Option<u32>,
    pub allowed_roots: Vec<String>,
}

/// 构造移植 Skill 的 manifest：固定 version/E2-D2 上限以外的档位。
/// `needs_approval=false` 走只读档（E0/D1 + None + verifier none，
/// 与 task.explain 一致）；否则 E2/D2 + verifier weak，审批模式由
/// `approval_required(id)` 白名单决定（名单内 PerStep，名单外 None）。
#[allow(clippy::too_many_arguments)]
pub fn simple_manifest(
    id: &str,
    title: &str,
    description: &str,
    keywords: &[&str],
    intent_examples: &[&str],
    inputs: Vec<SimpleInput>,
    needs_approval: bool,
    egress: EgressKind,
    tools: &[&str],
) -> SkillManifest {
    let mut map = HashMap::new();
    for i in inputs {
        map.insert(
            i.name.to_string(),
            SkillInput {
                input_type: i.input_type,
                required: i.required,
                allowed_roots: i.allowed_roots,
                allowed_values: vec![],
                max_length: i.max_length,
                default: None,
            },
        );
    }
    let (risk, data, approval, verifier) = if needs_approval {
        (
            ELevel::E2,
            DLevel::D2,
            ApprovalConfig {
                // 是否弹审批由白名单决定：名单内 PerStep，名单外 None。
                mode: if approval_required(id) {
                    ApprovalMode::PerStep
                } else {
                    ApprovalMode::None
                },
                required_for: "commit".to_string(),
                show_effect_manifest: approval_required(id),
                max_approval_scope: 1,
            },
            VerifierConfig {
                strategy: "weak".to_string(),
                recheck_after_seconds: 0,
            },
        )
    } else {
        (
            ELevel::E0,
            DLevel::D1,
            ApprovalConfig {
                mode: ApprovalMode::None,
                required_for: "commit".to_string(),
                show_effect_manifest: false,
                max_approval_scope: 0,
            },
            VerifierConfig {
                strategy: "none".to_string(),
                recheck_after_seconds: 0,
            },
        )
    };
    SkillManifest {
        id: id.to_string(),
        version: "1.0.0".to_string(),
        title: title.to_string(),
        description: description.to_string(),
        description_body: None,
        execution: None,
        intent_examples: intent_examples.iter().map(|s| s.to_string()).collect(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        inputs: map,
        risk_ceiling: risk,
        data_class_ceiling: data,
        egress,
        max_steps: 1,
        tools: tools.iter().map(|s| s.to_string()).collect(),
        approval,
        compensation: CompensationConfig {
            level: CompensationLevel::None,
            ttl_seconds: 0,
            conflict_policy: ConflictPolicy::AutoReverse,
        },
        verifier,
        failure_policy: FailurePolicy {
            max_retries: 0,
            allow_replan: false,
            on_fail: "ask_user".to_string(),
        },
    }
}

/// 标准执行骨架：校验输入 → 建任务/步骤 → 可选 PerStep 审批 → act →
/// 成功落 weak 证据。失败一律标 Failed 并返回原因（fail-closed，
/// 不撒谎）。返回 act 的人类可读输出，由 dispatcher 装入 output。
#[allow(clippy::too_many_arguments)]
pub fn run_simple(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    skill_id: &str,
    destination: &str,
    needs_approval: bool,
    manifest: &SkillManifest,
    inputs: &HashMap<String, serde_json::Value>,
    act: impl FnOnce() -> Result<String>,
) -> Result<String> {
    validate_input_against_manifest(inputs, manifest)?;
    kernel.create_task(task_id, &format!("{skill_id}:{destination}"))?;
    kernel.create_step(&StepRecord::new(
        step_id.to_string(),
        task_id.to_string(),
        1,
    ))?;
    kernel.update_step_status(step_id, StepStatus::Running)?;
    // 审批门：needs_approval（风险档）且在白名单内才真正弹窗。
    if needs_approval && approval_required(skill_id) {
        let mut hasher = Sha256::new();
        hasher.update(skill_id.as_bytes());
        hasher.update(b"\x00");
        hasher.update(destination.as_bytes());
        let preconditions_hash = format!("{:x}", hasher.finalize());
        let effect = EffectManifest {
            sources: vec![],
            destination: destination.to_string(),
            conflicts: vec![],
            total_bytes: 0,
        };
        let ctx = ApprovalContext {
            task_id,
            step_id,
            destination: &effect.destination,
            preconditions_hash: &preconditions_hash,
            e_level: ELevel::E2,
            d_level: DLevel::D2,
            approval_scope: ApprovalScope::Single,
        };
        match record_approval_decision(kernel, approver, &effect, &ctx)?.user_decision {
            ApprovalDecision::Allow => {}
            ApprovalDecision::Deny => {
                kernel.update_step_status(step_id, StepStatus::Cancelled)?;
                return Err(KernelError::Skill("user denied".to_string()));
            }
            ApprovalDecision::Modify => {
                kernel.update_step_status(step_id, StepStatus::Cancelled)?;
                return Err(KernelError::Skill("modify not supported".to_string()));
            }
        }
    }
    let out = act().inspect_err(|_| {
        let _ = kernel.update_step_status(step_id, StepStatus::Failed);
    })?;
    finalize_step_success(kernel, step_id, "weak", None).inspect_err(|_| {
        let _ = kernel.update_step_status(step_id, StepStatus::Failed);
    })?;
    Ok(out)
}

/// 从 dispatcher 的 resolved_input 取文本槽位。
pub fn slot_text(inputs: &serde_json::Value, name: &str) -> Result<String> {
    inputs
        .get(name)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| KernelError::Skill(format!("missing required slot: {name}")))
}

/// 可选文本槽位（缺席/空白 → None）。
pub fn slot_text_opt(inputs: &serde_json::Value, name: &str) -> Option<String> {
    inputs
        .get(name)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 带超时的本机进程调用。输出按字符截断并注明；超时杀掉报原因。
/// Windows 自带程序（curl/schtasks/taskkill/msg/powershell）零依赖可用。
pub fn run_cmd(
    program: &str,
    args: &[&str],
    timeout_secs: u64,
    max_chars: usize,
) -> Result<String> {
    use std::io::Read;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            KernelError::Skill(format!(
                "启动 {program} 失败: {e}（是否未安装或不在 PATH？）"
            ))
        })?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child
            .try_wait()
            .map_err(|e| KernelError::Skill(format!("等待 {program} 失败: {e}")))?
        {
            Some(status) => {
                let mut buf = Vec::new();
                if let Some(mut so) = child.stdout.take() {
                    let _ = so.read_to_end(&mut buf);
                }
                let mut err = Vec::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_end(&mut err);
                }
                // 子进程已退出：管道里只剩有限缓冲，读完安全。
                let text = String::from_utf8_lossy(&buf).into_owned();
                if !status.success() {
                    let e = String::from_utf8_lossy(&err).into_owned();
                    return Err(KernelError::Skill(format!(
                        "{program} 退出码 {status}: {}",
                        truncate_chars(&e, 500)
                    )));
                }
                return Ok(truncate_chars(&text, max_chars));
            }
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KernelError::Skill(format!(
                    "{program} 超时（>{timeout_secs}s），已终止"
                )));
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// HTTP GET（走系统 curl.exe，无新依赖）。Windows 10+ 自带。
pub fn curl_get(url: &str, max_chars: usize) -> Result<String> {
    run_cmd(
        "curl.exe",
        &[
            "-sSL",
            "--max-time",
            "25",
            "--max-filesize",
            &max_chars.to_string(),
            "--compressed",
            "-A",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
            url,
        ],
        35,
        max_chars,
    )
    .map_err(|e| KernelError::Skill(format!("下载失败 {url}: {e}")))
}

/// PowerShell 单行求值（剪贴板/按键/前台窗口等零依赖系统调用走这里）。
pub fn ps_eval(script: &str) -> Result<String> {
    run_cmd(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        30,
        8000,
    )
}

/// STA 版 powershell（剪贴板 cmdlet / Office COM 必须 STA；控制台默认 MTA 用不了）。
pub fn ps_sta(script: &str) -> Result<String> {
    run_cmd(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
        30,
        8000,
    )
}

/// 按字符截断（中文不拦腰），超限注记。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}\n…（已截断，只显示前 {max} 字）")
}

/// 极简 HTML→文本（无新依赖；深度抓取走 research.save_markdown）。
pub fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(8000));
    let mut in_tag = false;
    let lower = html.to_lowercase();
    // 先整块扔掉 script/style（大小写不敏感扫描）。
    let mut cleaned = String::with_capacity(html.len());
    let mut rest = lower.as_str();
    let mut orig = html;
    while let Some(pos) = {
        let s = rest.find("<script");
        let t = rest.find("<style");
        // 取位置更靠前的块（或逻辑会漏掉前面的 style）。
        match (s, t) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    } {
        cleaned.push_str(&orig[..pos]);
        let tag = if rest[pos..].starts_with("<script") {
            "</script>"
        } else {
            "</style>"
        };
        match rest[pos..].find(tag) {
            Some(end) => {
                let skip = pos + end + tag.len();
                orig = &orig[skip..];
                rest = &rest[skip..];
            }
            None => {
                orig = "";
                rest = "";
            }
        }
    }
    cleaned.push_str(orig);
    for ch in cleaned.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // 解常见实体 + 压空白。
    let text = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 外部二进制供给（对标 Daisy ffmpegManager/yt-dlp 打包）：优先 PATH，
/// 缺席则从 GitHub 自动下载 + SHA-256 校验 + 健康检查，落到
/// `%LOCALAPPDATA%/voicepilot/tools/bin`。下载发生在已审批的 act 内，
/// 校验失败 fail-closed。仅 x64 Windows。
pub fn tools_bin_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| KernelError::Skill("取不到数据目录".to_string()))?;
    let dir = base.join("voicepilot").join("tools").join("bin");
    std::fs::create_dir_all(&dir)
        .map_err(|e| KernelError::Skill(format!("建工具目录失败: {e}")))?;
    Ok(dir)
}

/// `certutil -hashfile` 取 SHA-256（系统自带，纯函数外壳可测解析）。
pub fn parse_certutil_sha256(output: &str) -> Option<String> {
    output
        .lines()
        .map(|l| l.trim().replace(' ', ""))
        .find(|l| l.len() == 64 && l.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn sha256_of_file(path: &std::path::Path) -> Result<String> {
    let out = run_cmd(
        "certutil",
        &["-hashfile", &path.to_string_lossy(), "SHA256"],
        120,
        500,
    )?;
    parse_certutil_sha256(&out)
        .ok_or_else(|| KernelError::Skill("certutil 未输出 SHA-256".to_string()))
}

/// SUMS 文件行解析：`"<hash>  <asset>"` → 指定 asset 的 hash。纯函数。
pub fn parse_sums_file(sums: &str, asset: &str) -> Option<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        let (hash, name) = (parts.next()?, parts.next()?);
        if name.trim_start_matches('*') == asset
            && hash.len() == 64
            && hash.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Some(hash.to_lowercase());
        }
    }
    None
}

/// 下载 + 验 SHA + 落盘。url 须为 https（调用方保证）。
pub fn download_verified(url: &str, asset: &str, sums_url: &str, dest: &Path) -> Result<()> {
    let sums = curl_get(sums_url, 100_000)
        .map_err(|e| KernelError::Skill(format!("下校验文件失败 {sums_url}: {e}")))?;
    let expect = parse_sums_file(&sums, asset)
        .ok_or_else(|| KernelError::Skill(format!("校验文件里找不到 {asset}，拒绝下载")))?;
    // 先下到临时文件，验过再落位。
    let tmp = dest.with_extension(format!(
        "dl-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    run_cmd(
        "curl.exe",
        &[
            "-sSL",
            "--max-time",
            "900",
            "-A",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
            "-o",
            &tmp.to_string_lossy(),
            url,
        ],
        920,
        500,
    )
    .map_err(|e| KernelError::Skill(format!("下载失败 {url}: {e}")))?;
    let actual = sha256_of_file(&tmp)?;
    if actual.to_lowercase() != expect {
        let _ = std::fs::remove_file(&tmp);
        return Err(KernelError::Skill(format!(
            "SHA-256 校验失败（期望 {expect}，实际 {actual}），已删除下载文件"
        )));
    }
    if dest.is_file() {
        let _ = std::fs::remove_file(dest);
    }
    std::fs::rename(&tmp, dest).map_err(|e| KernelError::Skill(format!("落盘失败: {e}")))?;
    Ok(())
}

/// yt-dlp：PATH 优先，否则自动供给（GitHub 最新稳定版 + SHA 校验）。
pub fn ensure_yt_dlp() -> Result<PathBuf> {
    if run_cmd("yt-dlp", &["--version"], 10, 100).is_ok() {
        return Ok(PathBuf::from("yt-dlp"));
    }
    let dest = tools_bin_dir()?.join("yt-dlp.exe");
    if !(dest.is_file() && run_cmd(&dest.to_string_lossy(), &["--version"], 15, 100).is_ok()) {
        let base = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
        download_verified(
            &format!("{base}/yt-dlp.exe"),
            "yt-dlp.exe",
            &format!("{base}/SHA2-256SUMS"),
            &dest,
        )?;
        // 健康检查。
        run_cmd(&dest.to_string_lossy(), &["--version"], 15, 100)
            .map_err(|e| KernelError::Skill(format!("yt-dlp 健康检查失败: {e}")))?;
    }
    Ok(dest)
}

/// ffmpeg：PATH 优先，否则自动供给（BtbN 最新 GPL 包 + SHA 校验 + 解压）。
/// 返回 ffmpeg.exe 所在目录（给 yt-dlp `--ffmpeg-location` 用）与 exe 路径。
pub fn ensure_ffmpeg() -> Result<(PathBuf, PathBuf)> {
    if run_cmd("ffmpeg", &["-version"], 10, 200).is_ok() {
        // PATH 版：目录取 exe 所在（解析 where 输出首行）。
        if let Ok(out) = run_cmd("where", &["ffmpeg"], 10, 500) {
            if let Some(first) = out.lines().next().map(|l| l.trim().to_string()) {
                let p = PathBuf::from(&first);
                if p.is_file() {
                    if let Some(dir) = p.parent() {
                        return Ok((dir.to_path_buf(), p));
                    }
                }
            }
        }
        return Ok((PathBuf::from("."), PathBuf::from("ffmpeg")));
    }
    let dir = tools_bin_dir()?.join("ffmpeg");
    let exe = dir.join("ffmpeg.exe");
    if !(exe.is_file() && run_cmd(&exe.to_string_lossy(), &["-version"], 15, 200).is_ok()) {
        std::fs::create_dir_all(&dir)
            .map_err(|e| KernelError::Skill(format!("建 ffmpeg 目录失败: {e}")))?;
        let asset = "ffmpeg-master-latest-win64-gpl.zip";
        let base = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
        let zip = dir.join(asset);
        download_verified(
            &format!("{base}/{asset}"),
            asset,
            &format!("{base}/SHA256SUMS"),
            &zip,
        )?;
        // 系统自带解压（零新依赖）。
        run_cmd(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    zip.to_string_lossy().replace('\'', "''"),
                    dir.to_string_lossy().replace('\'', "''")
                ),
            ],
            300,
            500,
        )
        .map_err(|e| KernelError::Skill(format!("解压 ffmpeg 失败: {e}")))?;
        // zip 内为 ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe：上移。
        if !exe.is_file() {
            let mut found = None;
            let walker = walkdir::WalkDir::new(&dir).max_depth(3).follow_links(false);
            for entry in walker.into_iter().filter_map(|e| e.ok()) {
                if entry.file_name().to_string_lossy() == "ffmpeg.exe" {
                    found = Some(entry.path().to_path_buf());
                    break;
                }
            }
            if let Some(src) = found {
                std::fs::rename(&src, &exe)
                    .map_err(|e| KernelError::Skill(format!("搬运 ffmpeg 失败: {e}")))?;
            }
        }
        let _ = std::fs::remove_file(&zip);
        run_cmd(&exe.to_string_lossy(), &["-version"], 15, 200)
            .map_err(|e| KernelError::Skill(format!("ffmpeg 健康检查失败: {e}")))?;
    }
    Ok((dir, exe))
}

/// 检测外部二进制（yt-dlp/ffmpeg/soffice），缺席给安装指引而非裸错。
pub fn require_bin(bin: &str, hint: &str) -> Result<()> {
    match run_cmd(bin, &["--version"], 10, 300) {
        Ok(_) => Ok(()),
        Err(_) => Err(KernelError::Skill(format!("找不到 {bin}：{hint}"))),
    }
}

/// 校验交给 yt-dlp 的 url 参数（media.download 与 web.fetch_file 共用）。
/// 拒绝以 `-` 开头（防参数注入，如把 `-o` 输出路径当输入），且必须匹配
/// http(s):// 或 ytsearch 或 bilisearch 前缀，否则显式报错。纯校验函数，可测。
pub fn validate_ytdlp_url(url: &str) -> std::result::Result<(), String> {
    let t = url.trim();
    if t.is_empty() {
        return Err("url 为空".to_string());
    }
    if t.starts_with('-') {
        return Err(format!("url 不能以 '-' 开头（疑似参数注入）: {url}"));
    }
    let lower = t.to_lowercase();
    let ok = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ytsearch")
        || lower.starts_with("bilisearch");
    if !ok {
        return Err(format!(
            "url 需以 http(s):// 或 ytsearch/bilisearch 开头: {url}"
        ));
    }
    Ok(())
}

/// 一次性计划任务名：秒/分+unix 秒，防同名顶掉（schtasks /f 静默覆盖）。
pub fn schtasks_name(prefix: &str, fire: chrono::DateTime<chrono::Local>) -> String {
    let ts = fire.timestamp();
    format!("{prefix}-{ts}-{}", std::process::id())
}
pub fn schtasks_once(
    task_name: &str,
    fire: chrono::DateTime<chrono::Local>,
    message: &str,
) -> Result<String> {
    let safe: String = task_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    if safe.is_empty() {
        return Err(KernelError::Skill("任务名无可用字符".to_string()));
    }
    let full = format!("VoicePilot\\{safe}");
    let msg = message.replace('"', "”");
    let tr = format!("msg * /TIME:120 \"{msg}\"");
    let st = fire.format("%H:%M").to_string();
    let sd = fire.format("%m/%d/%Y").to_string();
    let out = run_cmd(
        "schtasks",
        &[
            "/create", "/tn", &full, "/tr", &tr, "/sc", "once", "/st", &st, "/sd", &sd, "/f",
        ],
        30,
        1000,
    )?;
    if !(out.contains("成功") || out.to_lowercase().contains("success")) {
        return Err(KernelError::Skill(format!(
            "创建计划任务失败: {}",
            out.trim()
        )));
    }
    Ok(fire.format("%m月%d日 %H:%M").to_string())
}

/// 中文星期。
pub fn weekday_zh() -> &'static str {
    use chrono::Datelike;
    match chrono::Local::now().weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_cjk_intact() {
        assert_eq!(
            truncate_chars("你好世界", 2),
            "你好\n…（已截断，只显示前 2 字）"
        );
        assert_eq!(truncate_chars("abc", 5), "abc");
    }

    #[test]
    fn sums_and_certutil_parsers() {
        let h = "d3adb33f00112233445566778899aabbccddeeff00112233445566778899aabb";
        let sums = format!("{h}  yt-dlp.exe\nxxxx  other.zip\n");
        assert_eq!(parse_sums_file(&sums, "yt-dlp.exe"), Some(h.to_string()));
        assert_eq!(parse_sums_file(&sums, "missing.exe"), None);
        let cert = format!("SHA256 hash of file: \r\n{h}\r\nCertUtil done\r\n");
        assert_eq!(parse_certutil_sha256(&cert), Some(h.to_string()));
        assert_eq!(parse_certutil_sha256("no hash here"), None);
    }

    #[test]
    fn strip_html_drops_tags_and_scripts() {
        let html = "<html><head><style>.a{}</style><script>alert(1)</script></head>\
            <body><h1>标题</h1><p>正文&nbsp;文本</p></body></html>";
        let text = strip_html_tags(html);
        assert!(!text.contains("alert"), "{text}");
        assert!(!text.contains(".a{}"), "{text}");
        assert!(
            text.contains("标题") && text.contains("正文 文本"),
            "{text}"
        );
    }

    #[test]
    fn ytdlp_url_validation() {
        // 拒绝参数注入（以 - 开头的值）。
        assert!(validate_ytdlp_url("-o").is_err());
        assert!(validate_ytdlp_url("--sponsorblock-remove").is_err());
        // 普通 http(s) 直链通过。
        assert!(validate_ytdlp_url("https://example.com/a.mp4").is_ok());
        assert!(validate_ytdlp_url("http://example.com/a.mp4").is_ok());
        // ytsearch / bilisearch 搜索前缀通过。
        assert!(validate_ytdlp_url("ytsearch1:晴天").is_ok());
        assert!(validate_ytdlp_url("bilisearch1:晴天").is_ok());
        // 非白名单前缀 / 空 → 显式报错。
        assert!(validate_ytdlp_url("ftp://x").is_err());
        assert!(validate_ytdlp_url("随便").is_err());
        assert!(validate_ytdlp_url("").is_err());
    }

    #[test]
    fn simple_manifest_readonly_posture() {
        let m = simple_manifest(
            "x.y",
            "t",
            "d",
            &["k"],
            &["e"],
            vec![],
            false,
            EgressKind::LocalOnly,
            &[],
        );
        assert_eq!(m.id, "x.y");
        assert!(matches!(m.approval.mode, ApprovalMode::None));
        assert_eq!(m.verifier.strategy, "none");
    }

    #[test]
    fn approval_whitelist_only_file_and_shell() {
        // 2026 用户决定：仅 fs.*/shell 保留审批，其余一律免审批。
        for id in [
            "fs.write_file",
            "fs.create_file",
            "fs.delete_file",
            "shell.run",
        ] {
            assert!(approval_required(id), "{id} 必须在审批白名单内");
        }
        for id in [
            "fs.read_file",
            "sys.open_url",
            "clip.type_text",
            "web.scrape",
            "media.download",
            "pim.note_create",
            "doc.office",
            "quick.app_control",
            "note.capture",
            "form.prepare",
            "task.compensate",
        ] {
            assert!(
                !approval_required(id),
                "{id} 不应在审批白名单内（2026 免审批）"
            );
        }
    }

    #[test]
    fn simple_manifest_write_posture() {
        // 白名单内 id（fs.write_file）→ 审批档 PerStep + weak verifier。
        let m = simple_manifest(
            "fs.write_file",
            "t",
            "d",
            &["k"],
            &["e"],
            vec![SimpleInput {
                name: "q",
                input_type: SkillInputType::Text,
                required: true,
                max_length: Some(50),
                allowed_roots: vec![],
            }],
            true,
            EgressKind::LocalOnly,
            &["x.tool"],
        );
        assert!(matches!(m.approval.mode, ApprovalMode::PerStep));
        assert_eq!(m.inputs["q"].max_length, Some(50));

        // 白名单外 id：即使 needs_approval=true 也落 None（manifest 不弹审批）。
        let m = simple_manifest(
            "clip.type_text",
            "t",
            "d",
            &["k"],
            &["e"],
            vec![],
            true,
            EgressKind::LocalOnly,
            &[],
        );
        assert!(matches!(m.approval.mode, ApprovalMode::None));
    }
}
