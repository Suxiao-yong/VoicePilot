//! RouterBridge — voice-gated shim，兼容既有导入路径。
//!
//! Phase C：实现全部上移到常驻模块 `crate::route_bridge`（不 voice-gated），
//! 后台调度器（default-gated）复用 `route_text_with_dag` 唤醒 planner。
//! 本文件仅保留 re-export，既有调用方（ui voice_commands / CLI）不受影响。

pub use crate::route_bridge::{
    RouteOutcome, block_on_planner, route_text, route_text_with_dag, route_text_with_snapshot,
};
