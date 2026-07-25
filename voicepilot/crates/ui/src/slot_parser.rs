//! SlotParser — V1.1 §8.4 转写文本 Slot 提取。
//!
//! 从转写文本中提取 path / app / number 三类 Slot，用于 Chip 修改 UI。
//! 高风险 Slot(path/recipient/delete-target)在 UI 中需视觉确认。

use serde::{Deserialize, Serialize};

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
        let pattern = r"(?P<path>[A-Za-z]:[\\/][^\s,，。;.]+(?:\.[^\s,，。;.]+)*)";
        let re = regex::Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
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
        let pattern = r"(?:打开|启动|关闭|launch|open|quit)\s+(?P<app>[A-Za-z][A-Za-z0-9_\-.]*)";
        let re = regex::Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
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
        let pattern = r"(?P<num>\d+)\s*(?:个|条|份|次|张|篇|分钟|秒)?";
        let re = regex::Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
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
        let pattern = r"(?:发送给|邮件给|发给|mailto:)\s*(?P<rcp>[\w\.\-]+@[\w\.\-]+|[\u4e00-\u9fa5]{2,4})";
        let re = regex::Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
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
        let pattern = r"(?:删除|清空|移除)\s+(?P<target>[A-Za-z]:[\\/][^\s,，。;.]+(?:\.[^\s,，。;.]+)*|[^\s,，。;.]+(?:\.[^\s,，。;.]+)*)";
        let re = regex::Regex::new(pattern).unwrap();
        for cap in re.captures_iter(text) {
            let target = cap.name("target").unwrap();
            // 避免与 path slot 重复(若 target 已被 path slot 覆盖,跳过)
            let target_str = target.as_str();
            if target_str.len() >= 3 && target_str.chars().nth(1) == Some(':') {
                continue; // 路径,已由 parse_paths 处理
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
}
