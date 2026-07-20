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
}

pub type Result<T> = std::result::Result<T, KernelError>;
