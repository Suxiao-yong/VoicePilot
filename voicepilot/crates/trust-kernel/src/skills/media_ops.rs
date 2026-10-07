//! Daisy 移植：媒体与文档组（下载/剪视频/转视频/转文档）。
//!
//! 全部依赖外部二进制，不自动下载（与 Daisy 不同：我们不静默装第三方
//! exe，缺席给安装指引）：yt-dlp（下视频/音频）、ffmpeg（剪/转）、
//! soffice/LibreOffice（重型文档互转）。纯文本家族（txt/md/html/rtf/
//! csv）互转纯手写、零依赖。全部写文件 → PerStep 审批。

use crate::approval::approver::Approver;
use crate::compensation::types::{CompensationLevel, ConflictPolicy};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::common::create_post_commit_compensation_with_payload;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{
    SimpleInput, ensure_ffmpeg, ensure_yt_dlp, require_bin, run_cmd, run_simple, simple_manifest,
    slot_text, slot_text_opt, truncate_chars, validate_ytdlp_url,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn text_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::Text,
        required: true,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

fn file_slot(name: &'static str) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::File,
        required: true,
        max_length: Some(500),
        allowed_roots: vec![],
    }
}

/// 输出落到源同目录（不同名则指定）。纯函数，可测。
pub fn sibling_output(source: &str, suffix: &str, new_ext: Option<&str>) -> PathBuf {
    let p = Path::new(source.trim());
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let ext = new_ext
        .map(|e| e.to_string())
        .or_else(|| p.extension().map(|e| e.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "bin".to_string());
    p.with_file_name(format!("{stem}{suffix}.{ext}"))
}

/// 歌名搜索候选（top-N 标题 + 直链）。
#[derive(Debug, Clone, PartialEq)]
pub struct SearchCandidate {
    pub title: String,
    pub url: String,
}

/// 解析 `yt-dlp --flat-playlist --print "%(title)s\t%(webpage_url)s"` 的行。
/// 纯函数，可测；坏行跳过（有的条目无 url）。
pub fn parse_search_candidates(stdout: &str) -> Vec<SearchCandidate> {
    stdout
        .lines()
        .filter_map(|l| {
            let (title, url) = l.split_once('\t')?;
            let title = title.trim().to_string();
            let url = url.trim().to_string();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            Some(SearchCandidate { title, url })
        })
        .take(3)
        .collect()
}

/// 明确胜出判定：归一化（小写去空白）后恰好一个标题与 query 互含子串 → 直接下；
/// 否则 None（追问卡）。纯函数，可测；含糊时追问而非猜错。
pub fn clear_winner(query: &str, titles: &[String]) -> Option<usize> {
    let norm = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    let q = norm(query);
    if q.is_empty() {
        return None;
    }
    let hits: Vec<usize> = titles
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let n = norm(t);
            n.contains(&q) || q.contains(&n)
        })
        .map(|(i, _)| i)
        .collect();
    if hits.len() == 1 { Some(hits[0]) } else { None }
}

/// 歌名搜索消歧（P3 追问卡唯一调用点）：`ytsearch1:/bilisearch1:` 改拉 top-3 标题，
/// 明确胜出直接返回对应直链 + 标题，否则弹追问卡三选一（超时回 top-1）。
/// 非搜索 url 原样返回（None 标题）。拉取失败返回 Err，调用方回退直接下载（P1 行为）。
fn resolve_search_choice(
    ytdlp: &str,
    approver: &dyn Approver,
    url: &str,
) -> Result<(String, Option<String>)> {
    let t = url.trim();
    let lower = t.to_lowercase();
    let prefix = if lower.starts_with("ytsearch1:") {
        "ytsearch3:"
    } else if lower.starts_with("bilisearch1:") {
        "bilisearch3:"
    } else {
        return Ok((t.to_string(), None));
    };
    let query = t.split_once(':').map(|(_, q)| q.trim()).unwrap_or("");
    if query.is_empty() {
        return Err(KernelError::Skill("搜索表达式为空".to_string()));
    }
    let out = run_cmd(
        ytdlp,
        &[
            "--flat-playlist",
            "--print",
            "%(title)s\t%(webpage_url)s",
            &format!("{prefix}{query}"),
        ],
        60,
        2000,
    )?;
    let cands = parse_search_candidates(&out);
    if cands.is_empty() {
        return Err(KernelError::Skill("搜索无结果".to_string()));
    }
    let titles: Vec<String> = cands.iter().map(|c| c.title.clone()).collect();
    let idx = match clear_winner(query, &titles) {
        Some(i) => i,
        None => {
            let picked = approver.request_clarification(
                "搜到多个版本，下载哪一个？（超时默认第一个）",
                &titles,
                0,
            );
            picked.min(titles.len() - 1)
        }
    };
    Ok((cands[idx].url.clone(), Some(cands[idx].title.clone())))
}

pub fn media_download_manifest() -> SkillManifest {
    simple_manifest(
        "media.download",
        "下视频音频",
        "yt-dlp 下载网上视频/音频（YouTube/B站/抖音等）；yt-dlp 缺席时自动供给，免审批。无链接时可传搜索表达式：url=ytsearch1:资源名（YouTube）或 bilisearch1:资源名（B站），yt-dlp 会先搜索再下载首个结果 —— 用户说资源名（如“下载晴天”）由此直达。",
        &["下载视频", "下载音频", "下视频", "保存视频", "下歌"],
        &["下载这个视频", "把这首歌下载下来"],
        vec![
            text_slot("url", 2000),
            SimpleInput {
                name: "audio_only",
                input_type: SkillInputType::Text,
                required: false,
                max_length: Some(8),
                allowed_roots: vec![],
            },
        ],
        true,
        EgressKind::WebToLocal,
        &[],
    )
}

pub fn execute_media_download(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let url = slot_text(inputs, "url")?;
    let audio_only = slot_text_opt(inputs, "audio_only")
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "是"))
        .unwrap_or(false);
    let mut map = HashMap::new();
    map.insert("url".to_string(), serde_json::json!(url));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "media.download",
        &format!("download:{url}"),
        true,
        &media_download_manifest(),
        &map,
        || {
            // url 交给 yt-dlp 前校验：拒绝以 - 开头 + 限制 http(s)/ytsearch/bilisearch。
            validate_ytdlp_url(url.trim()).map_err(KernelError::Skill)?;
            // PATH 优先，缺席自动供给（GitHub 最新版 + SHA-256 校验，落本地 tools 目录）。
            let ytdlp = ensure_yt_dlp()?;
            let ytdlp_s = ytdlp.to_string_lossy().into_owned();
            // P3：歌名搜索先拉 top-3 消歧（明确胜出直下，否则追问卡；超时回 top-1）。
            // 拉取失败回退 P1 行为（ytsearch1: 直下首个结果），不新增失败面。
            let (final_url, assumed) = resolve_search_choice(&ytdlp_s, approver, url.trim())
                .unwrap_or_else(|_| (url.trim().to_string(), None));
            let dl_dir = dirs::download_dir().unwrap_or_else(|| PathBuf::from("."));
            let mut owned_args: Vec<String> = vec![];
            if audio_only {
                owned_args.extend([
                    "-x".to_string(),
                    "--audio-format".to_string(),
                    "mp3".to_string(),
                    "--audio-quality".to_string(),
                    "0".to_string(),
                ]);
            } else {
                owned_args.extend([
                    "-f".to_string(),
                    "bv*+ba/b".to_string(),
                    "--merge-output-format".to_string(),
                    "mp4".to_string(),
                ]);
                // 合并音视频轨需要 ffmpeg 位置（否则只留分开的两个文件）。
                if let Ok((ffmpeg_dir, _)) = ensure_ffmpeg() {
                    owned_args.extend([
                        "--ffmpeg-location".to_string(),
                        ffmpeg_dir.to_string_lossy().into_owned(),
                    ]);
                }
            }
            let out_tpl = dl_dir
                .join("%(title)s.%(ext)s")
                .to_string_lossy()
                .into_owned();
            owned_args.extend(["-o".to_string(), out_tpl, final_url.clone()]);
            let arg_refs: Vec<&str> = owned_args.iter().map(|s| s.as_str()).collect();
            let out = run_cmd(&ytdlp_s, &arg_refs, 600, 2000)?;
            // 从输出里抠目标文件名，抠不到就报目录。
            let dest = out
                .lines()
                .rev()
                .find_map(|l| {
                    l.find("Destination:")
                        .map(|p| l[p + 12..].trim().to_string())
                        .or_else(|| {
                            l.find("Merging formats into").map(|p| {
                                l[p..]
                                    .find('"')
                                    .and_then(|a| {
                                        l[p + a + 1..]
                                            .find('"')
                                            .map(|b| l[p + a + 1..p + a + 1 + b].to_string())
                                    })
                                    .unwrap_or_default()
                            })
                        })
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| dl_dir.display().to_string());
            // 搜索假设声明（P1 要求）：默认下载了哪个版本必须明说 + 进审计（任务落库已有）。
            let assumed_note = assumed
                .map(|t| format!("（搜索假设：默认下载《{t}》）"))
                .unwrap_or_default();
            Ok(format!("已下载到：{dest}{assumed_note}"))
        },
    )
}

pub fn media_trim_manifest() -> SkillManifest {
    simple_manifest(
        "media.trim_video",
        "剪视频片段",
        "ffmpeg 截取视频时间段存新文件（不重编码，快）。需先装 ffmpeg。",
        &["剪视频", "截取视频", "裁剪视频", "视频片段"],
        &["截取视频第1分钟到第2分钟", "剪出这段视频"],
        vec![
            file_slot("source"),
            text_slot("start", 16),
            text_slot("end", 16),
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_media_trim(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let source = slot_text(inputs, "source")?;
    let start = slot_text(inputs, "start")?;
    let end = slot_text(inputs, "end")?;
    let mut map = HashMap::new();
    map.insert("source".to_string(), serde_json::json!(source));
    map.insert("start".to_string(), serde_json::json!(start));
    map.insert("end".to_string(), serde_json::json!(end));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "media.trim_video",
        &format!("trim:{source}"),
        true,
        &media_trim_manifest(),
        &map,
        || {
            let (_, ffmpeg) = ensure_ffmpeg()?;
            let ffmpeg_s = ffmpeg.to_string_lossy().into_owned();
            let dest = sibling_output(&source, "-clip", None);
            let dest_s = dest.to_string_lossy().into_owned();
            run_cmd(
                &ffmpeg_s,
                &[
                    "-y",
                    "-ss",
                    start.trim(),
                    "-to",
                    end.trim(),
                    "-i",
                    source.trim(),
                    "-c",
                    "copy",
                    &dest_s,
                ],
                600,
                1500,
            )?;
            Ok(format!("已截取存为：{}", dest.display()))
        },
    )
}

/// 支持的视频目标格式白名单（防手滑打错，也防奇怪封装）。
pub fn video_format_ok(format: &str) -> bool {
    matches!(
        format.trim().to_lowercase().as_str(),
        "mp4" | "mov" | "avi" | "mkv" | "webm"
    )
}

pub fn media_convert_manifest() -> SkillManifest {
    simple_manifest(
        "media.convert_video",
        "转视频格式",
        "ffmpeg 转视频格式（mp4/mov/avi/mkv/webm）。需先装 ffmpeg。",
        &["转格式", "转视频", "转成mp4", "视频转换"],
        &["把这个视频转成mp4", "转视频格式"],
        vec![file_slot("source"), text_slot("format", 8)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_media_convert(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let source = slot_text(inputs, "source")?;
    let format = slot_text(inputs, "format")?;
    let mut map = HashMap::new();
    map.insert("source".to_string(), serde_json::json!(source));
    map.insert("format".to_string(), serde_json::json!(format));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "media.convert_video",
        &format!("convert:{source}"),
        true,
        &media_convert_manifest(),
        &map,
        || {
            let (_, ffmpeg) = ensure_ffmpeg()?;
            let ffmpeg_s = ffmpeg.to_string_lossy().into_owned();
            if !video_format_ok(&format) {
                return Err(KernelError::Skill(format!(
                    "不支持的视频格式: {format}（支持 mp4/mov/avi/mkv/webm）"
                )));
            }
            let fmt = format.trim().to_lowercase();
            let dest = sibling_output(&source, "-converted", Some(&fmt));
            let dest_s = dest.to_string_lossy().into_owned();
            run_cmd(&ffmpeg_s, &["-y", "-i", source.trim(), &dest_s], 900, 1500)?;
            Ok(format!("已转换存为：{}", dest.display()))
        },
    )
}

/// pychorus 版本锁定（pip `==` + TLS；hash pinning 记 follow-up，不夸大）。
pub const PYCHORUS_PIN: &str = "pychorus==0.1";

/// python 可执行名（`VP_PYTHON_BIN` 覆盖，单测 fail-closed 用；生产恒为 `python`）。
fn python_bin() -> String {
    std::env::var("VP_PYTHON_BIN").unwrap_or_else(|_| "python".to_string())
}

/// 副歌定位的 `python -c` 脚本：调 `find_and_output_chorus` 写出裁剪并打印起始秒。
/// 纯函数，可测。路径按 python 单引号字面量转义后嵌入。
pub fn chorus_detect_script(source: &str, dest: &str, seconds: u32) -> String {
    format!(
        "from pychorus import find_and_output_chorus as f; print(f({}, {}, {}))",
        py_str_lit(source),
        py_str_lit(dest),
        seconds
    )
}

fn py_str_lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// 解析 pychorus 输出的起始秒（取 stdout 首个可解析浮点数行）。
/// 纯函数，可测；脚本杂输出（librosa 警告等）自动跳过。
pub fn parse_chorus_start(stdout: &str) -> Option<f64> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.parse::<f64>().ok())
        .next()
}

/// 解析 `ffmpeg -i` stderr 里的 `Duration: HH:MM:SS.cs`（ffprobe 不另引）。
/// 纯函数，可测；无 Duration 行返回 None。
pub fn parse_ffmpeg_duration(stderr: &str) -> Option<f64> {
    let line = stderr.lines().find(|l| l.contains("Duration:"))?;
    let after = line.split("Duration:").nth(1)?.trim();
    let hms = after.split(',').next()?.trim().split('.').next()?;
    let mut parts = hms.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let s: f64 = parts.next()?.trim().parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// `seconds` 槽位：缺席/非法 → 默认 60（与 timer 同名字段，零 SlotKind 扩展）。
pub fn clip_seconds(inputs: &serde_json::Value) -> u32 {
    inputs
        .get("seconds")
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|n| (10..=600).contains(n))
        .unwrap_or(60)
}

pub fn clip_chorus_manifest() -> SkillManifest {
    simple_manifest(
        "media.clip_chorus",
        "截副歌片段",
        "pychorus 定位歌曲副歌（最重复段落）并截取约60秒存新文件（已知副歌时间戳请用 media.trim_video）。无监督启发式，对非典型结构会错判，结果仅供参考；python 缺席 fail-closed。",
        &["副歌", "高潮", "截副歌", "高潮部分", "副歌部分"],
        &["截这首歌的副歌", "截取高潮部分", "下载副歌一分钟"],
        vec![
            file_slot("path"),
            SimpleInput {
                name: "seconds",
                input_type: SkillInputType::Text,
                required: false,
                max_length: Some(8),
                allowed_roots: vec![],
            },
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_media_clip_chorus(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let source = slot_text(inputs, "path")?;
    let seconds = clip_seconds(inputs);
    let mut map = HashMap::new();
    map.insert("path".to_string(), serde_json::json!(source));
    map.insert(
        "seconds".to_string(),
        serde_json::json!(seconds.to_string()),
    );
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "media.clip_chorus",
        &format!("clip_chorus:{source}"),
        true,
        &clip_chorus_manifest(),
        &map,
        || {
            // python 探针：缺席 fail-closed，给安装指引（不自动装解释器）。
            let python = python_bin();
            run_cmd(&python, &["--version"], 10, 100).map_err(|_| {
                KernelError::Skill(
                    "找不到 python：去 https://www.python.org/downloads/ 安装并加入 PATH 后重试"
                        .to_string(),
                )
            })?;
            // pip pin 版本安装 pychorus（TLS；hash pinning 记 follow-up）。
            run_cmd(&python, &["-m", "pip", "install", PYCHORUS_PIN], 600, 500)
                .map_err(|e| KernelError::Skill(format!("安装 {PYCHORUS_PIN} 失败: {e}")))?;
            let dest = sibling_output(&source, "-chorus", None);
            let dest_s = dest.to_string_lossy().into_owned();
            let script = chorus_detect_script(source.trim(), &dest_s, seconds);
            let out = run_cmd(&python, &["-c", &script], 900, 500)
                .map_err(|e| KernelError::Skill(format!("副歌定位失败: {e}")))?;
            let start = parse_chorus_start(&out).ok_or_else(|| {
                KernelError::Skill(format!(
                    "副歌定位无结果（pychorus 输出不可解析）: {}",
                    truncate_chars(&out, 200)
                ))
            })?;
            // verifier：输出存在 + 非空；PATH 有 ffmpeg 才做 Duration ±5s 断言
            //（verifier 不触发自动供给，无 ffmpeg 时降级为存在性校验）。
            let meta = std::fs::metadata(&dest).map_err(|e| {
                KernelError::Skill(format!("副歌文件未生成 {}: {e}", dest.display()))
            })?;
            if meta.len() == 0 {
                return Err(KernelError::Skill(format!(
                    "副歌文件为空 {}",
                    dest.display()
                )));
            }
            let mut verified_note = "（存在性校验）";
            if run_cmd("ffmpeg", &["-version"], 10, 100).is_ok() {
                // `ffmpeg -i` 无输出文件必非零退出：Duration 在 stderr（Err 文案里），两边都解析。
                let probe_text = match run_cmd("ffmpeg", &["-i", &dest_s], 60, 1500) {
                    Ok(t) => t,
                    Err(e) => e.to_string(),
                };
                if let Some(dur) = parse_ffmpeg_duration(&probe_text) {
                    if (dur - seconds as f64).abs() > 5.0 {
                        return Err(KernelError::Skill(format!(
                            "副歌时长 {dur:.1}s 与请求 {seconds}s 差超 5s"
                        )));
                    }
                    verified_note = "（时长校验通过）";
                }
            }
            // 补偿：删输出文件（抄 reverse_note_capture 模式）。
            create_post_commit_compensation_with_payload(
                kernel,
                step_id,
                "media.reverse_clip_chorus",
                serde_json::json!({"save_path": dest_s}).to_string(),
                CompensationLevel::BestEffort,
                ConflictPolicy::AutoReverse,
                3600,
            )
            .map_err(|e| KernelError::Skill(format!("补偿登记失败: {e}")))?;
            Ok(format!(
                "已截取副歌存为：{}{}（起始 {start:.1}s，pychorus 启发式定位，仅供参考）",
                dest.display(),
                verified_note
            ))
        },
    )
}

/// 纯文本家族互转（txt/md/html/csv），零依赖。
/// rtf 源不透明解析（中文 RTF 多为 GBK \'xx 转义，无依赖解不了码），
/// 诚实返回 None 走 soffice；rtf 目标可手写（\uN? 编码）。
/// 返回 None 表示家族外 → 走 soffice 或报错。
pub fn text_family_convert(source_text: &str, from: &str, to: &str) -> Option<String> {
    // rtf 目标：最小合法 RTF（\uN? + ANSI 回退字符）。
    if matches!(to, "rtf") {
        let mut body = String::new();
        for ch in source_text.chars() {
            if ch == '\n' {
                body.push_str("\\par\n");
            } else if ch.is_ascii() && ch != '\\' && ch != '{' && ch != '}' {
                body.push(ch);
            } else if ch == '\\' || ch == '{' || ch == '}' {
                body.push('\\');
                body.push(ch);
            } else {
                body.push_str(&format!("\\u{}?", ch as u32));
            }
        }
        return Some(format!(
            "{{\\rtf1\\ansi\\ansicpg936\\deff0{{\\fonttbl{{\\f0 SimSun;}}}}\\f0\\fs24 {body}}}"
        ));
    }
    let plain = match from {
        "html" | "htm" => Some(crate::skills::simple::strip_html_tags(source_text)),
        "txt" | "md" | "markdown" | "csv" => Some(source_text.to_string()),
        // rtf 源：不透明，不硬转（交 soffice）。
        _ => None,
    }?;
    match to {
        "txt" => Some(plain),
        "md" | "markdown" => Some(plain),
        "html" | "htm" => {
            let body = plain
                .lines()
                .map(|l| {
                    let e = l
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    format!("<p>{e}</p>")
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!(
                "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head><body>\n{body}\n</body></html>"
            ))
        }
        "csv" => Some(
            plain
                .lines()
                .map(|l| {
                    l.split_whitespace()
                        .map(|c| {
                            if c.contains([',', '"', '\n']) {
                                format!("\"{}\"", c.replace('"', "\"\""))
                            } else {
                                c.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

pub fn doc_convert_manifest() -> SkillManifest {
    simple_manifest(
        "doc.convert",
        "转文档格式",
        "文档互转：txt/md/html/csv 互转与写 rtf 零依赖直转；rtf 源与重型格式走 LibreOffice（需先装）。",
        &["转文档", "转成pdf", "转成word", "文档转换", "转md"],
        &["把这个文档转成pdf", "转成markdown"],
        vec![file_slot("source"), text_slot("format", 8)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_doc_convert(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let source = slot_text(inputs, "source")?;
    let format = slot_text(inputs, "format")?.trim().to_lowercase();
    let mut map = HashMap::new();
    map.insert("source".to_string(), serde_json::json!(source));
    map.insert("format".to_string(), serde_json::json!(format));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "doc.convert",
        &format!("doc:{source}"),
        true,
        &doc_convert_manifest(),
        &map,
        || {
            let src_path = Path::new(source.trim());
            let from = src_path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            // 家族内：纯手写直转。
            if let Ok(bytes) = std::fs::read(src_path) {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if let Some(out) = text_family_convert(&text, &from, &format) {
                    let dest = sibling_output(&source, "-converted", Some(&format));
                    std::fs::write(&dest, out.as_bytes()).map_err(|e| {
                        KernelError::Skill(format!("写 {} 失败: {e}", dest.display()))
                    })?;
                    return Ok(format!("已转换存为：{}", dest.display()));
                }
            }
            // 家族外：LibreOffice 兜底。
            require_bin(
                "soffice",
                "重型文档互转需 LibreOffice：去 https://zh-cn.libreoffice.org/download/ 安装并加入 PATH",
            )?;
            let outdir = src_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".to_string());
            let out = run_cmd(
                "soffice",
                &[
                    "--headless",
                    "--convert-to",
                    &format,
                    "--outdir",
                    &outdir,
                    source.trim(),
                ],
                300,
                1500,
            )?;
            Ok(format!(
                "LibreOffice 转换完成：{}",
                truncate_chars(&out, 800)
            ))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_manifests_have_ids() {
        for m in [
            media_download_manifest(),
            media_trim_manifest(),
            media_convert_manifest(),
            clip_chorus_manifest(),
            doc_convert_manifest(),
        ] {
            assert!(!m.id.is_empty() && !m.keywords.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn sibling_output_names() {
        let p = sibling_output(r"C:\v\a.mp4", "-clip", None);
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "a-clip.mp4");
        let p = sibling_output(r"C:\v\a.mkv", "-converted", Some("mp4"));
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "a-converted.mp4");
    }

    #[test]
    fn video_format_whitelist() {
        assert!(video_format_ok("mp4"));
        assert!(video_format_ok("MKV"));
        assert!(!video_format_ok("exe"));
        assert!(!video_format_ok(""));
    }

    #[test]
    fn text_family_roundtrip() {
        let html = "<h1>题</h1><p>文</p>";
        let md = text_family_convert(html, "html", "md").unwrap();
        assert!(md.contains("题") && md.contains("文"), "{md}");
        let back = text_family_convert(&md, "md", "html").unwrap();
        assert!(back.contains("<p>"), "{back}");
        assert!(text_family_convert("x", "docx", "pdf").is_none());
        assert!(text_family_convert("x", "md", "docx").is_none());
        // rtf：可写不可读（读交 soffice）。
        let rtf = text_family_convert("你好", "txt", "rtf").unwrap();
        assert!(
            rtf.starts_with("{\\rtf1") && rtf.contains("\\u20320?"),
            "{rtf}"
        );
        assert!(text_family_convert("{\\rtf1 x}", "rtf", "md").is_none());
    }

    #[test]
    fn require_bin_missing_gives_hint() {
        let err = require_bin("vp-no-such-bin-xyz", "装一下").unwrap_err();
        assert!(format!("{err:?}").contains("装一下"), "{err:?}");
    }

    #[test]
    fn chorus_script_embeds_escaped_paths_and_seconds() {
        let s = chorus_detect_script(r"C:\v\a'b.mp3", r"C:\v\a-chorus.mp3", 60);
        assert!(s.contains("find_and_output_chorus"), "{s}");
        assert!(s.contains(r"C:\\v\\a\'b.mp3"), "{s}");
        assert!(s.contains(", 60)"), "{s}");
    }

    #[test]
    fn chorus_start_parses_first_float_skipping_noise() {
        assert_eq!(parse_chorus_start("librosa 警告\n  83.24\n"), Some(83.24));
        assert!(parse_chorus_start("no numbers here\n").is_none());
        assert!(parse_chorus_start("").is_none());
    }

    #[test]
    fn ffmpeg_duration_parses_hms() {
        let stderr =
            "ffmpeg version 7.1\n  Duration: 00:01:02.34, start: 0.000000, bitrate: 128 kb/s\n";
        let dur = parse_ffmpeg_duration(stderr).unwrap();
        assert!((dur - 62.0).abs() < 1.0, "{dur}");
        assert!(parse_ffmpeg_duration("no duration line\n").is_none());
    }

    #[test]
    fn clip_seconds_defaults_and_clamps() {
        assert_eq!(clip_seconds(&serde_json::json!({})), 60);
        assert_eq!(clip_seconds(&serde_json::json!({"seconds": "90"})), 90);
        assert_eq!(clip_seconds(&serde_json::json!({"seconds": "abc"})), 60);
        assert_eq!(clip_seconds(&serde_json::json!({"seconds": "5"})), 60);
    }

    #[test]
    fn search_candidates_parse_rows_and_skip_bad() {
        let out = "晴天 - 周杰伦\thttps://youtu.be/a\n坏行无tab\n\t\n孤勇者\thttps://youtu.be/b\n";
        let c = parse_search_candidates(out);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].title, "晴天 - 周杰伦");
        assert_eq!(c[1].url, "https://youtu.be/b");
    }

    #[test]
    fn clear_winner_only_when_single_match() {
        let titles = vec!["晴天 - 周杰伦".to_string()];
        assert_eq!(clear_winner("晴天", &titles), Some(0));
        // 多个都含 → 含糊，追问。
        let titles = vec!["晴天".to_string(), "晴天 (Live)".to_string()];
        assert_eq!(clear_winner("晴天", &titles), None);
        // 都不含 → 追问。
        assert_eq!(clear_winner("夜曲", &titles), None);
        assert_eq!(clear_winner("", &titles), None);
    }

    #[test]
    fn clip_chorus_without_python_fails_closed() {
        use crate::approval::approver::AutoApprover;
        use crate::kernel::TrustKernel;
        use std::sync::Mutex;
        static ENV_MUTEX: Mutex<()> = Mutex::new(());
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe {
            std::env::set_var("VP_PYTHON_BIN", "vp-no-such-python-xyz");
        }
        let kernel = TrustKernel::open_in_memory().unwrap();
        let err = execute_media_clip_chorus(
            &kernel,
            &AutoApprover,
            "t-clip",
            "s-clip",
            &serde_json::json!({"path": "C:/v/a.mp3"}),
        )
        .unwrap_err();
        unsafe {
            std::env::remove_var("VP_PYTHON_BIN");
        }
        assert!(format!("{err:?}").contains("python"), "{err:?}");
    }
}
