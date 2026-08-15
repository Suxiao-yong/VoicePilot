//! Egress policy — V1.1 §4.3.
//! Controls data flow from local sources to remote destinations.
//! Independent from E×D matrix; checked separately per §4.4 step 6.

use crate::error::Result;
use crate::policy::types::{DLevel, Effect, EgressDest};
use rusqlite::{params, Connection};

/// Check whether data of `data_class` may flow to `dest`.
///
/// V1.1 §4.3 table:
///   local_file → remote_llm:  D0/D1 allow, D2 confirm, D3 deny
///   local_file → remote_mcp:  D0/D1 allow, D2 confirm, D3 deny
///   web_page → local_file:    allow (caller tags taint=external_untrusted)
///   web_page → remote_llm:    allow (read-only context)
///   web_page → tool_argument: deny (taint cannot elevate to instruction)
///
/// W2 simplification: `data_class` is the local data's classification.
/// `web_page` provenance is handled by checking `EgressDest::ToolArgument`
/// — any data flowing into a tool argument is denied because taint may
/// not elevate to an instruction. Full provenance-aware egress lands in W7
/// when Taint Tracking is implemented.
pub fn check_egress(data_class: DLevel, dest: EgressDest) -> Effect {
    use DLevel::*;
    use EgressDest::*;
    use Effect::*;
    match (data_class, dest) {
        // Tool arguments are never allowed to carry data (taint elevation).
        (_, ToolArgument) => Deny,
        // Local file destination: always allow (it's a local write, E×D covers it).
        (_, LocalFile) => Allow,
        // Remote LLM / MCP
        (D0 | D1, RemoteLlm | RemoteMcp) => Allow,
        (D2, RemoteLlm | RemoteMcp) => Confirm,
        (D3, RemoteLlm | RemoteMcp) => Deny,
    }
}

// ===== W11 Plan 5: egress 记录 + D3 脱敏(spec §9.4 ⑤)=====

/// D3 敏感内容脱敏(spec §9.4 ⑤)。
///
/// 仅对 D3(凭据)执行脱敏:把 `key=value` / `key: value` 形式的凭据值替换为
/// `<REDACTED>`。D0-D2 原样返回(路径 / 文档不脱敏)。脱敏是外发前的最后一道闸,
/// 配合 D3 → RemoteLlm/RemoteMcp Deny 的硬拒,保证真实凭据不落入 LLM 上下文。
///
/// 评测用 fake 占位(如 "PASSWORD_PLACEHOLDER")避免评测集本身泄露真实凭据。
pub fn redact_sensitive_content(content: &str, data_class: DLevel) -> String {
    if data_class != DLevel::D3 {
        return content.to_string();
    }
    let re = regex::Regex::new(
        r#"(?i)(password|passwd|pwd|token|api[_-]?key|secret|credential|authorization|auth)[[:space:]]*[=:][[:space:]]*[^\s,;]+"#,
    )
    .expect("redact regex must be valid");
    re.replace_all(content, "$1=<REDACTED>").to_string()
}

/// 记录一条 egress 外发记录(`egress_log` 表,V1.1 §4.3 + §9.4 ⑤)。
///
/// `approved=true` 表示已确认的外发(用户确认或确认流程通过);`false` 表示
/// 未确认 / 被拦截的外发尝试。返回新 `egress_id`。
///
/// `egress_log.task_id` 是 `REFERENCES tasks(task_id)` FK,调用方必须提供
/// 已存在的 task(测试先 `kernel.create_task`)。
pub fn record_egress(
    conn: &Connection,
    task_id: &str,
    data_class: DLevel,
    source: &str,
    destination: &str,
    approved: bool,
    bytes: i64,
) -> Result<String> {
    let egress_id = format!("egr_{}", uuid::Uuid::new_v4());
    conn.execute(
        "INSERT INTO egress_log
            (egress_id, task_id, data_class, source, destination, approved, bytes, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            egress_id,
            task_id,
            data_class.as_str(),
            source,
            destination,
            approved as i64,
            bytes,
            crate::policy::taint_repo::now_iso8601(),
        ],
    )?;
    Ok(egress_id)
}

/// 查询某 task 的 egress 记录数(测试 / egress_block_scorer 计分用)。
pub fn count_egress(conn: &Connection, task_id: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM egress_log WHERE task_id = ?1",
        params![task_id],
        |r| r.get(0),
    )?;
    Ok(n)
}

/// 查询某 task 未确认(approved=0)的外发记录数。
///
/// spec §9.4 ⑤ 门禁:"0 未确认外发" —— 本函数是门禁断言的基础。
pub fn count_unconfirmed_egress(conn: &Connection, task_id: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM egress_log WHERE task_id = ?1 AND approved = 0",
        params![task_id],
        |r| r.get(0),
    )?;
    Ok(n)
}
