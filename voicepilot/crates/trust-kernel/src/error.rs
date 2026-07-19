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
}

pub type Result<T> = std::result::Result<T, KernelError>;
