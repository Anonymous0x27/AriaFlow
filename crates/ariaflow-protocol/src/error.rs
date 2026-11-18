//! Provider-scoped error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(String),

    #[error("timeout after {0}s")]
    Timeout(u64),

    #[error("unexpected HTTP status {status}: {message}")]
    BadStatus { status: u16, message: String },

    #[error("server does not support range requests")]
    RangeNotSupported,

    #[error("invalid content range: {0}")]
    InvalidContentRange(String),

    #[error("size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },

    #[error("remote file changed since download started (etag/last-modified mismatch)")]
    RemoteFileChanged,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("authentication failed: {0}")]
    Authentication(String),

    #[error("transfer aborted by orchestrator")]
    Aborted,
}
