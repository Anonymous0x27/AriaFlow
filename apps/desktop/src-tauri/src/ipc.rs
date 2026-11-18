//! HTTP-over-local-IPC client and daemon auto-start helpers.
//!
//! The desktop shell talks to `ariaflowd` over the platform's native local
//! channel instead of TCP:
//!
//! - **Unix (Linux/macOS)**: HTTP/1.1 over a Unix domain socket
//!   (mode 0600, owned by the user — no TCP port involved).
//! - **Windows**: HTTP/1.1 over a named pipe (`\\.\pipe\ariaflowd`).
//!
//! The protocol on the wire is identical to the web API: same routes, same
//! JSON bodies, same bearer-token semantics (a token is only attached when
//! one is configured, exactly like the web client). The only limitation is
//! streaming: SSE endpoints answer with an error over IPC (the UI polls, so
//! nothing here needs it).

use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Endpoint resolution: explicit config value wins, otherwise the platform
/// default (mirrors `ariaflow-core::paths::default_ipc_path`).
pub fn resolve_endpoint(explicit: Option<&str>) -> String {
    match explicit {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => default_ipc_path(),
    }
}

#[cfg(unix)]
pub fn default_ipc_path() -> String {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime)
            .join("ariaflowd.sock")
            .to_string_lossy()
            .into_owned();
    }
    dirs_fallback_data().join("ariaflowd.sock").to_string_lossy().into_owned()
}

#[cfg(not(unix))]
pub fn default_ipc_path() -> String {
    r"\\.\pipe\ariaflowd".to_string()
}

#[cfg(unix)]
fn dirs_fallback_data() -> PathBuf {
    if let Some(home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(home).join("ariaflow");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local").join("share").join("ariaflow");
    }
    PathBuf::from("./ariaflow-data")
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

/// Distinguishes "could not connect" (safe to fall back to web API) from
/// "got a response/parse failure" (do not retry — the request may have run).
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransportError {
    Connect { message: String },
    Protocol { message: String },
}

impl TransportError {
    fn connect(e: impl std::fmt::Display) -> Self {
        TransportError::Connect { message: e.to_string() }
    }
    pub(crate) fn protocol(e: impl std::fmt::Display) -> Self {
        TransportError::Protocol { message: e.to_string() }
    }
}

#[cfg(unix)]
async fn connect(endpoint: &str) -> Result<tokio::net::UnixStream, TransportError> {
    tokio::net::UnixStream::connect(endpoint)
        .await
        .map_err(TransportError::connect)
}

#[cfg(windows)]
async fn connect(endpoint: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, TransportError> {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint)
        .map_err(TransportError::connect)
}

/// One HTTP/1.1 request/response round-trip over the pipe.
///
/// `Connection: close` lets us read the response body until EOF without
/// parsing Content-Length bookkeeping (bodies are JSON for every endpoint
/// except SSE, which we explicitly reject).
pub(crate) async fn ipc_roundtrip(
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    token: Option<&str>,
    endpoint: &str,
) -> Result<IpcResponse, TransportError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = connect(endpoint).await?;

    let mut head = String::new();
    head.push_str(&format!("{method} {path} HTTP/1.1\r\n"));
    head.push_str("Host: localhost\r\n");
    head.push_str("Accept: application/json\r\n");
    if body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.map(|b| b.len()).unwrap_or(0)));
    }
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(TransportError::protocol)?;
    if let Some(body) = body {
        stream.write_all(body).await.map_err(TransportError::protocol)?;
    }

    let mut buf = Vec::with_capacity(16 * 1024);
    stream.read_to_end(&mut buf).await.map_err(TransportError::protocol)?;

    let split = find_header_end(&buf).ok_or_else(|| {
        TransportError::protocol("truncated IPC response (no header terminator)")
    })?;
    let head_bytes = &buf[..split];
    let body_bytes = &buf[split + 4..];
    let head_text = String::from_utf8_lossy(head_bytes);

    let mut lines = head_text.lines();
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| TransportError::protocol(format!("bad status line: {status_line:?}")))?;

    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                return Err(TransportError::protocol(
                    "streaming (chunked) responses are not supported over IPC",
                ));
            }
        }
    }

    let body = if body_bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice::<serde_json::Value>(body_bytes)
            .map_err(|e| TransportError::protocol(format!("invalid JSON from daemon: {e}")))?
    };
    Ok(IpcResponse { status, body })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

// ---------------------------------------------------------------------------
// Daemon probing & auto-start
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonCheck {
    /// "running" | "started" | "tcp-only" | "not-found" | "start-failed"
    pub state: String,
    pub endpoint: String,
    pub detail: String,
    pub version: Option<String>,
}

/// Quick liveness probe over IPC. Returns the daemon version on success.
async fn probe_ipc(endpoint: &str) -> Result<Option<String>, TransportError> {
    let resp = ipc_roundtrip("GET", "/api/v1/status", None, None, endpoint).await?;
    if resp.status != 200 {
        return Err(TransportError::protocol(format!(
            "unexpected status {} from /api/v1/status",
            resp.status
        )));
    }
    Ok(resp
        .body
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string))
}

fn probe_tcp(base: &str) -> bool {
    let authority = base
        .strip_prefix("http://").or_else(|| base.strip_prefix("https://"))
        .unwrap_or(base)
        .trim_end_matches('/');
    let authority = authority.split('/').next().unwrap_or(authority);
    let addr = match authority.rfind(':') {
        Some(_) => authority.to_string(),
        None if base.starts_with("https://") => format!("{authority}:443"),
        None => format!("{authority}:80"),
    };
    let Ok(sock) = addr.parse::<std::net::SocketAddr>() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&sock, Duration::from_millis(600)).is_ok()
}

/// Locate an `ariaflowd` binary: explicit PATH search plus common install
/// locations. Returns the first existing candidate.
pub fn find_ariaflowd() -> Option<String> {
    let exe = if cfg!(windows) { "ariaflowd.exe" } else { "ariaflowd" };

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(exe));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("target").join("release").join(exe));
        candidates.push(cwd.join("target").join("debug").join(exe));
    }
    #[cfg(unix)]
    {
        candidates.push(PathBuf::from("/usr/local/bin").join(exe));
        candidates.push(PathBuf::from("/usr/bin").join(exe));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.push(home.join(".local").join("bin").join(exe));
            candidates.push(home.join(".cargo").join("bin").join(exe));
        }
    }
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("ariaflow").join(exe));
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            candidates.push(PathBuf::from(profile).join(".cargo").join("bin").join(exe));
        }
    }

    candidates
        .into_iter()
        .find(|c| c.is_file())
        .map(|c| c.to_string_lossy().into_owned())
}

fn log_file_path() -> PathBuf {
    std::env::temp_dir().join("ariaflowd.log")
}

/// Spawn `ariaflowd --ipc [path]` detached. Returns an error string when the
/// binary cannot be spawned at all.
fn spawn_daemon(bin: &str, endpoint: &str, token: Option<&str>) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(bin);
    // Use the resolved endpoint's path on Unix; on Windows pass it as-is.
    cmd.arg("--ipc").arg(endpoint);
    // Keep the web API on the default port as well: CLI, MCP and the browser
    // extension all talk to it. Users who want pipe-only can run the daemon
    // themselves with --no-api.
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        cmd.arg("--token").arg(token);
    }
    let log = log_file_path();
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
        cmd.stdout(std::process::Stdio::from(file.try_clone()?));
        cmd.stderr(std::process::Stdio::from(file));
    } else {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    }
    cmd.stdin(std::process::Stdio::null());
    // Do not kill the daemon when the desktop exits: downloads must outlive the window.
    cmd.spawn().map(|_| ())
}

/// Probe only — never spawns anything. Used by the settings page to display
/// the current backend state.
pub async fn daemon_status(ipc_path: Option<&str>, webapi_base: Option<&str>) -> DaemonCheck {
    let endpoint = resolve_endpoint(ipc_path);

    if let Ok(version) = probe_ipc(&endpoint).await {
        return DaemonCheck {
            state: "running".into(),
            endpoint,
            detail: "ariaflowd is already listening on the local pipe".into(),
            version,
        };
    }

    if let Some(base) = webapi_base.filter(|b| !b.trim().is_empty()) {
        if probe_tcp(base) {
            return DaemonCheck {
                state: "tcp-only".into(),
                endpoint,
                detail: format!(
                    "ariaflowd is reachable at {base} but not on the local pipe; restart it with --ipc (or set ARIAFLOW_IPC) to use the pipe transport"
                ),
                version: None,
            };
        }
    }

    DaemonCheck {
        state: "not-running".into(),
        endpoint,
        detail: "ariaflowd is not running; it is started automatically at app launch, or via the Start button below".into(),
        version: None,
    }
}

/// Ensure the daemon is running; start it when it is not.
///
/// Order of operations (all local, no network):
/// 1. probe the IPC pipe — healthy daemon → report `running`
///    (or `tcp-only` when only the web API answers — never spawn a second)
/// 2. resolve the binary (explicit path → PATH/common locations) and spawn it
/// 3. poll the pipe for up to ~10s; report `started` or `start-failed`
pub async fn ensure_daemon(
    binary_path: Option<&str>,
    ipc_path: Option<&str>,
    token: Option<&str>,
    webapi_base: Option<&str>,
) -> DaemonCheck {
    let endpoint = resolve_endpoint(ipc_path);
    let probe = daemon_status(ipc_path, webapi_base).await;
    if probe.state != "not-running" {
        return probe;
    }

    let bin = match binary_path.filter(|p| !p.trim().is_empty()).map(str::to_string) {
        Some(p) => {
            if PathBuf::from(&p).is_file() {
                p
            } else {
                return DaemonCheck {
                    state: "not-found".into(),
                    endpoint,
                    detail: format!("ariaflowd binary not found at {p}"),
                    version: None,
                };
            }
        }
        None => match find_ariaflowd() {
            Some(p) => p,
            None => {
                return DaemonCheck {
                    state: "not-found".into(),
                    endpoint,
                    detail: "ariaflowd not found on PATH or common install locations; set its path in Settings → Daemon".into(),
                    version: None,
                }
            }
        },
    };

    if let Err(e) = spawn_daemon(&bin, &endpoint, token) {
        return DaemonCheck {
            state: "start-failed".into(),
            endpoint,
            detail: format!("failed to launch {bin}: {e}"),
            version: None,
        };
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if let Ok(version) = probe_ipc(&endpoint).await {
            return DaemonCheck {
                state: "started".into(),
                endpoint,
                detail: format!("ariaflowd started from {bin} (logs: {})", log_file_path().display()),
                version,
            };
        }
        if Instant::now() >= deadline {
            return DaemonCheck {
                state: "start-failed".into(),
                endpoint,
                detail: format!(
                    "ariaflowd did not come up within 10s; see {} for details",
                    log_file_path().display()
                ),
                version: None,
            };
        }
    }
}
