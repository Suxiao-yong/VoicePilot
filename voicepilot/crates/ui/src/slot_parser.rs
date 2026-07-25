//! SlotParser — V1.1 §8.4 转写文本 Slot 提取。
//!
//! 从转写文本中提取 path / app / number 三类 Slot，用于 Chip 修改 UI。
//! 高风险 Slot(path/recipient/delete-target)在 UI 中需视觉确认。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotKind {
    /// 文件系统路径(高风险)。
    Path,
    /// 应用名(低风险)。
    App,
    /// 数量(低风险)。
    Number,
    /// 收件人(高风险,如邮件/消息)。
    Recipient,
    /// 删除目标(高风险)。
    DeleteTarget,
    /// W7 新增:时间范围(LLM 提取,如"昨天"/"上周")。
    TimeRange,
    /// W7 新增:URL(LLM 提取,浏览器自动化场景)。
    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    pub kind: SlotKind,
    pub raw: String,
    pub start: usize,
    pub end: usize,
    /// 是否高风险(用于 UI 强制视觉确认)。
    pub high_risk: bool,
}

fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?P<path>[A-Za-z]:[\\/][^\s,，。;.]+(?:\.[^\s,，。;.]+)*)").unwrap()
    })
}

fn app_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:打开|启动|关闭|launch|open|quit)\s+(?P<app>[A-Za-z][A-Za-z0-9_\-.]*)").unwrap()
    })
}

fn number_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?P<num>\d+)\s*(?:个|条|份|次|张|篇|分钟|秒)?").unwrap())
}

fn recipient_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:发送给|邮件给|发给|mailto:)\s*(?P<rcp>[\w\.\-]+@[\w\.\-]+|[\u4e00-\u9fa5]{2,4})").unwrap()
    })
}

fn delete_target_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:删除|清空|移除)\s+(?P<target>[A-Za-z]:[\\/][^\s,，。;.]+(?:\.[^\s,，。;.]+)*|[^\s,，。;.]+(?:\.[^\s,，。;.]+)*)").unwrap()
    })
}

pub struct SlotParser;

impl SlotParser {
    /// 从转写文本中提取所有 Slot。
    pub fn parse(text: &str) -> Vec<Slot> {
        let mut slots = Vec::new();
        slots.extend(Self::parse_paths(text));
        slots.extend(Self::parse_apps(text));
        slots.extend(Self::parse_numbers(text));
        slots.extend(Self::parse_recipients(text));
        slots.extend(Self::parse_delete_targets(text));
        slots.sort_by_key(|s| s.start);
        slots
    }

    fn parse_paths(text: &str) -> Vec<Slot> {
        let mut result = Vec::new();
        // Windows 路径:C:\foo\bar 或 D:/foo/bar 或 E:\definitely_nonexistent
        // 允许内部点(扩展名),但不允许末尾点(句末标点)。
        for cap in path_regex().captures_iter(text) {
            let path = cap.name("path").unwrap();
            result.push(Slot {
                kind: SlotKind::Path,
                raw: path.as_str().to_string(),
                start: path.start(),
                end: path.end(),
                high_risk: true,
            });
        }
        result
    }

    fn parse_apps(text: &str) -> Vec<Slot> {
        let mut result = Vec::new();
        // "打开 X" / "启动 X" / "关闭 X" 中的 X(应用名)
        for cap in app_regex().captures_iter(text) {
            let app = cap.name("app").unwrap();
            result.push(Slot {
                kind: SlotKind::App,
                raw: app.as_str().to_string(),
                start: app.start(),
                end: app.end(),
                high_risk: false,
            });
        }
        result
    }

    fn parse_numbers(text: &str) -> Vec<Slot> {
        let mut result = Vec::new();
        // 数字 + 可选单位(个/条/份/次/张/篇/分钟/秒)
        for cap in number_regex().captures_iter(text) {
            let num = cap.name("num").unwrap();
            result.push(Slot {
                kind: SlotKind::Number,
                raw: num.as_str().to_string(),
                start: num.start(),
                end: num.end(),
                high_risk: false,
            });
        }
        result
    }

    fn parse_recipients(text: &str) -> Vec<Slot> {
        let mut result = Vec::new();
        // "发送给 X" / "邮件给 X" 中的 X(收件人)
        for cap in recipient_regex().captures_iter(text) {
            let rcp = cap.name("rcp").unwrap();
            result.push(Slot {
                kind: SlotKind::Recipient,
                raw: rcp.as_str().to_string(),
                start: rcp.start(),
                end: rcp.end(),
                high_risk: true,
            });
        }
        result
    }

    fn parse_delete_targets(text: &str) -> Vec<Slot> {
        let mut result = Vec::new();
        // "删除 X" / "清空 X" 中的 X(删除目标,可能是路径或文件名)
        // 允许内部点(扩展名),但不允许末尾点(句末标点)。
        for cap in delete_target_regex().captures_iter(text) {
            let target = cap.name("target").unwrap();
            // 避免与 path slot 重复(若 target 已被 path slot 覆盖,跳过)
            let target_str = target.as_str();
            // 第二字节为 ':' 即 Windows 盘符路径(如 "C:\..."),已由 parse_paths 处理,跳过避免重复。
            if target_str.as_bytes().get(1) == Some(&b':') {
                continue;
            }
            result.push(Slot {
                kind: SlotKind::DeleteTarget,
                raw: target_str.to_string(),
                start: target.start(),
                end: target.end(),
                high_risk: true,
            });
        }
        result
    }

    /// W7: regex 优先,0 命中时调 LLM fallback。
    ///
    /// 路由顺序:
    ///   1. regex `SlotParser::parse` 命中 → 直接返回(regex 提取的 Slot 带精确 start/end)
    ///   2. regex 0 命中且 `llm` 提供 + `is_enabled()` → 调 `classify_and_extract`
    ///   3. LLM 返回的 `ExtractedSlot` 转为 `Slot`(start=0/end=raw.len(),LLM 不返回位置)
    ///   4. LLM 失败 → 返回空 Vec(回退到 Planner)
    ///
    /// LLM 集成测试在 `tests/w7_router_llm_smoke.rs`(Task 16)端到端覆盖。
    #[cfg(feature = "llm")]
    pub async fn parse_with_llm_fallback(
        text: &str,
        llm: Option<&trust_kernel::llm::client::LlmClient>,
        candidate_skills: &[trust_kernel::skills::manifest::SkillManifest],
    ) -> Vec<Slot> {
        // Step 1: regex 优先
        let regex_slots = Self::parse(text);
        if !regex_slots.is_empty() {
            return regex_slots;
        }

        // Step 2-3: LLM fallback(regex 0 命中)
        if let Some(llm) = llm {
            if llm.is_enabled() {
                if let Ok(resp) = llm.classify_and_extract(text, candidate_skills).await {
                    return resp
                        .slots
                        .iter()
                        .map(|s| Slot {
                            kind: parse_slot_kind(&s.kind),
                            raw: s.raw.clone(),
                            // LLM 不返回字符位置,用 0..len 占位;UI 渲染时不依赖位置。
                            start: 0,
                            end: s.raw.chars().count(),
                            high_risk: s.high_risk,
                        })
                        .collect();
                }
            }
        }

        // Step 4: 空Vec(回退到 Planner)
        Vec::new()
    }
}

/// W7: 将 LLM 返回的 `kind` 字符串映射为 `SlotKind`。
///
/// LLM 的 `ExtractedSlot.kind` 是开放字符串(由 prompt 约束为 7 种之一),
/// 此处做白名单映射;未知 kind 默认 `App`(低风险,避免误判为 path/recipient)。
#[cfg(feature = "llm")]
fn parse_slot_kind(kind: &str) -> SlotKind {
    match kind {
        "path" => SlotKind::Path,
        "app" => SlotKind::App,
        "number" => SlotKind::Number,
        "recipient" => SlotKind::Recipient,
        "delete_target" => SlotKind::DeleteTarget,
        "time_range" => SlotKind::TimeRange,
        "url" => SlotKind::Url,
        _ => SlotKind::App,
    }
}

/// W7: 将 `LlmRouteResponse.slots`(`ExtractedSlot` 列表)转换为 UI `Slot` 列表。
///
/// LLM 不返回字符位置(start/end),用 `0..raw.chars().count()` 占位:
/// UI Chip 渲染只依赖 `raw` + `kind` + `high_risk`,不依赖位置;
/// 后续 Skill executor 也只读 `raw`,位置仅用于源文本高亮(可选)。
///
/// 调用方:`commands::route_text` 在 `RouteDecision::SkillWithSlots` 分支调用。
#[cfg(feature = "llm")]
pub fn convert_extracted_slots(
    extracted: &[trust_kernel::llm::types::ExtractedSlot],
) -> Vec<Slot> {
    extracted
        .iter()
        .map(|s| Slot {
            kind: parse_slot_kind(&s.kind),
            raw: s.raw.clone(),
            start: 0,
            end: s.raw.chars().count(),
            high_risk: s.high_risk,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_windows_path() {
        let slots = SlotParser::parse("整理 C:\\Users\\test\\downloads 的图片");
        let paths: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Path).collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "C:\\Users\\test\\downloads");
        assert!(paths[0].high_risk);
    }

    #[test]
    fn parse_extracts_forward_slash_path() {
        let slots = SlotParser::parse("整理 D:/downloads 的图片");
        let paths: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Path).collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "D:/downloads");
    }

    #[test]
    fn parse_extracts_app_name() {
        let slots = SlotParser::parse("打开 notepad 编辑文件");
        let apps: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::App).collect();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].raw, "notepad");
        assert!(!apps[0].high_risk);
    }

    #[test]
    fn parse_extracts_number_with_unit() {
        let slots = SlotParser::parse("整理 5 个文件");
        let nums: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Number).collect();
        assert_eq!(nums.len(), 1);
        assert_eq!(nums[0].raw, "5");
    }

    #[test]
    fn parse_extracts_recipient_email() {
        let slots = SlotParser::parse("发送给 alice@example.com 报告");
        let rcps: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Recipient).collect();
        assert_eq!(rcps.len(), 1);
        assert_eq!(rcps[0].raw, "alice@example.com");
        assert!(rcps[0].high_risk);
    }

    #[test]
    fn parse_extracts_delete_target_filename() {
        let slots = SlotParser::parse("删除 test.txt");
        let dts: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::DeleteTarget).collect();
        assert_eq!(dts.len(), 1);
        assert_eq!(dts[0].raw, "test.txt");
        assert!(dts[0].high_risk);
    }

    #[test]
    fn parse_returns_empty_for_plain_text() {
        let slots = SlotParser::parse("今天天气不错");
        assert!(slots.is_empty());
    }

    #[test]
    fn parse_handles_mixed_slots() {
        let slots = SlotParser::parse("打开 notepad 整理 C:\\temp 5 个文件");
        assert!(slots.len() >= 3, "expected >= 3 slots, got {}", slots.len());
        // 验证按 start 排序
        for i in 1..slots.len() {
            assert!(slots[i - 1].start <= slots[i].start, "slots should be sorted by start");
        }
    }

    #[test]
    fn slot_serializes_with_kind_tag() {
        let slot = Slot {
            kind: SlotKind::Path,
            raw: "C:\\foo".to_string(),
            start: 0,
            end: 6,
            high_risk: true,
        };
        let json = serde_json::to_string(&slot).unwrap();
        assert!(json.contains("\"kind\":\"path\""));
        assert!(json.contains("\"high_risk\":true"));
    }

    #[test]
    fn parse_handles_empty_string() {
        let slots = SlotParser::parse("");
        assert!(slots.is_empty());
    }

    #[test]
    fn parse_extracts_chinese_recipient() {
        let slots = SlotParser::parse("发送给 张三 报告");
        let rcps: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Recipient).collect();
        assert_eq!(rcps.len(), 1);
        assert_eq!(rcps[0].raw, "张三");
        assert!(rcps[0].high_risk);
    }

    #[test]
    fn parse_dedups_path_and_delete_target() {
        // "删除 C:\foo\bar" 中 C:\foo\bar 同时匹配 path 和 delete_target,应去重为 Path slot。
        let slots = SlotParser::parse("删除 C:\\foo\\bar");
        let paths: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Path).collect();
        let dts: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::DeleteTarget).collect();
        assert_eq!(paths.len(), 1, "path slot should be extracted");
        assert_eq!(dts.len(), 0, "delete_target should be deduped when it's a path");
    }

    #[test]
    fn parse_extracts_path_with_extension() {
        let slots = SlotParser::parse("整理 C:\\temp\\file.txt 的图片");
        let paths: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Path).collect();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].raw, "C:\\temp\\file.txt");
    }

    #[test]
    fn parse_extracts_number_without_unit() {
        let slots = SlotParser::parse("整理 5");
        let nums: Vec<&Slot> = slots.iter().filter(|s| s.kind == SlotKind::Number).collect();
        assert_eq!(nums.len(), 1);
        assert_eq!(nums[0].raw, "5");
    }

    #[test]
    fn slot_serializes_all_kinds_with_snake_case() {
        // 验证所有 SlotKind 变体的 snake_case 序列化(特别是 delete_target / time_range)。
        for (kind, expected) in [
            (SlotKind::Path, "path"),
            (SlotKind::App, "app"),
            (SlotKind::Number, "number"),
            (SlotKind::Recipient, "recipient"),
            (SlotKind::DeleteTarget, "delete_target"),
            (SlotKind::TimeRange, "time_range"),
            (SlotKind::Url, "url"),
        ] {
            let slot = Slot {
                kind: kind.clone(),
                raw: "x".to_string(),
                start: 0,
                end: 1,
                high_risk: false,
            };
            let json = serde_json::to_string(&slot).unwrap();
            assert!(
                json.contains(&format!("\"kind\":\"{}\"", expected)),
                "expected kind={} in json: {}", expected, json
            );
        }
    }
}
