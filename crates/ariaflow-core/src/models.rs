//! Core domain models. Shared by every AriaFlow frontend (CLI, TUI, API, MCP, GUI).

use crate::checksum::Checksum;
use crate::ids::DownloadId;
use crate::status::DownloadStatus;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Per-task proxy configuration. `None` means "inherit global settings".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyConfig {
    pub kind: ProxyKind,
    /// e.g. `http://127.0.0.1:7890` or `socks5://127.0.0.1:1080`
    pub address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Hosts that must bypass this proxy (glob patterns, comma separated).
    pub bypass: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyKind {
    Http,
    Https,
    Socks5,
}

/// Authentication material for a single download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    None,
    Basic { username: String, password: String },
    Bearer { token: String },
}

/// A single byte range of a (possibly segmented) download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentInfo {
    pub id: u32,
    pub start_offset: u64,
    /// Inclusive end offset, like HTTP `Content-Range`.
    pub end_offset: u64,
    pub downloaded_size: u64,
    pub status: SegmentStatus,
    pub retry_count: u32,
}

impl SegmentInfo {
    pub fn total_bytes(&self) -> u64 {
        self.end_offset.saturating_sub(self.start_offset) + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

/// Full snapshot of a download task (persisted + runtime-merged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: DownloadId,
    pub url: String,
    pub final_url: Option<String>,
    pub filename: String,
    pub save_path: PathBuf,
    pub total_size: Option<u64>,
    pub downloaded_size: u64,
    pub status: DownloadStatus,
    /// Instantaneous bytes/sec, computed by the engine.
    pub speed_bytes_per_sec: u64,
    /// Whole-task average bytes/sec over the active period.
    pub average_speed_bytes_per_sec: u64,
    /// 0.0 - 100.0
    pub progress: f64,
    /// Higher wins when slots are available. Range roughly -100..=100.
    pub priority: i32,
    pub retry_count: u32,
    pub max_connections: u16,
    pub current_connections: u16,
    pub category: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub checksum: Option<Checksum>,
    pub proxy: Option<ProxyConfig>,
    /// Per-task authentication material. Never serialized to API responses —
    /// the IPC/HTTP clients must not receive plaintext credentials back.
    #[serde(skip_serializing, default)]
    pub auth: Option<AuthConfig>,
    pub headers: Vec<(String, String)>,
    pub create_subdirs: bool,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub segments: Vec<SegmentInfo>,
    /// Per-task speed cap in bytes/sec; `None` = inherit the global setting.
    pub limit_rate_bytes_per_sec: Option<u64>,
    /// Per-task automatic retry override; `None` = inherit the global setting.
    pub max_retries: Option<u32>,
    /// Optional "start no earlier than" instant; `None` = start as soon as a slot frees.
    pub scheduled_start_at: Option<DateTime<Utc>>,
    /// Per-task User-Agent override; `None` = inherit the global setting.
    pub user_agent: Option<String>,
    /// SSH certificate (row id in `ssh_certificates`) used for ssh/sftp certificate
    /// authentication. Opaque id only — private key material never rides along.
    pub ssh_certificate_id: Option<String>,
}

impl Download {
    /// Remaining bytes, if total size is known.
    pub fn remaining_bytes(&self) -> Option<u64> {
        self.total_size
            .map(|t| t.saturating_sub(self.downloaded_size))
    }

    /// Estimated remaining seconds at the current speed.
    pub fn eta_seconds(&self) -> Option<u64> {
        match (self.remaining_bytes(), self.speed_bytes_per_sec) {
            (Some(rem), speed) if speed > 0 => Some(rem / speed),
            _ => None,
        }
    }
}

/// Request payload for adding a new download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDownloadRequest {
    pub url: String,
    /// Optional custom filename; sanitised before use.
    pub filename: Option<String>,
    /// Optional directory; defaults to the configured download directory.
    pub save_dir: Option<PathBuf>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub proxy: Option<ProxyConfig>,
    pub auth: Option<AuthConfig>,
    pub max_connections: Option<u16>,
    pub priority: Option<i32>,
    pub expected_checksum: Option<Checksum>,
    /// Start immediately (default) or enqueue paused.
    #[serde(default)]
    pub start_paused: bool,
    /// If a file with the same name exists, append " (n)" instead of overwriting.
    #[serde(default = "default_allow_rename")]
    pub allow_rename: bool,
    /// Optional per-task speed limit (bytes/sec). Overrides the global setting.
    #[serde(default)]
    pub limit_rate_bytes_per_sec: Option<u64>,
    /// Optional per-task automatic retry override. Overrides the global setting.
    #[serde(default)]
    pub max_retries: Option<u32>,
    /// Optional "start no earlier than" instant (RFC 3339). Tasks stay queued but
    /// are not eligible for execution until this instant.
    #[serde(default)]
    pub scheduled_start_at: Option<DateTime<Utc>>,
    /// Per-task User-Agent override. `None` = inherit the global setting.
    #[serde(default)]
    pub user_agent: Option<String>,
    /// SSH certificate (row id in `ssh_certificates`) for ssh/sftp certificate
    /// authentication. `None` = transport defaults (URL password / agent / ~/.ssh).
    #[serde(default)]
    pub ssh_certificate_id: Option<String>,
}

fn default_allow_rename() -> bool {
    true
}

impl NewDownloadRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            filename: None,
            save_dir: None,
            headers: Vec::new(),
            proxy: None,
            auth: None,
            max_connections: None,
            priority: None,
            expected_checksum: None,
            start_paused: false,
            allow_rename: true,
            limit_rate_bytes_per_sec: None,
            max_retries: None,
            scheduled_start_at: None,
            user_agent: None,
            ssh_certificate_id: None,
        }
    }
}

/// Lightweight task summary for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSummary {
    pub id: DownloadId,
    pub filename: String,
    pub total_size: Option<u64>,
    pub downloaded_size: u64,
    pub status: DownloadStatus,
    pub progress: f64,
    pub speed_bytes_per_sec: u64,
    pub priority: i32,
    pub category: String,
    pub created_at: DateTime<Utc>,
}

impl From<&Download> for DownloadSummary {
    fn from(d: &Download) -> Self {
        Self {
            id: d.id,
            filename: d.filename.clone(),
            total_size: d.total_size,
            downloaded_size: d.downloaded_size,
            status: d.status,
            progress: d.progress,
            speed_bytes_per_sec: d.speed_bytes_per_sec,
            priority: d.priority,
            category: d.category.clone(),
            created_at: d.created_at,
        }
    }
}

impl Download {
    /// Builds a fresh `Queued`/`Pending` download from a user request.
    ///
    /// Shared by the in-memory and the SQLite-backed services so behaviour stays identical.
    pub fn from_request(
        request: &NewDownloadRequest,
        settings: &crate::settings::GlobalSettings,
    ) -> crate::Result<Download> {
        use crate::category::{classify_by_filename, filename_from_url};
        use crate::filename::{safe_save_path, sanitize_filename, strip_torrent_suffix};

        let parsed = url::Url::parse(&request.url)?;
        if !matches!(
            parsed.scheme(),
            "http" | "https" | "ftp" | "ftps" | "sftp" | "ssh" | "magnet" | "metalink" | "file"
        ) {
            return Err(crate::AriaFlowError::InvalidRequest(format!(
                "unsupported URL scheme: {}",
                parsed.scheme()
            )));
        }
        let raw_name = match request.filename.clone() {
            Some(name) => name,
            None => filename_from_url(&parsed)
                // A ".torrent" URL yields the payload, not the manifest; don't
                // name the payload after the manifest file.
                .map(|name| strip_torrent_suffix(&name))
                .unwrap_or_else(|| "download".to_string()),
        };
        let filename = sanitize_filename(&raw_name);
        let save_dir = request
            .save_dir
            .clone()
            .unwrap_or_else(|| settings.download_dir.clone());
        let save_path = safe_save_path(&save_dir, &filename);
        let now = chrono::Utc::now();
        Ok(Download {
            id: crate::ids::DownloadId::new(),
            url: request.url.clone(),
            final_url: None,
            filename: filename.clone(),
            save_path,
            total_size: None,
            downloaded_size: 0,
            status: if request.start_paused {
                DownloadStatus::Pending
            } else {
                DownloadStatus::Queued
            },
            speed_bytes_per_sec: 0,
            average_speed_bytes_per_sec: 0,
            progress: 0.0,
            priority: request.priority.unwrap_or(0),
            retry_count: 0,
            max_connections: request
                .max_connections
                .unwrap_or(settings.max_connections_per_task),
            current_connections: 0,
            category: classify_by_filename(&filename).to_string(),
            etag: None,
            last_modified: None,
            checksum: request.expected_checksum.clone(),
            proxy: request.proxy.clone(),
            auth: request.auth.clone(),
            headers: request.headers.clone(),
            create_subdirs: false,
            created_at: now,
            started_at: None,
            completed_at: None,
            error_message: None,
            segments: Vec::new(),
            limit_rate_bytes_per_sec: request.limit_rate_bytes_per_sec,
            max_retries: request.max_retries,
            scheduled_start_at: request.scheduled_start_at,
            user_agent: request.user_agent.clone(),
            ssh_certificate_id: request.ssh_certificate_id.clone(),
        })
    }
}

/// One archived terminal record from `download_history` (completed/failed/cancelled).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Row id in `download_history`; used for targeted deletion.
    pub id: i64,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub total_size: Option<u64>,
    pub average_speed: Option<u64>,
    /// "completed" | "failed" | "cancelled"
    pub result: String,
    pub error_message: Option<String>,
    pub completed_at: String,
}

/// A configured proxy (the `proxies` table), used both for API editing and
/// for attaching to downloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyEntry {
    pub id: String,
    pub name: String,
    /// http | https | socks5 | socks5h
    pub kind: String,
    /// e.g. `http://127.0.0.1:7890`
    pub address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Hosts that bypass the proxy (glob patterns, comma separated).
    pub bypass: Option<String>,
    pub is_default: bool,
    pub created_at: Option<String>,
}

/// Payload for creating/updating a proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInput {
    pub name: String,
    pub kind: String,
    pub address: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub bypass: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

impl ProxyInput {
    /// Normalises and validates the input. Returns the cleaned record (without id/created_at).
    pub fn validated(&self) -> crate::Result<Self> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(crate::AriaFlowError::InvalidRequest(
                "proxy name must not be empty".to_string(),
            ));
        }
        let kind = self.kind.trim().to_ascii_lowercase();
        if !matches!(kind.as_str(), "http" | "https" | "socks5" | "socks5h") {
            return Err(crate::AriaFlowError::InvalidRequest(format!(
                "unsupported proxy kind {kind:?}; expected http, https, socks5 or socks5h"
            )));
        }
        let address = self.address.trim().to_string();
        if address.is_empty() {
            return Err(crate::AriaFlowError::InvalidRequest(
                "proxy address must not be empty".to_string(),
            ));
        }
        if !(address.starts_with("http://")
            || address.starts_with("https://")
            || address.starts_with("socks5://")
            || address.starts_with("socks5h://"))
        {
            return Err(crate::AriaFlowError::InvalidRequest(format!(
                "proxy address {address:?} must start with http://, https://, socks5:// or socks5h://"
            )));
        }
        Ok(Self {
            name,
            kind,
            address,
            username: self
                .username
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            password: self.password.clone(),
            bypass: self
                .bypass
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            is_default: self.is_default,
        })
    }
}
/// An SSH certificate identity stored for ssh/sftp transfers (the
/// `ssh_certificates` table). The private key material is **never** exposed
/// to frontends — only this metadata shape crosses the API boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshCertificateEntry {
    pub id: String,
    pub name: String,
    /// Key algorithm reported by OpenSSH (e.g. `ssh-ed25519`).
    pub algorithm: String,
    /// SHA256 fingerprint of the public key (`SHA256:...`).
    pub fingerprint: String,
    /// Whether the stored private key is protected by a passphrase.
    pub has_passphrase: bool,
    pub created_at: Option<String>,
}

/// Payload for creating/updating an SSH certificate.
///
/// Update semantics (`PUT`):
/// - `private_key` absent or blank → keep the stored key; set `passphrase` to
///   `""` to clear it, `None` to keep it, or a non-empty value to replace it
///   (verified against the stored key).
/// - `private_key` present → the new key is validated (and decrypted with the
///   given passphrase when encrypted); `algorithm`/`fingerprint` are
///   recomputed and any previous passphrase is discarded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshCertificateInput {
    pub name: String,
    /// OpenSSH PEM private key. Required for create; empty/absent keeps the
    /// stored key on update. Never returned by any API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// Passphrase for an encrypted private key. `None` (absent) = keep/no
    /// change; `""` = clear (only when the stored or supplied key is not
    /// encrypted); non-empty = set.
    #[serde(default)]
    pub passphrase: Option<String>,
}
