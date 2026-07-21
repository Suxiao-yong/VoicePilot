use thiserror::Error;

#[derive(Debug, Error)]
pub enum UiError {
    #[error("kernel error: {0}")]
    Kernel(#[from] trust_kernel::error::KernelError),
    #[error("tauri error: {0}")]
    Tauri(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("approval timed out")]
    ApprovalTimeout,
    #[error("approval request not found: {0}")]
    ApprovalNotFound(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

impl From<UiError> for String {
    fn from(e: UiError) -> String {
        e.to_string()
    }
}

pub type UiResult<T> = Result<T, UiError>;
