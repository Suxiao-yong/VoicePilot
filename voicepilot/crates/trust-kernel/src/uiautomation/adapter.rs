//! `McpUiaAdapter` — `UiaAdapter` over the external `mcp-windows` MCP server.
//!
//! The kernel keeps the whole trust shell (whitelist → PerStep approval →
//! audit); this adapter only drives UI by spawning the `mcp-windows`
//! child process (`mcp_servers` row `mcp-windows`, seeded by
//! `insert_default_servers`). No COM, no in-process UIA, no thread affinity.
//!
//! Handle encoding: a window-only handle is the bare server string; an
//! element handle is JSON `{"w": window, "name"? / "id"? / "type"?}` so
//! click/type/read can re-specify selectors (the server is stateless per
//! call). Encoding details stay inside this module.
//!
//! Every method is one bounded MCP round trip
//! (`run_mcp_call_limited`: spawn + initialize + tools/call under
//! `McpCallLimits`). Response shapes below were captured live against
//! `Sbroenne.WindowsMcp.exe` 1.3.22 — see unit fixtures.

use super::{UiaAdapter, UiaElementHandle, UiaSelector};
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::mcp::client::McpCallLimits;
use crate::mcp::repo::{MCP_WINDOWS_SERVER_ID, McpServerRepo};
use std::path::{Path, PathBuf};

/// MCP tool names (sbroenne/mcp-windows).
const TOOL_APP: &str = "app";
const TOOL_WINDOW: &str = "window_management";
const TOOL_FIND: &str = "ui_find";
const TOOL_CLICK: &str = "ui_click";
const TOOL_TYPE: &str = "ui_type";
const TOOL_READ: &str = "ui_read";
const TOOL_SHOT: &str = "screenshot_control";

/// UIA adapter backed by the `mcp-windows` MCP server.
///
/// Resolved once from the `mcp_servers` table; each method spawns a fresh
/// child call. Plain data (`Send`), unlike the retired COM adapter.
#[derive(Debug)]
pub struct McpUiaAdapter {
    server_id: String,
    command: String,
    args: Vec<String>,
    env: serde_json::Value,
}

impl McpUiaAdapter {
    /// Resolve spawn config for `server_id` (missing / disabled / bad row →
    /// readable `Err`, fail closed before any child process runs). The
    /// `command` is additionally resolved to something spawnable (see
    /// `resolve_server_command`): a bare filename that resolves nowhere
    /// fails HERE with a setup hint instead of failing every tool call
    /// later with a bare OS error.
    pub fn for_server(kernel: &TrustKernel, server_id: &str) -> Result<Self> {
        let (command, args, env) = {
            let conn = kernel.conn();
            McpServerRepo::new().spawn_config(&conn, server_id)?
        };
        // Phase A 密钥收编：spawn 前解析 env 里的 keyring 引用（fail-closed）。
        let env = crate::skills::common::resolve_mcp_env(kernel, &env)?;
        let command = resolve_server_command(&command)?;
        Ok(Self {
            server_id: server_id.to_string(),
            command,
            args,
            env,
        })
    }

    /// Resolve the bundled backend (`mcp_servers` row `mcp-windows`).
    pub fn windows_default(kernel: &TrustKernel) -> Result<Self> {
        Self::for_server(kernel, MCP_WINDOWS_SERVER_ID).map_err(|e| match e {
            KernelError::Mcp(msg) => KernelError::Mcp(format!(
                "{msg} — UIA 后端缺失：下载 Sbroenne.WindowsMcp.exe 并在 Trust Center 登记/启用 `mcp-windows`"
            )),
            other => other,
        })
    }

    fn call(&self, tool: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        self.call_with(tool, args, McpCallLimits::default())
    }

    /// Same round trip under explicit limits. `find_window` fans out to up
    /// to 3 sequential calls (title → processName → processName+.exe), so it
    /// passes a shorter budget to bound the worst case.
    fn call_with(
        &self,
        tool: &str,
        args: serde_json::Value,
        limits: McpCallLimits,
    ) -> Result<serde_json::Value> {
        crate::skills::common::run_mcp_call_limited(
            &self.server_id,
            &self.command,
            &self.args,
            &self.env,
            tool,
            args,
            limits,
        )
    }
}

/// Resolve a server `command` to something spawnable.
///
/// - Absolute or path-like command: must exist, else a precise error
///   (points at the Trust Center row).
/// - Bare filename: probe order is (1) sibling of our own exe (shipped
///   layout: backend exe next to the app — zero user config), (2) PATH
///   lookup (bare + `.exe`, mirroring CreateProcess semantics).
///
/// Anything else fails HERE with a setup hint listing what was tried,
/// instead of failing every later tool call with a bare OS error.
/// (This exact failure — exe present on disk but on neither location —
/// is why "打不开任何软件": one manual PATH step, never done.)
fn resolve_server_command(command: &str) -> Result<String> {
    let extra: Vec<PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .into_iter()
        .collect();
    resolve_server_command_in(command, &extra)
}

fn resolve_server_command_in(command: &str, extra_dirs: &[PathBuf]) -> Result<String> {
    let p = Path::new(command);
    if p.is_absolute() || command.contains('/') || command.contains('\\') {
        if p.is_file() {
            return Ok(command.to_string());
        }
        return Err(KernelError::Mcp(format!(
            "UIA 后端可执行文件不存在: {command}（检查 Trust Center `mcp-windows` 行的 command）"
        )));
    }
    let mut tried = Vec::new();
    for dir in extra_dirs {
        let cand = dir.join(command);
        tried.push(cand.display().to_string());
        if cand.is_file() {
            return Ok(cand.to_string_lossy().into_owned());
        }
    }
    // PATH (bare + `.exe`, mirroring CreateProcess semantics).
    let mut names = vec![command.to_string()];
    if !command.to_lowercase().ends_with(".exe") {
        names.push(format!("{command}.exe"));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in &names {
                let full = dir.join(name);
                if full.is_file() {
                    return Ok(full.to_string_lossy().into_owned());
                }
            }
        }
    }
    tried.push("PATH".to_string());
    Err(KernelError::Mcp(format!(
        "找不到 UIA 后端 {command}（已尝试：{}）。请把 Sbroenne.WindowsMcp.exe 放到程序目录旁或加入 PATH，或在 Trust Center 把 `mcp-windows` 行的 command 改成完整路径",
        tried.join("、")
    )))
}

/// Short budget for the `find_window` fallback chain (3 sequential calls).
fn find_limits() -> McpCallLimits {
    McpCallLimits {
        timeout: std::time::Duration::from_secs(20),
        ..McpCallLimits::default()
    }
}

/// Resolve a launch target to something the backend `app` tool accepts.
///
/// Probe order: (1) absolute / path-like that exists → as-is; (2) PATH
/// lookup (bare + `.exe`, same as `resolve_server_command_in`); (3) Start
/// Menu shortcut — the backend launches a `.lnk` via shell semantics
/// (probed live: `QQ.lnk` → QQ up, then closed cleanly), so a `.lnk`
/// whose file stem matches the requested name (case-insensitive,
/// with/without `.exe`) is returned directly — no `.lnk` parsing, no new
/// dependency (`walkdir` is already one); (4) App Paths registry;
/// (5) Uninstall registry DisplayName (often Chinese, e.g. “飞书”) with
/// exe located via DisplayIcon; (6) UWP via Get-StartApps (`uwp:` marker).
/// This is what makes non-PATH installs (QQ/飞书 on E:) launchable with
/// zero user config.
/// Returns `None` when nothing matches: the caller passes the raw name
/// through and the backend reports the precise failure (behavior for
/// unknown apps is unchanged).
pub(crate) fn resolve_launch_target(app_name: &str) -> Option<String> {
    // 用户开始菜单优先于全机开始菜单：同名时以用户安装为准。
    let roots: Vec<PathBuf> = [
        std::env::var_os("APPDATA")
            .map(|v| PathBuf::from(v).join(r"Microsoft\Windows\Start Menu\Programs")),
        std::env::var_os("PROGRAMDATA")
            .map(|v| PathBuf::from(v).join(r"Microsoft\Windows\Start Menu\Programs")),
    ]
    .into_iter()
    .flatten()
    .collect();
    resolve_launch_target_in(app_name, &roots)
}

fn resolve_launch_target_in(app_name: &str, menu_roots: &[PathBuf]) -> Option<String> {
    let p = Path::new(app_name);
    if p.is_absolute() || app_name.contains('/') || app_name.contains('\\') {
        return p.is_file().then(|| app_name.to_string());
    }
    let mut names = vec![app_name.to_string()];
    if !app_name.to_lowercase().ends_with(".exe") {
        names.push(format!("{app_name}.exe"));
    }
    // PATH first: system apps resolve bit-for-bit like before.
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in &names {
                let full = dir.join(name);
                if full.is_file() {
                    return Some(full.to_string_lossy().into_owned());
                }
            }
        }
    }
    let want: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let mut walk_errs = 0u32;
    for root in menu_roots {
        let walker = walkdir::WalkDir::new(root).max_depth(4).follow_links(false);
        for entry in walker
            .into_iter()
            .filter_map(|e| e.map_err(|_| walk_errs += 1).ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
            {
                continue;
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if want
                .iter()
                .any(|w| *w == stem || w.strip_suffix(".exe").unwrap_or(w) == stem)
            {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    if walk_errs > 0 {
        tracing::debug!(app = %app_name, walk_errs, "start-menu scan skipped entries");
    }
    // App Paths 注册表（PATH 外的传统安装，如 E 盘 QQ）。
    if let Some(hit) = find_in_app_paths_registry(&want) {
        return Some(hit);
    }
    // 卸载表 DisplayName（中文名，如“飞书”）：App Paths 键多为英文，
    // 但卸载表 DisplayName 常是中文。用图标路径定位 exe，只读不执行。
    if let Some(hit) = find_in_uninstall_registry(&want) {
        return Some(hit);
    }
    // UWP（Get-StartApps）：返回 uwp: 标记，由 launch_app 经 explorer 拉起。
    if let Some(aumid) = find_uwp_app(&want) {
        return Some(format!("uwp:{aumid}"));
    }
    None
}

/// App Paths 注册表三源扫描（只读）。`want` 为小写候选名。
fn find_in_app_paths_registry(want: &[String]) -> Option<String> {
    for hive in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths",
        r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths",
    ] {
        let out = std::process::Command::new("reg")
            .args(["query", hive, "/s"])
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        if let Some(hit) = parse_app_paths(&String::from_utf8_lossy(&out.stdout), want) {
            return Some(hit);
        }
    }
    None
}

/// `reg query … /s` 输出解析：key 行取 exe 名，`(Default) REG_SZ` 行取值
///（存在的文件才算命中）。纯函数，可测。
fn parse_app_paths(text: &str, want: &[String]) -> Option<String> {
    fn stem_eq(name: &str, w: &str) -> bool {
        w == name
            || w.strip_suffix(".exe").unwrap_or(w) == name.strip_suffix(".exe").unwrap_or(name)
    }
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            let t = line.trim();
            if t.starts_with("(Default)") {
                if let Some(name) = current.as_deref() {
                    if want.iter().any(|w| stem_eq(name, w)) {
                        if let Some(p) = t.find("REG_SZ").map(|i| t[i + 6..].trim()) {
                            if !p.is_empty() && std::path::Path::new(p).is_file() {
                                return Some(p.to_string());
                            }
                        }
                    }
                }
            }
        } else if !line.trim().is_empty() {
            current = line.trim().rsplit('\\').next().map(|s| s.to_lowercase());
        }
    }
    None
}

/// 卸载表三源扫描（只读）：按 DisplayName 找中文名应用，用 DisplayIcon
/// 定位 exe。绝不执行 UninstallString。精确优先、包含兜底（与 UWP 同策略）。
fn find_in_uninstall_registry(want: &[String]) -> Option<String> {
    for hive in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ] {
        let out = std::process::Command::new("reg")
            .args(["query", hive, "/s"])
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        if let Some(hit) = parse_uninstall_display_name(&String::from_utf8_lossy(&out.stdout), want)
        {
            return Some(hit);
        }
    }
    None
}

/// 取 `NAME  TYPE  VALUE` 行的 VALUE（只认 REG_SZ / REG_EXPAND_SZ，
/// 其他类型返回空串，调用方按“无值”跳过）。
fn reg_value(rest: &str) -> String {
    let rest = rest.trim_start();
    for kind in ["REG_SZ", "REG_EXPAND_SZ"] {
        if let Some(v) = rest.strip_prefix(kind) {
            return v.trim().to_string();
        }
    }
    String::new()
}

/// DisplayName → exe：精确遍全表，再模糊遍全表（注册表顺序不可靠，
/// 不能单遍短路——精确命中可能排在模糊命中之后）。
/// 图标去 `,N` 后缀 + %VAR% 展开后文件必须存在。纯函数（除 is_file 外），可测。
fn parse_uninstall_display_name(text: &str, want: &[String]) -> Option<String> {
    // 先收集 (小写DisplayName, 图标原值)。
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut name = String::new();
    let mut icon = String::new();
    let mut flush = |name: &mut String, icon: &mut String| {
        if !name.is_empty() {
            entries.push((std::mem::take(name), std::mem::take(icon)));
        } else {
            name.clear();
            icon.clear();
        }
    };
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("DisplayName") {
                name = reg_value(rest).to_lowercase();
            } else if let Some(rest) = t.strip_prefix("DisplayIcon") {
                icon = reg_value(rest);
            }
        } else {
            // key 行或空行：结算上一项（无 DisplayName 的项直接丢弃）。
            flush(&mut name, &mut icon);
        }
    }
    flush(&mut name, &mut icon);
    // 精确：去 .exe 后缀再比（"飞书.exe" 的 query 也能命中 "飞书"）。
    for (n, i) in &entries {
        let exact = want.iter().any(|w| {
            let ws = w.strip_suffix(".exe").unwrap_or(w);
            let ns = n.strip_suffix(".exe").unwrap_or(n);
            *w == *n || ws == ns
        });
        if exact {
            if let Some(p) = clean_icon_path(i) {
                return Some(p);
            }
        }
    }
    // 模糊：双向包含，query 侧 ≥2 字节（与 UWP 同策略，防单字母误中）。
    for (n, i) in &entries {
        let fuzzy = want.iter().any(|w| {
            let ws = w.strip_suffix(".exe").unwrap_or(w);
            ws.len() >= 2 && (n.contains(ws) || ws.contains(&n[..]))
        });
        if fuzzy {
            if let Some(p) = clean_icon_path(i) {
                return Some(p);
            }
        }
    }
    None
}

/// DisplayIcon 清洗：去引号 → 去尾部 `,N` 图标索引 → %VAR% 展开 →
/// 存在才返回。纯函数（除 is_file 外）。
fn clean_icon_path(raw: &str) -> Option<String> {
    // 顺序关键：先剥尾部 `,N` 图标索引，再去引号。
    // `"C:\a\b.exe",3` 若先去引号会变成 `C:\a\b.exe",3`，
    // 尾部不再是 `.exe`，索引剥离失败（uninstall DisplayIcon 实测用例）。
    let mut s = raw.trim().to_string();
    // 尾部 `,N`（N 全数字）且其前是 .exe（允许前后带引号）→ 剥索引。
    // 先对“去引号版本”做判断，成功则在原串上切掉 `,N`。
    let unquoted = s.trim_matches('"');
    if let Some((head, tail)) = unquoted.rsplit_once(',') {
        // head 尾部可能带引号（如 `"C:\a\b.exe"`），判断 .exe 后缀时先去引号。
        if !tail.is_empty()
            && tail.chars().all(|c| c.is_ascii_digit())
            && head.trim_matches('"').to_lowercase().ends_with(".exe")
        {
            // 原串去掉尾部 `,N`（保留原引号，后面统一去）。
            let cut = s.len() - tail.len() - 1;
            s.truncate(cut);
        }
    }
    s = s.trim().trim_matches('"').trim().to_string();
    if s.contains('%') {
        s = expand_env_vars(&s);
    }
    if !s.is_empty() && std::path::Path::new(&s).is_file() {
        Some(s)
    } else {
        None
    }
}

/// `%NAME%` 展开（未知变量保留原文，由 is_file 兜底判否）。
fn expand_env_vars(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.find('%') {
            Some(j) => {
                let var = &tail[..j];
                out.push_str(&std::env::var(var).unwrap_or_else(|_| format!("%{var}%")));
                rest = &tail[j + 1..];
            }
            None => {
                out.push('%');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// UWP 应用枚举（`Get-StartApps`，只读）。精确优先、包含兜底。
fn find_uwp_app(want: &[String]) -> Option<String> {
    let mut child = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-StartApps | ForEach-Object { $_.Name + \"`t\" + $_.AppID }",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        match child.try_wait().ok()? {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
    let mut buf = Vec::new();
    use std::io::Read;
    child.stdout.take()?.read_to_end(&mut buf).ok()?;
    parse_start_apps(&String::from_utf8_lossy(&buf), want)
}

/// `Name\tAppID` 行解析。纯函数，可测。
fn parse_start_apps(text: &str, want: &[String]) -> Option<String> {
    let mut fuzzy: Option<String> = None;
    for line in text.lines() {
        let (name, appid) = match line.split_once('\t') {
            Some(pair) => pair,
            None => continue,
        };
        let nl = name.trim().to_lowercase();
        let id = appid.trim();
        if nl.is_empty() || id.is_empty() {
            continue;
        }
        if want
            .iter()
            .any(|w| *w == nl || w.strip_suffix(".exe").unwrap_or(w) == nl)
        {
            return Some(id.to_string());
        }
        // 模糊：双向包含，但 query 侧须 ≥2 字（防 "Q" 误命中首个含 q 的应用）。
        if fuzzy.is_none()
            && want.iter().any(|w| {
                let w = w.strip_suffix(".exe").unwrap_or(w);
                w.len() >= 2 && (nl.contains(w) || w.contains(&nl))
            })
        {
            fuzzy = Some(id.to_string());
        }
    }
    fuzzy
}

/// Pure request builder: the `find_window` fallback sequence
/// (title → catalog title hints → processName → processName+.exe).
/// Unit-tested without a server; `find_window` iterates it and
/// short-circuits on the first hit.
fn build_find_requests(title: &str, extra_titles: &[&str]) -> Vec<serde_json::Value> {
    let mut reqs = vec![serde_json::json!({"action": "find", "title": title})];
    for extra in extra_titles {
        reqs.push(serde_json::json!({"action": "find", "title": extra}));
    }
    reqs.push(serde_json::json!({"action": "find", "processName": title}));
    reqs.push(serde_json::json!({"action": "find", "processName": format!("{title}.exe")}));
    reqs
}

/// All window handles in a `window_management` list/find response.
fn window_handles(value: &serde_json::Value) -> Vec<String> {
    value
        .get("windows")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|w| w.get("handle"))
                .map(|h| match h {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Handles present in `after` but not in `before`: windows born between
/// two snapshots. Pure so the launch fallback is unit-testable.
fn new_handles(before: &[String], after: &serde_json::Value) -> Vec<String> {
    window_handles(after)
        .into_iter()
        .filter(|h| !before.iter().any(|b| b == h))
        .collect()
}

/// mcp-windows 在 launcher 进程退出后报多种措辞（真机实测两种）：
/// "Process has exited, so ..." / "Process 'x' exited unexpectedly with code 0"。
/// 统一按 "exited" 识别；名字不存在是另一种错误（not found 类），不会误触。
/// 误触成本也可接受：兜底超时后返回的仍是原始错误，只是多等几秒。
fn is_stub_exit_error(e: &KernelError) -> bool {
    matches!(e, KernelError::Mcp(msg) if msg.contains("exited"))
}

/// Pure request builder for `ui_find` (window + partial-match selectors).
fn build_find_args(window: &str, sel: &ElementSel) -> serde_json::Value {
    let mut args = serde_json::Value::Object(sel.params(false));
    args["windowHandle"] = serde_json::json!(window);
    args["timeoutMs"] = serde_json::json!(3000);
    args
}

/// Pure request builder for `ui_click` (window + exact-name selectors).
fn build_click_args(window: &str, sel: Option<&ElementSel>) -> serde_json::Value {
    let mut args = serde_json::Value::Object(sel.map(|s| s.params(true)).unwrap_or_default());
    args["windowHandle"] = serde_json::json!(window);
    args
}

/// Pure request builder for `window_management` close.
fn build_close_args(handle: &str, discard_changes: bool) -> serde_json::Value {
    serde_json::json!({
        "action": "close",
        "handle": handle,
        "discardChanges": discard_changes,
    })
}

/// Outcome of a `ui_find` response: found (with server-resolved selector),
/// not-found, or a real failure.
#[derive(Debug)]
enum FindOutcome {
    Found(ElementSel),
    NotFound,
}

/// Pure mapping of a `ui_find` response body (no server needed).
///
/// - `success: false` + not-found marker → `NotFound` (callers return None).
/// - `success: false` + anything else → `Err` (verbatim server text).
/// - `success: true` + non-empty `items` → `Found`, with the name replaced
///   by the server-returned exact `items[0].name` when the query selected
///   by name (find matched broadly, click must act exactly).
/// - `success: true` + empty/missing `items` → `NotFound`.
fn map_find_response(value: &serde_json::Value, query: &ElementSel) -> Result<FindOutcome> {
    if value.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let detail = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
            + value
                .get("errorCode")
                .and_then(|v| v.as_str())
                .unwrap_or("");
        if is_not_found_text(&detail) {
            return Ok(FindOutcome::NotFound);
        }
        let short: String = detail.chars().take(300).collect();
        return Err(KernelError::Uia(format!(
            "mcp-windows tool 'ui_find' failed: {short}"
        )));
    }
    let first_name = value
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let has_items = value
        .get("items")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if !has_items {
        return Ok(FindOutcome::NotFound);
    }
    let mut resolved = query.clone();
    if query.name.is_some() {
        if let Some(exact) = first_name {
            resolved.name = Some(exact);
        }
    }
    Ok(FindOutcome::Found(resolved))
}

/// Both not-found spellings observed from mcp-windows (`errorType` snake
/// vs `errorCode` Pascal). Single predicate so the `success:false` and
/// `isError` branches can never disagree on casing again.
fn is_not_found_text(s: &str) -> bool {
    s.contains("element_not_found") || s.contains("ElementNotFound")
}

/// Element selector attached to a window handle.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ElementSel {
    name: Option<String>,
    automation_id: Option<String>,
    control_type: Option<String>,
}

impl ElementSel {
    fn from_uia(selector: &UiaSelector) -> Self {
        match selector {
            // Native semantics were case-insensitive contains → nameContains.
            UiaSelector::ByName(s) => Self {
                name: Some(s.clone()),
                automation_id: None,
                control_type: None,
            },
            UiaSelector::ById(s) => Self {
                name: None,
                automation_id: Some(s.clone()),
                control_type: None,
            },
            UiaSelector::ByRole(s) => Self {
                name: None,
                automation_id: None,
                control_type: Some(s.clone()),
            },
        }
    }

    /// Selector params for ui_find / ui_click / ui_type / ui_read.
    /// `exact_name` only for ui_click-style exact matching; find/type/read
    /// use partial `nameContains` (native parity).
    fn params(&self, exact_name: bool) -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        match (&self.name, exact_name) {
            (Some(n), true) => {
                map.insert("name".to_string(), serde_json::json!(n));
            }
            (Some(n), false) => {
                map.insert("nameContains".to_string(), serde_json::json!(n));
            }
            _ => {}
        }
        if let Some(id) = &self.automation_id {
            map.insert("automationId".to_string(), serde_json::json!(id));
        }
        if let Some(t) = &self.control_type {
            map.insert("controlType".to_string(), serde_json::json!(t));
        }
        map
    }
}

/// Encode a handle: bare window string, or JSON when an element selector
/// is attached. Decoding inverts it (`decode_handle`).
fn encode_handle(window: &str, selector: Option<&ElementSel>) -> String {
    match selector {
        None => window.to_string(),
        Some(sel) => {
            let mut obj = serde_json::Map::new();
            obj.insert("w".to_string(), serde_json::json!(window));
            if let Some(n) = &sel.name {
                obj.insert("name".to_string(), serde_json::json!(n));
            }
            if let Some(id) = &sel.automation_id {
                obj.insert("id".to_string(), serde_json::json!(id));
            }
            if let Some(t) = &sel.control_type {
                obj.insert("type".to_string(), serde_json::json!(t));
            }
            serde_json::Value::Object(obj).to_string()
        }
    }
}

/// Split a handle into `(window, element selector)`. Bare strings are
/// window-only; anything parsing as `{"w": ...}` is an element reference.
fn decode_handle(handle: &str) -> (String, Option<ElementSel>) {
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(handle) {
        if let Some(w) = obj.get("w").and_then(|v| v.as_str()) {
            let sel = ElementSel {
                name: obj.get("name").and_then(|v| v.as_str()).map(str::to_string),
                automation_id: obj.get("id").and_then(|v| v.as_str()).map(str::to_string),
                control_type: obj.get("type").and_then(|v| v.as_str()).map(str::to_string),
            };
            return (w.to_string(), Some(sel));
        }
    }
    (handle.to_string(), None)
}

/// Require the server's `success: true`, else surface its `error` text.
/// `invoke_tool` already turns `isError` into `Err`, so reaching here with
/// `success: false` is the defensive second net.
fn require_success(tool: &str, value: &serde_json::Value) -> Result<()> {
    if value.get("success").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(());
    }
    let detail = value
        .get("error")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("message").and_then(|v| v.as_str()))
        .unwrap_or("unknown error");
    let short: String = detail.chars().take(300).collect();
    Err(KernelError::Uia(format!(
        "mcp-windows tool '{tool}' failed: {short}"
    )))
}

/// Extract `window.handle` (string or number) from an `app` / `activate`
/// style response.
fn window_handle(tool: &str, value: &serde_json::Value) -> Result<String> {
    let handle = value
        .get("window")
        .and_then(|w| w.get("handle"))
        .map(|h| match h {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    handle.ok_or_else(|| {
        KernelError::Uia(format!(
            "mcp-windows tool '{tool}' returned no window.handle: {}",
            truncate_value(value)
        ))
    })
}

/// First entry of a `window_management find` style `windows: [...]` list
/// as `(handle, title)`; title may be empty when the server omits it.
/// Pure — fixture-tested.
fn first_window_titled(value: &serde_json::Value) -> Option<(String, String)> {
    let first = value.get("windows")?.as_array()?.first()?;
    let handle = match first.get("handle")? {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let title = first
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((handle, title))
}

/// `ui_read` style `text` field.
fn result_text(tool: &str, value: &serde_json::Value) -> Result<String> {
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            KernelError::Uia(format!(
                "mcp-windows tool '{tool}' returned no text: {}",
                truncate_value(value)
            ))
        })
}

/// `screenshot_control` inline image (`image` / `data` / `png`, base64).
fn image_bytes(tool: &str, value: &serde_json::Value) -> Result<Vec<u8>> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let encoded = ["image", "data", "png"]
        .iter()
        .filter_map(|k| value.get(*k))
        .filter_map(|v| v.as_str())
        .next()
        .ok_or_else(|| {
            KernelError::Uia(format!(
                "mcp-windows tool '{tool}' returned no inline image (ask for includeImage): {}",
                truncate_value(value)
            ))
        })?;
    STANDARD.decode(encoded).map_err(|e| {
        KernelError::Uia(format!(
            "mcp-windows tool '{tool}' image is not base64: {e}"
        ))
    })
}

fn truncate_value(value: &serde_json::Value) -> String {
    let s = value.to_string();
    s.chars().take(300).collect()
}

fn mcp_handle_of(handle: &UiaElementHandle, method: &str) -> Result<String> {
    handle.mcp_handle().map(str::to_string).ok_or_else(|| {
        KernelError::Uia(format!(
            "{method}: handle wraps no reference (mock handle?)"
        ))
    })
}

impl McpUiaAdapter {
    /// Delegate-and-exit 式启动的兜底：不再等待 launcher 进程，直接按
    /// “启动前后窗口快照差集”找新生窗口。超时则返回最初的 fast-path 错误。
    fn launch_via_window_diff(
        &self,
        app_name: &str,
        before: Vec<String>,
    ) -> Result<UiaElementHandle> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        // 先轮询：fast 尝试报失败时应用往往已经起来了（如 calc），此时
        // 绝不再起第二个实例；2.5s 内无新生窗口才补发一次无等待启动。
        let refire_at = std::time::Instant::now() + std::time::Duration::from_millis(2500);
        let mut refired = false;
        while std::time::Instant::now() < deadline {
            if let Ok(list) = self.list_windows() {
                if let Some(handle) = new_handles(&before, &list).into_iter().next() {
                    return Ok(UiaElementHandle::from_mcp_handle(handle));
                }
            }
            if !refired && std::time::Instant::now() >= refire_at {
                // fire-and-forget：窗口出现才是信号，返回值忽略。
                let _ = self.call(
                    TOOL_APP,
                    serde_json::json!({"programPath": app_name, "waitForWindow": false}),
                );
                refired = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
        Err(KernelError::Uia(format!(
            "launch '{app_name}' produced no new window within 8s"
        )))
    }

    /// Shared find core: iterate the fallback sequence, return the first
    /// hit as `(handle, title)`. `find_window` drops the title,
    /// `find_window_titled` keeps it.
    fn find_first_titled(&self, title: &str) -> Result<Option<(String, String)>> {
        for args in build_find_requests(title, crate::skills::manifest::app_window_titles(title)) {
            let result = self.call_with(TOOL_WINDOW, args, find_limits())?;
            require_success(TOOL_WINDOW, &result)?;
            if let Some(pair) = first_window_titled(&result) {
                return Ok(Some(pair));
            }
        }
        Ok(None)
    }

    /// Full window list (for launch fallback diffing).
    fn list_windows(&self) -> Result<serde_json::Value> {
        let result = self.call(TOOL_WINDOW, serde_json::json!({"action": "list"}))?;
        require_success(TOOL_WINDOW, &result)?;
        Ok(result)
    }
}

impl UiaAdapter for McpUiaAdapter {
    fn launch_app(&self, app_name: &str) -> Result<UiaElementHandle> {
        // 先给快照：stub 退出式启动（Store 应用）即使报失败也可能已经把
        // 应用拉起来，快照是兜底 diff 的基线。list 若失败则退化为旧行为。
        let before: Option<Vec<String>> = self.list_windows().ok().map(|v| window_handles(&v));
        // 非 PATH 安装（如 QQ/飞书）：解析为可启动路径（绝对路径/PATH/
        // 开始菜单 .lnk）；解析失败则原样透传，后端报精确失败——未知
        // 应用的行为与之前完全一致。
        let target = resolve_launch_target(app_name).unwrap_or_else(|| app_name.to_string());
        tracing::debug!(app = %app_name, target = %target, "launch target resolved");
        // UWP：后端 app 工具不认 AUMID，自己用 explorer 拉起窗口，
        // 后续 diff 照走（refire 的 backend 调用失败会被忽略）。
        if let Some(aumid) = target.strip_prefix("uwp:") {
            std::process::Command::new("explorer.exe")
                .arg(format!("shell:AppsFolder\\{aumid}"))
                .spawn()
                .map_err(|e| KernelError::Uia(format!("launch UWP {aumid} 失败: {e}")))?;
            return self.launch_via_window_diff(&target, before.unwrap_or_default());
        }
        match self.call(
            TOOL_APP,
            serde_json::json!({"programPath": target, "waitForWindow": true}),
        ) {
            Ok(result) => {
                require_success(TOOL_APP, &result)?;
                let handle = window_handle(TOOL_APP, &result)?;
                Ok(UiaElementHandle::from_mcp_handle(handle))
            }
            Err(e) if is_stub_exit_error(&e) => self
                .launch_via_window_diff(&target, before.unwrap_or_default())
                .map_err(|_| e),
            Err(e) => Err(e),
        }
    }

    fn find_window(&self, title_contains: &str) -> Result<Option<UiaElementHandle>> {
        // 标题未命中（如中文系统“画图” vs app 名 mspaint）时，按进程名兜底：
        // 调用方传的本就是应用名，命中自家进程正是预期语义。
        // Request shape is pinned by `build_find_requests` unit tests.
        Ok(self
            .find_first_titled(title_contains)?
            .map(|(handle, _)| UiaElementHandle::from_mcp_handle(handle)))
    }

    fn find_window_titled(&self, query: &str) -> Result<Option<(UiaElementHandle, String)>> {
        Ok(self
            .find_first_titled(query)?
            .map(|(handle, title)| (UiaElementHandle::from_mcp_handle(handle), title)))
    }

    fn find_element(
        &self,
        root: &UiaElementHandle,
        selector: &UiaSelector,
    ) -> Result<Option<UiaElementHandle>> {
        let raw = mcp_handle_of(root, "find_element")?;
        let (window, _) = decode_handle(&raw);
        let sel = ElementSel::from_uia(selector);
        let args = build_find_args(&window, &sel);
        match self.call(TOOL_FIND, args) {
            Ok(result) => match map_find_response(&result, &sel)? {
                FindOutcome::Found(resolved) => Ok(Some(UiaElementHandle::from_mcp_handle(
                    encode_handle(&window, Some(&resolved)),
                ))),
                FindOutcome::NotFound => Ok(None),
            },
            Err(KernelError::Mcp(msg)) if is_not_found_text(&msg) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn click(&self, element: &UiaElementHandle) -> Result<()> {
        let raw = mcp_handle_of(element, "click")?;
        let (window, sel) = decode_handle(&raw);
        let args = build_click_args(&window, sel.as_ref());
        let result = self.call(TOOL_CLICK, args)?;
        require_success(TOOL_CLICK, &result)
    }

    fn set_text(&self, element: &UiaElementHandle, text: &str) -> Result<()> {
        let raw = mcp_handle_of(element, "set_text")?;
        let (window, sel) = decode_handle(&raw);
        let mut args = serde_json::Value::Object(sel.map(|s| s.params(false)).unwrap_or_default());
        args["windowHandle"] = serde_json::json!(window);
        args["text"] = serde_json::json!(text);
        // Native set_text replaced the value → clear first for parity.
        args["clearFirst"] = serde_json::json!(true);
        let result = self.call(TOOL_TYPE, args)?;
        require_success(TOOL_TYPE, &result)
    }

    fn get_text(&self, element: &UiaElementHandle) -> Result<String> {
        let raw = mcp_handle_of(element, "get_text")?;
        let (window, sel) = decode_handle(&raw);
        let mut args = serde_json::Value::Object(sel.map(|s| s.params(false)).unwrap_or_default());
        args["windowHandle"] = serde_json::json!(window);
        let result = self.call(TOOL_READ, args)?;
        require_success(TOOL_READ, &result)?;
        result_text(TOOL_READ, &result)
    }

    fn screenshot(&self, element: &UiaElementHandle) -> Result<Vec<u8>> {
        let raw = mcp_handle_of(element, "screenshot")?;
        let (window, _) = decode_handle(&raw);
        let result = self.call(
            TOOL_SHOT,
            serde_json::json!({
                "windowHandle": window,
                "includeImage": true,
                "imageFormat": "png",
                "outputMode": "inline",
            }),
        )?;
        require_success(TOOL_SHOT, &result)?;
        image_bytes(TOOL_SHOT, &result)
    }

    fn close_window(&self, window: &UiaElementHandle, discard_changes: bool) -> Result<()> {
        let raw = mcp_handle_of(window, "close_window")?;
        let (handle, _) = decode_handle(&raw);
        let result = self.call(TOOL_WINDOW, build_close_args(&handle, discard_changes))?;
        require_success(TOOL_WINDOW, &result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Live-captured fixtures (Sbroenne.WindowsMcp.exe 1.3.22, notepad).
    const APP_OK: &str = r#"{"success":true,"window":{"handle":"919760","title":"x - Notepad","className":"Notepad","processName":"Notepad","pid":5432},"message":"Launched."}"#;
    const FIND_OK: &str =
        r#"{"success":true,"windows":[{"handle":"919760","title":"x - Notepad"}],"count":1}"#;
    const FIND_EMPTY: &str = r#"{"success":true,"windows":[],"count":0}"#;
    const READ_OK: &str = r#"{"success":true,"action":"get_text","text":"hello"}"#;
    const SHOT_OK: &str = r#"{"success":true,"image":"aGk="}"#;
    const FAIL: &str = r#"{"success":false,"errorCode":"CloseFailed","error":"Window 1 did not close within 3000ms"}"#;

    #[test]
    fn handle_encode_decode_roundtrip() {
        let sel = ElementSel::from_uia(&UiaSelector::ByName("Close".to_string()));
        let encoded = encode_handle("123", Some(&sel));
        let (window, back) = decode_handle(&encoded);
        assert_eq!(window, "123");
        assert_eq!(back, Some(sel));
        // Bare strings stay window-only (server-issued handles pass through).
        assert_eq!(decode_handle("919760"), ("919760".to_string(), None));
    }

    #[test]
    fn selector_params_match_native_semantics() {
        // ByName was case-insensitive contains → nameContains (find/type/read);
        // click uses exact name.
        let sel = ElementSel::from_uia(&UiaSelector::ByName("Close".to_string()));
        assert_eq!(
            sel.params(false)
                .get("nameContains")
                .and_then(|v| v.as_str()),
            Some("Close")
        );
        assert_eq!(
            sel.params(true).get("name").and_then(|v| v.as_str()),
            Some("Close")
        );
        let id = ElementSel::from_uia(&UiaSelector::ById("x".to_string()));
        assert_eq!(
            id.params(false)
                .get("automationId")
                .and_then(|v| v.as_str()),
            Some("x")
        );
        let role = ElementSel::from_uia(&UiaSelector::ByRole("Edit".to_string()));
        assert_eq!(
            role.params(false)
                .get("controlType")
                .and_then(|v| v.as_str()),
            Some("Edit")
        );
    }

    #[test]
    fn response_parsers_match_live_shapes() {
        let app: serde_json::Value = serde_json::from_str(APP_OK).unwrap();
        require_success(TOOL_APP, &app).unwrap();
        assert_eq!(window_handle(TOOL_APP, &app).unwrap(), "919760");

        let find: serde_json::Value = serde_json::from_str(FIND_OK).unwrap();
        assert_eq!(
            first_window_titled(&find),
            Some(("919760".to_string(), "x - Notepad".to_string()))
        );
        let empty: serde_json::Value = serde_json::from_str(FIND_EMPTY).unwrap();
        assert_eq!(first_window_titled(&empty), None);

        let read: serde_json::Value = serde_json::from_str(READ_OK).unwrap();
        assert_eq!(result_text(TOOL_READ, &read).unwrap(), "hello");

        let shot: serde_json::Value = serde_json::from_str(SHOT_OK).unwrap();
        assert_eq!(image_bytes(TOOL_SHOT, &shot).unwrap(), b"hi");

        let fail: serde_json::Value = serde_json::from_str(FAIL).unwrap();
        let err = require_success(TOOL_WINDOW, &fail).unwrap_err();
        assert!(format!("{err:?}").contains("did not close"));
    }

    #[test]
    fn find_requests_fan_out_title_then_process() {
        let reqs = build_find_requests("mspaint", &[]);
        assert_eq!(reqs.len(), 3);
        assert_eq!(reqs[0]["title"].as_str(), Some("mspaint"));
        assert_eq!(reqs[1]["processName"].as_str(), Some("mspaint"));
        assert_eq!(reqs[2]["processName"].as_str(), Some("mspaint.exe"));
        for r in &reqs {
            assert_eq!(r["action"].as_str(), Some("find"));
        }
        // catalog 标题提示插在 query 标题之后、processName 之前
        // （Store 应用标题与名实不符，process 链够不到）。
        let reqs = build_find_requests("calc", &["计算器", "Calculator"]);
        assert_eq!(reqs.len(), 5);
        assert_eq!(reqs[1]["title"].as_str(), Some("计算器"));
        assert_eq!(reqs[2]["title"].as_str(), Some("Calculator"));
        assert_eq!(reqs[3]["processName"].as_str(), Some("calc"));
    }

    #[test]
    fn window_handles_and_new_handles_diff_snapshots() {
        let before: serde_json::Value = serde_json::from_str(
            r#"{"success":true,"windows":[{"handle":"1"},{"handle":"2"}],"count":2}"#,
        )
        .unwrap();
        let after: serde_json::Value = serde_json::from_str(
            r#"{"success":true,"windows":[{"handle":"1"},{"handle":"2"},{"handle":"9"}],"count":3}"#,
        )
        .unwrap();
        assert_eq!(
            window_handles(&before),
            vec!["1".to_string(), "2".to_string()]
        );
        assert_eq!(
            new_handles(&window_handles(&before), &after),
            vec!["9".to_string()]
        );
        assert!(new_handles(&window_handles(&after), &after).is_empty());
    }

    #[test]
    fn stub_exit_error_gate_only_matches_launcher_exit() {
        // 两种实测措辞都要接住（同一 stub 退出，server 端竞态导致文案不同）。
        assert!(is_stub_exit_error(&KernelError::Mcp(
            "tool 'app' returned isError: Process has exited".to_string()
        )));
        assert!(is_stub_exit_error(&KernelError::Mcp(
            "tool 'app' returned isError: Process 'calc' exited unexpectedly with code 0"
                .to_string()
        )));
        assert!(!is_stub_exit_error(&KernelError::Mcp(
            "tool 'app' returned isError: not found".to_string()
        )));
        assert!(!is_stub_exit_error(&KernelError::Uia(
            "Process has exited".to_string()
        )));
    }

    #[test]
    fn find_response_mapping_covers_not_found_variants() {
        let query = ElementSel::from_uia(&UiaSelector::ByName("Close".to_string()));
        // success:false + element_not_found (either key spelling) → NotFound.
        for body in [
            r#"{"success":false,"error":"No element found matching: element_not_found"}"#,
            r#"{"success":false,"errorCode":"ElementNotFound","error":"x"}"#,
            r#"{"success":true,"items":[]}"#,
        ] {
            let v: serde_json::Value = serde_json::from_str(body).unwrap();
            assert!(
                matches!(map_find_response(&v, &query), Ok(FindOutcome::NotFound)),
                "body: {body}"
            );
        }
        // success:false + other error → Err (verbatim server text).
        let other: serde_json::Value =
            serde_json::from_str(r#"{"success":false,"error":"boom"}"#).unwrap();
        let err = map_find_response(&other, &query).unwrap_err();
        assert!(format!("{err:?}").contains("boom"));
    }

    #[test]
    fn find_resolves_exact_name_for_click() {
        // Server broad-matches nameContains=Close but returns the exact
        // items[0].name; the encoded handle must carry the exact name so
        // click (exact `name=`) hits the same element find saw.
        let query = ElementSel::from_uia(&UiaSelector::ByName("Close".to_string()));
        let v: serde_json::Value = serde_json::from_str(
            r#"{"success":true,"items":[{"name":"Close tab","type":"Button"}]}"#,
        )
        .unwrap();
        let resolved = match map_find_response(&v, &query).unwrap() {
            FindOutcome::Found(sel) => sel,
            FindOutcome::NotFound => panic!("expected Found"),
        };
        assert_eq!(resolved.name.as_deref(), Some("Close tab"));
        let click_args = build_click_args("9", Some(&resolved));
        assert_eq!(click_args["name"].as_str(), Some("Close tab"));
        assert!(click_args.get("nameContains").is_none());
        assert_eq!(click_args["windowHandle"].as_str(), Some("9"));
    }

    #[test]
    fn first_window_titled_extracts_handle_and_title() {
        let v: serde_json::Value = serde_json::from_str(FIND_OK).unwrap();
        assert_eq!(
            first_window_titled(&v),
            Some(("919760".to_string(), "x - Notepad".to_string()))
        );
        let empty: serde_json::Value = serde_json::from_str(FIND_EMPTY).unwrap();
        assert_eq!(first_window_titled(&empty), None);
        // Missing title degrades to empty string, never to an error.
        let no_title: serde_json::Value =
            serde_json::from_str(r#"{"success":true,"windows":[{"handle":"7"}]}"#).unwrap();
        assert_eq!(
            first_window_titled(&no_title),
            Some(("7".to_string(), String::new()))
        );
    }

    #[test]
    fn close_args_carry_handle_and_discard_flag() {
        let args = build_close_args("919760", false);
        assert_eq!(args["action"].as_str(), Some("close"));
        assert_eq!(args["handle"].as_str(), Some("919760"));
        assert_eq!(args["discardChanges"].as_bool(), Some(false));
    }

    #[test]
    fn for_server_resolves_command_to_spawnable_path() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // Deterministic positive: point the row at a file that exists.
        let exe = std::env::current_exe().unwrap();
        let exe_str = exe.to_string_lossy().into_owned();
        {
            let repo = McpServerRepo::new();
            let conn = kernel.conn();
            let mut rec = repo.get(&conn, MCP_WINDOWS_SERVER_ID).unwrap().unwrap();
            rec.command = Some(exe_str.clone());
            repo.update(&conn, &rec).unwrap();
        }
        let adapter = McpUiaAdapter::for_server(&kernel, MCP_WINDOWS_SERVER_ID).unwrap();
        assert_eq!(adapter.command, exe_str);
    }

    #[test]
    fn for_server_seeded_bare_name_fails_fast_with_hint_when_exe_missing() {
        // Seeded command is the bare filename. If no backend is installed
        // anywhere discoverable, construction itself must say so (not every
        // later tool call with a bare OS error).
        let kernel = TrustKernel::open_in_memory().unwrap();
        match McpUiaAdapter::for_server(&kernel, MCP_WINDOWS_SERVER_ID) {
            Ok(_) => {} // backend installed on this machine — fine
            Err(e) => assert!(
                format!("{e:?}").contains("找不到 UIA 后端"),
                "unexpected error: {e:?}"
            ),
        }
    }

    #[test]
    fn resolve_server_command_absolute_paths() {
        let exe = std::env::current_exe().unwrap();
        let exe_str = exe.to_string_lossy().into_owned();
        assert_eq!(resolve_server_command(&exe_str).unwrap(), exe_str);
        let missing = exe.with_file_name("definitely-not-here-12345.exe");
        let err = resolve_server_command(missing.to_str().unwrap()).unwrap_err();
        assert!(format!("{err:?}").contains("可执行文件不存在"), "{err:?}");
    }

    #[test]
    fn resolve_server_command_searches_extra_dirs_then_path() {
        let dir = std::env::temp_dir().join(format!("vp-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dummy = dir.join("dummy-backend-xyz.exe");
        std::fs::write(&dummy, b"x").unwrap();
        let got = resolve_server_command_in("dummy-backend-xyz.exe", &[dir.clone()]).unwrap();
        assert_eq!(got, dummy.to_string_lossy());
        std::fs::remove_dir_all(&dir).unwrap();
        // 未命中：报错里列出尝试过的位置（含 PATH），且不读写进程环境。
        let err = resolve_server_command_in("dummy-backend-xyz.exe", &[dir]).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("找不到 UIA 后端"), "{msg}");
        assert!(msg.contains("PATH"), "{msg}");
    }

    #[test]
    fn resolve_server_command_finds_system_binary_on_path() {
        // cmd.exe 在 Windows 上必在 PATH —— 顺带锁定 PATH 查找本身可用。
        let got = resolve_server_command("cmd.exe").unwrap();
        assert!(got.to_lowercase().ends_with("cmd.exe"), "{got}");
    }

    #[test]
    fn launch_target_resolves_start_menu_shortcut() {
        // 真实结构复刻：子目录 + 中英文混排（腾讯软件\QQ\QQ.lnk、飞书.lnk）。
        // 只匹配文件名，不读 .lnk 内容，所以空文件固件足够。
        let root = std::env::temp_dir().join(format!("vp-lnkscan-{}", std::process::id()));
        let qq_dir = root.join("腾讯软件").join("QQ");
        std::fs::create_dir_all(&qq_dir).unwrap();
        std::fs::write(qq_dir.join("QQ.lnk"), b"").unwrap();
        std::fs::write(root.join("飞书.lnk"), b"").unwrap();
        std::fs::write(root.join("readme.txt"), b"x").unwrap();
        let roots = vec![root.clone()];
        let got = resolve_launch_target_in("QQ", &roots).unwrap();
        assert!(got.ends_with("QQ.lnk"), "{got}");
        // 大小写 + `.exe` 后缀变体同样命中同一快捷方式。
        let got2 = resolve_launch_target_in("qq.exe", &roots).unwrap();
        assert!(got2.ends_with("QQ.lnk"), "{got2}");
        // 中文名直命快捷方式。
        let got3 = resolve_launch_target_in("飞书", &roots).unwrap();
        assert!(got3.ends_with("飞书.lnk"), "{got3}");
        // 非 .lnk 不参与、未知名返回 None（调用方原样透传）。
        // 注意：resolve_launch_target_in 会先扫 PATH,再扫 menu_roots,最后查
        // 注册表 / UWP。所以 None 断言只能用"任何机器上都不存在"的虚构名 ——
        // 原来这里用 "readme"（作者以为虚构，但 CI runner 上真能解析到东西），
        // 已换成 vp- 前缀的虚构名。
        assert!(resolve_launch_target_in("vp-no-such-app-xyz.txt", &roots).is_none());
        assert!(resolve_launch_target_in("vp-no-such-app-xyz", &roots).is_none());
        assert!(resolve_launch_target_in("vp-no-such-app-xyz", &[]).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn app_paths_parser_extracts_existing_exe() {
        // parse_app_paths 只返回磁盘上真实存在的路径,所以固件路径必须真的存在,
        // 否则测试只能在"作者本机恰好装了这个软件"的机器上通过(原实现硬编码
        // E:\TenXun\QQ\QQ.exe,在干净 CI runner 上必然失败)。这里用 tempdir 自建。
        let probe_dir = std::env::temp_dir().join(format!("vp-apppaths-{}", std::process::id()));
        std::fs::create_dir_all(&probe_dir).unwrap();
        let probe_exe = probe_dir.join("probeqq.exe");
        std::fs::write(&probe_exe, b"mz").unwrap();
        let probe_str = probe_exe.to_string_lossy().to_string();

        // 注册表输出是"reg query"的原样文本:反斜杠是路径本身的单反斜杠
        // (Rust 源串里的 \\\\ 只是写一个 \)。tempdir 路径直接拼进去即可。
        let out = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\probeqq.exe\n    (Default)    REG_SZ    {probe_str}\n\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\other.exe\n    (Default)    REG_SZ    C:\\no\\such\\other.exe\n"
        );
        // 真实存在的 exe 被解析出来;不存在的 C 盘假路径跳过。
        let want = vec!["probeqq".to_string(), "probeqq.exe".to_string()];
        assert_eq!(parse_app_paths(&out, &want).as_deref(), Some(&*probe_str));
        let want2 = vec!["other".to_string(), "other.exe".to_string()];
        assert_eq!(parse_app_paths(&out, &want2), None);
        let want3 = vec!["vp-no-such-app-xyz".to_string()];
        assert_eq!(parse_app_paths(&out, &want3), None);
        // 空 REG_SZ 值不算命中。
        let empty_out = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\probeqq.exe\n    (Default)    REG_SZ    \n"
        );
        assert_eq!(parse_app_paths(&empty_out, &want), None);
        std::fs::remove_dir_all(&probe_dir).unwrap();
    }

    #[test]
    fn uninstall_display_name_resolves_chinese_name() {
        // 飞书式布局：DisplayName 中文 + DisplayIcon 带 ,0 后缀。
        // 图标指向真实临时文件（is_file 门槛），C 盘假路径跳过。
        let dir = std::env::temp_dir().join(format!("vp-uninstall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Feishu.exe");
        std::fs::write(&exe, b"x").unwrap();
        let path = exe.to_string_lossy().into_owned();
        let out = format!(
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{{068D2D8F}}\n    DisplayName    REG_SZ    飞书\n    DisplayIcon    REG_SZ    {path},0\n    DisplayVersion    REG_SZ    5.0\n\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Other\n    DisplayName    REG_SZ    Other App\n    DisplayIcon    REG_SZ    C:\\no\\such\\other.exe,0\n"
        );
        let want = vec!["飞书".to_string(), "飞书.exe".to_string()];
        assert_eq!(
            parse_uninstall_display_name(&out, &want).as_deref(),
            Some(path.as_str())
        );
        // 版本号后缀（"飞书 5.12"）走模糊分支同样命中。
        let out2 = out.replacen("飞书\n", "飞书 5.12\n", 1);
        assert_eq!(
            parse_uninstall_display_name(&out2, &want).as_deref(),
            Some(path.as_str())
        );
        // 引号 + 非零索引后缀同样清洗。
        let out3 = out.replacen(&format!("{path},0"), &format!("\"{path}\",3"), 1);
        assert_eq!(
            parse_uninstall_display_name(&out3, &want).as_deref(),
            Some(path.as_str())
        );
        // 无图标 / 图标不存在 → 跳过，不误报。
        let out4 = "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\X\n    DisplayName    REG_SZ    飞书\n";
        assert_eq!(parse_uninstall_display_name(out4, &want), None);
        let want5 = vec!["vp-no-such-app-xyz".to_string()];
        assert_eq!(parse_uninstall_display_name(&out, &want5), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn expand_env_vars_keeps_unknown() {
        assert_eq!(expand_env_vars("%nosuchvar_xyz%/a"), "%nosuchvar_xyz%/a");
        assert_eq!(expand_env_vars("plain"), "plain");
    }

    #[test]
    fn start_apps_parser_exact_beats_fuzzy() {
        let out = "Calculator\tMicrosoft.WindowsCalculator_8wekyb3d8bbwe!App\nQQ\tTencent.QQ_abc!App\nQQMusic\tTencent.QQMusic_xyz!App\n";
        let want = vec!["qq".to_string(), "qq.exe".to_string()];
        assert_eq!(
            parse_start_apps(out, &want).as_deref(),
            Some("Tencent.QQ_abc!App")
        );
        let want2 = vec!["calc".to_string()];
        // 无精确命中时包含兜底生效（calc → Calculator，期望行为）。
        assert_eq!(
            parse_start_apps(out, &want2).as_deref(),
            Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App")
        );
        let want3 = vec!["vp-no-such-app-xyz".to_string()];
        assert_eq!(parse_start_apps(out, &want3), None);
    }

    #[test]
    fn launch_target_prefers_absolute_path() {
        let dir = std::env::temp_dir().join(format!("vp-lnkabs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app.exe");
        std::fs::write(&exe, b"x").unwrap();
        let s = exe.to_string_lossy().into_owned();
        assert_eq!(
            resolve_launch_target_in(&s, &[]).as_deref(),
            Some(s.as_str())
        );
        let missing = dir.join("nope.exe").to_string_lossy().into_owned();
        assert!(resolve_launch_target_in(&missing, &[]).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn windows_default_missing_row_points_to_setup() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        McpServerRepo::new()
            .delete(&kernel.conn(), MCP_WINDOWS_SERVER_ID)
            .unwrap();
        let err = McpUiaAdapter::windows_default(&kernel).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("not found in mcp_servers table"), "{msg}");
        assert!(msg.contains("UIA 后端缺失"), "{msg}");
    }

    #[test]
    fn spawn_config_rejects_bad_rows_verbatim() {
        // Pinned wordings shared with invoke_mcp_tool / dispatcher arm.
        let kernel = TrustKernel::open_in_memory().unwrap();
        let repo = McpServerRepo::new();
        let conn = kernel.conn();
        let base = repo.get(&conn, MCP_WINDOWS_SERVER_ID).unwrap().unwrap();
        let mut rec = base.clone();
        rec.server_id = "t-nocmd".to_string();
        rec.command = None;
        repo.create(&conn, &rec).unwrap();
        let err = repo.spawn_config(&conn, "t-nocmd").unwrap_err();
        assert!(format!("{err:?}").contains("missing command field"));
        rec.server_id = "t-badargs".to_string();
        rec.command = Some("exe".to_string());
        rec.args = Some("[bad".to_string());
        repo.create(&conn, &rec).unwrap();
        let err = repo.spawn_config(&conn, "t-badargs").unwrap_err();
        assert!(format!("{err:?}").contains("args parse error"));
        rec.server_id = "t-badenv".to_string();
        rec.args = Some("[]".to_string());
        rec.env = Some("{bad".to_string());
        repo.create(&conn, &rec).unwrap();
        let err = repo.spawn_config(&conn, "t-badenv").unwrap_err();
        assert!(format!("{err:?}").contains("env parse error"));
    }

    #[test]
    fn for_server_rejects_missing_and_disabled() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        // Seeded row exists (insert_default_servers at boot) but the exe is
        // not on PATH in CI → resolution succeeds, spawn would fail later.
        // Missing id fails here with the pinned wording.
        let err = McpUiaAdapter::for_server(&kernel, "no-such-server").unwrap_err();
        assert!(format!("{err:?}").contains("not found in mcp_servers table"));
        // Disabled row fails closed.
        McpServerRepo::new()
            .toggle_enabled(&kernel.conn(), MCP_WINDOWS_SERVER_ID, false)
            .unwrap();
        let err = McpUiaAdapter::for_server(&kernel, MCP_WINDOWS_SERVER_ID).unwrap_err();
        assert!(format!("{err:?}").contains("is disabled"));
    }
}
