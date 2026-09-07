//! Daisy 移植：联网查询组（搜索/抓取/壁纸/天气/赛程）。
//!
//! 全部走系统 curl.exe（零新依赖）：DuckDuckGo HTML、Wallhaven 免费
//! API、wttr.in、thesportsdb 免费 key。抓取只做正文提取，深度需求
//! 走 research.save_markdown。纯只读，免审批、审计留痕。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{
    curl_get, ensure_yt_dlp, run_cmd, run_simple, simple_manifest, slot_text, strip_html_tags,
    truncate_chars, validate_ytdlp_url, SimpleInput,
};
use std::collections::HashMap;

fn query_slot() -> SimpleInput {
    SimpleInput {
        name: "query",
        input_type: SkillInputType::Text,
        required: true,
        max_length: Some(300),
        allowed_roots: vec![],
    }
}

/// DDG HTML 结果提取：取 result__a 链接文本（纯函数，可测）。
pub fn parse_ddg_results(html: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = html;
    while let Some(a) = rest.find("result__a") {
        let seg = &rest[a..];
        let href = seg
            .find("href=\"")
            .and_then(|p| {
                let s = &seg[p + 6..];
                s.find('"').map(|e| s[..e].to_string())
            })
            .unwrap_or_default();
        let text_start = seg.find('>').map(|p| p + 1).unwrap_or(0);
        let text = seg[text_start..]
            .find("</a>")
            .map(|e| strip_html_tags(&seg[text_start..text_start + e]))
            .unwrap_or_default();
        // DDG 跳转链接 //duckduckgo.com/l/?uddg= 真目标。
        let url = if let Some(p) = href.find("uddg=") {
            let enc = &href[p + 5..];
            let end = enc.find('&').unwrap_or(enc.len());
            percent_decode(&enc[..end])
        } else {
            href.clone()
        };
        if !text.trim().is_empty() && !url.is_empty() {
            out.push((text.trim().to_string(), url));
        }
        rest = &seg[text_start.min(seg.len())..];
        if out.len() >= 8 {
            break;
        }
    }
    out
}

/// 最小 percent-decode（UTF-8 序列按字节拼）。
pub fn percent_decode(s: &str) -> String {
    let mut bytes = vec![];
    let mut it = s.as_bytes().iter().peekable();
    while let Some(&b) = it.next() {
        if b == b'%' {
            let mut h = || it.next().copied().unwrap_or(b'0');
            let hi = (h() as char).to_digit(16).unwrap_or(0);
            let lo = (h() as char).to_digit(16).unwrap_or(0);
            bytes.push((hi * 16 + lo) as u8);
        } else if b == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn web_search_manifest() -> SkillManifest {
    simple_manifest(
        "web.search",
        "联网搜索",
        "DuckDuckGo 搜最新信息，返回标题+链接（前 8 条）。",
        &["搜索", "搜一下", "查一下", "最新", "新闻"],
        &["搜一下今天的新闻", "帮我查一下这个"],
        vec![query_slot()],
        false,
        EgressKind::WebToLocal,
        &[],
    )
}

pub fn execute_web_search(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text(inputs, "query")?;
    let mut map = HashMap::new();
    map.insert("query".to_string(), serde_json::json!(query));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "web.search",
        &format!("search:{query}"),
        false,
        &web_search_manifest(),
        &map,
        || {
            let url = format!(
                "https://html.duckduckgo.com/html/?q={}",
                percent_encode(&query)
            );
            let html = curl_get(&url, 120_000)?;
            let hits = parse_ddg_results(&html);
            if hits.is_empty() {
                return Ok(format!("没搜到“{query}”的相关结果（可能网络受限）"));
            }
            let lines: Vec<String> = hits
                .iter()
                .enumerate()
                .map(|(i, (t, u))| format!("{}. {t}\n   {u}", i + 1))
                .collect();
            Ok(format!("“{query}”的搜索结果：\n{}", lines.join("\n")))
        },
    )
}

/// 最小 percent-encode（非 unreserved 按 UTF-8 字节转义）。
pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn web_scrape_manifest() -> SkillManifest {
    simple_manifest(
        "web.scrape",
        "读网页",
        "抓取指定网址网页正文（轻量提取；需登录/强反爬的页面可能不全）。URL 由审批人确认（防 prompt 注入内网探测）。",
        &["读网页", "打开链接看", "这篇讲什么", "抓取"],
        &["读一下这个网页", "这个链接讲了什么"],
        vec![SimpleInput {
            name: "url",
            input_type: SkillInputType::Url,
            required: true,
            max_length: Some(2000),
            allowed_roots: vec![],
        }],
        true,
        EgressKind::WebToLocal,
        &[],
    )
}

pub fn execute_web_scrape(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let url = slot_text(inputs, "url")?;
    let mut map = HashMap::new();
    map.insert("url".to_string(), serde_json::json!(url));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "web.scrape",
        &format!("scrape:{url}"),
        true,
        &web_scrape_manifest(),
        &map,
        || {
            let lower = url.trim().to_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                return Err(KernelError::Skill(format!("只抓取 http(s) 网页: {url}")));
            }
            let html = curl_get(url.trim(), 150_000)?;
            let text = strip_html_tags(&html);
            if text.trim().is_empty() {
                return Ok(format!("{url} 没抓到正文（可能需登录或反爬）"));
            }
            Ok(format!("{url} 正文：\n{}", truncate_chars(&text, 6000)))
        },
    )
}

pub fn web_wallpapers_manifest() -> SkillManifest {
    simple_manifest(
        "web.wallpapers",
        "找壁纸",
        "Wallhaven 高清壁纸库搜直链（免费 API）。",
        &["壁纸", "换壁纸", "桌面背景", "高清图"],
        &["找一张壁纸", "换个桌面背景"],
        vec![query_slot()],
        false,
        EgressKind::WebToLocal,
        &[],
    )
}

// ===== web.fetch_file：通用资源“自行找链接并下载”（用户诉求：说资源名直接下）=====
//
// 调研结论（本地 skills 检索）：三目录 ~50 个外部 skill 无一能执行下载
// （anysearch 是指令型，display-only）；VoicePilot 内置件组合即可：
// web.search 的 DDG 解析（parse_ddg_results）挑出文件直链，
// media.download 的 yt-dlp 自动供给负责落盘（generic extractor 支持任意
// http(s) 直链）。音视频场景走 media.download 的 ytsearch 重写，本技能补
// 软件安装包/文档等通用文件。

/// 可自动下载的文档类直链扩展名（大小写不敏感，匹配 URL path 尾部）。
/// 文档类无执行风险，命中即自动落盘。
const AUTO_DOWNLOAD_EXTS: &[&str] = &[
    ".pdf", ".epub", ".mobi", ".csv", ".xlsx", ".docx", ".pptx", ".ttf", ".otf",
];

/// 可执行/归档类直链扩展名：命中只出候选，**不自动下载**，等用户确认直链。
const CONFIRM_EXTS: &[&str] = &[
    ".exe", ".msi", ".msix", ".apk", ".iso", ".zip", ".7z", ".rar", ".gz", ".tgz",
];

/// 直链分类结果（纯函数输出）：auto = 自动落盘；confirm = 只出候选。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DownloadHits {
    pub auto: Vec<(String, String)>,
    pub confirm: Vec<(String, String)>,
}

/// 从 DDG 结果分类可下载直链（纯函数）：URL path 尾部命中扩展名白名单，
/// 去重（同 URL 只留一条），命中顺序即 DDG 相关度顺序。可执行/归档类
///（CONFIRM_EXTS）永远只进 confirm 桶 —— 落盘前必须人工确认。
pub fn pick_download_hits(hits: &[(String, String)]) -> DownloadHits {
    let mut seen = std::collections::HashSet::new();
    let mut out = DownloadHits::default();
    for (title, url) in hits {
        let path = url.split(['?', '#']).next().unwrap_or(url).to_lowercase();
        let hit_ext = |exts: &[&str]| {
            exts.iter().any(|ext| {
                path.rfind(ext)
                    .map(|p| p + ext.len() == path.len())
                    .unwrap_or(false)
            })
        };
        if !hit_ext(AUTO_DOWNLOAD_EXTS) && !hit_ext(CONFIRM_EXTS) {
            continue;
        }
        if !seen.insert(url.clone()) {
            continue;
        }
        if hit_ext(CONFIRM_EXTS) {
            out.confirm.push((title.clone(), url.clone()));
        } else {
            out.auto.push((title.clone(), url.clone()));
        }
    }
    out
}

pub fn web_fetch_file_manifest() -> SkillManifest {
    simple_manifest(
        "web.fetch_file",
        "下载资源",
        "搜索并下载通用文件（文档/压缩包/软件安装包等）：先 DuckDuckGo 搜索资源名，按扩展名分类直链：纯文档（.pdf/.docx/.xlsx 等）自动用 yt-dlp 下载到下载目录；可执行/压缩包类（.exe/.msi/.apk/.zip 等）只给候选，需人工确认直链后才下载。没有直链时返回候选列表。音频/视频请用 media.download（说歌名/视频名直达）。",
        &["下载软件", "下安装包", "下载文档", "下载资源"],
        &["下载微信", "帮我下个VS Code", "下载那份PDF报告"],
        vec![query_slot()],
        true,
        EgressKind::WebToLocal,
        &[],
    )
}

pub fn execute_web_fetch_file(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text(inputs, "query")?;
    let mut map = HashMap::new();
    map.insert("query".to_string(), serde_json::json!(query));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "web.fetch_file",
        &format!("fetch_file:{query}"),
        true,
        &web_fetch_file_manifest(),
        &map,
        || {
            let url = format!(
                "https://html.duckduckgo.com/html/?q={}%E4%B8%8B%E8%BD%BD",
                percent_encode(&query)
            );
            let html = curl_get(&url, 120_000)?;
            let hits = parse_ddg_results(&html);
            let downloads = pick_download_hits(&hits);
            // 可执行/归档类命中：不自动下载，只出候选等人确认直链。
            if downloads.auto.is_empty() {
                if !downloads.confirm.is_empty() {
                    let mut lines = vec![format!(
                        "“{query}”找到 {} 个可执行/压缩包直链，需人工确认后才下载：",
                        downloads.confirm.len()
                    )];
                    for (i, (t, u)) in downloads.confirm.iter().take(5).enumerate() {
                        lines.push(format!("{}. {t}\n   {u}", i + 1));
                    }
                    lines.push(
                        "可执行文件需人工确认，把要下的直链发我（“下载 <url>”）。".to_string(),
                    );
                    return Ok(lines.join("\n"));
                }
                // 没挑到直链：人在环 —— 给出候选，用户可给直链或换说法。
                let mut lines = vec![format!("没在“{query}”的结果里找到可下载直链。相关网页：")];
                for (i, (t, u)) in hits.iter().take(5).enumerate() {
                    lines.push(format!("{}. {t}\n   {u}", i + 1));
                }
                lines.push("可把直链发我（“下载 <url>”）或换个说法重试。".to_string());
                return Ok(lines.join("\n"));
            }
            // 取第一个可自动下载的直链（DDG 相关度序）交给 yt-dlp generic 下载。
            let (title, url) = &downloads.auto[0];
            // url 交给 yt-dlp 前校验：拒绝以 - 开头 + 限制 http(s)/ytsearch/bilisearch。
            validate_ytdlp_url(url).map_err(KernelError::Skill)?;
            let ytdlp = ensure_yt_dlp()?;
            let ytdlp_s = ytdlp.to_string_lossy().into_owned();
            let dl_dir = dirs::download_dir().unwrap_or_default();
            let out_tpl = dl_dir
                .join("%(title)s.%(ext)s")
                .to_string_lossy()
                .into_owned();
            let out = run_cmd(&ytdlp_s, &["-o", &out_tpl, "--no-playlist", url], 600, 2000)?;
            let dest = out
                .lines()
                .rev()
                .find_map(|l| {
                    l.find("Destination:")
                        .map(|p| l[p + 12..].trim().to_string())
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| dl_dir.display().to_string());
            Ok(format!(
                "已下载“{title}”到：{dest}\n来源：{url}（共 {} 个可自动下载直链）",
                downloads.auto.len()
            ))
        },
    )
}

pub fn execute_web_wallpapers(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text(inputs, "query")?;
    let mut map = HashMap::new();
    map.insert("query".to_string(), serde_json::json!(query));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "web.wallpapers",
        &format!("wallpapers:{query}"),
        false,
        &web_wallpapers_manifest(),
        &map,
        || {
            let url = format!(
                "https://wallhaven.cc/api/v1/search?q={}&sorting=favorites&order=desc&page=1",
                percent_encode(&query)
            );
            let body = curl_get(&url, 30_000)?;
            let v: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| KernelError::Skill(format!("壁纸结果解析失败: {e}")))?;
            let mut lines = vec![];
            if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                for item in arr.iter().take(5) {
                    let link = item
                        .get("path")
                        .and_then(|p| p.as_str())
                        .unwrap_or_default();
                    let reso = item
                        .get("resolution")
                        .and_then(|p| p.as_str())
                        .unwrap_or_default();
                    if !link.is_empty() {
                        lines.push(format!("- {reso} {link}"));
                    }
                }
            }
            if lines.is_empty() {
                return Ok(format!("没找到“{query}”的壁纸"));
            }
            Ok(format!("“{query}”壁纸直链：\n{}", lines.join("\n")))
        },
    )
}

pub fn web_weather_manifest() -> SkillManifest {
    simple_manifest(
        "web.weather",
        "查天气",
        "wttr.in 免费查任意城市天气（实时+今明两天）。",
        &["天气", "气温", "下雨", "穿什么", "冷不冷"],
        &["今天天气怎么样", "北京明天什么天气"],
        vec![query_slot()],
        false,
        EgressKind::WebToLocal,
        &[],
    )
}

pub fn execute_web_weather(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text(inputs, "query")?;
    let mut map = HashMap::new();
    map.insert("query".to_string(), serde_json::json!(query));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "web.weather",
        &format!("weather:{query}"),
        false,
        &web_weather_manifest(),
        &map,
        || {
            // %3Ccity%3E：一行式文本，中文城市名直接可用。
            let url = format!(
                "https://wttr.in/{}?format=%l:+%c+%t+%h+%w+%p&lang=zh",
                percent_encode(&query)
            );
            let line = curl_get(&url, 2000)?.trim().to_string();
            if line.is_empty() || line.to_lowercase().contains("unknown location") {
                return Ok(format!("查不到“{query}”的天气（换个城市名试试）"));
            }
            Ok(format!("{query}天气：{line}"))
        },
    )
}

/// 联赛名→thesportsdb id（免费测试 key，与 Daisy 一致）。
pub fn league_id(name: &str) -> Option<&'static str> {
    let n = name.trim().to_lowercase();
    for (keys, id) in [
        (
            &["英超", "english premier league", "premier league"] as &[&str],
            "4328",
        ),
        (&["西甲", "la liga"], "4335"),
        (&["德甲", "bundesliga"], "4331"),
        (&["意甲", "serie a"], "4332"),
        (&["法甲", "ligue 1"], "4334"),
        (&["欧冠", "champions league"], "4480"),
        (&["欧联", "europa league"], "4481"),
        (&["中超", "chinese super league"], "4376"),
        (&["世界杯", "world cup", "fifa world cup"], "worldcup"),
        (&["欧洲杯", "euro"], "euro"),
        (&["美洲杯", "copa"], "copa"),
    ] {
        if keys.iter().any(|k| n.contains(k)) {
            return Some(id);
        }
    }
    None
}

pub fn web_sports_manifest() -> SkillManifest {
    simple_manifest(
        "web.sports",
        "查赛程",
        "查足球联赛赛程（英超/西甲/德甲/意甲/法甲/欧冠/中超/世界杯等）。",
        &["赛程", "比赛", "英超", "欧冠", "世界杯", "中超", "比分"],
        &["英超赛程", "欧冠最近有什么比赛", "世界杯赛程"],
        vec![query_slot()],
        false,
        EgressKind::WebToLocal,
        &[],
    )
}

pub fn execute_web_sports(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text(inputs, "query")?;
    let mut map = HashMap::new();
    map.insert("query".to_string(), serde_json::json!(query));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "web.sports",
        &format!("sports:{query}"),
        false,
        &web_sports_manifest(),
        &map,
        || {
            let id = league_id(&query).ok_or_else(|| {
                KernelError::Skill(format!(
                    "暂不支持“{query}”（支持：英超/西甲/德甲/意甲/法甲/欧冠/欧联/中超/世界杯/欧洲杯/美洲杯）"
                ))
            })?;
            // 数字 id 走联赛近况，字符串 id（杯赛）走赛季查询。
            let url = if id.chars().all(|c| c.is_ascii_digit()) {
                format!("https://www.thesportsdb.com/api/v1/json/3/eventsnextleague.php?id={id}")
            } else {
                format!("https://www.thesportsdb.com/api/v1/json/3/search_all_seasons.php?id={id}")
            };
            let body = curl_get(&url, 20_000)?;
            let v: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| KernelError::Skill(format!("赛程解析失败: {e}")))?;
            let mut lines = vec![];
            if let Some(arr) = v.get("events").and_then(|e| e.as_array()) {
                for ev in arr.iter().take(8) {
                    let date = ev.get("dateEvent").and_then(|d| d.as_str()).unwrap_or("?");
                    let home = ev
                        .get("strHomeTeam")
                        .and_then(|t| t.as_str())
                        .unwrap_or("?");
                    let away = ev
                        .get("strAwayTeam")
                        .and_then(|t| t.as_str())
                        .unwrap_or("?");
                    let hs = ev
                        .get("intHomeScore")
                        .and_then(|s| s.as_str())
                        .unwrap_or("-");
                    let as_ = ev
                        .get("intAwayScore")
                        .and_then(|s| s.as_str())
                        .unwrap_or("-");
                    lines.push(format!("- {date} {home} {hs}:{as_} {away}"));
                }
            }
            if lines.is_empty() {
                return Ok(format!("“{query}”暂无赛程数据"));
            }
            Ok(format!("{query}赛程：\n{}", lines.join("\n")))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;

    #[test]
    fn web_manifests_have_ids() {
        for m in [
            web_search_manifest(),
            web_scrape_manifest(),
            web_fetch_file_manifest(),
            web_wallpapers_manifest(),
            web_weather_manifest(),
            web_sports_manifest(),
        ] {
            assert!(!m.id.is_empty() && !m.keywords.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn pick_download_hits_filters_by_extension_and_dedupes() {
        let hits = vec![
            (
                "VS Code 官方下载".to_string(),
                "https://code.visualstudio.com/docs?dv=win64".to_string(),
            ),
            (
                "VS Code 安装包".to_string(),
                "https://vscode.download.com/stable/xyz/VSCodeSetup-x64.exe".to_string(),
            ),
            (
                "重复直链".to_string(),
                "https://vscode.download.com/stable/xyz/VSCodeSetup-x64.exe".to_string(),
            ),
            (
                "介绍页（无扩展名）".to_string(),
                "https://example.com/download".to_string(),
            ),
            (
                "PDF 文档".to_string(),
                "https://example.com/report.PDF?dl=1".to_string(),
            ),
            (
                "伪装尾缀".to_string(),
                "https://example.com/exe.html".to_string(),
            ),
            (
                "Zip 归档".to_string(),
                "https://example.com/tool.zip".to_string(),
            ),
        ];
        let picked = pick_download_hits(&hits);
        // exe/zip → confirm 桶（可执行/归档类不自动下载，只出候选）。
        assert_eq!(
            picked.confirm,
            vec![
                (
                    "VS Code 安装包".to_string(),
                    "https://vscode.download.com/stable/xyz/VSCodeSetup-x64.exe".to_string(),
                ),
                (
                    "Zip 归档".to_string(),
                    "https://example.com/tool.zip".to_string()
                ),
            ]
        );
        // pdf → auto 桶（query 后缀不干扰匹配、去重、.html 伪装不算）。
        assert_eq!(
            picked.auto,
            vec![(
                "PDF 文档".to_string(),
                "https://example.com/report.PDF?dl=1".to_string(),
            )]
        );
    }

    #[test]
    fn percent_codecs_roundtrip_cjk() {
        let enc = percent_encode("天安门");
        assert!(!enc.contains("天"), "{enc}");
        assert_eq!(percent_decode(&enc), "天安门");
        assert_eq!(percent_decode("a+b%20c"), "a b c");
    }

    #[test]
    fn ddg_parser_extracts_title_and_link() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fx&amp;rut=1">示例标题</a>"#;
        let hits = parse_ddg_results(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "示例标题");
        assert_eq!(hits[0].1, "https://example.com/x");
    }

    #[test]
    fn league_id_matches_zh_and_en() {
        assert_eq!(league_id("英超赛程"), Some("4328"));
        assert_eq!(league_id("champions league"), Some("4480"));
        assert_eq!(league_id("世界杯"), Some("worldcup"));
        assert_eq!(league_id("NBA"), None);
    }

    #[test]
    fn weather_live_smoke() {
        // 轻量活探：需要联网；失败只打印不断言（CI 无网不断链）。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"query": "北京"});
        match execute_web_weather(&kernel, &AutoApprover, "t1", "s1", &inputs) {
            Ok(out) => assert!(out.contains("北京"), "{out}"),
            Err(e) => eprintln!("SKIP weather live: {e:?}"),
        }
    }
}
