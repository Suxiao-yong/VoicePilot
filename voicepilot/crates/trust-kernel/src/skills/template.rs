//! SlotTemplateEngine — W8 §2.1.
//!
//! 解析 LLM 生成的模板字符串 → 编译为 `TemplateExpr` AST →
//! 用上游节点 outputs + 用户 Slot + 循环变量渲染为最终值。
//!
//! 模板语法:
//! - `"notepad"` — 字面量
//! - `"${prev.output.path}"` — 紧邻上游节点的 output.path
//! - `"${n1.output.path}"` — 指定节点 n1 的 output.path
//! - `"${user.profile_name}"` — 用户审批阶段填的 Slot
//! - `"${item}"` — 循环变量
//! - `"C:/Users/${user.name}/Documents/${item.name}"` — 字面量 + 变量拼接
//! - `"${prev.output.files}[?size > 1048576]"` — Filter 过滤(W8 仅支持 `[?size > N]`)

use serde::{Deserialize, Serialize};

use crate::llm::types::ExtractedSlot;

/// Slot 种类(从 W6b 的 SlotKind 复用语义,本 plan 重新定义避免跨 crate 依赖)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Path,
    App,
    Number,
    TimeRange,
    Url,
    /// 文件列表(循环迭代常用)
    Files,
    /// 文本(默认 / fallback)
    Text,
}

/// LLM 拆解时为每个节点提供的 input 描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotTemplate {
    pub kind: SlotKind,
    pub template: TemplateExpr,
}

/// 编译后的模板 AST。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateExpr {
    /// 纯字面量,如 "notepad"
    Literal(String),
    /// 单个变量引用,如 ${prev.output.path}
    Var(VarRef),
    /// 多段拼接,如 "C:/Users/" + ${user.name} + "/Documents"
    Concat(Vec<TemplateExpr>),
    /// Filter 过滤,如 ${prev.output.files}[?size > 1048576]
    Filter {
        source: Box<TemplateExpr>,
        predicate: String,
    },
}

/// 变量引用。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VarRef {
    pub scope: VarScope,
    /// dotted path,如 "output.path" / "name"
    pub path: String,
}

/// 变量作用域。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VarScope {
    /// 紧邻上游节点(拓扑序中前一个)
    Prev,
    /// 指定 node_id
    Step(String),
    /// 用户审批阶段填的 Slot
    User,
    /// 循环变量 ${item}
    Iter,
}

pub struct SlotTemplateEngine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// 词法错误:未闭合的 ${...}
    UnclosedVar { at: usize, src: String },
    /// 解析错误:语法不合法
    Parse { at: usize, src: String, msg: String },
    /// 渲染错误:变量未找到
    VarNotFound { scope: String, path: String },
    /// 渲染错误:类型不匹配(如对非数组应用 filter)
    TypeMismatch { expected: &'static str, got: String },
    /// 校验错误:引用了未声明的 node_id
    UnknownNodeId { node_id: String },
    /// 校验错误:引用了未声明的 user slot kind
    UnknownSlotKind { kind: String },
    /// Filter predicate 不支持(W8 仅支持 `[?size > N]` / `[?size < N]`)
    UnsupportedPredicate { predicate: String },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnclosedVar { at, src } => {
                write!(f, "unclosed ${{...}} at offset {} in {:?}", at, src)
            }
            Self::Parse { at, src, msg } => {
                write!(f, "parse error at offset {} in {:?}: {}", at, src, msg)
            }
            Self::VarNotFound { scope, path } => {
                write!(f, "variable not found: scope={}, path={}", scope, path)
            }
            Self::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {}, got {}", expected, got)
            }
            Self::UnknownNodeId { node_id } => {
                write!(f, "unknown node_id: {}", node_id)
            }
            Self::UnknownSlotKind { kind } => {
                write!(f, "unknown slot kind: {}", kind)
            }
            Self::UnsupportedPredicate { predicate } => {
                write!(f, "unsupported filter predicate: {}", predicate)
            }
        }
    }
}

impl std::error::Error for TemplateError {}

impl SlotTemplateEngine {
    /// 解析模板字符串 → 编译后的 `TemplateExpr`。
    ///
    /// 算法:单遍扫描,识别 `${...}` 占位符 + 字面量文本。
    /// 若整串只有一个 `${...}` 且无前后字面量 → 直接返回 Var。
    /// 若有多个段 → Concat。
    /// 若占位符后有 `[?...]` → Filter。
    pub fn parse(template_str: &str) -> Result<TemplateExpr, TemplateError> {
        let mut parts: Vec<TemplateExpr> = Vec::new();
        let mut buf = String::new();
        let chars: Vec<char> = template_str.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
                // flush 字面量 buf
                if !buf.is_empty() {
                    parts.push(TemplateExpr::Literal(std::mem::take(&mut buf)));
                }
                // 找匹配的 '}'
                let start = i + 2;
                let mut j = start;
                let mut depth = 1;
                while j < chars.len() && depth > 0 {
                    if chars[j] == '{' {
                        depth += 1;
                    } else if chars[j] == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if depth != 0 {
                    return Err(TemplateError::UnclosedVar {
                        at: i,
                        src: template_str.into(),
                    });
                }
                let var_src: String = chars[start..j].iter().collect();
                let var = Self::parse_var(&var_src, start, template_str)?;
                // 检查紧跟的 [?...] filter
                let mut k = j + 1;
                if k < chars.len() && chars[k] == '[' {
                    let filter_start = k;
                    while k < chars.len() && chars[k] != ']' {
                        k += 1;
                    }
                    if k >= chars.len() {
                        return Err(TemplateError::Parse {
                            at: filter_start,
                            src: template_str.into(),
                            msg: "unclosed filter [?...]".into(),
                        });
                    }
                    let predicate: String = chars[filter_start + 1..k].iter().collect();
                    let pred_trim = predicate.trim();
                    if !pred_trim.starts_with('?') {
                        return Err(TemplateError::UnsupportedPredicate {
                            predicate: predicate.clone(),
                        });
                    }
                    parts.push(TemplateExpr::Filter {
                        source: Box::new(TemplateExpr::Var(var)),
                        predicate: pred_trim[1..].trim().to_string(),
                    });
                    i = k + 1;
                } else {
                    parts.push(TemplateExpr::Var(var));
                    i = j + 1;
                }
            } else {
                buf.push(chars[i]);
                i += 1;
            }
        }
        if !buf.is_empty() {
            parts.push(TemplateExpr::Literal(buf));
        }
        match parts.len() {
            0 => Ok(TemplateExpr::Literal(String::new())),
            1 => Ok(parts.into_iter().next().unwrap()),
            _ => Ok(TemplateExpr::Concat(parts)),
        }
    }

    /// 解析单个 `${...}` 内容 → `VarRef`。
    /// 格式:`<scope>.<path>`,scope ∈ {prev, user, item, <node_id>}。
    fn parse_var(src: &str, at: usize, full: &str) -> Result<VarRef, TemplateError> {
        let s = src.trim();
        let (scope, path) = if let Some(p) = s.strip_prefix("prev.") {
            (VarScope::Prev, p.to_string())
        } else if let Some(p) = s.strip_prefix("user.") {
            (VarScope::User, p.to_string())
        } else if s == "item" || s.starts_with("item.") {
            (VarScope::Iter, s.strip_prefix("item.").unwrap_or("").to_string())
        } else if let Some(dot) = s.find('.') {
            let scope_str = &s[..dot];
            let path_str = &s[dot + 1..];
            (VarScope::Step(scope_str.to_string()), path_str.to_string())
        } else {
            return Err(TemplateError::Parse {
                at,
                src: full.into(),
                msg: format!("invalid var reference: {}", s),
            });
        };
        Ok(VarRef { scope, path })
    }

    /// 执行模板:用 node_outputs + user_slots + iter_var 渲染最终值。
    ///
    /// - `node_outputs`:key = node_id,value = 节点 output(serde_json::Value)
    /// - `user_slots`:用户审批阶段填的 Slot 列表
    /// - `iter_var`:循环变量当前值(Some 时表示在循环体内)
    /// - `prev_node_id`:拓扑序中前一个节点的 id(用于 VarScope::Prev 解析)
    pub fn resolve(
        expr: &TemplateExpr,
        node_outputs: &std::collections::HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        iter_var: Option<&serde_json::Value>,
        prev_node_id: Option<&str>,
    ) -> Result<serde_json::Value, TemplateError> {
        match expr {
            TemplateExpr::Literal(s) => Ok(serde_json::Value::String(s.clone())),
            TemplateExpr::Var(var) => Self::resolve_var(
                var,
                node_outputs,
                user_slots,
                iter_var,
                prev_node_id,
            ),
            TemplateExpr::Concat(parts) => {
                let mut s = String::new();
                for p in parts {
                    let v = Self::resolve(p, node_outputs, user_slots, iter_var, prev_node_id)?;
                    match v {
                        serde_json::Value::String(t) => s.push_str(&t),
                        other => s.push_str(&other.to_string()),
                    }
                }
                Ok(serde_json::Value::String(s))
            }
            TemplateExpr::Filter { source, predicate } => {
                let src_val = Self::resolve(source, node_outputs, user_slots, iter_var, prev_node_id)?;
                Self::apply_filter(&src_val, predicate)
            }
        }
    }

    fn resolve_var(
        var: &VarRef,
        node_outputs: &std::collections::HashMap<String, serde_json::Value>,
        user_slots: &[ExtractedSlot],
        iter_var: Option<&serde_json::Value>,
        prev_node_id: Option<&str>,
    ) -> Result<serde_json::Value, TemplateError> {
        match &var.scope {
            VarScope::Prev => {
                let nid = prev_node_id.ok_or_else(|| TemplateError::VarNotFound {
                    scope: "prev".into(),
                    path: var.path.clone(),
                })?;
                let node_out = node_outputs.get(nid).ok_or_else(|| TemplateError::VarNotFound {
                    scope: format!("prev({})", nid),
                    path: var.path.clone(),
                })?;
                Self::extract_path(node_out, &var.path)
            }
            VarScope::Step(step_id) => {
                let node_out = node_outputs.get(step_id).ok_or_else(|| TemplateError::VarNotFound {
                    scope: format!("step({})", step_id),
                    path: var.path.clone(),
                })?;
                Self::extract_path(node_out, &var.path)
            }
            VarScope::User => {
                // ExtractedSlot.kind 是 String(如 "path" / "app" / "text"),
                // 直接比较小写形式;同时也允许 raw 文本子串匹配作为 fallback。
                let path_lower = var.path.to_lowercase();
                let slot = user_slots.iter().find(|s| {
                    s.kind.to_lowercase() == path_lower || s.raw.contains(&var.path)
                });
                let slot = slot.ok_or_else(|| TemplateError::VarNotFound {
                    scope: "user".into(),
                    path: var.path.clone(),
                })?;
                Ok(serde_json::Value::String(slot.raw.clone()))
            }
            VarScope::Iter => {
                let val = iter_var.ok_or_else(|| TemplateError::VarNotFound {
                    scope: "item".into(),
                    path: var.path.clone(),
                })?;
                if var.path.is_empty() {
                    Ok(val.clone())
                } else {
                    Self::extract_path(val, &var.path)
                }
            }
        }
    }

    /// 从 JSON value 中按 dotted path 提取(如 "output.path" → obj["output"]["path"])。
    fn extract_path(val: &serde_json::Value, path: &str) -> Result<serde_json::Value, TemplateError> {
        if path.is_empty() {
            return Ok(val.clone());
        }
        let mut current = val;
        for key in path.split('.') {
            current = match current {
                serde_json::Value::Object(map) => map.get(key).ok_or_else(|| {
                    TemplateError::VarNotFound {
                        scope: "json_path".into(),
                        path: format!("{}.{}", path, key),
                    }
                })?,
                _ => {
                    return Err(TemplateError::TypeMismatch {
                        expected: "object",
                        got: current.to_string(),
                    })
                }
            };
        }
        Ok(current.clone())
    }

    /// W8 仅支持 `[?size > N]` / `[?size < N]` / `[?size >= N]` / `[?size <= N]`。
    /// 完整 JSONPath filter(如 `[?@.type == 'image']`)延后 W9+(spec §8)。
    fn apply_filter(
        src: &serde_json::Value,
        predicate: &str,
    ) -> Result<serde_json::Value, TemplateError> {
        let arr = match src {
            serde_json::Value::Array(a) => a,
            _ => {
                return Err(TemplateError::TypeMismatch {
                    expected: "array",
                    got: src.to_string(),
                })
            }
        };
        let p = predicate.trim();
        let (op, n_str) = if let Some(rest) = p.strip_prefix("size >=") {
            (">=", rest.trim())
        } else if let Some(rest) = p.strip_prefix("size <=") {
            ("<=", rest.trim())
        } else if let Some(rest) = p.strip_prefix("size >") {
            (">", rest.trim())
        } else if let Some(rest) = p.strip_prefix("size <") {
            ("<", rest.trim())
        } else {
            return Err(TemplateError::UnsupportedPredicate {
                predicate: predicate.into(),
            });
        };
        let threshold: u64 = n_str.parse().map_err(|_| TemplateError::UnsupportedPredicate {
            predicate: predicate.into(),
        })?;
        let filtered: Vec<serde_json::Value> = arr
            .iter()
            .filter(|item| {
                let size = item.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                match op {
                    ">" => size > threshold,
                    "<" => size < threshold,
                    ">=" => size >= threshold,
                    "<=" => size <= threshold,
                    _ => false,
                }
            })
            .cloned()
            .collect();
        Ok(serde_json::Value::Array(filtered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_literal() {
        let expr = SlotTemplateEngine::parse("notepad").unwrap();
        assert_eq!(expr, TemplateExpr::Literal("notepad".into()));
    }

    #[test]
    fn parse_empty_string() {
        let expr = SlotTemplateEngine::parse("").unwrap();
        assert_eq!(expr, TemplateExpr::Literal(String::new()));
    }

    #[test]
    fn parse_prev_var() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Prev,
                path: "output.path".into(),
            })
        );
    }

    #[test]
    fn parse_step_var() {
        let expr = SlotTemplateEngine::parse("${n1.output.path}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Step("n1".into()),
                path: "output.path".into(),
            })
        );
    }

    #[test]
    fn parse_user_var() {
        let expr = SlotTemplateEngine::parse("${user.profile_name}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::User,
                path: "profile_name".into(),
            })
        );
    }

    #[test]
    fn parse_iter_var() {
        let expr = SlotTemplateEngine::parse("${item}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Iter,
                path: String::new(),
            })
        );
    }

    #[test]
    fn parse_iter_with_path() {
        let expr = SlotTemplateEngine::parse("${item.name}").unwrap();
        assert_eq!(
            expr,
            TemplateExpr::Var(VarRef {
                scope: VarScope::Iter,
                path: "name".into(),
            })
        );
    }

    #[test]
    fn parse_concat() {
        let expr =
            SlotTemplateEngine::parse("C:/Users/${user.name}/Documents/${item.name}").unwrap();
        match expr {
            TemplateExpr::Concat(parts) => {
                // 4 parts: "C:/Users/" + ${user.name} + "/Documents/" + ${item.name}
                assert_eq!(parts.len(), 4);
                assert_eq!(parts[0], TemplateExpr::Literal("C:/Users/".into()));
                assert_eq!(
                    parts[1],
                    TemplateExpr::Var(VarRef {
                        scope: VarScope::User,
                        path: "name".into(),
                    })
                );
                assert_eq!(parts[2], TemplateExpr::Literal("/Documents/".into()));
                assert_eq!(
                    parts[3],
                    TemplateExpr::Var(VarRef {
                        scope: VarScope::Iter,
                        path: "name".into(),
                    })
                );
            }
            other => panic!("expected Concat, got {:?}", other),
        }
    }

    #[test]
    fn parse_filter() {
        let expr =
            SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
        match expr {
            TemplateExpr::Filter { source, predicate } => {
                assert_eq!(
                    *source,
                    TemplateExpr::Var(VarRef {
                        scope: VarScope::Prev,
                        path: "output.files".into(),
                    })
                );
                assert_eq!(predicate, "size > 1048576");
            }
            other => panic!("expected Filter, got {:?}", other),
        }
    }

    #[test]
    fn parse_unclosed_var_errors() {
        let err = SlotTemplateEngine::parse("${prev.output.path").unwrap_err();
        assert!(matches!(err, TemplateError::UnclosedVar { .. }));
    }

    #[test]
    fn parse_invalid_var_errors() {
        let err = SlotTemplateEngine::parse("${nopdot}").unwrap_err();
        assert!(matches!(err, TemplateError::Parse { .. }));
    }

    // ===== resolve tests (Task 4) =====

    use std::collections::HashMap;

    fn make_node_outputs() -> HashMap<String, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert(
            "n1".into(),
            serde_json::json!({
                "output": {
                    "path": "C:/Users/test/Documents/file.txt",
                    "files": [
                        {"name": "a.txt", "size": 500},
                        {"name": "b.txt", "size": 2_000_000}
                    ]
                }
            }),
        );
        m
    }

    #[test]
    fn resolve_literal() {
        let expr = TemplateExpr::Literal("hello".into());
        let v = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::String("hello".into()));
    }

    #[test]
    fn resolve_prev_var() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
        assert_eq!(v, serde_json::Value::String("C:/Users/test/Documents/file.txt".into()));
    }

    #[test]
    fn resolve_step_var() {
        let expr = SlotTemplateEngine::parse("${n1.output.path}").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
        assert_eq!(v, serde_json::Value::String("C:/Users/test/Documents/file.txt".into()));
    }

    #[test]
    fn resolve_concat() {
        let expr = SlotTemplateEngine::parse("Path: ${n1.output.path}").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, None).unwrap();
        assert_eq!(
            v,
            serde_json::Value::String("Path: C:/Users/test/Documents/file.txt".into())
        );
    }

    #[test]
    fn resolve_iter_var() {
        let expr = SlotTemplateEngine::parse("${item}").unwrap();
        let v = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            Some(&serde_json::json!("item_value")),
            None,
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::String("item_value".into()));
    }

    #[test]
    fn resolve_iter_with_path() {
        let expr = SlotTemplateEngine::parse("${item.name}").unwrap();
        let v = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            Some(&serde_json::json!({"name": "x.txt", "size": 100})),
            None,
        )
        .unwrap();
        assert_eq!(v, serde_json::Value::String("x.txt".into()));
    }

    #[test]
    fn resolve_filter_gt() {
        let expr = SlotTemplateEngine::parse("${prev.output.files}[?size > 1048576]").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
        match v {
            serde_json::Value::Array(a) => {
                assert_eq!(a.len(), 1, "only b.txt > 1MB");
                assert_eq!(a[0]["name"], "b.txt");
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn resolve_filter_lt() {
        let expr = SlotTemplateEngine::parse("${prev.output.files}[?size < 1048576]").unwrap();
        let outs = make_node_outputs();
        let v = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap();
        match v {
            serde_json::Value::Array(a) => {
                assert_eq!(a.len(), 1, "only a.txt < 1MB");
                assert_eq!(a[0]["name"], "a.txt");
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn resolve_var_not_found_errors() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}").unwrap();
        let err = SlotTemplateEngine::resolve(
            &expr,
            &HashMap::new(),
            &[],
            None,
            None, // no prev_node_id
        )
        .unwrap_err();
        assert!(matches!(err, TemplateError::VarNotFound { .. }));
    }

    #[test]
    fn resolve_filter_on_non_array_errors() {
        let expr = SlotTemplateEngine::parse("${prev.output.path}[?size > 100]").unwrap();
        let outs = make_node_outputs();
        let err = SlotTemplateEngine::resolve(&expr, &outs, &[], None, Some("n1")).unwrap_err();
        assert!(matches!(err, TemplateError::TypeMismatch { .. }));
    }
}
