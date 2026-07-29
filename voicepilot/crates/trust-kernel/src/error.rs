use thiserror::Error;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid state transition: from={from:?} to={to:?}")]
    InvalidTransition { from: crate::state::TaskState, to: crate::state::TaskState },
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cedar policy parse error: {0}")]
    CedarParse(String),
    #[error("cedar authorization error: {0}")]
    CedarAuthz(String),
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("constraint violation: {0}")]
    ConstraintViolation(String),
    #[error("transaction precondition mismatch: expected={expected} actual={actual}")]
    PreconditionMismatch { expected: String, actual: String },
    #[error("prepare token not found or expired: {0}")]
    InvalidPrepareToken(String),
    /// Reserved for W4 approval flow (UI-driven confirmation).
    #[error("approval required but not granted")]
    ApprovalRequired,
    #[error("filesystem error: {0}")]
    Filesystem(String),
    #[error("compensation error: {0}")]
    Compensation(String),
    #[error("verification failed: {message}")]
    Verification { message: String },
    #[error("approval error: {0}")]
    Approval(String),
    #[error("skill error: {0}")]
    Skill(String),
    #[error("mcp error: {0}")]
    Mcp(String),
    #[error("path not allowed: {0}")]
    PathNotAllowed(String),
    /// W7 Plan 4: Windows UIA automation error (wraps `uiautomation::Error`).
    #[error("uia error: {0}")]
    Uia(String),
    /// W9 Plan 1: Stronghold 加密错误(包装 StrongholdError)。
    /// 用字符串而非 #[from]:StrongholdError 在 feature 关闭时不存在,
    /// 但 KernelError 必须在所有 feature 组合下都编译通过。
    #[cfg(feature = "stronghold")]
    #[error("stronghold error: {0}")]
    Stronghold(#[from] crate::crypto::stronghold::StrongholdError),
    /// W9 Plan 1: privacy_mode=true 但 Stronghold vault 未解锁 / 未注入。
    /// 防止高隐私模式下 reverse_payload 明文落盘(spec §2.1 privacy_mode 联动)。
    #[error("stronghold vault required: privacy_mode is true but vault not unlocked")]
    StrongholdRequired,
}

pub type Result<T> = std::result::Result<T, KernelError>;
