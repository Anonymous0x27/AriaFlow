//! BitTorrent transport (magnet links, BTv1).
//!
//! Based on `librqbit` (pure-Rust BitTorrent client). The task URL is either
//! a magnet link, e.g.
//! `magnet:?xt=urn:btih:<40-hex-info-hash>&dn=ubuntu-24.04.iso&tr=<tracker>`,
//! or an http(s) URL pointing at a `.torrent` manifest file; librqbit fetches
//! the manifest and resolves the metainfo from there.
//!
//! Implementation notes:
//! - BitTorrent cannot write to an arbitrary offset, so the transfer happens in
//!   a per-task staging directory `<dest_part-dir>/.ariaflow-bt-<infohash>/`.
//!   On completion the artifact (single file, or the torrent's folder for
//!   multi-file torrents) is moved atomically into `dest_part`.
//! - On abort (pause/cancel) the staging directory is left in place; the next
//!   run reuses it with `overwrite` so already-verified pieces are re-checked
//!   instead of re-downloaded (piece-level resume).
//! - A http(s) URL whose path ends in `.torrent` is BT-routed by the provider
//!   registry; librqbit fetches the manifest and resolves the metainfo.
//!
//! Live transfer requires network access to trackers/peers (DHT is enabled by
//! default). In offline environments the task fails with a tracker/DHT error,
//! which surfaces as a normal `failed` task with a readable message.
//!
//! A `.torrent` URL is recognised when the URL path ends in `.torrent`
//! (case-insensitive, query string ignored); those URLs are routed here from
//! the provider registry instead of to the HTTP provider.

use ariaflow_core::filename::strip_torrent_suffix;
use ariaflow_protocol::{
    DownloadProvider, ProviderError, ProviderOptions, RemoteFileInfo, TransferOutcome,
    TransferProgress,
};
use async_trait::async_trait;
use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct TorrentProvider;

impl Default for TorrentProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TorrentProvider {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MagnetInfo {
    /// Lowercase hex (40 chars) info hash.
    info_hash: String,
    display_name: Option<String>,
    trackers: usize,
    exact_length: Option<u64>,
}

/// What a torrent task URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TorrentSource {
    Magnet { info: MagnetInfo, raw: String },
    TorrentUrl { url: String },
}

impl TorrentSource {
    fn staging_dir_id(&self) -> String {
        match self {
            Self::Magnet { info, .. } => info.info_hash.clone(),
            // The metainfo hash is unavailable without fetching the manifest;
            // a deterministic hash of the URL is enough to namespace staging.
            Self::TorrentUrl { url } => hex_sha256(url.as_bytes()),
        }
    }
}

/// True for http(s) URLs whose path ends in `.torrent` (case-insensitive).
/// Keep in sync with the registry special case in `ariaflow-protocol`.
pub fn is_torrent_file_url(url: &str) -> bool {
    match url::Url::parse(url) {
        Ok(parsed) => {
            matches!(parsed.scheme(), "http" | "https")
                && parsed.path().to_ascii_lowercase().ends_with(".torrent")
        }
        Err(_) => false,
    }
}

fn classify(url: &str) -> Result<TorrentSource, ProviderError> {
    if let Ok(info) = parse_magnet(url) {
        return Ok(TorrentSource::Magnet {
            info,
            raw: url.to_string(),
        });
    }
    if is_torrent_file_url(url) {
        return Ok(TorrentSource::TorrentUrl {
            url: url.to_string(),
        });
    }
    match url::Url::parse(url) {
        Ok(parsed) => Err(ProviderError::Unsupported(format!(
            "Torrent provider cannot handle scheme {:?} (expected magnet: or an http(s) .torrent URL)",
            parsed.scheme()
        ))),
        Err(_) => Err(ProviderError::Unsupported(format!(
            "invalid torrent URL: {url}"
        ))),
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn torrent_basename_hint(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.rfind(|s| !s.is_empty())?;
    let decoded = percent_decode(last);
    if decoded.is_empty() || !decoded.to_ascii_lowercase().ends_with(".torrent") {
        // Only a real manifest URL yields a meaningful hint.
        return None;
    }
    Some(ariaflow_core::filename::sanitize_filename(
        &strip_torrent_suffix(&decoded),
    ))
}

fn parse_magnet(raw: &str) -> Result<MagnetInfo, ProviderError> {
    let url = url::Url::parse(raw)
        .map_err(|e| ProviderError::Unsupported(format!("invalid magnet URL: {e}")))?;
    if url.scheme() != "magnet" {
        return Err(ProviderError::Unsupported(format!(
            "Torrent provider cannot handle scheme {:?} (expected magnet:)",
            url.scheme()
        )));
    }
    let query = url
        .query()
        .ok_or_else(|| ProviderError::Unsupported("magnet link has no query parameters".into()))?;

    let mut info_hash_hex = None;
    let mut display_name = None;
    let mut trackers = 0usize;
    let mut exact_length = None;

    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "xt" => {
                if let Some(hash) = value.strip_prefix("urn:btih:") {
                    info_hash_hex = Some(normalize_info_hash(hash)?);
                } else if value.starts_with("urn:btmh:") {
                    return Err(ProviderError::Unsupported(
                        "BTv2 (btmh) magnet links are not supported yet; use a BTv1 (btih) link"
                            .into(),
                    ));
                }
            }
            "dn" => display_name = Some(percent_decode(value)),
            "tr" => trackers += 1,
            "xl" => exact_length = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let info_hash = info_hash_hex.ok_or_else(|| {
        ProviderError::Unsupported("magnet link has no urn:btih:<hash> parameter".into())
    })?;

    Ok(MagnetInfo {
        info_hash,
        display_name: display_name.filter(|s| !s.is_empty()),
        trackers,
        exact_length,
    })
}

/// Accepts 40-char hex or 32-char base32, returns lowercase hex.
fn normalize_info_hash(hash: &str) -> Result<String, ProviderError> {
    let hash = hash.trim();
    if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(hash.to_ascii_lowercase());
    }
    if hash.len() == 32 {
        // base32 (RFC 4648, no padding)
        let mut out = Vec::with_capacity(20);
        let mut buffer = 0u32;
        let mut bits = 0u8;
        for c in hash.to_ascii_uppercase().chars() {
            let value = match c {
                'A'..='Z' => c as u32 - 'A' as u32,
                '2'..='7' => c as u32 - '2' as u32 + 26,
                '=' => break,
                _ => {
                    return Err(ProviderError::Unsupported(format!(
                        "invalid base32 character in info hash: {c:?}"
                    )))
                }
            };
            buffer = (buffer << 5) | value;
            bits += 5;
            if bits >= 8 {
                out.push(((buffer >> (bits - 8)) & 0xff) as u8);
                bits -= 8;
            }
        }
        return Ok(out.iter().map(|b| format!("{b:02x}")).collect());
    }
    Err(ProviderError::Unsupported(format!(
        "info hash must be 40 hex chars or 32 base32 chars (got {} chars)",
        hash.len()
    )))
}

#[async_trait]
impl DownloadProvider for TorrentProvider {
    fn schemes(&self) -> &'static [&'static str] {
        &["magnet"]
    }

    fn supports_resume(&self) -> bool {
        true
    }

    async fn probe(
        &self,
        url: &str,
        options: &ProviderOptions,
    ) -> Result<RemoteFileInfo, ProviderError> {
        let _ = options;
        match classify(url)? {
            TorrentSource::Magnet { info, .. } => Ok(RemoteFileInfo {
                final_url: url.to_string(),
                total_size: info.exact_length,
                // BT is piece-based; "range" semantics don't apply.
                accept_ranges: false,
                etag: Some(format!("\"btih-{}\"", info.info_hash)),
                last_modified: None,
                content_type: Some("application/x-bittorrent".to_string()),
                filename_hint: info
                    .display_name
                    .map(|n| ariaflow_core::filename::sanitize_filename(&n)),
                headers: vec![("x-bt-tracker-count".to_string(), info.trackers.to_string())],
            }),
            TorrentSource::TorrentUrl { url } => {
                // Offline probe: the payload name/size only become known once
                // librqbit resolves the metainfo at download time. The size is
                // intentionally unknown (None) instead of reporting the size of
                // the manifest itself.
                Ok(RemoteFileInfo {
                    filename_hint: torrent_basename_hint(&url),
                    final_url: url,
                    total_size: None,
                    accept_ranges: false,
                    etag: None,
                    last_modified: None,
                    content_type: Some("application/x-bittorrent".to_string()),
                    headers: vec![("x-bt-source".to_string(), "torrent-url".to_string())],
                })
            }
        }
    }

    async fn download(
        &self,
        url: &str,
        options: &ProviderOptions,
        dest_part: &Path,
        progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<TransferOutcome, ProviderError> {
        let source = classify(url)?;
        let _ = options;

        let parent = dest_part.parent().ok_or_else(|| {
            ProviderError::Unsupported("dest path must have a parent directory".into())
        })?;
        let bt_dir = parent.join(format!(".ariaflow-bt-{}", source.staging_dir_id()));
        tokio::fs::create_dir_all(&bt_dir)
            .await
            .map_err(ProviderError::Io)?;

        let cancellation = tokio_util::sync::CancellationToken::new();
        let session_opts = SessionOptions {
            cancellation_token: Some(cancellation.clone()),
            ..SessionOptions::default()
        };
        let session = Session::new_with_opts(bt_dir.clone(), session_opts)
            .await
            .map_err(|e| ProviderError::Network(format!("failed to start BT session: {e:#}")))?;

        let add = AddTorrent::Url(Cow::Borrowed(url));
        let response = session
            .add_torrent(
                add,
                Some(AddTorrentOptions {
                    output_folder: Some(bt_dir.to_string_lossy().into_owned()),
                    overwrite: true,
                    ..AddTorrentOptions::default()
                }),
            )
            .await
            .map_err(|e| ProviderError::Network(format!("failed to add torrent: {e:#}")))?;

        let handle = response.into_handle().ok_or_else(|| {
            ProviderError::Unsupported("torrent was registered as list-only".into())
        })?;

        let mut downloaded_total: u64;
        loop {
            let stats = handle.stats();
            if let Some(err) = &stats.error {
                cancellation.cancel();
                return Err(ProviderError::Network(format!("BT error: {err}")));
            }
            let total = (stats.total_bytes > 0).then_some(stats.total_bytes);
            downloaded_total = stats.progress_bytes;
            progress(TransferProgress {
                downloaded_bytes: downloaded_total,
                total_bytes: total,
            });
            if stats.finished {
                break;
            }
            if should_abort() {
                cancellation.cancel();
                // Leave the staging dir in place for piece-level resume.
                return Err(ProviderError::Aborted);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Metadata may resolve only after start; now find what landed on disk.
        let torrent_name = handle.name();
        cancellation.cancel(); // stop the session; files stay

        let produced = locate_produced_artifact(&bt_dir, torrent_name.as_deref())?;
        // Move artifact into dest_part (file or directory).
        if dest_part.exists() {
            tokio::fs::remove_file(dest_part).await.ok();
        }
        tokio::fs::rename(&produced, dest_part)
            .await
            .map_err(ProviderError::Io)?;
        let _ = tokio::fs::remove_dir_all(&bt_dir).await;

        let written = tokio::fs::metadata(dest_part)
            .await
            .map(|m| m.len())
            .unwrap_or(downloaded_total);

        Ok(TransferOutcome {
            bytes_written: written,
            resumed_from: 0,
            segment_snapshot: None,
        })
    }
}

/// Finds the artifact librqbit wrote inside the staging directory.
/// Single-file torrents produce a file; multi-file torrents a folder.
fn locate_produced_artifact(
    bt_dir: &Path,
    expected_name: Option<&str>,
) -> Result<PathBuf, ProviderError> {
    // 1) Exact match with the torrent's name.
    if let Some(name) = expected_name {
        let candidate = bt_dir.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    // 2) Otherwise: if the staging dir contains exactly one non-hidden entry, use it.
    let entries: Vec<_> = std::fs::read_dir(bt_dir)
        .map_err(ProviderError::Io)?
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    match entries.as_slice() {
        [single] => Ok(single.path()),
        [] => Err(ProviderError::Network(
            "torrent finished but produced no output".into(),
        )),
        many => {
            // Multiple top-level entries with unknown torrent name: report them.
            let names: Vec<_> = many
                .iter()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            Err(ProviderError::Unsupported(format!(
                "torrent produced multiple top-level entries {names:?}; cannot map to a single output"
            )))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_magnet() {
        let m = parse_magnet(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=ubuntu.iso&tr=https%3A%2F%2Ftracker.example.com%2Fannounce",
        )
        .unwrap();
        assert_eq!(m.info_hash, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(m.display_name.as_deref(), Some("ubuntu.iso"));
        assert_eq!(m.trackers, 1);
    }

    #[test]
    fn parses_base32_magnet() {
        let m =
            parse_magnet("magnet:?xt=urn:btih:BRPVNWIRX5C5ZBIMTXQ3XUSXAMBA5XAS&xl=1234").unwrap();
        assert_eq!(m.info_hash.len(), 40);
        assert_eq!(m.exact_length, Some(1234));
    }

    #[test]
    fn rejects_non_magnet() {
        assert!(parse_magnet("https://example.com/file.iso").is_err());
        assert!(parse_magnet("magnet:?xt=urn:btmh:abc").is_err());
    }

    #[test]
    fn classifies_torrent_urls() {
        assert!(is_torrent_file_url("https://example.com/linux.torrent"));
        assert!(is_torrent_file_url(
            "http://example.com/dir/Foo.TORRENT?token=abc"
        ));
        assert!(!is_torrent_file_url("https://example.com/file.iso"));
        assert!(!is_torrent_file_url(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(!is_torrent_file_url("not a url"));
        match classify("https://example.com/linux.torrent").unwrap() {
            TorrentSource::TorrentUrl { url } => {
                assert_eq!(url, "https://example.com/linux.torrent")
            }
            _ => panic!("expected TorrentUrl"),
        }
        assert!(matches!(
            classify("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567").unwrap(),
            TorrentSource::Magnet { .. }
        ));
        assert!(classify("https://example.com/file.iso").is_err());
        assert!(classify("ftp://example.com/file.torrent").is_err());
    }

    #[test]
    fn staging_dir_id_is_deterministic() {
        let source = classify("https://example.com/linux.torrent").unwrap();
        assert_eq!(source.staging_dir_id(), source.staging_dir_id());
        assert_eq!(source.staging_dir_id().len(), 64);
        let other = classify("https://example.com/other.torrent").unwrap();
        assert_ne!(source.staging_dir_id(), other.staging_dir_id());
    }

    #[test]
    fn torrent_url_hint_strips_extension() {
        assert_eq!(
            torrent_basename_hint("https://example.com/isos/ubuntu-24.04.iso.torrent"),
            Some("ubuntu-24.04.iso".to_string())
        );
        assert_eq!(torrent_basename_hint("https://example.com/dir/"), None);
    }
}
