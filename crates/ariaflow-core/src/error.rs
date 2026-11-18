//! Unified error type for the AriaFlow workspace.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AriaFlowError>;

#[derive(Debug, Error)]
pub enum AriaFlowError {
    #[error("download not found: {0}")]
    NotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("invalid state transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },

    #[error("storage error: {0}")]
    Storage(String),

    #[error("engine error: {0}")]
    Engine(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("verification failed: {0}")]
    Verification(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("unauthorized")]
    Unauthorized,

    #[error("rate limited")]
    RateLimited,

    #[error("internal error: {0}")]
    Internal(String),
}

impl AriaFlowError {
    /// Stable machine-readable error code, used by the CLI/API/MCP layers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::InvalidRequest(_) => "invalid_request",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::Storage(_) => "storage",
            Self::Engine(_) => "engine",
            Self::Protocol(_) => "protocol",
            Self::Network(_) => "network",
            Self::Verification(_) => "verification",
            Self::Io(_) => "io",
            Self::Serde(_) => "serde",
            Self::Url(_) => "url",
            Self::Unauthorized => "unauthorized",
            Self::RateLimited => "rate_limited",
            Self::Internal(_) => "internal",
        }
    }
}
