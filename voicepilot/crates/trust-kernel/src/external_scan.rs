//! External discovery: scan well-known global locations for Agent Skills
//! (`SKILL.md`) and MCP server configs — read-only, nothing is enabled.
//!
//! Background: VoicePilot only knew its own `%APPDATA%\voicepilot\skills`
//! dir plus hand-pasted MCP JSON. Skills/MCPs the user already installed
//! for Claude Code, Cursor or VS Code were invisible. This module finds
//! them; *enabling* still goes through the existing explicit paths
//! (skill toggle, MCP register), which default to off for imports.
//!
//! Security contract:
//! - Scan never writes, never spawns, never touches the network.
//! - Missing/unreadable/oversize/malformed inputs are warn-skipped.
//! - Every function takes explicit paths (testable with TempDir); the
//!   `default_windows_*` wrappers only resolve well-known locations.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::skills::manifest::SkillExecutionSpec;
use crate::skills::user_loader::scan_user_skill_files;

/// Max config file size: 1 MiB (same bound as skill files).
const MAX_CONFIG_FILE_BYTES: u64 = 1024 * 1024;
/// Max MCP entries honored per config file (bounds pathological files).
const MAX_MCP_ENTRIES_PER_FILE: usize = 128;

/// A third-party skill found outside our own skills dir. Enabling happens
/// only via explicit import + toggle (default off).
///
/// `description` + binding are included so the UI can show WHAT would be
/// imported and enabled — importing blind is what makes attacker routing
/// text + trusted-server piggybacking a one-click mistake.
///
/// Cross-source dedup (same skill id installed for both Claude Code and
/// Codex): identical SKILL.md content collapses into ONE hit whose
/// `sources` lists every origin; different content stays separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSkillHit {
    /// First (display) source, e.g. `"claude-code"`. Kept alongside
    /// `sources` for backward compatibility with existing callers.
    pub source: String,
    /// Every source this (id, content) was found at, e.g.
    /// `["claude-code", "codex"]`. Single-element list when unique.
    pub sources: Vec<String>,
    /// Directory containing `SKILL.md` (absolute, as discovered).
    pub dir: PathBuf,
    /// Manifest id (== directory name per the standard layout).
    pub id: String,
    pub title: String,
    pub description: String,
    /// True when the manifest binds an MCP tool (mirror of the installed
    /// table's 可执行 pill, computed the same way).
    pub executable: bool,
    /// Present iff `executable`: which server/tool it would call once enabled.
    pub exec_server: Option<String>,
    pub exec_tool: Option<String>,
}

/// MCP config file flavors we can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpConfigFormat {
    /// `{"mcpServers": {id: {command, args?, env?}}}` — Claude Desktop.
    ClaudeDesktop,
    /// Same shape as Claude Desktop — Cursor (`~/.cursor/mcp.json`).
    Cursor,
    /// `{"servers": {id: {"type": "stdio", "command", "args"?, "env"?}}}`
    /// — VS Code. Only `type: "stdio"` entries are honored.
    VsCode,
    /// `~/.codex/config.toml` — minimal TOML subset: only the
    /// `[mcp_servers.<name>]` tables with `command` / `args` / `env` are
    /// honored; everything else is ignored. Hand-parsed (no toml crate —
    /// the dependency table deliberately carries none).
    CodexToml,
}

impl McpConfigFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            McpConfigFormat::ClaudeDesktop => "claude-desktop",
            McpConfigFormat::Cursor => "cursor",
            McpConfigFormat::VsCode => "vscode",
            McpConfigFormat::CodexToml => "codex-toml",
        }
    }
}

/// An MCP server entry found in someone else's config file. Importing
/// writes a row with `enabled = false`; the user enables it in Trust Center.
///
/// Cross-file dedup: the same server name in two configs with identical
/// spawn spec (command + args + env keys) collapses into one hit whose
/// `sources` lists every file; different spawn specs stay separate.
///
/// Only key NAMES of `env` cross into the DTO (`env_keys`): values are
/// third-party secrets (API tokens) the scan table never renders, so they
/// must not sit in renderer memory. Import re-reads values kernel-side
/// from the referenced file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpHit {
    /// First source file (used by the import path to re-read values).
    pub source_file: PathBuf,
    /// Every `"<file> (<format>)"` origin this entry was found at.
    pub sources: Vec<String>,
    pub format: String,
    pub server_id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env_keys: Vec<String>,
}

/// One server entry the scanner refused, with a human-readable reason.
/// Surfaced in the UI next to hits so skipped entries (e.g. non-stdio
/// VS Code types) are visible instead of silently vanishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSkippedEntry {
    pub server_id: String,
    pub reason: String,
}

/// Full result of scanning MCP config files: hits plus skip accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpScanReport {
    pub hits: Vec<ExternalMcpHit>,
    pub skipped: Vec<McpSkippedEntry>,
}

/// Scan explicit skill roots (`{root}/{name}/SKILL.md`, standard layout).
/// Missing roots are silently skipped (normal on fresh machines).
///
/// Cross-source dedup: the same skill id often lives in two roots
/// (`~/.claude/skills` and `~/.codex/skills` install the same 50 skills).
/// Identical SKILL.md content (sha256) collapses into one hit with a
/// merged `sources` list; different content keeps both hits (R1 finding 4:
/// only same-name-and-same-content dedupes).
pub fn scan_external_skill_roots(roots: &[(PathBuf, &'static str)]) -> Vec<ExternalSkillHit> {
    let mut out: Vec<ExternalSkillHit> = Vec::new();
    // id → content hashes already seen (dedup key: same id AND same bytes).
    let mut seen: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (root, source) in roots {
        if !root.is_dir() {
            continue;
        }
        for file in scan_user_skill_files(root) {
            let (executable, exec_server, exec_tool) = match &file.manifest.execution {
                Some(SkillExecutionSpec::McpTool {
                    server_id,
                    tool_name,
                }) => (true, Some(server_id.clone()), Some(tool_name.clone())),
                _ => (false, None, None),
            };
            let dir = file
                .path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| root.clone());
            // Dedupe by (id, SKILL.md content hash). Same content → merge
            // sources into the existing hit; different content → keep both.
            let content_hash = skill_content_hash(&dir);
            let hashes = seen.entry(file.manifest.id.clone()).or_default();
            if hashes.contains(&content_hash) {
                if let Some(existing) = out.iter_mut().find(|e| {
                    e.id == file.manifest.id && skill_content_hash(&e.dir) == content_hash
                }) {
                    if !existing.sources.contains(&source.to_string()) {
                        existing.sources.push(source.to_string());
                        existing.source = existing.sources[0].clone();
                    }
                }
                continue;
            }
            hashes.push(content_hash);
            out.push(ExternalSkillHit {
                source: source.to_string(),
                sources: vec![source.to_string()],
                dir,
                id: file.manifest.id.clone(),
                title: file.manifest.title.clone(),
                description: file.manifest.description.clone(),
                executable,
                exec_server,
                exec_tool,
            });
        }
    }
    out.sort_by(|a, b| a.source.cmp(&b.source).then(a.id.cmp(&b.id)));
    out
}

/// sha256 of `{dir}/SKILL.md`, used for cross-source dedup. Unreadable →
/// unique sentinel (never merges two hits by accident).
fn skill_content_hash(dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    match std::fs::read(dir.join("SKILL.md")) {
        Ok(bytes) => format!("sha256:{:x}", Sha256::digest(&bytes)),
        Err(_) => format!("missing:{}", uuid::Uuid::new_v4()),
    }
}

/// Well-known global skill locations (Windows). Existence is NOT required;
/// the scanner skips missing roots.
pub fn default_windows_skill_roots() -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push((home.join(".claude").join("skills"), "claude-code"));
        out.push((home.join(".agents").join("skills"), "agents-spec"));
        // Codex CLI also uses the Agent Skills standard layout.
        out.push((home.join(".codex").join("skills"), "codex"));
    } else {
        tracing::warn!("dirs::home_dir() returned None; skipping global skill scan");
    }
    out
}

/// Shape dispatch shared by scan + import-time re-read: which JSON object
/// holds the server map for a given config flavor.
fn server_map(
    value: &serde_json::Value,
    format: McpConfigFormat,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let entries = match format {
        McpConfigFormat::ClaudeDesktop | McpConfigFormat::Cursor => value.get("mcpServers"),
        McpConfigFormat::VsCode => value.get("servers"),
        // The codex parser re-shapes its output into the same "mcpServers"
        // map shape, so the existing entry parsing path is reused as-is.
        McpConfigFormat::CodexToml => value.get("mcpServers"),
    };
    entries.and_then(|v| v.as_object())
}

// ===== Codex config.toml 最小子集解析（不引 toml crate，见计划 R4-4）=====
//
// 只认 `[mcp_servers.<name>]` 表 + `command` / `args` / `env` 三个键；
// 其余键（url、type 等）一律忽略。输出 re-shape 成 `{"mcpServers": {...}}`
// JSON 结构，复用既有 entry 解析 / 校验路径。解析失败返回 None（调用方
// 按"损坏文件 warn-skip"处理，含跳过报告）。
//
// 支持面（按真实 Codex config.toml 的形状裁剪）：
// - `#` 注释、空行
// - `key = "value"`（含 \\ \" 转义）
// - `args = ["a", "b"]` 单行数组；数组跨行时逐行续读直到 `]`
// - `env = { KEY = "v", K2 = "v2" }` 单行 inline table
// - 表头 `[mcp_servers.name]` 与 `[mcp_servers."name"]`（去引号）
pub(crate) fn parse_codex_toml_mcp_servers(content: &str) -> Option<serde_json::Value> {
    let mut servers = serde_json::Map::new();
    let mut current: Option<(String, serde_json::Map<String, serde_json::Value>)> = None;
    let mut in_multiline_array = false;
    let mut pending_array = String::new();
    let mut pending_key: Option<String> = None;

    for raw_line in content.lines() {
        // Naive comment strip BEFORE any other handling (also applies to
        // multi-line array continuation lines): '#' inside quoted strings is
        // rare in command/args/env shapes; acceptable for this minimal
        // subset (a '#' inside a value truncates that one value, the entry
        // then fails validation and lands in the skip report — fail-closed,
        // never wrong-spawn).
        let stripped = match raw_line.find('#') {
            Some(idx) => raw_line[..idx].trim(),
            None => raw_line.trim(),
        };
        let line = if in_multiline_array {
            if stripped.is_empty() {
                continue;
            }
            pending_array.push(' ');
            pending_array.push_str(stripped);
            if !stripped.contains(']') {
                continue;
            }
            let done = pending_array.clone();
            in_multiline_array = false;
            pending_array.clear();
            // 完成：直接用暂存的 key 解析数组，绕过表头处理（done 以 '['
            // 开头，若继续走常规流程会被误判成新表头）。
            let table = current.as_mut().map(|(_, t)| t)?;
            let key = pending_key.take()?;
            let parsed = parse_toml_string_array(&done)?;
            table.insert(
                key,
                serde_json::Value::Array(
                    parsed.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
            continue;
        } else {
            stripped.to_string()
        };
        if line.is_empty() {
            continue;
        }
        if line.starts_with("[") {
            // flush previous table
            if let Some((name, table)) = current.take() {
                servers.insert(name, serde_json::Value::Object(table));
            }
            let inner = line.trim_start_matches('[').trim_end_matches(']').trim();
            if let Some(name) = inner.strip_prefix("mcp_servers.") {
                let name = name.trim().trim_matches('"').to_string();
                // 嵌套子表（[mcp_servers.x.env] 等，合法 TOML 但本子集不支持）：
                // 忽略该表头，不新建假服务器；父表内容继续累积。
                if name.is_empty() || name.contains('.') {
                    continue;
                }
                current = Some((name, serde_json::Map::new()));
            }
            continue;
        }
        let Some((_, table)) = current.as_mut() else {
            continue; // key outside [mcp_servers.*] — ignored
        };
        let Some((key, value_raw)) = line.split_once('=') else {
            return None; // malformed line — reject the whole file
        };
        let key = key.trim().trim_matches('"').to_string();
        let value_raw = value_raw.trim();
        if key != "command" && key != "args" && key != "env" {
            continue; // unknown keys ignored (fail-closed scope: subset only)
        }
        // Multi-line array: buffer until the closing bracket arrives.
        if value_raw.starts_with('[') && !value_raw.contains(']') {
            in_multiline_array = true;
            pending_array = value_raw.to_string();
            pending_key = Some(key);
            continue;
        }
        let parsed = if key == "command" {
            let s = parse_toml_string(value_raw)?;
            serde_json::Value::String(s)
        } else if key == "args" {
            serde_json::Value::Array(
                parse_toml_string_array(value_raw)?
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            )
        } else {
            serde_json::Value::Object(parse_toml_inline_table(value_raw)?)
        };
        table.insert(key, parsed);
    }
    // EOF 时未闭合的多行数组 = 文件被截断/损坏：整个文件按损坏处理，
    // 走既有 warn-skip 路径，绝不静默丢弃暂存 key 产出错误 spawn spec。
    if in_multiline_array {
        return None;
    }
    if let Some((name, table)) = current.take() {
        servers.insert(name, serde_json::Value::Object(table));
    }
    if servers.is_empty() {
        return None;
    }
    Some(serde_json::json!({ "mcpServers": serde_json::Value::Object(servers) }))
}

/// Parse one TOML basic string (`"..."`, with \\ \" escapes) or literal
/// string (`'...'`, raw — Codex config.toml 用单引号包 Windows 路径，实证）。
fn parse_toml_string(raw: &str) -> Option<String> {
    let s = raw.trim();
    if let Some(unquoted) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(unescape_toml(unquoted));
    }
    if let Some(unquoted) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return Some(unquoted.to_string());
    }
    None
}

/// Parse `["a", "b"]` (single line, may be empty `[]`).
fn parse_toml_string_array(raw: &str) -> Option<Vec<String>> {
    let inner = raw.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    split_toml_items(inner)
        .iter()
        .map(|item| parse_toml_string(item))
        .collect()
}

/// Parse `{ KEY = "v", K2 = "v2" }` (single line inline table, may be `{}`).
fn parse_toml_inline_table(raw: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let inner = raw.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let mut out = serde_json::Map::new();
    if inner.is_empty() {
        return Some(out);
    }
    for item in split_toml_items(inner) {
        let (k, v) = item.split_once('=')?;
        let key = k.trim().trim_matches('"').to_string();
        let value = parse_toml_string(v)?;
        out.insert(key, serde_json::Value::String(value));
    }
    Some(out)
}

/// Split top-level comma-separated items (no nesting inside this subset's
/// string-only items; commas inside quoted strings are preserved because we
/// track quote state instead of blindly splitting).
fn split_toml_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => {
                current.push(ch);
                escaped = true;
            }
            '"' => {
                in_string = !in_string;
                current.push(ch);
            }
            ',' if !in_string => {
                items.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }
    items
}

/// Minimal escape handling: \\ \" \n \t. Unknown escapes keep the backslash
/// (never panic, never drop data silently).
fn unescape_toml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Format one scan source for the UI list: `"<path> (<format>)"`.
fn format_mcp_source(path: &Path, format: McpConfigFormat) -> String {
    format!("{} ({})", path.display(), format.as_str())
}

/// Full entry with secret values, for kernel-side import re-read ONLY.
/// Never serialized to the renderer (see `ExternalMcpHit.env_keys`).
#[derive(Debug, Clone)]
pub struct ExternalMcpEntryFull {
    pub source_file: PathBuf,
    pub format: McpConfigFormat,
    pub server_id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// Re-read one server entry from disk for import. The renderer only ever
/// passes back `(source_file, format, server_id)` it was shown, so a
/// compromised webview cannot substitute command/env — values always come
/// from the file. Returns None when the entry vanished or no longer parses.
///
/// Note: a writer racing the file between scan-display and import-click
/// could still swap content; that race is inherent to trusting the user's
/// own files (a post-trust swap is equivalent and out of scope).
/// What this closes is renderer-side substitution without filesystem access.
pub fn read_external_mcp_entry(
    source_file: &Path,
    format: McpConfigFormat,
    server_id: &str,
) -> Option<ExternalMcpEntryFull> {
    let content = read_text_capped(source_file, MAX_CONFIG_FILE_BYTES).ok()?;
    let value: serde_json::Value = match format {
        McpConfigFormat::CodexToml => parse_codex_toml_mcp_servers(&content)?,
        _ => serde_json::from_str(&content).ok()?,
    };
    let map = server_map(&value, format)?;
    if map.len() > MAX_MCP_ENTRIES_PER_FILE {
        tracing::warn!(path = ?source_file, "MCP config exceeds entry cap on re-read");
        return None;
    }
    let entry = map.get(server_id)?;
    let (id, parsed) = parse_mcp_entry(server_id, entry, format).ok()?;
    Some(ExternalMcpEntryFull {
        source_file: source_file.to_path_buf(),
        format,
        server_id: id,
        name: parsed.name,
        command: parsed.command,
        args: parsed.args,
        env: parsed.env,
    })
}

/// Parse one MCP config file into hits + skip accounting. Malformed entries
/// are warn-skipped individually with reasons; a bad file yields zero hits,
/// never an error.
pub fn scan_external_mcp_files(files: &[(PathBuf, McpConfigFormat)]) -> McpScanReport {
    let mut hits: Vec<ExternalMcpHit> = Vec::new();
    let mut skipped = Vec::new();
    for (path, format) in files {
        let content = match read_capped(path) {
            Some(c) => c,
            None => continue,
        };
        let value: serde_json::Value = match format {
            McpConfigFormat::CodexToml => match parse_codex_toml_mcp_servers(&content) {
                Some(v) => v,
                None => {
                    tracing::warn!(path = ?path, "skipping unparsable codex config.toml");
                    continue;
                }
            },
            _ => match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(path = ?path, error = ?e, "skipping unparsable MCP config");
                    continue;
                }
            },
        };
        let map = match server_map(&value, *format) {
            Some(m) => m,
            None => {
                tracing::warn!(path = ?path, "MCP config has no server map; skipping file");
                continue;
            }
        };
        for (server_id, entry) in map.iter().take(MAX_MCP_ENTRIES_PER_FILE) {
            match parse_mcp_entry(server_id, entry, *format) {
                Ok((id, parsed)) => {
                    let hit = ExternalMcpHit {
                        source_file: path.clone(),
                        sources: vec![format_mcp_source(path, *format)],
                        format: format.as_str().to_string(),
                        server_id: id,
                        name: parsed.name,
                        command: parsed.command,
                        args: parsed.args,
                        env_keys: parsed.env.keys().cloned().collect(),
                    };
                    // Cross-file dedup (R1 finding 4): same server id with an
                    // identical spawn spec collapses into one candidate; a
                    // different spec (e.g. diverging args) keeps both hits so
                    // the user can choose which to import.
                    let fingerprint = (hit.command.clone(), hit.args.join("\u{1}"), {
                        let mut k = hit.env_keys.clone();
                        k.sort();
                        k
                    });
                    if let Some(existing) = hits
                        .iter_mut()
                        .find(|e| e.server_id == hit.server_id)
                        .filter(|e| {
                            e.command == fingerprint.0 && e.args.join("\u{1}") == fingerprint.1 && {
                                let mut k = e.env_keys.clone();
                                k.sort();
                                k == fingerprint.2
                            }
                        })
                    {
                        if !existing.sources.contains(&hit.sources[0]) {
                            existing.sources.push(hit.sources[0].clone());
                        }
                    } else {
                        hits.push(hit);
                    }
                }
                Err(reason) => skipped.push(McpSkippedEntry {
                    server_id: server_id.to_string(),
                    reason,
                }),
            }
        }
    }
    hits.sort_by(|a, b| {
        a.server_id
            .cmp(&b.server_id)
            .then(a.source_file.cmp(&b.source_file))
    });
    skipped.sort_by(|a, b| a.server_id.cmp(&b.server_id).then(a.reason.cmp(&b.reason)));
    McpScanReport { hits, skipped }
}

// Parsed-then-reassembled entry (keeps parse_* helpers total).
struct ParsedMcpEntry {
    name: String,
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
}

/// Validate + normalize one server entry. Returns the reject reason instead
/// of silent None so the scan report can show WHY an entry was skipped.
fn parse_mcp_entry(
    server_id: &str,
    entry: &serde_json::Value,
    format: McpConfigFormat,
) -> Result<(String, ParsedMcpEntry), String> {
    if server_id.trim().is_empty() {
        tracing::warn!("skipping MCP entry with blank server_id");
        return Err("blank server_id".to_string());
    }
    if format == McpConfigFormat::VsCode {
        match entry.get("type").and_then(|v| v.as_str()) {
            Some("stdio") => {}
            other => {
                tracing::warn!(
                    server_id = server_id,
                    server_type = ?other,
                    "skipping non-stdio VS Code MCP entry"
                );
                return Err(format!(
                    "unsupported transport (stdio only): {}",
                    other.unwrap_or("<missing>")
                ));
            }
        }
    }
    let command = match entry.get("command").and_then(|v| v.as_str()) {
        Some(c) if !c.trim().is_empty() => c,
        _ => {
            tracing::warn!(
                server_id = server_id,
                "skipping MCP entry with blank command"
            );
            return Err("blank command".to_string());
        }
    };
    let args = match entry.get("args") {
        None => Vec::new(),
        Some(v) => match v.as_array() {
            Some(arr) => arr
                .iter()
                .map(|a| a.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| "args must be an array of strings".to_string())?,
            None => return Err("args must be an array of strings".to_string()),
        },
    };
    let mut env = BTreeMap::new();
    if let Some(obj) = entry.get("env").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            match v.as_str() {
                Some(s) => {
                    env.insert(k.clone(), s.to_string());
                }
                None => return Err(format!("env value for key '{k}' must be a string")),
            }
        }
    }
    let name = entry
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(server_id)
        .to_string();
    Ok((
        server_id.to_string(),
        ParsedMcpEntry {
            name,
            command: command.to_string(),
            args,
            env,
        },
    ))
}

/// 封顶读文本：常规文件且 ≤ max_bytes 才返回内容，否则返回原因串。
/// 用 `File::take` 封顶，不存在“stat 后无界读”窗口（TOCTOU 只能导致
/// 报错，不能绕过上限）。调用方映射到各自错误类型。
pub(crate) fn read_text_capped(path: &Path, max_bytes: u64) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open failed: {e}"))?;
    let is_file = file.metadata().map(|m| m.is_file()).unwrap_or(false);
    if !is_file {
        return Err("not a regular file".to_string());
    }
    let mut content = String::new();
    std::io::Read::read_to_string(&mut file.take(max_bytes + 1), &mut content)
        .map_err(|e| format!("read failed: {e}"))?;
    if content.len() as u64 > max_bytes {
        return Err(format!("file exceeds cap of {max_bytes} bytes",));
    }
    Ok(content)
}

/// Read a file capped at 1 MiB. Missing/unreadable/oversize → None (warn).
fn read_capped(path: &Path) -> Option<String> {
    match read_text_capped(path, MAX_CONFIG_FILE_BYTES) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(path = ?path, error = ?e, "skipping unreadable MCP config");
            None
        }
    }
}

/// Well-known global MCP config files (Windows). Existence is NOT required.
pub fn default_windows_mcp_files() -> Vec<(PathBuf, McpConfigFormat)> {
    let mut out = Vec::new();
    if let Some(data) = dirs::data_dir() {
        out.push((
            data.join("Claude").join("claude_desktop_config.json"),
            McpConfigFormat::ClaudeDesktop,
        ));
        out.push((
            data.join("Code").join("User").join("mcp.json"),
            McpConfigFormat::VsCode,
        ));
    }
    if let Some(home) = dirs::home_dir() {
        out.push((
            home.join(".cursor").join("mcp.json"),
            McpConfigFormat::Cursor,
        ));
        // Phase A (2026-09): Claude Code's settings.json carries an
        // `mcpServers` map with the same shape as Claude Desktop's config
        // (R4 finding 6) — reuse that parser with a new path.
        out.push((
            home.join(".claude").join("settings.json"),
            McpConfigFormat::ClaudeDesktop,
        ));
        // Codex CLI stores MCP servers in a TOML config (minimal subset
        // parser above; unknown keys and tables are ignored).
        out.push((
            home.join(".codex").join("config.toml"),
            McpConfigFormat::CodexToml,
        ));
    }
    if out.is_empty() {
        tracing::warn!("no base dirs resolved; skipping global MCP scan");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str, body: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    const GOOD_SKILL: &str = "---\nname: ext-demo\ndescription: External demo skill for tests.\n---\n\n# Demo\n\nBody.\n";

    #[test]
    fn scan_skills_finds_valid_and_skips_broken_and_missing_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude").join("skills");
        write_skill(&claude, "ext-demo", GOOD_SKILL);
        write_skill(
            &claude,
            "broken",
            "---\nname: Bad-Name\ndescription: d\n---\nbody\n",
        );
        std::fs::create_dir_all(claude.join("empty-dir")).unwrap();
        let missing = tmp.path().join("does-not-exist");
        let hits = scan_external_skill_roots(&[(claude, "claude-code"), (missing, "agents-spec")]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "ext-demo");
        assert_eq!(hits[0].source, "claude-code");
        assert!(hits[0].dir.ends_with("ext-demo"));
        assert_eq!(hits[0].title, "ext-demo");
        assert!(!hits[0].description.is_empty());
        assert!(!hits[0].executable);
        assert_eq!(hits[0].exec_server, None);
    }

    #[test]
    fn scan_mcp_parses_all_three_formats_and_skips_bad_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_cfg = tmp.path().join("claude.json");
        std::fs::write(
            &claude_cfg,
            r#"{"mcpServers":{
                "good": {"command": "npx", "args": ["-y", "x"], "env": {"A": "b"}},
                "no-command": {"args": []},
                "": {"command": "y"}
            }}"#,
        )
        .unwrap();
        let cursor_cfg = tmp.path().join("cursor-mcp.json");
        std::fs::write(&cursor_cfg, r#"{"mcpServers":{"cur": {"command": "uvx"}}}"#).unwrap();
        let vscode_cfg = tmp.path().join("vscode-mcp.json");
        std::fs::write(
            &vscode_cfg,
            r#"{"servers":{
                "ok": {"type": "stdio", "command": "node", "args": ["s.js"], "env": {}},
                "http": {"type": "http", "url": "http://x"},
                "bad-args": {"type": "stdio", "command": "node", "args": "nope"}
            }}"#,
        )
        .unwrap();
        let missing = tmp.path().join("nope.json");
        let report = scan_external_mcp_files(&[
            (claude_cfg.clone(), McpConfigFormat::ClaudeDesktop),
            (cursor_cfg, McpConfigFormat::Cursor),
            (vscode_cfg, McpConfigFormat::VsCode),
            (missing, McpConfigFormat::ClaudeDesktop),
        ]);
        let hits = &report.hits;
        let ids: Vec<&str> = hits.iter().map(|h| h.server_id.as_str()).collect();
        let mut right = ["good", "cur", "ok"];
        right.sort();
        assert_eq!(ids, right, "sorted by server_id (dedup order)");
        let good = hits.iter().find(|h| h.server_id == "good").unwrap();
        assert_eq!(good.command, "npx");
        assert_eq!(good.args, vec!["-y".to_string(), "x".to_string()]);
        // Secrets stay out of hits: only key names cross.
        assert_eq!(good.env_keys, vec!["A".to_string()]);
        assert_eq!(good.format, "claude-desktop");
        assert_eq!(good.source_file, claude_cfg);
        let ok = hits.iter().find(|h| h.server_id == "ok").unwrap();
        assert_eq!(ok.format, "vscode");
        // Skipped entries carry reasons instead of vanishing.
        let skipped_ids: Vec<&str> = report
            .skipped
            .iter()
            .map(|s| s.server_id.as_str())
            .collect();
        for expected in ["no-command", "http", "bad-args"] {
            assert!(
                skipped_ids.contains(&expected),
                "{expected} must be reported skipped, got {skipped_ids:?}"
            );
        }
        assert!(
            report.skipped.iter().any(|s| s.reason.contains("stdio")),
            "non-stdio skip must say why"
        );
    }

    #[test]
    fn scan_mcp_rejects_garbage_and_oversize_files() {
        let tmp = tempfile::tempdir().unwrap();
        let garbage = tmp.path().join("garbage.json");
        std::fs::write(&garbage, "{not json").unwrap();
        let empty_obj = tmp.path().join("empty.json");
        std::fs::write(&empty_obj, "{}").unwrap();
        // Oversize: header says JSON but body exceeds the cap.
        let big = tmp.path().join("big.json");
        let mut content = String::from("{\"mcpServers\":{\"big\":{\"command\":\"x\",\"args\":[\"");
        while content.len() <= MAX_CONFIG_FILE_BYTES as usize + 16 {
            content.push('y');
        }
        content.push_str("\"]}}");
        std::fs::write(&big, content).unwrap();
        let report = scan_external_mcp_files(&[
            (garbage, McpConfigFormat::ClaudeDesktop),
            (empty_obj, McpConfigFormat::Cursor),
            (big, McpConfigFormat::VsCode),
        ]);
        assert!(report.hits.is_empty());
    }

    #[test]
    fn scan_skills_dedupes_same_content_across_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = tmp.path().join(".claude").join("skills");
        let codex = tmp.path().join(".codex").join("skills");
        let agents = tmp.path().join(".agents").join("skills");
        // Same skill in claude + codex, identical content → one hit.
        write_skill(&claude, "ext-demo", GOOD_SKILL);
        write_skill(&codex, "ext-demo", GOOD_SKILL);
        // Same id but different content → separate hit (R1 finding 4).
        write_skill(
            &agents,
            "ext-demo",
            &GOOD_SKILL.replace("for tests", "variant body"),
        );
        let hits = scan_external_skill_roots(&[
            (claude, "claude-code"),
            (codex, "codex"),
            (agents, "agents-spec"),
        ]);
        assert_eq!(hits.len(), 2, "identical content merges, variant stays");
        let merged = hits.iter().find(|h| h.sources.len() == 2).unwrap();
        assert!(merged.sources.contains(&"claude-code".to_string()));
        assert!(merged.sources.contains(&"codex".to_string()));
        assert_eq!(merged.source, "claude-code");
        let variant = hits.iter().find(|h| h.sources.len() == 1).unwrap();
        assert_eq!(variant.sources[0], "agents-spec");
        assert!(variant.description.contains("variant body"));
    }

    #[test]
    fn scan_skills_codex_root_picked_up() {
        let tmp = tempfile::tempdir().unwrap();
        let codex = tmp.path().join(".codex").join("skills");
        write_skill(&codex, "ext-demo", GOOD_SKILL);
        let hits = scan_external_skill_roots(&[(codex, "codex")]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "codex");
        assert_eq!(hits[0].sources, vec!["codex".to_string()]);
    }

    #[test]
    fn scan_mcp_dedupes_same_server_same_spec_across_files() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.json");
        let b = tmp.path().join("b.json");
        std::fs::write(
            &a,
            r#"{"mcpServers":{"ctx7":{"command":"npx","args":["-y","m"],"env":{"API_KEY":"x"}}}}"#,
        )
        .unwrap();
        std::fs::write(
            &b,
            r#"{"mcpServers":{"ctx7":{"command":"npx","args":["-y","m"],"env":{"API_KEY":"y"}}}}"#,
        )
        .unwrap();
        // Different args → separate hit (same spec merges even when secret
        // values differ: only key names + spawn spec are compared).
        let c = tmp.path().join("c.json");
        std::fs::write(
            &c,
            r#"{"mcpServers":{"ctx7":{"command":"npx","args":["-y","other"]}}}"#,
        )
        .unwrap();
        let report = scan_external_mcp_files(&[
            (a.clone(), McpConfigFormat::ClaudeDesktop),
            (b, McpConfigFormat::ClaudeDesktop),
            (c, McpConfigFormat::ClaudeDesktop),
        ]);
        assert_eq!(report.hits.len(), 2);
        let merged = report.hits.iter().find(|h| h.sources.len() == 2).unwrap();
        assert_eq!(merged.server_id, "ctx7");
        assert_eq!(merged.source_file, a);
    }

    #[test]
    fn parse_codex_toml_minimal_subset() {
        let toml = r#"
# top comment
model = "gpt-5"            # unknown top-level key → ignored

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
env = { "CONTEXT7_API_KEY" = "kcy_123" }

[mcp_servers.brave]
command = "npx -y @brave/mcp"
args = [
  "a",
  "b",
]                            # multi-line array

[other_table]
x = 1
"#;
        let value = parse_codex_toml_mcp_servers(toml).unwrap();
        let servers = value["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 2);
        let ctx7 = &servers["context7"];
        assert_eq!(ctx7["command"], "npx");
        assert_eq!(
            ctx7["args"],
            serde_json::json!(["-y", "@upstash/context7-mcp"])
        );
        assert_eq!(ctx7["env"]["CONTEXT7_API_KEY"], "kcy_123");
        let brave = &servers["brave"];
        // multi-line array parsed (with a trailing comment after `]`).
        assert_eq!(brave["command"], "npx -y @brave/mcp");
        assert_eq!(brave["args"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn parse_codex_toml_literal_strings_and_bare_parent_table() {
        // 真机 config.toml 形状：单引号 literal string（Windows 路径）、
        // 裸 [mcp_servers] 父表头、type/startup_timeout_ms 等未知键。
        let toml = r#"
[mcp_servers]

[mcp_servers.filesystem]
type = "stdio"
command = "cmd"
args = ["/c", "npx", "@modelcontextprotocol/server-filesystem", 'C:\Users\x\Documents']
startup_timeout_ms = 120000
"#;
        let value = parse_codex_toml_mcp_servers(toml).unwrap();
        let fs = &value["mcpServers"]["filesystem"];
        assert_eq!(fs["command"], "cmd");
        assert_eq!(
            fs["args"],
            serde_json::json!([
                "/c",
                "npx",
                "@modelcontextprotocol/server-filesystem",
                "C:\\Users\\x\\Documents"
            ])
        );
    }

    #[test]
    fn parse_codex_toml_unterminated_array_is_corrupt() {
        // EOF 时未闭合多行数组：整文件按损坏处理（返回 None），
        // 不静默产出缺 args 的错误 spawn spec。
        let toml = "[mcp_servers.x]\ncommand = \"npx\"\nargs = [\"a\",\n";
        assert!(parse_codex_toml_mcp_servers(toml).is_none());
    }

    #[test]
    fn parse_codex_toml_rejects_nested_subtable_head() {
        // [mcp_servers.x.env] 是合法 TOML 但本子集不支持：忽略该表头，
        // 不建名为 "x.env" 的假服务器；父表的 env 因未知键忽略保持空。
        let toml = "[mcp_servers.x]\ncommand = \"npx\"\n\n[mcp_servers.x.env]\nKEY = \"v\"\n";
        let value = parse_codex_toml_mcp_servers(toml).unwrap();
        let servers = value["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1, "only parent x, no x.env pseudo-server");
        assert!(servers.contains_key("x"));
        assert_eq!(servers["x"]["command"], "npx");
        assert!(
            servers["x"].get("env").is_none(),
            "nested-table keys must not leak into parent env"
        );
    }

    #[test]
    fn parse_codex_toml_garbage_returns_none() {
        assert!(parse_codex_toml_mcp_servers("not [valid toml @@@").is_none());
        assert!(parse_codex_toml_mcp_servers("").is_none());
        // No mcp_servers tables → None (nothing to import).
        assert!(parse_codex_toml_mcp_servers("model = \"gpt\"").is_none());
    }

    #[test]
    fn scan_mcp_reads_codex_toml_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path().join("config.toml");
        std::fs::write(
            &cfg,
            "[mcp_servers.filesystem]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-filesystem\"]\n",
        )
        .unwrap();
        let report = scan_external_mcp_files(&[(cfg.clone(), McpConfigFormat::CodexToml)]);
        assert_eq!(report.hits.len(), 1);
        let hit = &report.hits[0];
        assert_eq!(hit.server_id, "filesystem");
        assert_eq!(hit.format, "codex-toml");
        assert!(hit.sources[0].contains("codex-toml"));
        // Re-read path (import) works for toml format too.
        let entry =
            read_external_mcp_entry(&cfg, McpConfigFormat::CodexToml, "filesystem").unwrap();
        assert_eq!(entry.command, "npx");
        assert_eq!(
            entry.args,
            vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string()
            ]
        );
    }

    #[test]
    #[ignore]
    fn manual_real_machine_scan_dump() {
        // 手测用（cargo test -- --ignored --nocapture manual_real）：只打
        // server_id / env key 名 / 来源，绝不打 env 值。
        let files = default_windows_mcp_files();
        let report = scan_external_mcp_files(&files);
        for h in &report.hits {
            println!(
                "HIT {} cmd={} args={:?} env_keys={:?} sources={:?}",
                h.server_id, h.command, h.args, h.env_keys, h.sources
            );
        }
        for s in &report.skipped {
            println!("SKIP {} : {}", s.server_id, s.reason);
        }
        let roots = default_windows_skill_roots();
        for h in scan_external_skill_roots(&roots) {
            println!("SKILL {} sources={:?}", h.id, h.sources);
        }
    }

    #[test]
    fn default_windows_locations_resolve_without_panic() {
        // Existence varies per machine; the contract is only: no panic,
        // and scanning whatever resolves never errors.
        let roots = default_windows_skill_roots();
        assert!(!roots.is_empty());
        let _ = scan_external_skill_roots(&roots);
        let files = default_windows_mcp_files();
        assert!(!files.is_empty());
        let _ = scan_external_mcp_files(&files);
    }
}
