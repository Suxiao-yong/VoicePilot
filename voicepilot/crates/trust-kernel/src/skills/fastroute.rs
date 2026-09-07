//! 原子快路由：意图匹配 + 参数提取一步完成（对标 Daisy `tryLocalCommand`）。
//!
//! 每条规则都是全句锚定（`^...$`）：命中即参数齐备，直接返回
//! `(skill_id, slots)`；形状含糊的一律返回 `None` 落到 LLM。
//! `kind` 必须等于后端 executor 取指的字段名（见各 execute_* 的
//! `slot_text` key），前端通用拼装器按名直映，无需逐 skill 写 builder。
//!
//! 有意不覆盖、留给 keyword/LLM 的：
//! - files.organize（source/filter/destination 三切分需文件系统判断）
//! - task.repeat_verified / task.compensate（需 DB task_id，文本里没有）
//! - mail.compose / form.prepare（多字段拆分歧义大：to/subject/body、fields map）
//! - pim.calendar_create（title/time 切分歧义大）
//! - quick.app_control（keyword + 前端文本兜底已覆盖全量）
//! - 无需参数的 skill（datetime/frontmost/quit_all/quit_browsers/lock_screen/
//!   clip.read/selected/save_image/selected_files/task.explain/pim.search/
//!   pim.calendar_list）：keyword 命中即原子（空 slots 可直接执行）
//!
//! 约定：
//! - `norm()` 只去空白和标点（不动正文，避免“太好了”→“太好”这类内容损坏）。
//! - 标识类捕获（url/path/keys/command/app/query/title）统一过 `trim_tail`
//!   去尾随语气词；自由正文（text/content）保持原样，卡片上可见。
//! - 文件路径一律过 `looks_like_path` + `strip_file_suffix` 两道门。

use crate::llm::types::ExtractedSlot;
use regex::Regex;
use std::sync::OnceLock;

fn slot(kind: &str, raw: impl Into<String>, high_risk: bool) -> ExtractedSlot {
    ExtractedSlot {
        kind: kind.to_string(),
        raw: raw.into(),
        high_risk,
    }
}

/// 入口规范化：去首尾空白 + 尾部标点（抄 Daisy tryLocalCommand）。
/// 只碰标点空白，不碰正文字符；返回借用输入的切片（零分配）。
fn norm(text: &str) -> &str {
    text.trim()
        .trim_end_matches([
            '，', ',', '。', '！', '!', '？', '?', '、', '~', '～', ' ', '\t',
        ])
        .trim()
}

/// 去尾随语气词（吧/呢/啊/哈）。只用于标识类捕获（url/path/keys/command/
/// app/query/title），自由正文（text/content）不用，保证不损坏内容。
fn trim_tail(s: &str) -> &str {
    s.trim_end_matches(['吧', '呢', '啊', '哈', ' ', '\t'])
}

/// 去掉开头请/帮我/麻烦/请问。
fn strip_polite(s: &str) -> &str {
    let mut t = s.trim();
    loop {
        let n = t
            .strip_prefix("帮我")
            .or_else(|| t.strip_prefix("麻烦"))
            .or_else(|| t.strip_prefix("请问"))
            .or_else(|| t.strip_prefix("请"));
        match n {
            Some(rest) => t = rest.trim(),
            None => break,
        }
    }
    t
}

/// 路径形状门卫：绝对路径 / ~/ / 已知目录前缀 / 引号包裹 / 含分隔符。
/// 通不过的一律 None（fail-closed），防止把自然语言当路径执行。
fn looks_like_path(s: &str) -> bool {
    let t = s
        .trim()
        .trim_matches(['《', '》', '「', '」', '"', '\'', '“', '”'])
        .trim();
    if t.is_empty() {
        return false;
    }
    if t.len() > 260 {
        return false;
    }
    let lower = t.to_lowercase();
    t.contains('/')
        || t.contains('\\')
        || (t.len() >= 2 && t.as_bytes()[1] == b':' && t.as_bytes()[0].is_ascii_alphabetic())
        || t.starts_with("~/")
        || t.starts_with("documents/")
        || t.starts_with("desktop/")
        || lower.starts_with("文档/")
        || lower.starts_with("桌面/")
        || lower.starts_with("下载/目录")
}

fn strip_quotes(s: &str) -> String {
    s.trim()
        .trim_matches(['《', '》', '「', '」', '"', '\'', '“', '”'])
        .trim()
        .to_string()
}

/// 去掉路径末尾口语“文件”二字，但仅当去掉后仍带扩展名时
/// （“D:/x.md文件”→“D:/x.md”；“D:/笔记文件”保持原样，避免吃掉真目录名）。
fn strip_file_suffix(s: &str) -> &str {
    if let Some(stripped) = s.strip_suffix("文件") {
        let t = stripped.trim_end();
        if t.rsplit(['/', '\\', '：', ':'])
            .next()
            .is_some_and(|last| last.contains('.') && !last.starts_with('.'))
        {
            return t;
        }
    }
    s
}

fn rx(pattern: &str) -> Regex {
    Regex::new(pattern).expect("fastroute regex must compile")
}

/// 文本里找第一个 http(s) URL。
fn find_url(text: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r#"https?://[^\s，,。！!？?、~～"'“”]+"#));
    re.find(text).map(|m| {
        m.as_str()
            .trim_end_matches(['.', ')', ']', '>', '!', '?', ';', ':', '、', '。'])
            .to_string()
    })
}

/// 网页操作动词（存/下/抓）出现即不属于“打开网址”。
fn has_save_like_verb(text: &str) -> bool {
    [
        "保存", "存为", "存到", "存下", "下载", "抓取", "爬取", "提取", "转存", "另存",
    ]
    .iter()
    .any(|w| text.contains(w))
}

// ===== sys.open_url：裸 URL（打开/访问/进入，或裸 URL 本身） =====

pub(crate) fn parse_open_url(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = strip_polite(norm(text));
    // “搜/查 + URL”多半是想打开它（如“搜一下https://x”）。
    static RE_SEARCH_URL: OnceLock<Regex> = OnceLock::new();
    if let Some(cap) = RE_SEARCH_URL
        .get_or_init(|| rx(r"^(?:搜一下|搜|查一下|找一下)\s*(https?://\S+)\s*$"))
        .captures(t)
    {
        let url = trim_tail(cap.get(1)?.as_str()).to_string();
        if find_url(&url).is_some() {
            return Some(vec![slot("url", url, false)]);
        }
        return None;
    }
    let has_open_verb = ["打开", "启动", "访问", "进入", "浏览"]
        .iter()
        .any(|w| t.contains(w));
    let url = find_url(t)?;
    if has_save_like_verb(t) {
        return None;
    }
    if !has_open_verb {
        // 纯裸 URL 整句才接，避免从长句里误掏。
        let stripped: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        if stripped != url {
            return None;
        }
    }
    Some(vec![slot("url", trim_tail(&url), false)])
}

// ===== web.scrape：抓取/爬取 URL =====

pub(crate) fn parse_scrape(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    let has_scrape_verb = ["抓取", "爬取", "爬一下", "抓一下"]
        .iter()
        .any(|w| t.contains(w));
    if !has_scrape_verb {
        return None;
    }
    let url = find_url(t)?;
    Some(vec![slot("url", trim_tail(&url), false)])
}

// ===== research.save_markdown：把URL存到路径 =====

pub(crate) fn parse_research_save(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:把)?(https?://\S+?)(?:这个|该)?(?:网页|页面|文章)?(?:存(?:为|到|成)|保存到|下载到)(.+)$")
    });
    let cap = re.captures(t)?;
    let url = trim_tail(cap.get(1)?.as_str()).to_string();
    let save_path = strip_quotes(trim_tail(cap.get(2)?.as_str()));
    if find_url(&url).is_none() || !looks_like_path(&save_path) {
        return None;
    }
    Some(vec![
        slot("url", url, false),
        slot("save_path", save_path, false),
    ])
}

// ===== media.download：下载 URL =====

/// web.fetch_file：下载软件安装包/文档等通用文件（无 URL 形态）。
/// 软件类关键词命中才接（其余“下载X”归 media.download 的 ytsearch）。
/// 排在 media.download 规则之前（特异度优先）。
pub(crate) fn parse_fetch_file(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    if !["下载", "下个", "下一个"].iter().any(|w| t.contains(w)) {
        return None;
    }
    if t.contains("存到") || t.contains("存为") {
        return None;
    }
    if find_url(t).is_some() {
        return None; // 带 URL 的归 media.download
    }
    let query = extract_download_query(t)?;
    let software_words = [
        "软件",
        "安装包",
        "安装程序",
        "安装器",
        "exe",
        "msi",
        "apk",
        "安装版",
        "客户端",
        "驱动",
        "补丁",
        "文档",
        "报告",
        "手册",
        "说明书",
        "pdf",
    ];
    if software_words
        .iter()
        .any(|w| query.to_lowercase().contains(w))
    {
        return Some(vec![slot("query", query, false)]);
    }
    None
}

pub(crate) fn parse_download(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    if !["下载", "下歌", "下视频"].iter().any(|w| t.contains(w)) {
        return None;
    }
    // 存文件类归 research.save；这里只要“下载 + URL”。
    if t.contains("存到") || t.contains("存为") {
        return None;
    }
    // “整理下载目录”里的“下载”是目录名（Downloads），动词是整理：
    // 含整理/归档一律让给 files.organize keyword 路由，不劫持。
    if ["整理", "归档"].iter().any(|w| t.contains(w)) {
        return None;
    }
    let audio_only = ["音频", "音乐", "歌", "mp3", "wav", "flac", "一首"]
        .iter()
        .any(|w| t.contains(w));
    if let Some(url) = find_url(t) {
        let mut slots = vec![slot("url", trim_tail(&url), false)];
        if audio_only {
            slots.push(slot("audio_only", "true", false));
        }
        return Some(slots);
    }
    // 无 URL：音视频 → yt-dlp 原生搜索表达式（ytsearch1:/bilisearch1:），
    // “自行找源再下载”的极简形态：yt-dlp 先搜索再下载首个结果。
    // 软件安装包/文档类由 parse_fetch_file（web.fetch_file 规则，排在前）接走。
    // 指代词（这个/那个）没有实体，两者都 fail-closed 返回 None 落 LLM。
    let mut query = extract_download_query(t)?;
    // 提及 B站 → bilisearch 前缀；查询词里的“B站的”冗余，一并去掉。
    if ["B站", "b站", "哔哩", "bilibili"]
        .iter()
        .any(|w| t.contains(w))
    {
        for p in [
            "B站的",
            "b站的",
            "哔哩的",
            "bilibili的",
            "B站",
            "b站",
            "哔哩",
            "bilibili",
        ] {
            query = query
                .strip_prefix(p)
                .map(str::trim)
                .unwrap_or(&query)
                .to_string();
        }
        let mut slots = vec![slot("url", format!("bilisearch1:{query}"), false)];
        if audio_only {
            slots.push(slot("audio_only", "true", false));
        }
        return Some(slots);
    }
    let mut slots = vec![slot("url", format!("ytsearch1:{query}"), false)];
    if audio_only {
        slots.push(slot("audio_only", "true", false));
    }
    Some(slots)
}

/// 从“下载晴天”这类无链接请求里提取资源名（去动词/量词前缀，去指代词）。
fn extract_download_query(t: &str) -> Option<String> {
    let mut q = t.trim();
    loop {
        let n = q
            .strip_prefix("下载")
            .or_else(|| q.strip_prefix("下歌"))
            .or_else(|| q.strip_prefix("下视频"))
            .or_else(|| q.strip_prefix("一首"))
            .or_else(|| q.strip_prefix("一个"))
            .or_else(|| q.strip_prefix("个"))
            .or_else(|| q.strip_prefix("首"));
        match n {
            Some(rest) => q = rest.trim(),
            None => break,
        }
    }
    if q.is_empty() || q.chars().count() > 40 {
        return None;
    }
    // 指代词/省略宾语：不知道用户要什么，fail-closed 落 LLM。
    if ["这个", "那个", "它", "此", "刚才", "这些", "那些"]
        .iter()
        .any(|w| q.contains(w))
    {
        return None;
    }
    Some(q.to_string())
}

// ===== clip.write / clip.type_text：动词 + 剩余全文（正文原样保留） =====

pub(crate) fn parse_clip_write(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:复制|拷贝|存剪贴板|放到剪贴板|存到剪贴板)(?:以下内容)?[:：]?\s*(.+)$")
    });
    let cap = re.captures(strip_polite(norm(text)))?;
    let content = cap.get(1)?.as_str().trim().to_string();
    if content.is_empty() || content.chars().count() > 8000 {
        return None;
    }
    Some(vec![slot("text", content, false)])
}

pub(crate) fn parse_clip_type(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(?:在这里)?(?:打字|键入|输入|替我输入|帮我输)[:：]?\s*(.+)$"));
    let cap = re.captures(strip_polite(norm(text)))?;
    let content = cap.get(1)?.as_str().trim().to_string();
    if content.is_empty() || content.chars().count() > 2000 {
        return None;
    }
    Some(vec![slot("text", content, false)])
}

// ===== clip.press_keys：按X（中文键名归一 + 白名单校验） =====

fn normalize_key_name(raw: &str) -> Option<String> {
    let t: String = raw
        .trim()
        .replace('＋', "+")
        .replace(' ', "")
        .to_lowercase();
    // 中文名单 → spec。
    let mapped = match t.as_str() {
        "回车" => "enter",
        "空格" | "空格键" => "space",
        "退格" | "退格键" => "backspace",
        "删除" | "删除键" => "delete",
        "插入" => "insert",
        "上" | "上箭头" | "方向键上" => "up",
        "下" | "下箭头" | "方向键下" => "down",
        "左" | "左箭头" | "方向键左" => "left",
        "右" | "右箭头" | "方向键右" => "right",
        _ => t.as_str(),
    }
    .to_string();
    // 与后端 sendkeys_for_spec 同构的白名单校验：解析失败即形状不对。
    crate::skills::clip_ops::sendkeys_for_spec(&mapped).ok()?;
    Some(mapped)
}

pub(crate) fn parse_clip_press(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^按\s*(.+?)\s*$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let keys = normalize_key_name(cap.get(1)?.as_str())?;
    Some(vec![slot("keys", keys, false)])
}

// ===== shell.run：运行/执行 + 剩余全文 =====

pub(crate) fn parse_shell(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(?:运行|执行)(?:命令|脚本|cmd)?[:：]?\s*(.+)$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let command = cap.get(1)?.as_str().trim().to_string();
    if command.is_empty() || command.chars().count() > 2000 {
        return None;
    }
    // shell 保留审批（PerStep），误解析仍有确认卡兜底。
    Some(vec![slot("command", command, true)])
}

// ===== fs.read/list：读/列 + 路径形状 =====

pub(crate) fn parse_fs_read(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(re_capture_read);
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let mut p = cap.get(1)?.as_str().trim().to_string();
    for suf in ["文件的内容", "的内容", "文件", "内容"] {
        if let Some(s) = p.strip_suffix(suf) {
            p = s.trim().to_string();
            break;
        }
    }
    let p = strip_file_suffix(&p);
    if !looks_like_path(p) {
        return None;
    }
    Some(vec![slot("path", strip_quotes(p), false)])
}

fn re_capture_read() -> Regex {
    rx(r"^(?:读|查看|看看|看一下|显示|浏览)\s*(.+?)\s*$")
}

pub(crate) fn parse_fs_list(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(?:列出|列一下|看看|查看|显示)\s*(.+?)\s*$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let mut p = cap.get(1)?.as_str().trim().to_string();
    for suf in [
        "目录下所有文件",
        "目录下的文件",
        "目录下文件",
        "目录下",
        "目录里",
        "目录",
        "文件夹下",
        "文件夹",
        "下所有文件",
        "下的文件",
        "下文件",
    ] {
        if let Some(s) = p.strip_suffix(suf) {
            let s = s.trim();
            if !s.is_empty() {
                p = s.to_string();
            }
            break;
        }
    }
    let p = strip_file_suffix(&p);
    if !looks_like_path(p) {
        return None;
    }
    Some(vec![slot("path", strip_quotes(p), false)])
}

// ===== fs.delete：删除 + 路径形状（高风险） =====

pub(crate) fn parse_fs_delete(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(?:删除|删掉|移除|删)\s*(.+?)\s*$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let p = strip_file_suffix(cap.get(1)?.as_str().trim());
    if !looks_like_path(p) {
        return None;
    }
    Some(vec![slot("path", strip_quotes(p), true)])
}

// ===== fs.write：把X写入Y / 写X到Y（正文原样保留） =====

pub(crate) fn parse_fs_write(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = strip_polite(norm(text));
    static RE1: OnceLock<Regex> = OnceLock::new();
    if let Some(cap) = RE1.get_or_init(|| rx(r"^把(.+?)写入(.+?)$")).captures(t) {
        let content = cap.get(1)?.as_str().trim().to_string();
        let p = strip_file_suffix(trim_tail(cap.get(2)?.as_str().trim()));
        if content.is_empty() || !looks_like_path(p) {
            return None;
        }
        return Some(vec![
            slot("path", strip_quotes(p), false),
            slot("content", content, false),
        ]);
    }
    static RE2: OnceLock<Regex> = OnceLock::new();
    if let Some(cap) = RE2
        .get_or_init(|| rx(r"^(?:写|保存|存)(?:入)?(.+?)到(.+?)$"))
        .captures(t)
    {
        let content = cap.get(1)?.as_str().trim().to_string();
        let mut p = cap.get(2)?.as_str().trim().to_string();
        for suf in ["文件里", "文件", "里", "中"] {
            if let Some(s) = p.strip_suffix(suf) {
                if !s.trim().is_empty() {
                    p = s.trim().to_string();
                }
                break;
            }
        }
        let p = strip_file_suffix(trim_tail(&p));
        if content.is_empty() || !looks_like_path(p) {
            return None;
        }
        // “保存图片/保存网页”类归 clip.save_image / research.save，不归写文件。
        if content.contains("图片") || content.contains("网页") {
            return None;
        }
        return Some(vec![
            slot("path", strip_quotes(p), false),
            slot("content", content, false),
        ]);
    }
    None
}

// ===== fs.create：新建文件（路径明确才接） =====

pub(crate) fn parse_fs_create(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    static RE: OnceLock<Regex> = OnceLock::new();
    let cap = RE
        .get_or_init(|| rx(r"^新建(?:一个|一份|个)?(?:空)?文件\s*(.+?)\s*$"))
        .captures(t)?;
    let p = strip_file_suffix(cap.get(1)?.as_str().trim());
    if !looks_like_path(p) {
        return None;
    }
    Some(vec![slot("path", strip_quotes(p), false)])
}

// ===== sys.volume / sys.playback：固定词表 → 常量 =====

pub(crate) fn parse_volume(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    if !(t.contains("音量") || t.contains("声音") || t.contains("静音")) {
        return None;
    }
    // 必须整句都是调音量意图（锚定），避免从长句误掏。
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:把)?(?:音量|声音)?(调高|调低|增大|减小|调大|调小|开大|开小|静音)(?:音量|声音)?$")
    });
    let cap = re.captures(t)?;
    let direction = match cap.get(1)?.as_str() {
        "静音" => "mute",
        w if ["调高", "增大", "调大", "开大"].contains(&w) => "up",
        _ => "down",
    };
    Some(vec![slot("direction", direction, false)])
}

pub(crate) fn parse_playback(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(暂停|播放|继续播放|下一首|下一曲|切歌|上一首|上一曲)$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let action = match cap.get(1)?.as_str() {
        "下一首" | "下一曲" | "切歌" => "next",
        "上一首" | "上一曲" => "prev",
        _ => "playpause",
    };
    Some(vec![slot("action", action, false)])
}

// ===== 时间表达式：HH:mm 归一（12 小时制按时段词换算） =====

/// 解析 `上午8点 / 下午3点半 / 14:05 / 7点05分` → `HH:mm`；含糊返回 None。
fn parse_clock_expr(head: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(上午|下午|晚上|早上|凌晨|中午)?\s*(\d{1,2})(?::|点)\s*(\d{1,2})?\s*(分)?(半)?$")
    });
    let cap = re.captures(head.trim())?;
    let period = cap.get(1).map(|m| m.as_str()).unwrap_or("");
    let mut h: u32 = cap.get(2)?.as_str().parse().ok()?;
    let m: u32 = if cap.get(5).is_some() {
        30
    } else {
        cap.get(3)
            .map(|x| x.as_str().parse().ok())
            .unwrap_or(Some(0))?
    };
    if h > 23 || m > 59 {
        return None;
    }
    if ["下午", "晚上"].contains(&period) && h < 12 {
        h += 12;
    }
    // 凌晨 0-5 点不动；上午/早上不动；无标记不动。
    Some(format!("{h:02}:{m:02}"))
}

// ===== sys.timer_set：倒计时N秒/分/时 =====

pub(crate) fn parse_timer(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(?:倒计时|计时)\s*(\d+)\s*(秒|秒钟|分钟|分|小时|时)$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let n: i64 = cap.get(1)?.as_str().parse().ok()?;
    let seconds = match cap.get(2)?.as_str() {
        "秒" | "秒钟" => n,
        "分钟" | "分" => n * 60,
        _ => n * 3600,
    };
    if !(1..=86400).contains(&seconds) {
        return None;
    }
    Some(vec![slot("seconds", seconds.to_string(), false)])
}

// ===== sys.alarm_set：时间 [+叫我类动词]，无标题尾巴 =====
// ===== pim.reminder_create：时间 + 提醒我 + 标题 =====

/// “叫我/叫醒/起床/闹钟”类动词 → 闹钟；“提醒我”带标题 → 备忘提醒（先跑）。
pub(crate) fn parse_alarm(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^((?:上午|下午|晚上|早上|凌晨|中午)?\s*\d{1,2}(?::|点)\s*(?:\d{1,2}\s*(?:分)?|半)?)\s*(叫我|叫醒我|喊我|起床|闹钟)?\s*$")
    });
    let t = trim_tail(strip_polite(norm(text)));
    let cap = re.captures(t)?;
    // 有标题尾巴的“提醒我X”归 reminder 解析器（它先跑）；这里只接无尾巴形状。
    let clock = cap.get(1)?.as_str();
    let time = parse_clock_expr(clock)?;
    Some(vec![slot("time", time, false)])
}

pub(crate) fn parse_reminder(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(上午|下午|晚上|早上|凌晨|中午)?\s*(\d{1,2})(?::|点)\s*(\d{1,2})?\s*(分)?(半)?\s*提醒我\s*(.+?)\s*$")
    });
    let t = trim_tail(strip_polite(norm(text)));
    let cap = re.captures(t)?;
    let period = cap.get(1).map(|m| m.as_str()).unwrap_or("");
    let h = cap.get(2)?.as_str();
    let mut clock = if period.is_empty() {
        h.to_string()
    } else {
        format!("{period}{h}")
    };
    clock.push('点');
    if let Some(mm) = cap.get(3) {
        clock.push_str(&format!("{}分", mm.as_str()));
    } else if cap.get(5).is_some() {
        clock.push('半');
    }
    // 标题原样保留（卡片可见）；超长拒绝。
    let title = cap.get(6)?.as_str().trim().to_string();
    if title.is_empty() || title.chars().count() > 200 {
        return None;
    }
    Some(vec![
        slot("time", parse_clock_expr(&clock)?, false),
        slot("title", title, false),
    ])
}

// ===== sys.maps_search：导航到X / X怎么走 =====

/// 裸动词（打开/搜/查/找等）不算目的地，避免“打开导航”误命中。
fn is_bare_verb(s: &str) -> bool {
    matches!(
        s,
        "打开"
            | "启动"
            | "关闭"
            | "搜索"
            | "查找"
            | "搜"
            | "查"
            | "找"
            | "看"
            | "听"
            | "读"
            | "写"
            | "删"
            | "运行"
            | "执行"
            | "设置"
            | "换"
    )
}

pub(crate) fn parse_maps(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:导航到|导航去|带我去|送我去|怎么去|怎么走|路线)(.+?)$|^(?:帮我|请)?(?:查一下|搜一下)?(.+?)(?:怎么走|怎么去|导航|路线)$")
    });
    let cap = re.captures(t)?;
    let query = trim_tail(cap.get(1).or_else(|| cap.get(2))?.as_str().trim()).to_string();
    if query.is_empty() || is_bare_verb(&query) || query.chars().count() > 200 {
        return None;
    }
    Some(vec![slot("query", query, false)])
}

// ===== sys.diagnose_app：为什么X打不开 / 诊断X =====

pub(crate) fn parse_diagnose(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:为什么|为啥)(.+?)(打不开|启动不了|启动失败|闪退|没反应|卡死)$|^诊断(.+?)(打不开的原因)?$")
    });
    let cap = re.captures(t)?;
    let app = trim_tail(cap.get(1).or_else(|| cap.get(3))?.as_str().trim()).to_string();
    if app.is_empty() || is_bare_verb(&app) || app.chars().count() > 100 {
        return None;
    }
    Some(vec![slot("app_name", app, false)])
}

// ===== web.search / wallpapers / weather / sports =====

pub(crate) fn parse_web_search(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    // 备忘/笔记/邮件/日程/提醒归 pim，不归网页搜索。
    if ["备忘", "笔记", "邮件", "日程", "提醒", "日历"]
        .iter()
        .any(|w| t.contains(w))
    {
        return None;
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(re_capture_web_search);
    let cap = re.captures(t)?;
    let query = trim_tail(cap.get(1)?.as_str().trim()).to_string();
    if query.is_empty() || query.chars().count() > 200 {
        return None;
    }
    // 路径形状归 fs（“查找D盘文件”不是网页搜索）。
    if looks_like_path(&query) {
        return None;
    }
    Some(vec![slot("query", query, false)])
}

fn re_capture_web_search() -> Regex {
    rx(r"^(?:帮我|请)?(?:搜(?:索|一下)?|查一下|查|百度一下|搜搜)(.+)$")
}

pub(crate) fn parse_weather(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:帮我|请)?(?:查一下|查|看一下|看)?(.+?)天气(?:怎么样|如何|预报|情况)?$|^(.+?)今天穿什么$")
    });
    let t = trim_tail(strip_polite(norm(text)));
    let cap = re.captures(t)?;
    let city = trim_tail(cap.get(1).or_else(|| cap.get(2))?.as_str().trim()).to_string();
    if city.is_empty() || is_bare_verb(&city) || city.chars().count() > 50 {
        return None;
    }
    Some(vec![slot("query", city, false)])
}

pub(crate) fn parse_sports(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(.+?)(比分|比赛结果|战绩|赛程)$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let query = trim_tail(cap.get(1)?.as_str().trim()).to_string();
    if query.is_empty() || query.chars().count() > 100 {
        return None;
    }
    Some(vec![slot("query", query, false)])
}

// ===== Phase A：Playwright MCP 一等公民化（极简快路由形态）=====
//
// 网页截图：只接“网页/页面截图”裸形态（当前自动化页面，零参数）。
// 带目标 URL 的形状（“截取 example.com”）需要 navigate+截图两步，
// 单技能接不住 —— fail-closed 返回 None 落 LLM 拆步（既有 playwright
// MCP server + 捆绑 web.page_* 技能）。
//
// 网页点击 / 网页填表：需要 navigate + 快照 + ref 引用号，天然多步，
// 无单步可接的极简形态 —— 故意不加快路由，一律落 LLM（同 fail-closed）。
// “打开网站”已由 sys.open_url::parse_open_url 覆盖（OS 浏览器，先于本表）。
pub(crate) fn parse_page_screenshot(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:帮我|请)?(?:网页|页面|当前网页|当前页面)(?:个)?截图(?:一下|一张)?$")
    });
    if re.is_match(trim_tail(strip_polite(norm(text)))) {
        // 零参数：截当前自动化页面（browser_take_screenshot 无参形态）。
        Some(Vec::new())
    } else {
        None
    }
}

pub(crate) fn parse_wallpapers(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^(?:换|找|搜|下载)?(.+?)(壁纸|桌面背景|桌面壁纸)$|^(?:换壁纸|换桌面)(?:成)?(.+)$")
    });
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let raw = cap
        .get(1)
        .or_else(|| cap.get(3))?
        .as_str()
        .trim()
        .to_string();
    // 去掉 leading 动词/量词（“换张猫壁纸”→“猫”）。
    let mut query = raw.as_str();
    loop {
        let n = query
            .strip_prefix(['换', '找', '搜'])
            .or_else(|| query.strip_prefix("下载"))
            .or_else(|| query.strip_prefix(['一', '张', '个']));
        match n {
            Some(rest) => query = rest.trim(),
            None => break,
        }
        if query.is_empty() {
            break;
        }
    }
    let query = query.to_string();
    // 裸“换壁纸”：默认搜最新（只读搜索，卡片可见）。
    let query = if query.is_empty() {
        "最新".to_string()
    } else {
        query
    };
    if query.chars().count() > 100 {
        return None;
    }
    Some(vec![slot("query", query, false)])
}

// ===== pim.note_create：记住X（标题原样保留，卡片可见） =====

pub(crate) fn parse_pim_note(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^(?:记住|记一下|记一条|备忘)[:：]?\s*(.+)$"));
    let cap = re.captures(strip_polite(norm(text)))?;
    let title = cap.get(1)?.as_str().trim().to_string();
    if title.is_empty() || title.chars().count() > 200 {
        return None;
    }
    Some(vec![slot("title", title, false)])
}

// ===== media.trim / media.convert / doc.convert =====

pub(crate) fn parse_media_trim(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^把(.+?)从(.+?)(?:剪|截|裁)到(.+?)$|^截取(.+?)的(.+?)到(.+)$"));
    let cap = re.captures(trim_tail(strip_polite(norm(text))))?;
    let (source, start, end) = if cap.get(4).is_some() {
        (
            cap.get(4)?.as_str().trim().to_string(),
            trim_tail(cap.get(5)?.as_str().trim()).to_string(),
            trim_tail(cap.get(6)?.as_str().trim()).to_string(),
        )
    } else {
        (
            cap.get(1)?.as_str().trim().to_string(),
            trim_tail(cap.get(2)?.as_str().trim()).to_string(),
            trim_tail(cap.get(3)?.as_str().trim()).to_string(),
        )
    };
    let source = strip_file_suffix(&source);
    if !looks_like_path(source) || source.len() > 500 {
        return None;
    }
    Some(vec![
        slot("source", strip_quotes(source), false),
        slot("start", start, false),
        slot("end", end, false),
    ])
}

pub(crate) fn parse_media_convert(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    // 形状1：把X转成<固定格式白名单>；形状2：把X转换成Y（开放格式）。
    // 分开两个正则，避免 alternation 分组编号错位。
    static RE1: OnceLock<Regex> = OnceLock::new();
    if let Some(cap) = RE1
        .get_or_init(|| rx(r"^把(.+?)转成(mp3|mp4|wav|avi|mkv|mov|flac|ogg|m4a)$"))
        .captures(t)
    {
        let source = strip_file_suffix(cap.get(1)?.as_str().trim());
        let format = cap.get(2)?.as_str().trim().to_lowercase();
        if !looks_like_path(source) {
            return None;
        }
        return Some(vec![
            slot("source", strip_quotes(source), false),
            slot("format", format, false),
        ]);
    }
    static RE2: OnceLock<Regex> = OnceLock::new();
    let cap = RE2
        .get_or_init(|| rx(r"^把(.+?)转换成(.+?)(格式)?$"))
        .captures(t)?;
    let source = strip_file_suffix(cap.get(1)?.as_str().trim());
    let format = trim_tail(cap.get(2)?.as_str().trim()).to_lowercase();
    if !looks_like_path(source) || format.is_empty() || format.chars().count() > 10 {
        return None;
    }
    Some(vec![
        slot("source", strip_quotes(source), false),
        slot("format", format, false),
    ])
}

pub(crate) fn parse_doc_convert(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = trim_tail(strip_polite(norm(text)));
    static RE1: OnceLock<Regex> = OnceLock::new();
    if let Some(cap) = RE1
        .get_or_init(|| rx(r"^把(.+?)转成(pdf|docx|xlsx|pptx)$"))
        .captures(t)
    {
        let source = strip_file_suffix(cap.get(1)?.as_str().trim());
        if !looks_like_path(source) {
            return None;
        }
        return Some(vec![
            slot("source", strip_quotes(source), false),
            slot("format", cap.get(2)?.as_str().trim().to_lowercase(), false),
        ]);
    }
    static RE2: OnceLock<Regex> = OnceLock::new();
    let cap = RE2
        .get_or_init(|| rx(r"^把(.+?)转换成(pdf|docx|xlsx|pptx)$"))
        .captures(t)?;
    let source = strip_file_suffix(cap.get(1)?.as_str().trim());
    if !looks_like_path(source) {
        return None;
    }
    Some(vec![
        slot("source", strip_quotes(source), false),
        slot("format", cap.get(2)?.as_str().trim().to_lowercase(), false),
    ])
}

// ===== doc.office：新建 word/excel/ppt（应用明确才接） =====

pub(crate) fn parse_doc_office(text: &str) -> Option<Vec<ExtractedSlot>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        rx(r"^新建(?:一个|一份|个)?(word文档|word|excel表格|excel|表格|ppt|幻灯片|演示文稿|文档)?[:：]?\s*(.*)$")
    });
    let cap = re.captures(strip_polite(norm(text)))?;
    let app_word = cap.get(1).map(|m| m.as_str()).unwrap_or("");
    let text_body = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
    // 应用不明（如“新建文件D:/a.txt”）一律落空，归 fs.create。
    if app_word.is_empty() {
        return None;
    }
    let app = match app_word {
        w if w.contains("excel") || w.contains("表格") => "excel",
        w if w.contains("ppt") || w.contains("幻灯片") || w.contains("演示") => "ppt",
        _ => "word",
    };
    Some(vec![
        slot("operation", "create", false),
        slot("app", app, false),
        slot("text", text_body, false),
    ])
}

// ===== note.capture：把X记到Y（路径明确才接） =====

pub(crate) fn parse_note_capture(text: &str) -> Option<Vec<ExtractedSlot>> {
    let t = strip_polite(norm(text));
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| rx(r"^把(.+?)记到(.+?)$|^记(.+?)存到(.+?)$"));
    let cap = re.captures(t)?;
    let (content, save_path) = if cap.get(3).is_some() {
        (cap.get(3)?.as_str(), cap.get(4)?.as_str())
    } else {
        (cap.get(1)?.as_str(), cap.get(2)?.as_str())
    };
    let content = content.trim().to_string();
    let save_path = strip_file_suffix(trim_tail(save_path.trim()));
    if content.is_empty() || content.chars().count() > 10000 || !looks_like_path(save_path) {
        return None;
    }
    Some(vec![
        slot("content", content, false),
        slot("save_path", strip_quotes(save_path), false),
    ])
}

// ===== 入口：按特异度排序，命中即返回；全落空返回 None（调用方走 keyword/LLM） =====

/// 快路由规则表项：(skill_id, 全句锚定解析器)。
type FastRule = (&'static str, fn(&str) -> Option<Vec<ExtractedSlot>>);
pub fn match_fast_route(
    text: &str,
    manifests: &[crate::skills::manifest::SkillManifest],
) -> Option<(String, Vec<ExtractedSlot>)> {
    let has = |id: &str| manifests.iter().any(|m| m.id == id);
    // 实现注意：尽量纯函数 + 全句锚定；顺序 = 特异度降序。
    let rules: &[FastRule] = &[
        ("web.scrape", parse_scrape),
        ("research.save_markdown", parse_research_save),
        // 通用文件（软件/文档）排在音视频之前：软件关键词特异度更高。
        ("web.fetch_file", parse_fetch_file),
        ("media.download", parse_download),
        ("sys.open_url", parse_open_url),
        ("media.trim_video", parse_media_trim),
        ("media.convert_video", parse_media_convert),
        ("doc.convert", parse_doc_convert),
        ("doc.office", parse_doc_office),
        ("fs.write_file", parse_fs_write),
        ("fs.create_file", parse_fs_create),
        ("fs.delete_file", parse_fs_delete),
        ("fs.read_file", parse_fs_read),
        ("fs.list_dir", parse_fs_list),
        ("clip.write", parse_clip_write),
        ("clip.type_text", parse_clip_type),
        ("clip.press_keys", parse_clip_press),
        ("shell.run", parse_shell),
        ("sys.volume", parse_volume),
        ("sys.playback", parse_playback),
        ("sys.timer_set", parse_timer),
        ("pim.reminder_create", parse_reminder),
        ("sys.alarm_set", parse_alarm),
        ("sys.maps_search", parse_maps),
        ("sys.diagnose_app", parse_diagnose),
        ("web.search", parse_web_search),
        ("web.weather", parse_weather),
        ("web.sports", parse_sports),
        ("web.wallpapers", parse_wallpapers),
        ("pim.note_create", parse_pim_note),
        ("note.capture", parse_note_capture),
        // Phase A：Playwright 一等公民化。零参数裸形态；其余形状落 LLM。
        ("web.page_screenshot", parse_page_screenshot),
    ];
    for (id, parse) in rules {
        if !has(id) {
            continue;
        }
        if let Some(slots) = parse(text) {
            return Some((id.to_string(), slots));
        }
    }
    None
}

#[cfg(test)]
mod fastroute_tests {
    use super::*;

    fn slots_to_map(slots: &[ExtractedSlot]) -> std::collections::HashMap<String, String> {
        slots
            .iter()
            .map(|s| (s.kind.clone(), s.raw.clone()))
            .collect()
    }

    #[test]
    fn open_url_bare_and_verb_forms() {
        let m = slots_to_map(&parse_open_url("打开 https://example.com/a").unwrap());
        assert_eq!(m["url"], "https://example.com/a");
        let m = slots_to_map(&parse_open_url("https://example.com/a").unwrap());
        assert_eq!(m["url"], "https://example.com/a");
        // 存/下/抓动词不接。
        assert!(parse_open_url("把这个网页存为备忘.md").is_none());
        assert!(parse_open_url("下载https://x.com/a.mp4").is_none());
        assert!(parse_open_url("抓取https://x.com/a").is_none());
    }

    #[test]
    fn scrape_needs_url_and_verb() {
        let m = slots_to_map(&parse_scrape("抓取https://example.com/a").unwrap());
        assert_eq!(m["url"], "https://example.com/a");
        assert!(parse_scrape("抓取这个网页").is_none());
        assert!(parse_scrape("打开https://example.com/a").is_none());
    }

    #[test]
    fn research_save_needs_url_and_path() {
        let m = slots_to_map(
            &parse_research_save("把https://example.com/a存到Documents/x.md").unwrap(),
        );
        assert_eq!(m["url"], "https://example.com/a");
        assert_eq!(m["save_path"], "Documents/x.md");
        assert!(parse_research_save("把这个网页存一下").is_none());
    }

    #[test]
    fn download_detects_audio_flag() {
        let m = slots_to_map(&parse_download("下载https://x.com/a.mp4").unwrap());
        assert_eq!(m["url"], "https://x.com/a.mp4");
        assert!(
            slots_to_map(&parse_download("下载一首歌 https://x.com/a").unwrap()).get("audio_only")
                == Some(&"true".to_string())
        );
        assert!(parse_download("下载这个视频").is_none());
    }

    #[test]
    fn fetch_file_routes_software_queries() {
        // 软件安装包/文档类 → web.fetch_file（query 槽位）。
        let m = slots_to_map(&parse_fetch_file("下载微信安装包").unwrap());
        assert_eq!(m["query"], "微信安装包");
        let m = slots_to_map(&parse_fetch_file("帮我下载VS Code 安装包").unwrap());
        assert_eq!(m["query"], "VS Code 安装包");
        let m = slots_to_map(&parse_fetch_file("下载那份PDF报告").unwrap());
        assert_eq!(m["query"], "那份PDF报告");
        // 裸应用名（“下载微信”）与歌名不可区分 → 不接，落 LLM classify
        // （LLM 能同时看到 web.fetch_file / media.download 的描述再定）。
        assert!(parse_fetch_file("下载微信").is_none());
        // 音视频/普通资源不接（归 media.download 的 ytsearch）。
        assert!(parse_fetch_file("下载晴天").is_none());
        // URL 形态不接（归 media.download）。
        assert!(parse_fetch_file("下载 https://x.com/a.exe").is_none());
        // 指代词 fail-closed。
        assert!(parse_fetch_file("下载这个软件").is_none());
    }

    #[test]
    fn download_without_url_rewrites_to_search_expression() {
        // 无链接：资源名 → yt-dlp 原生搜索表达式，自行找源再下载。
        let m = slots_to_map(&parse_download("下载晴天").unwrap());
        assert_eq!(m["url"], "ytsearch1:晴天");
        assert!(m.get("audio_only").is_none());
        // “一首”暗示歌曲 → audio_only。
        let m = slots_to_map(&parse_download("下载一首晴天").unwrap());
        assert_eq!(m["url"], "ytsearch1:晴天");
        assert_eq!(m["audio_only"], "true");
        // 提及 B站 → bilisearch 前缀。
        let m = slots_to_map(&parse_download("下载B站的原神动画").unwrap());
        assert_eq!(m["url"], "bilisearch1:原神动画");
        // 指代词/纯动词没有实体 → fail-closed 落 LLM。
        assert!(parse_download("下载这个").is_none());
        assert!(parse_download("下歌").is_none());
        assert!(parse_download("下载").is_none());
        // URL 形态不受影响。
        assert_eq!(
            slots_to_map(&parse_download("下载一首歌 https://x.com/a").unwrap())["url"],
            "https://x.com/a"
        );
    }

    #[test]
    fn download_does_not_hijack_organize_verb() {
        // “整理下载目录”的动词是整理（下载是目录名）：让给 files.organize。
        assert!(parse_download("整理下载目录").is_none());
        assert!(parse_download("归档下载目录").is_none());
        // 纯下载意图不受影响。
        assert!(parse_download("下载晴天").is_some());
    }

    #[test]
    fn clip_type_rejects_ambiguous_shapes() {
        // “在记事本打字”缺打字对象归属判定，fail-closed 落 LLM。
        assert!(parse_clip_type("帮我在记事本打字hello").is_none());
    }

    #[test]
    fn page_screenshot_bare_form_only() {
        // Phase A：零参数裸形态直接命中（Playwright 一等公民化）。
        assert!(parse_page_screenshot("网页截图").is_some());
        assert!(parse_page_screenshot("帮我页面截图一下").is_some());
        assert!(parse_page_screenshot("当前网页截图").is_some());
        // 带目标 URL/宾语的多步形状 fail-closed 落 LLM 拆步。
        assert!(parse_page_screenshot("网页截图 example.com").is_none());
        assert!(parse_page_screenshot("打开网页").is_none());
        assert!(parse_page_screenshot("截图保存到桌面").is_none());
    }

    #[test]
    fn clip_type_basic_shapes() {
        let m = slots_to_map(&parse_clip_type("输入hello world").unwrap());
        assert_eq!(m["text"], "hello world");
        assert!(parse_clip_type("输入").is_none());
    }

    #[test]
    fn press_keys_maps_chinese_names() {
        let m = slots_to_map(&parse_clip_press("按回车").unwrap());
        assert_eq!(m["keys"], "enter");
        let m = slots_to_map(&parse_clip_press("按ctrl+c").unwrap());
        assert_eq!(m["keys"], "ctrl+c");
        assert!(parse_clip_press("按").is_none());
        assert!(parse_clip_press("按win+r").is_none());
    }

    #[test]
    fn shell_takes_remainder() {
        let m = slots_to_map(&parse_shell("运行命令dir").unwrap());
        assert_eq!(m["command"], "dir");
        assert!(parse_shell("运行").is_none());
    }

    #[test]
    fn fs_read_list_delete_need_path_shape() {
        let m = slots_to_map(&parse_fs_read("读D:/a.txt文件").unwrap());
        assert_eq!(m["path"], "D:/a.txt");
        assert!(parse_fs_read("读一下这个文件").is_none());
        assert!(parse_fs_read("打开记事本").is_none());
        let m = slots_to_map(&parse_fs_list("列一下D:/work目录").unwrap());
        assert_eq!(m["path"], "D:/work");
        assert!(parse_fs_list("列一下这个目录").is_none());
        let m = slots_to_map(&parse_fs_delete("删除D:/a.txt").unwrap());
        assert_eq!(m["path"], "D:/a.txt");
        assert!(parse_fs_delete("删除这个文件").is_none());
    }

    #[test]
    fn fs_write_splits_content_and_path() {
        let m = slots_to_map(&parse_fs_write("把hello写入D:/a.txt").unwrap());
        assert_eq!(m["content"], "hello");
        assert_eq!(m["path"], "D:/a.txt");
        assert!(parse_fs_write("把这段文字写入文件").is_none());
    }

    #[test]
    fn fs_create_needs_explicit_path() {
        let m = slots_to_map(&parse_fs_create("新建文件D:/a.txt").unwrap());
        assert_eq!(m["path"], "D:/a.txt");
        assert!(parse_fs_create("新建一个word文档").is_none());
    }

    #[test]
    fn volume_playback_fixed_vocab() {
        let m = slots_to_map(&parse_volume("把声音调高").unwrap());
        assert_eq!(m["direction"], "up");
        assert_eq!(
            slots_to_map(&parse_volume("静音").unwrap())["direction"],
            "mute"
        );
        assert!(parse_volume("调高").is_none());
        assert!(parse_volume("取消静音").is_none());
        let m = slots_to_map(&parse_playback("下一首").unwrap());
        assert_eq!(m["action"], "next");
        assert_eq!(
            slots_to_map(&parse_playback("暂停").unwrap())["action"],
            "playpause"
        );
    }

    #[test]
    fn timer_parses_units() {
        let m = slots_to_map(&parse_timer("倒计时5分钟").unwrap());
        assert_eq!(m["seconds"], "300");
        assert_eq!(
            slots_to_map(&parse_timer("计时30秒").unwrap())["seconds"],
            "30"
        );
        assert!(parse_timer("倒计时0秒").is_none());
        assert!(parse_timer("提醒我").is_none());
    }

    #[test]
    fn alarm_and_reminder_split() {
        // 有标题尾巴 → reminder。
        let m = slots_to_map(&parse_reminder("下午3点提醒我开会").unwrap());
        assert_eq!(m["time"], "15:00");
        assert_eq!(m["title"], "开会");
        // 叫我类动词 / 裸时间 → alarm。
        let m = slots_to_map(&parse_alarm("早上7点叫我").unwrap());
        assert_eq!(m["time"], "07:00");
        assert!(parse_alarm("下午3点提醒我开会").is_none());
    }

    #[test]
    fn maps_and_diagnose_shapes() {
        let m = slots_to_map(&parse_maps("导航到天安门").unwrap());
        assert_eq!(m["query"], "天安门");
        let m = slots_to_map(&parse_maps("公司怎么走").unwrap());
        assert_eq!(m["query"], "公司");
        let m = slots_to_map(&parse_diagnose("为什么QQ打不开").unwrap());
        assert_eq!(m["app_name"], "QQ");
        assert!(parse_diagnose("去洗澡").is_none());
    }

    #[test]
    fn web_search_guards() {
        let m = slots_to_map(&parse_web_search("搜一下猫meme").unwrap());
        assert_eq!(m["query"], "猫meme");
        assert!(parse_web_search("搜索备忘录").is_none());
        assert!(parse_web_search("查找D:/a.txt").is_none());
    }

    #[test]
    fn weather_sports_wallpapers_shapes() {
        let m = slots_to_map(&parse_weather("北京天气怎么样").unwrap());
        assert_eq!(m["query"], "北京");
        let m = slots_to_map(&parse_sports("欧冠比分").unwrap());
        assert_eq!(m["query"], "欧冠");
        let m = slots_to_map(&parse_wallpapers("换壁纸").unwrap());
        assert_eq!(m["query"], "最新");
        let m = slots_to_map(&parse_wallpapers("换张猫壁纸").unwrap());
        assert_eq!(m["query"], "猫");
    }

    #[test]
    fn pim_note_shape() {
        let m = slots_to_map(&parse_pim_note("记住明天带伞").unwrap());
        assert_eq!(m["title"], "明天带伞");
        assert!(parse_pim_note("记住").is_none());
    }

    #[test]
    fn media_trim_convert_shapes() {
        let m = slots_to_map(&parse_media_trim("把D:/a.mp4从00:10剪到00:20").unwrap());
        assert_eq!(m["source"], "D:/a.mp4");
        assert_eq!(m["start"], "00:10");
        assert_eq!(m["end"], "00:20");
        let m = slots_to_map(&parse_media_convert("把D:/a.mp4转成mp3").unwrap());
        assert_eq!(m["source"], "D:/a.mp4");
        assert_eq!(m["format"], "mp3");
        let m = slots_to_map(&parse_doc_convert("把D:/a.docx转成pdf").unwrap());
        assert_eq!(m["source"], "D:/a.docx");
        assert_eq!(m["format"], "pdf");
    }

    #[test]
    fn doc_office_create_shape() {
        let m = slots_to_map(&parse_doc_office("新建word文档：会议纪要").unwrap());
        assert_eq!(m["operation"], "create");
        assert_eq!(m["app"], "word");
        assert_eq!(m["text"], "会议纪要");
        // “新建文件Xxx”归 fs.create，不归 office。
        assert!(parse_doc_office("新建文件D:/a.txt").is_none());
    }

    #[test]
    fn note_capture_needs_path() {
        let m = slots_to_map(&parse_note_capture("把hello记到Documents/a.txt").unwrap());
        assert_eq!(m["content"], "hello");
        assert_eq!(m["save_path"], "Documents/a.txt");
        assert!(parse_note_capture("记一条笔记").is_none());
    }
}
