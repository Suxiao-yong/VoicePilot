//! Compensation types — V1.1 §7.2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationLevel {
    None,
    BestEffort,
    Strong,
}

impl CompensationLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            CompensationLevel::None => "none",
            CompensationLevel::BestEffort => "best_effort",
            CompensationLevel::Strong => "strong",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(CompensationLevel::None),
            "best_effort" => Some(CompensationLevel::BestEffort),
            "strong" => Some(CompensationLevel::Strong),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    AutoReverse,
    RequireConfirmation,
    Fail,
}

impl ConflictPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictPolicy::AutoReverse => "auto_reverse",
            ConflictPolicy::RequireConfirmation => "require_confirmation",
            ConflictPolicy::Fail => "fail",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto_reverse" => Some(ConflictPolicy::AutoReverse),
            "require_confirmation" => Some(ConflictPolicy::RequireConfirmation),
            "fail" => Some(ConflictPolicy::Fail),
            _ => None,
        }
    }
}

/// Persisted compensation record. Mirrors V1.1 §8.1 `compensations` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensationRecord {
    pub comp_id: String,
    pub step_id: String,
    pub level: CompensationLevel,
    /// 加密快照 blob。W9 Plan 2:stronghold feature 启用 + vault 解锁时存
    /// `bincode::serialize(EncryptedPayload)`;降级模式 / feature 未启用时为 None。
    /// spec §2.2 + §6.1。
    pub snapshot_encrypted: Option<Vec<u8>>,
    pub ttl_expires: String, // RFC3339
    pub status: String,      // 'active' | 'consumed' | 'expired' | 'failed'
    /// W9 Plan 2:加密成功时为 UUID v4;降级模式为 "degraded";feature 未启用时为 None。
    pub snapshot_vault_ref: Option<String>,
    pub conflict_policy: ConflictPolicy,
    /// Function name to invoke for compensation (e.g. "filesystem.reverse_move").
    pub compensate_fn: String,
    /// JSON payload for the compensate_fn (e.g. reverse source/dest paths).
    /// W9 Plan 2:stronghold 启用 + 解锁时此字段为空字符串(明文不落盘,密文在 snapshot_encrypted);
    /// 降级 / feature 未启用时保留明文 JSON。
    pub reverse_payload: String,
}
