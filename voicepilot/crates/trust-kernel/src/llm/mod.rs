//! LLM 客户端 — W7 §2.1.
//!
//! OpenAI 兼容 API(默认 DeepSeek),用于意图分类 + Slot 提取 fallback。
//! LLM 仅在路由阶段调用,Skill 执行不调 LLM。
//!
//! `types` 模块始终编译(只依赖 serde/thiserror);
//! `client` 模块在 `llm` feature 下编译(依赖 reqwest),Task 3 引入。

pub mod types;
#[cfg(feature = "llm")]
pub mod client;

