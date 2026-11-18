//! # AriaFlow Protocol
//!
//! Protocol-agnostic provider abstraction. HTTP/HTTPS, FTP/FTPS, SFTP,
//! BitTorrent (.torrent / magnet) and Metalink are all implemented behind the
//! same [`DownloadProvider`] interface without touching the core.
//!
//! Design rules:
//! - Providers never write to the database directly; they report through callback handles.
//! - Providers never panic on remote failure; they return [`ProviderError`].

use ariaflow_core::{ProxyConfig, Result as CoreResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub mod error;
pub mod throttle;
pub use error::ProviderError;

/// Wire protocols AriaFlow knows how to talk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    Https,
    Ftp,
    Ftps,
    Sftp,
    BitTorrent,
    Metalink,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Ftp => "ftp",
            Self::Ftps => "ftps",
            Self::Sftp => "sftp",
            Self::BitTorrent => "bittorrent",
            Self::Metalink => "metalink",
        }
    }

    /// Detects the protocol from a URL. Returns `None` for unknown/unsupported schemes.
    pub fn from_url(url: &url::Url) -> Option<Self> {
        let scheme = url.scheme();
        // http(s) URLs ending in `.torrent` are BitTorrent tasks (the manifest
        // fetch + metainfo resolution happens inside the torrent provider).
        let is_torrent_file = matches!(scheme, "http" | "https")
            && url.path().to_ascii_lowercase().ends_with(".torrent");
        match scheme {
            "http" => Some(if is_torrent_file {
                Self::BitTorrent
            } else {
                Self::Http
            }),
            "https" => Some(if is_torrent_file {
                Self::BitTorrent
            } else {
                Self::Https
            }),
            "ftp" => Some(Self::Ftp),
            "ftps" => Some(Self::Ftps),
            "sftp" | "ssh" => Some(Self::Sftp),
            "magnet" => Some(Self::BitTorrent),
            "metalink" => Some(Self::Metalink),
            _ => None,
        }
    }
}

/// Metadata about a remote resource, gathered before (and revalidated during) download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileInfo {
    /// URL after following redirects.
    pub final_url: String,
    pub total_size: Option<u64>,
    /// Whether the server honours `Range` requests.
    pub accept_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    /// Filename suggested by `Content-Disposition`, if any.
    pub filename_hint: Option<String>,
    /// Raw response headers (lower-cased names) for checksum discovery.
    pub headers: Vec<(String, String)>,
}

/// Private key material for ssh/sftp certificate authentication.
///
/// Resolved by the daemon from the task's `ssh_certificate_id` right before a
/// transfer starts and handed to the provider in memory. Never persisted,
/// never serialized, never sent back over the API.
#[derive(Debug, Clone)]
pub struct SshCertificateAuth {
    /// Certificate display name (used in error messages only).
    pub name: String,
    /// OpenSSH PEM private key.
    pub private_key: String,
    /// Passphrase for an encrypted key, if any.
    pub passphrase: Option<String>,
}

/// Per-task transport options handed to a provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOptions {
    pub headers: Vec<(String, String)>,
    pub proxy: Option<ProxyConfig>,
    pub user_agent: Option<String>,
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
    /// Overrides TLS certificate validation only when explicitly requested by the user.
    pub danger_accept_invalid_certs: bool,
    /// Additional root CA PEM bundles.
    pub extra_ca_certs: Vec<String>,
    /// Per-transfer cap in bytes/second. `0` means unlimited.
    #[serde(default)]
    pub rate_limit_bytes_per_sec: u64,
    /// Maximum connections for segmented HTTP downloads. `0` or `1` = single stream.
    #[serde(default)]
    pub max_connections: u16,
    /// Files at least this large may be split into segments.
    #[serde(default)]
    pub min_segment_size: u64,
    /// Private key resolved from the task's certificate selection. Skipped in
    /// serde so it can never be written into a payload by accident.
    #[serde(skip)]
    pub ssh_certificate: Option<SshCertificateAuth>,
    /// Shared aggregate limiter set by the daemon when a *global* speed cap is
    /// configured. Never serialized; deserializes to `None`. When set it paces
    /// the aggregate of every task using it (in addition to any per-task cap).
    #[serde(skip)]
    pub shared_limiter: Option<std::sync::Arc<crate::throttle::RateLimiter>>,
}

/// Progress snapshot delivered to [`DownloadProvider::download`] callers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// Successful outcome of a byte transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutcome {
    pub bytes_written: u64,
    /// Non-zero when the transfer resumed from a pre-existing `.ariaflow-part` file.
    pub resumed_from: u64,
    /// Final per-segment state, present when a segmented transfer just
    /// completed. Callers persist it so the detail view can show the chunks.
    #[serde(default)]
    pub segment_snapshot: Option<Vec<ariaflow_core::models::SegmentInfo>>,
}

/// Metadata probe used by the scheduler to decide segmentation and resume validity.
#[async_trait]
pub trait DownloadProvider: Send + Sync {
    /// Scheme prefixes this provider handles, e.g. `["http", "https"]`.
    fn schemes(&self) -> &'static [&'static str];

    /// Fetch metadata for `url` (typically a `HEAD` request or equivalent).
    async fn probe(
        &self,
        url: &str,
        options: &ProviderOptions,
    ) -> std::result::Result<RemoteFileInfo, ProviderError>;

    /// Whether the provider can resume partial downloads for this resource.
    fn supports_resume(&self) -> bool {
        true
    }

    /// Perform the actual byte transfer to `dest_part` (a temporary path; the
    /// orchestrator renames it to the final target once the checksum verifies).
    ///
    /// The transfer must:
    /// - append to `dest_part` when it already contains a resumable prefix
    ///   (providers without resume support must truncate and restart instead),
    /// - invoke `progress` periodically with cumulative byte counts,
    /// - observe `should_abort` between chunks and stop early, returning
    ///   `ProviderError::Aborted`, leaving the partial file on disk for resume.
    async fn download(
        &self,
        url: &str,
        options: &ProviderOptions,
        dest_part: &std::path::Path,
        progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> std::result::Result<TransferOutcome, ProviderError>;
}

/// Registry of providers keyed by scheme. Lookups are cheap and clone-free.
pub struct ProviderRegistry {
    providers: HashMap<&'static str, std::sync::Arc<dyn DownloadProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: std::sync::Arc<dyn DownloadProvider>) {
        for scheme in provider.schemes() {
            self.providers.insert(scheme, provider.clone());
        }
    }

    pub fn provider_for(&self, url: &str) -> CoreResult<std::sync::Arc<dyn DownloadProvider>> {
        let parsed = url::Url::parse(url)?;
        let scheme = parsed.scheme().to_ascii_lowercase();
        // http(s) URLs whose path ends in `.torrent` are routed to the
        // BitTorrent provider: librqbit fetches the manifest and resolves the
        // metainfo from it, then downloads the payload pieces. Registered
        // under "magnet" (the TorrentProvider).
        if matches!(scheme.as_str(), "http" | "https")
            && parsed.path().to_ascii_lowercase().ends_with(".torrent")
        {
            if let Some(torrent) = self.providers.get("magnet") {
                return Ok(torrent.clone());
            }
        }
        self.providers.get(scheme.as_str()).cloned().ok_or_else(|| {
            ariaflow_core::AriaFlowError::Protocol(format!(
                "no provider registered for scheme: {scheme}"
            ))
        })
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DownloadProvider for ProviderRegistry {
    fn schemes(&self) -> &'static [&'static str] {
        // All schemes any built-in provider may register.
        &[
            "http",
            "https",
            "ftp",
            "ftps",
            "sftp",
            "ssh",
            "bittorrent",
            "magnet",
            "metalink",
        ]
    }

    async fn probe(
        &self,
        url: &str,
        options: &ProviderOptions,
    ) -> std::result::Result<RemoteFileInfo, ProviderError> {
        let provider = self
            .provider_for(url)
            .map_err(|e| ProviderError::Unsupported(e.to_string()))?;
        provider.probe(url, options).await
    }

    async fn download(
        &self,
        url: &str,
        options: &ProviderOptions,
        dest_part: &std::path::Path,
        progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> std::result::Result<TransferOutcome, ProviderError> {
        let provider = self
            .provider_for(url)
            .map_err(|e| ProviderError::Unsupported(e.to_string()))?;
        provider
            .download(url, options, dest_part, progress, should_abort)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_protocols() {
        assert_eq!(
            Protocol::from_url(&url::Url::parse("https://a.b/c").unwrap()),
            Some(Protocol::Https)
        );
        assert_eq!(
            Protocol::from_url(&url::Url::parse("magnet:?xt=urn:btih:abc").unwrap()),
            Some(Protocol::BitTorrent)
        );
        assert_eq!(
            Protocol::from_url(&url::Url::parse("gopher://old/").unwrap()),
            None
        );
        assert_eq!(
            Protocol::from_url(&url::Url::parse("https://a.b/file.TORRENT?x=1").unwrap()),
            Some(Protocol::BitTorrent)
        );
    }

    struct MockProvider {
        schemes: &'static [&'static str],
    }

    #[async_trait]
    impl DownloadProvider for MockProvider {
        fn schemes(&self) -> &'static [&'static str] {
            self.schemes
        }
        async fn probe(
            &self,
            _url: &str,
            _options: &ProviderOptions,
        ) -> Result<RemoteFileInfo, ProviderError> {
            unimplemented!()
        }
        async fn download(
            &self,
            _url: &str,
            _options: &ProviderOptions,
            _dest_part: &std::path::Path,
            _progress: std::sync::Arc<dyn Fn(TransferProgress) + Send + Sync>,
            _should_abort: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        ) -> Result<TransferOutcome, ProviderError> {
            unimplemented!()
        }
    }

    #[test]
    fn torrent_file_url_routes_to_torrent_provider() {
        let mut registry = ProviderRegistry::new();
        registry.register(std::sync::Arc::new(MockProvider {
            schemes: &["http", "https"],
        }));
        registry.register(std::sync::Arc::new(MockProvider {
            schemes: &["magnet"],
        }));
        let torrent = registry
            .provider_for("https://example.com/linux.torrent")
            .unwrap();
        assert_eq!(torrent.schemes(), &["magnet"]);
        // Query strings and case-insensitive extension do not change routing.
        let via_query = registry
            .provider_for("https://example.com/dir/Foo.TORRENT?dl=1")
            .unwrap();
        assert_eq!(via_query.schemes(), &["magnet"]);
        // Ordinary http URLs stay on the http provider.
        let http = registry
            .provider_for("https://example.com/file.iso")
            .unwrap();
        assert_eq!(http.schemes(), &["http", "https"]);
    }
}
