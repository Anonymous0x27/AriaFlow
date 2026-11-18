//! HTTP/1.1-over-local-IPC client for ariaflowd.
//!
//! The wire protocol is identical to the Web API (same routes, same JSON
//! bodies, same bearer-token semantics); the only difference is the channel:
//!
//! - **Unix (Linux/macOS)**: a Unix domain socket (mode 0600).
//! - **Windows**: a named pipe (`\\.\pipe\ariaflowd`).
//!
//! This mirrors `ServeIpc` on the server side and lets every first-party
//! client (CLI, TUI, MCP) talk to the daemon without touching TCP. Streaming
//! endpoints answer with an error over IPC — the CLI/TUI poll, so nothing
//! here needs them.

use std::time::Duration;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Distinguishes "never reached the daemon" (safe to fall back to the Web
/// API) from "the daemon answered / something broke mid-request" (do not
/// retry — the request may have run).
#[derive(Debug)]
pub enum IpcError {
    Connect(String),
    Protocol(String),
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::Connect(m) => write!(f, "cannot connect to {m}"),
            IpcError::Protocol(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for IpcError {}

/// Platform-default pipe endpoint, mirroring `ariaflow-core::paths::default_ipc_path`.
pub fn default_endpoint() -> String {
    ariaflow_core::paths::default_ipc_path()
}

#[cfg(unix)]
async fn connect(endpoint: &str) -> Result<tokio::net::UnixStream, IpcError> {
    tokio::net::UnixStream::connect(endpoint)
        .await
        .map_err(|e| IpcError::Connect(format!("{endpoint}: {e}")))
}

#[cfg(windows)]
async fn connect(
    endpoint: &str,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, IpcError> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint)
        .map_err(|e| IpcError::Connect(format!("{endpoint}: {e}")))
}

/// One HTTP/1.1 request/response round-trip over the pipe.
///
/// `Connection: close` lets us read until EOF without Content-Length
/// bookkeeping; every endpoint answers with a Content-Length body anyway
/// (SSE is rejected by the server-side guard for chunked encoding).
pub async fn request(
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    token: Option<&str>,
    endpoint: &str,
    timeout: Duration,
) -> Result<IpcResponse, IpcError> {
    tokio::time::timeout(timeout, request_inner(method, path, body, token, endpoint))
        .await
        .map_err(|_| {
            IpcError::Protocol(format!(
                "timed out after {timeout:?} waiting for {endpoint}"
            ))
        })?
}

async fn request_inner(
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    token: Option<&str>,
    endpoint: &str,
) -> Result<IpcResponse, IpcError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = connect(endpoint).await?;

    let mut head = String::new();
    head.push_str(&format!("{method} {path} HTTP/1.1\r\n"));
    head.push_str("Host: localhost\r\n");
    head.push_str("Accept: application/json\r\n");
    if let Some(body) = body {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|e| IpcError::Protocol(format!("write to {endpoint} failed: {e}")))?;
    if let Some(body) = body {
        stream
            .write_all(body)
            .await
            .map_err(|e| IpcError::Protocol(format!("write to {endpoint} failed: {e}")))?;
    }

    let mut buf = Vec::with_capacity(16 * 1024);
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| IpcError::Protocol(format!("read from {endpoint} failed: {e}")))?;

    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| {
            IpcError::Protocol("truncated IPC response (no header terminator)".into())
        })?;
    let head_bytes = &buf[..split];
    let body = &buf[split + 4..];
    let head_text = String::from_utf8_lossy(head_bytes);

    let mut lines = head_text.lines();
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| IpcError::Protocol(format!("bad status line: {status_line:?}")))?;

    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                return Err(IpcError::Protocol(
                    "streaming (chunked) responses are not supported over IPC".into(),
                ));
            }
        }
    }

    Ok(IpcResponse {
        status,
        body: body.to_vec(),
    })
}
