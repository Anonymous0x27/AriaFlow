//! SFTP transport (SSH file transfer, protocol v3+).
//!
//! Implemented with the `ssh2` crate (libssh2) on a dedicated OS thread
//! (`tokio::task::spawn_blocking`). Authentication is attempted in this order:
//!   1. password embedded in the URL (`sftp://user:pass@host/path`),
//!   2. SSH agent (`SSH_AUTH_SOCK`),
//!   3. public key at `~/.ssh/id_ed25519` then `~/.ssh/id_rsa` (no passphrase).
//!
//! Resume: `seek` to the byte offset of the existing `.ariaflow-part` file
//! before reading. SFTP supports random access, so arbitrary-offset resume
//! always works.
//!
//! `ssh://` is accepted as an alias for `sftp://` (same transport), and when
//! the task selected a certificate (`ProviderOptions::ssh_certificate`) that
//! key is used instead of the password/agent/known-file chain.

use crate::throttle::resolve_limiter;
use ariaflow_protocol::{
    DownloadProvider, ProviderError, ProviderOptions, RemoteFileInfo, TransferOutcome,
    TransferProgress,
};
use async_trait::async_trait;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SftpProvider;

impl Default for SftpProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SftpProvider {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
struct SftpTarget {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    /// Percent-decoded absolute remote path.
    path: String,
}

fn parse_url(raw: &str) -> Result<SftpTarget, ProviderError> {
    let url = url::Url::parse(raw)
        .map_err(|e| ProviderError::Unsupported(format!("invalid URL: {e}")))?;
    let scheme = url.scheme();
    if !matches!(scheme, "sftp" | "ssh") {
        return Err(ProviderError::Unsupported(format!(
            "SFTP provider cannot handle scheme {scheme:?} (expected sftp:// or ssh://)"
        )));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ProviderError::Unsupported("SFTP URL missing host".into()))?
        .to_string();
    let port = url.port().unwrap_or(22);
    let username = if url.username().is_empty() {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "anonymous".to_string())
    } else {
        percent_decode(url.username())
    };
    let password = url.password().map(percent_decode);
    let path = percent_decode(url.path());
    if path.is_empty() || path == "/" {
        return Err(ProviderError::Unsupported(
            "SFTP url must point to a file, not the root directory".into(),
        ));
    }
    Ok(SftpTarget {
        host,
        port,
        username,
        password,
        path,
    })
}

fn basename(path: &str) -> Option<String> {
    path.rsplit('/')
        .find(|seg| !seg.is_empty())
        .map(ariaflow_core::filename::sanitize_filename)
}

fn connect_and_auth(
    target: &SftpTarget,
    options: &ProviderOptions,
) -> Result<ssh2::Session, ProviderError> {
    let addr = format!("{}:{}", target.host, target.port);
    let timeout = Duration::from_secs(options.connect_timeout_secs.max(1));
    let socket_addr: std::net::SocketAddr = addr
        .to_socket_addrs()
        .map_err(|e| ProviderError::Network(format!("cannot resolve {addr}: {e}")))?
        .next()
        .ok_or_else(|| ProviderError::Network(format!("cannot resolve {addr}")))?;
    let tcp = TcpStream::connect_timeout(&socket_addr, timeout)
        .map_err(|e| ProviderError::Network(format!("connect {addr}: {e}")))?;
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(options.read_timeout_secs.max(1))));

    let mut session = ssh2::Session::new()
        .map_err(|e| ProviderError::Network(format!("failed to initialize SSH session: {e}")))?;
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|e| ProviderError::Network(format!("SSH handshake failed: {e}")))?;

    // 0) certificate selected for this task — authoritative. On failure we
    //    do not silently fall back to other methods: mixing would mask a
    //    misconfigured certificate.
    if let Some(certificate) = &options.ssh_certificate {
        session
            .userauth_pubkey_memory(
                &target.username,
                None,
                &certificate.private_key,
                certificate.passphrase.as_deref(),
            )
            .map_err(|e| {
                ProviderError::Authentication(format!(
                    "certificate {:?} auth failed: {e}",
                    certificate.name
                ))
            })?;
        return Ok(session);
    }

    // 1) password auth (from URL)
    if let Some(password) = &target.password {
        session
            .userauth_password(&target.username, password)
            .map_err(|e| ProviderError::Authentication(format!("password auth failed: {e}")))?;
        return Ok(session);
    }

    // 2) SSH agent
    if session.userauth_agent(&target.username).is_ok() {
        return Ok(session);
    }

    // 3) public key files
    let home = dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    for key_name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
        let key_path = home.join(".ssh").join(key_name);
        if key_path.is_file()
            && session
                .userauth_pubkey_file(&target.username, None, &key_path, None)
                .is_ok()
        {
            return Ok(session);
        }
    }

    Err(ProviderError::Authentication(format!(
        "all auth methods failed for user {:?} (no URL password, agent unavailable, no usable key pair in ~/.ssh)",
        target.username
    )))
}

fn rfc2822_from_epoch(epoch_secs: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(epoch_secs as i64, 0)
        .map(|dt| dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
        .unwrap_or_default()
}

#[async_trait]
impl DownloadProvider for SftpProvider {
    fn schemes(&self) -> &'static [&'static str] {
        &["sftp", "ssh"]
    }

    fn supports_resume(&self) -> bool {
        true
    }

    async fn probe(
        &self,
        url: &str,
        options: &ProviderOptions,
    ) -> Result<RemoteFileInfo, ProviderError> {
        let target = parse_url(url)?;
        let options = options.clone();
        let (size, mtime) = tokio::task::spawn_blocking(move || {
            let session = connect_and_auth(&target, &options)?;
            let sftp = session
                .sftp()
                .map_err(|e| ProviderError::Network(format!("SFTP subsystem failed: {e}")))?;
            let stat = sftp
                .stat(std::path::Path::new(&target.path))
                .map_err(|e| ProviderError::Network(format!("stat {}: {e}", target.path)))?;
            Ok::<_, ProviderError>((stat.size, stat.mtime))
        })
        .await
        .map_err(|e| ProviderError::Network(format!("SFTP probe task failed: {e}")))??;

        let last_modified = mtime.map(rfc2822_from_epoch);
        let etag = size.map(|s| format!("\"sftp-{s:x}\""));

        Ok(RemoteFileInfo {
            final_url: url.to_string(),
            total_size: size,
            accept_ranges: true,
            etag,
            last_modified,
            content_type: None,
            filename_hint: basename(&parse_url(url)?.path),
            headers: vec![],
        })
    }

    async fn download(
        &self,
        url: &str,
        options: &ProviderOptions,
        dest_part: &std::path::Path,
        progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<TransferOutcome, ProviderError> {
        let target = parse_url(url)?;
        let options = options.clone();
        let dest_part = dest_part.to_path_buf();
        let offset = tokio::fs::metadata(&dest_part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        tokio::task::spawn_blocking(move || {
            sftp_download(
                &target,
                &options,
                &dest_part,
                offset,
                progress,
                should_abort,
            )
        })
        .await
        .map_err(|e| ProviderError::Network(format!("SFTP transfer task failed: {e}")))?
    }
}

fn sftp_download(
    target: &SftpTarget,
    options: &ProviderOptions,
    dest_part: &std::path::Path,
    offset: u64,
    progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
    should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<TransferOutcome, ProviderError> {
    let session = connect_and_auth(target, options)?;
    let sftp = session
        .sftp()
        .map_err(|e| ProviderError::Network(format!("SFTP subsystem failed: {e}")))?;
    let expected_total = sftp
        .stat(std::path::Path::new(&target.path))
        .ok()
        .and_then(|s| s.size);

    let mut remote = sftp
        .open(&target.path)
        .map_err(|e| ProviderError::Network(format!("open {}: {e}", target.path)))?;
    if offset > 0 {
        remote
            .seek(SeekFrom::Start(offset))
            .map_err(ProviderError::Io)?;
    }

    let mut file = if offset > 0 {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dest_part)
    } else {
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(dest_part)
    }
    .map_err(ProviderError::Io)?;

    let limiter = resolve_limiter(options);
    let mut downloaded = offset;
    let mut last_emit = Instant::now();
    let mut buf = vec![0u8; 256 * 1024];

    loop {
        if should_abort() {
            let _ = file.flush();
            return Err(ProviderError::Aborted);
        }
        let n = remote.read(&mut buf).map_err(ProviderError::Io)?;
        if n == 0 {
            break;
        }
        limiter.acquire_blocking(n as u64);
        file.write_all(&buf[..n]).map_err(ProviderError::Io)?;
        downloaded += n as u64;
        if last_emit.elapsed().as_millis() >= 250 {
            last_emit = Instant::now();
            progress(TransferProgress {
                downloaded_bytes: downloaded,
                total_bytes: expected_total,
            });
        }
    }
    file.flush().map_err(ProviderError::Io)?;

    // Corruption guard: appended bytes must line up with the server's size.
    if offset > 0 {
        if let Some(expected) = expected_total {
            if downloaded > expected {
                return Err(ProviderError::Network(
                    "SFTP resume produced more bytes than the remote file size".into(),
                ));
            }
        }
    }

    progress(TransferProgress {
        downloaded_bytes: downloaded,
        total_bytes: expected_total,
    });
    Ok(TransferOutcome {
        bytes_written: downloaded,
        resumed_from: offset,
        segment_snapshot: None,
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 {
            let h = bytes.get(i + 1).copied().and_then(hex_val);
            let l = bytes.get(i + 2).copied().and_then(hex_val);
            if let (Some(h), Some(l)) = (h, l) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
