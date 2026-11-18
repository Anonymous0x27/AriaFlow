//! FTP / explicit FTPS transport (RFC 959 + AUTH TLS).
//!
//! Implemented with the `suppaftp` blocking client running on dedicated OS
//! threads (`tokio::task::spawn_blocking`), which keeps the async runtime free.
//!
//! Resume: classic `REST` offset before `RETR`. After the transfer, the byte
//! count is verified against a fresh `SIZE`; if the server silently ignored
//! `REST` (observed with some embedded FTP servers), the partial file is
//! truncated and retried once from offset 0.
//!
//! Limitations (documented in docs/BUILD.md):
//! - implicit FTPS (port 990) is not supported; use explicit AUTH TLS,
//! - active mode is not enabled; passive mode is used,
//! - FTP has no integrity checksums; use `ariaflow add --header` / HTTP for that.

use crate::throttle::resolve_limiter;
use ariaflow_protocol::{
    DownloadProvider, ProviderError, ProviderOptions, RemoteFileInfo, TransferOutcome,
    TransferProgress,
};
use async_trait::async_trait;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suppaftp::types::FileType;
use suppaftp::{FtpResult, FtpStream, RustlsFtpStream};

pub struct FtpProvider;

impl FtpProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FtpProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct FtpTarget {
    host: String,
    port: u16,
    username: String,
    password: String,
    /// Percent-decoded absolute remote path.
    path: String,
    /// `true` for `ftps://` (explicit AUTH TLS, default port 21).
    ftps: bool,
}

impl FtpTarget {
    fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn parse_url(raw: &str) -> Result<FtpTarget, ProviderError> {
    let url = url::Url::parse(raw)
        .map_err(|e| ProviderError::Unsupported(format!("invalid URL: {e}")))?;
    let scheme = url.scheme().to_ascii_lowercase();
    let ftps = match scheme.as_str() {
        "ftp" => false,
        "ftps" => true,
        other => {
            return Err(ProviderError::Unsupported(format!(
                "FTP provider cannot handle scheme {other:?} (expected ftp:// or ftps://)"
            )))
        }
    };
    let host = url
        .host_str()
        .ok_or_else(|| ProviderError::Unsupported("FTP URL missing host".into()))?
        .to_string();
    let port = url.port().unwrap_or(21);
    let username = if url.username().is_empty() {
        "anonymous".to_string()
    } else {
        percent_decode(url.username())
    };
    let password = match url.password() {
        Some(p) => percent_decode(p),
        None if username == "anonymous" => "anonymous".to_string(),
        None => String::new(),
    };
    let path = percent_decode(url.path());
    if path.is_empty() || path == "/" {
        return Err(ProviderError::Unsupported(
            "FTP url must point to a file, not the root directory".into(),
        ));
    }
    Ok(FtpTarget {
        host,
        port,
        username,
        password,
        path,
        ftps,
    })
}

fn basename(path: &str) -> Option<String> {
    path.rsplit('/')
        .find(|seg| !seg.is_empty())
        .map(ariaflow_core::filename::sanitize_filename)
}

enum Session {
    Plain(FtpStream),
    Secure(RustlsFtpStream),
}

macro_rules! session_call {
    ($session:expr, $method:ident( $($arg:expr),* )) => {{
        match &mut *$session {
            Session::Plain(ftp) => ftp.$method($($arg),*),
            Session::Secure(ftp) => ftp.$method($($arg),*),
        }
    }};
}

impl Session {
    fn transfer_type(&mut self, t: FileType) -> FtpResult<()> {
        session_call!(self, transfer_type(t))
    }
    fn size(&mut self, path: &str) -> FtpResult<usize> {
        session_call!(self, size(path))
    }
    fn mdtm(&mut self, path: &str) -> FtpResult<chrono::NaiveDateTime> {
        session_call!(self, mdtm(path))
    }
    fn resume_transfer(&mut self, offset: usize) -> FtpResult<()> {
        session_call!(self, resume_transfer(offset))
    }
    fn retr<F, D>(&mut self, path: &str, read: F) -> FtpResult<D>
    where
        F: FnMut(&mut dyn std::io::Read) -> FtpResult<D>,
    {
        match self {
            Session::Plain(ftp) => ftp.retr(path, read),
            Session::Secure(ftp) => ftp.retr(path, read),
        }
    }
    fn quit(mut self) {
        match &mut self {
            Session::Plain(ftp) => {
                let _ = ftp.quit();
            }
            Session::Secure(ftp) => {
                let _ = ftp.quit();
            }
        }
    }
}

fn map_ftp_err(e: suppaftp::FtpError, op: &str) -> ProviderError {
    let msg = e.to_string();
    if msg.contains("530") || msg.to_ascii_lowercase().contains("not logged") {
        return ProviderError::Authentication(format!("{op}: {msg}"));
    }
    ProviderError::Network(format!("{op}: {msg}"))
}

/// Client configuration for explicit FTPS. Uses the embedded Mozilla roots by
/// default; with the explicit opt-in flag (`danger_accept_invalid_certs`) any
/// certificate is accepted, mirroring the HTTP provider's behavior.
fn ftps_client_config(danger_accept_invalid_certs: bool) -> suppaftp::rustls::ClientConfig {
    let provider = Arc::new(suppaftp::rustls::crypto::ring::default_provider());
    let builder = suppaftp::rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("rustls safe protocol versions are always supported");
    if danger_accept_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert))
            .with_no_client_auth()
    } else {
        let roots = suppaftp::rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    }
}

/// `danger_accept_invalid_certs` verifier: accepts every certificate. Only
/// used when the user explicitly opted in for this download.
#[derive(Debug)]
struct AcceptAnyServerCert;

impl suppaftp::rustls::client::danger::ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &suppaftp::rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[suppaftp::rustls::pki_types::CertificateDer<'_>],
        _server_name: &suppaftp::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: suppaftp::rustls::pki_types::UnixTime,
    ) -> Result<suppaftp::rustls::client::danger::ServerCertVerified, suppaftp::rustls::Error> {
        Ok(suppaftp::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &suppaftp::rustls::pki_types::CertificateDer<'_>,
        _dss: &suppaftp::rustls::DigitallySignedStruct,
    ) -> Result<suppaftp::rustls::client::danger::HandshakeSignatureValid, suppaftp::rustls::Error>
    {
        Ok(suppaftp::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &suppaftp::rustls::pki_types::CertificateDer<'_>,
        _dss: &suppaftp::rustls::DigitallySignedStruct,
    ) -> Result<suppaftp::rustls::client::danger::HandshakeSignatureValid, suppaftp::rustls::Error>
    {
        Ok(suppaftp::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<suppaftp::rustls::SignatureScheme> {
        vec![
            suppaftp::rustls::SignatureScheme::RSA_PKCS1_SHA256,
            suppaftp::rustls::SignatureScheme::RSA_PKCS1_SHA384,
            suppaftp::rustls::SignatureScheme::RSA_PKCS1_SHA512,
            suppaftp::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            suppaftp::rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            suppaftp::rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            suppaftp::rustls::SignatureScheme::RSA_PSS_SHA256,
            suppaftp::rustls::SignatureScheme::RSA_PSS_SHA384,
            suppaftp::rustls::SignatureScheme::RSA_PSS_SHA512,
            suppaftp::rustls::SignatureScheme::ED25519,
        ]
    }
}

fn connect_and_login(
    target: &FtpTarget,
    options: &ProviderOptions,
) -> Result<Session, ProviderError> {
    let connect_timeout = Duration::from_secs(options.connect_timeout_secs.max(1));
    let addr = target
        .addr()
        .as_str()
        .to_socket_addrs()
        .map_err(|e| ProviderError::Network(format!("cannot resolve {}: {e}", target.addr())))?
        .next()
        .ok_or_else(|| ProviderError::Network(format!("cannot resolve {}", target.addr())))?;

    if target.ftps {
        let ftp = RustlsFtpStream::connect_timeout(addr, connect_timeout)
            .map_err(|e| map_ftp_err(e, "connect"))?;
        let config = ftps_client_config(options.danger_accept_invalid_certs);
        let ftp = ftp
            .into_secure(
                suppaftp::RustlsConnector::from(Arc::new(config)),
                &target.host,
            )
            .map_err(|e| map_ftp_err(e, "AUTH TLS"))?;
        let mut session = Session::Secure(ftp);
        session_login(&mut session, target)?;
        session
            .transfer_type(FileType::Binary)
            .map_err(|e| map_ftp_err(e, "TYPE I"))?;
        Ok(session)
    } else {
        let ftp = FtpStream::connect_timeout(addr, connect_timeout)
            .map_err(|e| map_ftp_err(e, "connect"))?;
        let mut session = Session::Plain(ftp);
        session_login(&mut session, target)?;
        session
            .transfer_type(FileType::Binary)
            .map_err(|e| map_ftp_err(e, "TYPE I"))?;
        Ok(session)
    }
}

fn session_login(session: &mut Session, target: &FtpTarget) -> Result<(), ProviderError> {
    let result = match session {
        Session::Plain(ftp) => ftp.login(&target.username, &target.password),
        Session::Secure(ftp) => ftp.login(&target.username, &target.password),
    };
    result.map_err(|e| map_ftp_err(e, "login"))
}

#[async_trait]
impl DownloadProvider for FtpProvider {
    fn schemes(&self) -> &'static [&'static str] {
        &["ftp", "ftps"]
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
        let (size, mdtm) = tokio::task::spawn_blocking(move || {
            let mut session = connect_and_login(&target, &options)?;
            let size = session.size(&target.path).ok().map(|s| s as u64);
            let mdtm = session.mdtm(&target.path).ok();
            session.quit();
            Ok::<_, ProviderError>((size, mdtm))
        })
        .await
        .map_err(|e| ProviderError::Network(format!("FTP probe task failed: {e}")))??;

        let last_modified = mdtm.map(|ndt| {
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc)
                .format("%a, %d %b %Y %H:%M:%S GMT")
                .to_string()
        });
        let etag = size.map(|s| format!("\"ftp-{s:x}\""));

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
            ftp_download(
                &target,
                &options,
                &dest_part,
                offset,
                progress,
                should_abort,
            )
        })
        .await
        .map_err(|e| ProviderError::Network(format!("FTP transfer task failed: {e}")))?
    }
}

fn ftp_download(
    target: &FtpTarget,
    options: &ProviderOptions,
    dest_part: &std::path::Path,
    offset: u64,
    progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
    should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<TransferOutcome, ProviderError> {
    let mut session = connect_and_login(target, options)?;
    let expected_total = session.size(&target.path).ok().map(|s| s as u64);

    // First attempt: resume at `offset`.
    match retr_into_file(
        &mut session,
        RetrRequest {
            path: &target.path,
            dest_part,
            offset,
            options,
            progress: &progress,
            should_abort: &should_abort,
            expected_total,
        },
    ) {
        Ok(outcome) => {
            session.quit();
            return Ok(outcome);
        }
        Err(ProviderError::Aborted) => {
            session.quit();
            return Err(ProviderError::Aborted);
        }
        Err(first_err) => {
            if offset == 0 {
                session.quit();
                return Err(first_err);
            }
            tracing::warn!(
                error = %first_err,
                "FTP resume failed; restarting transfer from byte 0"
            );
            // Fall through to a clean restart.
            let _ = std::fs::remove_file(dest_part);
        }
    }

    let outcome = retr_into_file(
        &mut session,
        RetrRequest {
            path: &target.path,
            dest_part,
            offset: 0,
            options,
            progress: &progress,
            should_abort: &should_abort,
            expected_total,
        },
    );
    session.quit();
    outcome
}

/// Arguments for one `RETR` into the partial file (grouped to satisfy the
/// seven-argument lint and keep call sites readable).
struct RetrRequest<'a> {
    path: &'a str,
    dest_part: &'a std::path::Path,
    offset: u64,
    options: &'a ProviderOptions,
    progress: &'a Arc<dyn Fn(TransferProgress) + Send + Sync>,
    should_abort: &'a Arc<dyn Fn() -> bool + Send + Sync>,
    expected_total: Option<u64>,
}

fn retr_into_file(
    session: &mut Session,
    request: RetrRequest<'_>,
) -> Result<TransferOutcome, ProviderError> {
    let RetrRequest {
        path,
        dest_part,
        offset,
        options,
        progress,
        should_abort,
        expected_total,
    } = request;
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

    if offset > 0 {
        session
            .resume_transfer(offset as usize)
            .map_err(|e| map_ftp_err(e, "REST"))?;
    }

    // Shared aggregate bucket when the daemon configured a global cap; else per-task bucket.
    // The Arc is cloned into the blocking thread.
    let limiter = resolve_limiter(options);
    let mut downloaded = offset;
    let mut last_emit = Instant::now();

    let retr_result = session.retr(path, |reader| {
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if should_abort() {
                return Err(suppaftp::FtpError::ConnectionError(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "aborted by user",
                )));
            }
            let n = reader
                .read(&mut buf)
                .map_err(suppaftp::FtpError::ConnectionError)?;
            if n == 0 {
                break;
            }
            limiter.acquire_blocking(n as u64);
            file.write_all(&buf[..n])
                .map_err(suppaftp::FtpError::ConnectionError)?;
            downloaded += n as u64;
            if last_emit.elapsed().as_millis() >= 250 {
                last_emit = Instant::now();
                progress(TransferProgress {
                    downloaded_bytes: downloaded,
                    total_bytes: expected_total,
                });
            }
        }
        Ok(())
    });

    match retr_result {
        Ok(()) => {}
        Err(e) => {
            // The reader signalled an abort — treat as Aborted, everything else as network.
            if e.to_string().contains("aborted by user") {
                return Err(ProviderError::Aborted);
            }
            return Err(map_ftp_err(e, "RETR"));
        }
    }

    // If the server ignored REST, we appended a from-0 stream; detect via SIZE.
    if offset > 0 {
        if let Some(expected) = expected_total {
            if downloaded == offset + expected {
                return Err(ProviderError::Network(
                    "server ignored REST offset".to_string(),
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
