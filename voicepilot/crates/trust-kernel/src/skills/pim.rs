//! Daisy 移植：邮件与本地 PIM（备忘/提醒/日历）。
//!
//! Daisy 这组全是 macOS 专属（Mail/Notes/Reminders/Calendar.app），
//! Windows 版直接不支持。我们在 Windows 上给等价实现：
//! - 发邮件：`mailto:` 打开默认邮件客户端写信窗口（收件人/主题/正文预填，
//!   **发送键永远在用户手里**——比 Daisy 的静默直发更安全，PerStep 审批）；
//! - 备忘/提醒/日历：本地 JSON 存储（`%APPDATA%/voicepilot/pim.json`），
//!   提醒到点走一次性计划任务弹窗。读/查免审批，写 PerStep。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{
    SimpleInput, run_simple, schtasks_once, simple_manifest, slot_text, slot_text_opt,
    truncate_chars,
};
use crate::skills::sys_ops::next_alarm_time;
use std::collections::HashMap;
use std::path::PathBuf;

fn text_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::Text,
        required: true,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

fn opt_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::Text,
        required: false,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

/// mailto 链接构造（纯函数，可测）。正文换行转 CRLF。
pub fn build_mailto(to: &str, subject: &str, body: &str) -> String {
    fn enc(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            // mailto 收件人的 @ 保留明文可读，其余按 UTF-8 字节转义。
            if b.is_ascii_alphanumeric() || b"-_.~@".contains(&b) {
                out.push(b as char);
            } else if b == b' ' {
                out.push_str("%20");
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }
    format!(
        "mailto:{}?subject={}&body={}",
        enc(to.trim()),
        enc(subject.trim()),
        enc(&body.replace('\n', "\r\n"))
    )
}

pub fn mail_compose_manifest() -> SkillManifest {
    simple_manifest(
        "mail.compose",
        "写邮件",
        "打开默认邮件客户端写信窗口（收件人/主题/正文预填），你点发送才发出（免审批）。",
        &["发邮件", "写邮件", "发信", "邮件"],
        &["给张三发封邮件", "帮我写封邮件"],
        vec![
            text_slot("to", 200),
            text_slot("subject", 200),
            text_slot("body", 8000),
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_mail_compose(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let to = slot_text(inputs, "to")?;
    let subject = slot_text(inputs, "subject")?;
    let body = slot_text(inputs, "body")?;
    if !to.contains('@') {
        return Err(KernelError::Skill(format!("收件人不是邮箱: {to}")));
    }
    let mut map = HashMap::new();
    map.insert("to".to_string(), serde_json::json!(to));
    map.insert("subject".to_string(), serde_json::json!(subject));
    map.insert("body".to_string(), serde_json::json!(body));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "mail.compose",
        &format!("mailto:{to}"),
        true,
        &mail_compose_manifest(),
        &map,
        || {
            let link = build_mailto(&to, &subject, &body);
            // mailto: 不是 http(s)，不走 open_url（会被拦截）；直调系统打开。
            crate::skills::simple::run_cmd("cmd", &["/c", "start", "", &link], 15, 500)?;
            Ok(format!("已打开写信窗口（收件人 {to}），你检查后点发送"))
        },
    )
}

// ---------- 本地 PIM 存储 ----------

/// 记录类型：note / reminder / event。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PimRecord {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// reminder/event 的触发或开始时刻（RFC3339 本地），note 为空。
    #[serde(default)]
    pub due: String,
    pub created: String,
}

pub fn pim_store_path() -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| KernelError::Skill("取不到数据目录".to_string()))?;
    Ok(base.join("voicepilot").join("pim.json"))
}

pub fn pim_load() -> Result<Vec<PimRecord>> {
    let path = pim_store_path()?;
    if !path.is_file() {
        return Ok(vec![]);
    }
    let bytes =
        std::fs::read(&path).map_err(|e| KernelError::Skill(format!("读 PIM 存储失败: {e}")))?;
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    match serde_json::from_slice(&bytes) {
        Ok(records) => Ok(records),
        // 损坏不丢数据：旧文件改名备份，返回空库继续服务。
        Err(e) => {
            let bak = path.with_extension(format!(
                "corrupt-{}.bak",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ));
            let _ = std::fs::rename(&path, &bak);
            Err(KernelError::Skill(format!(
                "PIM 存储损坏（{e}），旧文件已备份到 {}，请检查后重试",
                bak.display()
            )))
        }
    }
}

pub fn pim_save(records: &[PimRecord]) -> Result<()> {
    let path = pim_store_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| KernelError::Skill(format!("建 PIM 目录失败: {e}")))?;
    }
    let bytes = serde_json::to_string_pretty(records)
        .map_err(|e| KernelError::Skill(format!("序列化失败: {e}")))?;
    // 原子写：先落临时文件再改名，崩溃不截断旧库。
    // 并发说明：语音轮次串行执行，无并行写；DAG 并行节点同写极罕见，
    // 后写覆盖（rename 原子，无半截文件）。
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)
        .map_err(|e| KernelError::Skill(format!("写 PIM 临时文件失败: {e}")))?;
    std::fs::rename(&tmp, &path).map_err(|e| KernelError::Skill(format!("PIM 落盘失败: {e}")))?;
    Ok(())
}

pub fn pim_add(record: PimRecord) -> Result<usize> {
    let mut all = pim_load()?;
    all.push(record);
    let n = all.len();
    pim_save(&all)?;
    Ok(n)
}

pub fn pim_search(kind: &str, query: &str) -> Result<Vec<PimRecord>> {
    let q = query.trim().to_lowercase();
    Ok(pim_load()?
        .into_iter()
        .filter(|r| {
            (kind.is_empty() || r.kind == kind)
                && (q.is_empty()
                    || r.title.to_lowercase().contains(&q)
                    || r.body.to_lowercase().contains(&q))
        })
        .collect())
}

pub fn pim_note_manifest() -> SkillManifest {
    simple_manifest(
        "pim.note_create",
        "记备忘",
        "记一条本地备忘（标题+正文，可搜索）（免审批）。",
        &["备忘", "记一下", "记住", "笔记"],
        &["记一下：明天带伞", "帮我记住这件事"],
        vec![text_slot("title", 200), opt_slot("body", 4000)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_pim_note(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let title = slot_text(inputs, "title")?;
    let body = slot_text_opt(inputs, "body").unwrap_or_default();
    let mut map = HashMap::new();
    map.insert("title".to_string(), serde_json::json!(title));
    if inputs.get("body").is_some() {
        map.insert("body".to_string(), serde_json::json!(body));
    }
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "pim.note_create",
        &format!("note:{title}"),
        true,
        &pim_note_manifest(),
        &map,
        || {
            let n = pim_add(PimRecord {
                kind: "note".to_string(),
                title: title.clone(),
                body,
                due: String::new(),
                created: chrono::Local::now().to_rfc3339(),
            })?;
            Ok(format!("已记下（第 {n} 条）：{title}"))
        },
    )
}

pub fn pim_search_manifest() -> SkillManifest {
    simple_manifest(
        "pim.notes_search",
        "搜备忘",
        "按关键词搜本地备忘/提醒/日历。只读。",
        &["搜备忘", "找备忘", "记得什么", "备忘录"],
        &["搜一下备忘", "我之前记了什么"],
        vec![opt_slot("query", 200)],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_pim_search(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text_opt(inputs, "query").unwrap_or_default();
    let mut map = HashMap::new();
    if inputs.get("query").is_some() {
        map.insert("query".to_string(), serde_json::json!(query));
    }
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "pim.notes_search",
        "pim:search",
        false,
        &pim_search_manifest(),
        &map,
        || {
            let hits = pim_search("", &query)?;
            if hits.is_empty() {
                return Ok("没有匹配的备忘".to_string());
            }
            let lines: Vec<String> = hits
                .iter()
                .take(20)
                .map(|r| {
                    let when = if r.due.is_empty() {
                        "".to_string()
                    } else {
                        format!("（{}）", r.due)
                    };
                    format!(
                        "- [{}]{}{}: {}",
                        r.kind,
                        when,
                        r.title,
                        truncate_chars(&r.body, 120)
                    )
                })
                .collect();
            Ok(format!("找到 {} 条：\n{}", hits.len(), lines.join("\n")))
        },
    )
}

pub fn pim_reminder_manifest() -> SkillManifest {
    simple_manifest(
        "pim.reminder_create",
        "设提醒",
        "到指定时刻（HH:mm）弹窗提醒并留档，可搜索（免审批）。",
        &["提醒", "到时提醒", "别忘了", "提醒我"],
        &["下午3点提醒我开会", "提醒我交作业"],
        vec![text_slot("time", 8), text_slot("title", 200)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_pim_reminder(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let time = slot_text(inputs, "time")?;
    let title = slot_text(inputs, "title")?;
    let mut map = HashMap::new();
    map.insert("time".to_string(), serde_json::json!(time));
    map.insert("title".to_string(), serde_json::json!(title));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "pim.reminder_create",
        &format!("reminder:{time}:{title}"),
        true,
        &pim_reminder_manifest(),
        &map,
        || {
            crate::skills::simple::require_bin("msg", "家庭版 Windows 没有 msg.exe，改用专业版")?;
            let fire = next_alarm_time(&time)?;
            let when = schtasks_once(
                &crate::skills::simple::schtasks_name("rem", fire),
                fire,
                &title,
            )?;
            pim_add(PimRecord {
                kind: "reminder".to_string(),
                title: title.clone(),
                body: String::new(),
                due: fire.to_rfc3339(),
                created: chrono::Local::now().to_rfc3339(),
            })?;
            Ok(format!("提醒已设置：{title}，{when} 弹窗"))
        },
    )
}

pub fn pim_calendar_manifest() -> SkillManifest {
    simple_manifest(
        "pim.calendar_create",
        "记日程",
        "记一条本地日程（标题+开始时刻 HH:mm，可选天数偏移）（免审批）。",
        &["日程", "安排", "约会", "会议安排"],
        &["记个日程：明天下午3点开会", "安排一下"],
        vec![
            text_slot("title", 200),
            text_slot("time", 8),
            opt_slot("days_ahead", 4),
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_pim_calendar(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let title = slot_text(inputs, "title")?;
    let time = slot_text(inputs, "time")?;
    let days: i64 = slot_text_opt(inputs, "days_ahead")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .clamp(0, 365);
    let mut map = HashMap::new();
    map.insert("title".to_string(), serde_json::json!(title));
    map.insert("time".to_string(), serde_json::json!(time));
    if inputs.get("days_ahead").is_some() {
        map.insert("days_ahead".to_string(), serde_json::json!(days));
    }
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "pim.calendar_create",
        &format!("event:{title}"),
        true,
        &pim_calendar_manifest(),
        &map,
        || {
            let mut fire = next_alarm_time(&time)?;
            if days > 0 {
                fire += chrono::Duration::days(days);
            }
            pim_add(PimRecord {
                kind: "event".to_string(),
                title: title.clone(),
                body: String::new(),
                due: fire.to_rfc3339(),
                created: chrono::Local::now().to_rfc3339(),
            })?;
            Ok(format!(
                "日程已记：{title}，{}",
                fire.format("%m月%d日 %H:%M")
            ))
        },
    )
}

pub fn pim_calendar_list_manifest() -> SkillManifest {
    simple_manifest(
        "pim.calendar_list",
        "查日程",
        "查未来 N 天内的本地日程（默认 7 天）。只读。",
        &["日程", "安排", "有什么会", "行程"],
        &["查一下未来几天的日程", "明天有什么安排"],
        vec![opt_slot("days", 4)],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_pim_calendar_list(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let days: i64 = slot_text_opt(inputs, "days")
        .and_then(|v| v.parse().ok())
        .unwrap_or(7)
        .clamp(1, 365);
    let mut map = HashMap::new();
    if inputs.get("days").is_some() {
        map.insert("days".to_string(), serde_json::json!(days));
    }
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "pim.calendar_list",
        "pim:calendar",
        false,
        &pim_calendar_list_manifest(),
        &map,
        || {
            let now = chrono::Local::now();
            let end = now + chrono::Duration::days(days);
            let mut hits: Vec<PimRecord> = pim_load()?
                .into_iter()
                .filter(|r| {
                    (r.kind == "event" || r.kind == "reminder")
                        && !r.due.is_empty()
                        && r.due
                            .parse::<chrono::DateTime<chrono::Local>>()
                            .map(|t| t >= now && t <= end)
                            .unwrap_or(false)
                })
                .collect();
            hits.sort_by(|a, b| a.due.cmp(&b.due));
            if hits.is_empty() {
                return Ok(format!("未来 {days} 天没有日程"));
            }
            let lines: Vec<String> = hits
                .iter()
                .take(30)
                .map(|r| format!("- {} [{}]{}", r.due, r.kind, r.title))
                .collect();
            Ok(format!(
                "未来 {days} 天日程（{}）：\n{}",
                hits.len(),
                lines.join("\n")
            ))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::skills::manifest::ApprovalMode;

    // 真存储文件是进程级全局状态：碰它的测试必须串行。
    static PIM_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    fn pim_lock() -> std::sync::MutexGuard<'static, ()> {
        PIM_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap()
    }

    #[test]
    fn pim_manifests_have_ids() {
        for m in [
            mail_compose_manifest(),
            pim_note_manifest(),
            pim_search_manifest(),
            pim_reminder_manifest(),
            pim_calendar_manifest(),
            pim_calendar_list_manifest(),
        ] {
            assert!(!m.id.is_empty() && !m.keywords.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn mailto_encodes_fields() {
        let link = build_mailto("a@b.com", "你好", "第一行\n第二行");
        assert!(link.starts_with("mailto:a@b.com?"), "{link}");
        assert!(!link.contains("你好"), "{link}");
        assert!(link.contains("%0D%0A"), "{link}");
    }

    #[test]
    fn mail_rejects_non_email() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"to": "张三", "subject": "x", "body": "y"});
        assert!(execute_mail_compose(&kernel, &AutoApprover, "t1", "s1", &inputs).is_err());
    }

    #[test]
    fn mail_approval_free_posture() {
        // 2026：mail.compose 不在审批白名单 —— manifest 姿态 None。
        let m = mail_compose_manifest();
        assert!(matches!(m.approval.mode, ApprovalMode::None), "{}", m.id);

        // 免审批后非邮箱地址校验照常生效。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"to": "张三", "subject": "x", "body": "y"});
        assert!(execute_mail_compose(&kernel, &AutoApprover, "t1", "s1", &inputs).is_err());
    }

    #[test]
    fn pim_corrupt_store_backs_up_and_errors() {
        // 损坏文件 → 报错 + 改名备份（不静默丢数据）。
        let _guard = pim_lock();
        let path = pim_store_path().unwrap();
        let existed = path.is_file();
        let backup = std::fs::read(&path).ok();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{not json").unwrap();
        let err = pim_load().unwrap_err();
        assert!(format!("{err:?}").contains("备份"), "{err:?}");
        assert!(!path.exists());
        // 恢复现场。
        if let Some(bytes) = backup {
            if existed {
                std::fs::write(&path, bytes).unwrap();
            }
        }
        for entry in std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
        {
            if entry.file_name().to_string_lossy().contains("corrupt-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    #[test]
    fn pim_store_add_and_search_roundtrip() {
        // 写真实存储再清理：保证隔离，用唯一标题。
        let _guard = pim_lock();
        // 写真实存储再清理：保证隔离，用唯一标题。
        let title = format!("vp-test-{}", std::process::id());
        pim_add(PimRecord {
            kind: "note".to_string(),
            title: title.clone(),
            body: "正文".to_string(),
            due: String::new(),
            created: chrono::Local::now().to_rfc3339(),
        })
        .unwrap();
        let hits = pim_search("note", &title).unwrap();
        assert!(hits.iter().any(|r| r.title == title));
        // 清理。
        let rest: Vec<PimRecord> = pim_load()
            .unwrap()
            .into_iter()
            .filter(|r| r.title != title)
            .collect();
        pim_save(&rest).unwrap();
    }
}
