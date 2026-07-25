//! LLM 类型定义 — W7 §2.1.
//!
//! 这些类型只依赖 serde/thiserror,无需 `llm` feature 即可编译。
//! `LlmClient`(依赖 reqwest)在 `client.rs` 中,需 `llm` feature。

use serde::{Deserialize, Serialize};

/// LLM 路由响应
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmRouteResponse {
    /// 匹配的 Skill ID(None = 走 Planner)
    pub matched_skill_id: Option<String>,
    /// 置信度 [0.0, 1.0]
    pub confidence: f32,
    /// LLM 提取的 Slot 列表
    pub slots: Vec<ExtractedSlot>,
    /// LLM 推理过程(用于审计日志)
    pub reasoning: String,
}

/// LLM 提取的 Slot
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractedSlot {
    /// "path" / "app" / "number" / "recipient" / "delete_target" / "time_range" / "url"
    pub kind: String,
    /// Slot 原始文本
    pub raw: String,
    /// 是否高风险(UI 强制视觉确认)
    pub high_risk: bool,
}

/// LLM 错误
#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    #[error("LLM HTTP error: {0}")]
    Http(String),
    #[error("LLM JSON parse error: {0}")]
    Parse(String),
    #[error("LLM timeout after {0:?}")]
    Timeout(std::time::Duration),
    #[error("LLM not configured (api_key empty)")]
    NotConfigured,
    #[error("LLM disabled by privacy_mode")]
    DisabledByPrivacy,
}

pub type LlmResult<T> = std::result::Result<T, LlmError>;
