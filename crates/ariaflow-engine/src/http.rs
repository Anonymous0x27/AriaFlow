//! HTTP/HTTPS transport (the first-phase protocol of AriaFlow).
//!
//! Implemented today:
//! - `HEAD` probe with automatic `GET` (Range `bytes=0-0`) fallback for servers
//!   that reject HEAD (405 / 501)
//! - redirect-following metadata capture (final URL, ETag, Last-Modified, Accept-Ranges)
//! - custom headers, cookies (via jar), User-Agent, Basic/Bearer auth
//! - HTTP/HTTPS/SOCKS5 proxy support (scheme derived from proxy address)
//! - optional extra CA bundle & explicit per-request invalid-cert override
//! - Content-Disposition filename extraction (RFC 6266 basics)

use crate::throttle::{resolve_limiter, RateLimiter};
use ariaflow_core::segment::{plan_segments, SegmentRange};
use ariaflow_core::ProxyKind;
use ariaflow_protocol::{
    DownloadProvider, ProviderError, ProviderOptions, RemoteFileInfo, TransferOutcome,
    TransferProgress,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{Client, Method, StatusCode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::debug;

pub struct HttpProvider;

impl Default for HttpProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpProvider {
    pub fn new() -> Self {
        Self
    }

    fn build_client(options: &ProviderOptions) -> Result<Client, ProviderError> {
        let mut builder = Client::builder()
            .redirect(Policy::limited(20))
            .connect_timeout(std::time::Duration::from_secs(
                options.connect_timeout_secs.max(1),
            ))
            .read_timeout(std::time::Duration::from_secs(
                options.read_timeout_secs.max(1),
            ))
            .cookie_store(true)
            .no_gzip() // content-length must stay byte-accurate for range math
            .user_agent(
                options
                    .user_agent
                    .clone()
                    .unwrap_or_else(|| ariaflow_core::settings::DEFAULT_USER_AGENT.to_string()),
            );

        if let Some(proxy) = &options.proxy {
            let p = match proxy.kind {
                ProxyKind::Http | ProxyKind::Https => reqwest::Proxy::all(&proxy.address),
                ProxyKind::Socks5 => reqwest::Proxy::all(&proxy.address),
            }
            .map_err(|e| ProviderError::Network(format!("invalid proxy {}: {e}", proxy.address)))?;
            let p = if let Some(user) = &proxy.username {
                p.basic_auth(user, proxy.password.as_deref().unwrap_or(""))
            } else {
                p
            };
            builder = builder.proxy(p);
        }

        for ca_pem in &options.extra_ca_certs {
            let cert = reqwest::Certificate::from_pem(ca_pem.as_bytes())
                .map_err(|e| ProviderError::Network(format!("invalid CA certificate PEM: {e}")))?;
            builder = builder.add_root_certificate(cert);
        }

        if options.danger_accept_invalid_certs {
            // Explicit user opt-in only; never the default.
            builder = builder.danger_accept_invalid_certs(true);
        }

        builder
            .build()
            .map_err(|e| ProviderError::Network(format!("failed to build HTTP client: {e}")))
    }

    fn apply_headers(
        req: reqwest::RequestBuilder,
        options: &ProviderOptions,
    ) -> reqwest::RequestBuilder {
        let mut headers = HeaderMap::new();
        for (name, value) in &options.headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(n, v);
            }
        }
        req.headers(headers)
    }

    fn parse_content_disposition(value: &str) -> Option<String> {
        // filename*=UTF-8''<percent-encoded> takes precedence over filename="..."
        let mut filename = None;
        let mut filename_star = None;
        for part in value.split(';') {
            let part = part.trim();
            if let Some(rest) = part.strip_prefix("filename*=") {
                let rest = rest.trim();
                if let Some((_, enc)) = rest.split_once("''") {
                    filename_star = Some(percent_decode_uri(enc));
                }
            } else if let Some(rest) = part.strip_prefix("filename=") {
                let rest = rest.trim();
                filename = Some(rest.trim_matches('"').to_string());
            }
        }
        filename_star.or(filename)
    }
}

#[async_trait]
impl DownloadProvider for HttpProvider {
    fn schemes(&self) -> &'static [&'static str] {
        &["http", "https"]
    }

    async fn probe(
        &self,
        url: &str,
        options: &ProviderOptions,
    ) -> Result<RemoteFileInfo, ProviderError> {
        let client = Self::build_client(options)?;

        let send = |method: Method| {
            let mut req = client.request(method, url);
            req = Self::apply_headers(req, options);
            req
        };

        // 1) Try HEAD.
        let head = send(Method::HEAD).send().await;
        let resp = match head {
            Ok(r) if r.status().is_success() => r,
            Ok(r)
                if matches!(
                    r.status(),
                    StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
                ) =>
            {
                debug!("HEAD not allowed, falling back to ranged GET probe");
                let mut req = client.get(url).header("Range", "bytes=0-0");
                req = Self::apply_headers(req, options);
                req.send().await.map_err(map_send_error)?
            }
            Ok(r) if !r.status().is_success() => {
                return Err(ProviderError::BadStatus {
                    status: r.status().as_u16(),
                    message: format!("probe failed for {url}"),
                })
            }
            Ok(r) => r,
            Err(e) => return Err(map_send_error(e)),
        };

        let status = resp.status();
        if !(status.is_success() || status == StatusCode::PARTIAL_CONTENT) {
            return Err(ProviderError::BadStatus {
                status: status.as_u16(),
                message: format!("probe failed for {url}"),
            });
        }

        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                Some((
                    k.as_str().to_ascii_lowercase(),
                    v.to_str().ok()?.to_string(),
                ))
            })
            .collect();
        let header_lookup = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };

        // Size: prefer Content-Range (ranged GET probe) over Content-Length.
        let total_size = header_lookup("content-range")
            .and_then(|v| v.rsplit('/').next().map(|s| s.trim().to_string()))
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| header_lookup("content-length").and_then(|v| v.parse::<u64>().ok()));

        let accept_ranges = header_lookup("accept-ranges")
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false)
            || status == StatusCode::PARTIAL_CONTENT;

        let filename_hint = header_lookup("content-disposition")
            .as_deref()
            .and_then(Self::parse_content_disposition)
            .map(|f| ariaflow_core::filename::sanitize_filename(&f));

        Ok(RemoteFileInfo {
            final_url: resp.url().to_string(),
            total_size,
            accept_ranges,
            etag: header_lookup("etag"),
            last_modified: header_lookup("last-modified"),
            content_type: header_lookup("content-type"),
            filename_hint,
            headers,
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
        // Segmented (multi-connection) path: opt-in via `max_connections` and
        // only when the server honours Range and the file is big enough. Any
        // failure to verify that falls back to the single-stream path.
        if options.max_connections > 1 {
            match self.probe(url, options).await {
                Ok(info) => {
                    if let Some(total) = info.total_size {
                        let min_seg = options.min_segment_size.max(1);
                        if info.accept_ranges && total >= min_seg {
                            debug!(
                                "http segmented path: total={total} connections={}",
                                options.max_connections
                            );
                            return Self::download_segmented(
                                url,
                                options,
                                dest_part,
                                total,
                                progress,
                                should_abort,
                            )
                            .await;
                        }
                    }
                }
                Err(e) => {
                    debug!("segmented pre-probe failed ({e}); falling back to single stream")
                }
            }
        }

        let client = Self::build_client(options)?;

        // Resume from whatever prefix is already on disk.
        let existing = tokio::fs::metadata(dest_part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        let mut req = client.get(url);
        req = Self::apply_headers(req, options);
        if existing > 0 {
            req = req.header("Range", format!("bytes={existing}-"));
        }
        let resp = req.send().await.map_err(map_send_error)?;

        let status = resp.status();
        if !(status.is_success() || status == StatusCode::PARTIAL_CONTENT) {
            return Err(ProviderError::BadStatus {
                status: status.as_u16(),
                message: format!("GET {url} failed"),
            });
        }
        let resumed_from = match (existing > 0, status) {
            (true, StatusCode::PARTIAL_CONTENT) => existing,
            _ => 0,
        };

        let total_bytes = resp.content_length().map(|l| l + resumed_from);

        // Append when resuming, otherwise truncate/create.
        let file = if resumed_from > 0 {
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dest_part)
                .await
        } else {
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(dest_part)
                .await
        }
        .map_err(ProviderError::Io)?;

        let mut file = file;
        let limiter = resolve_limiter(options);
        let mut downloaded = resumed_from;
        let mut last_emit = std::time::Instant::now();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            if should_abort() {
                file.flush().await.ok();
                return Err(ProviderError::Aborted);
            }
            let chunk = chunk.map_err(map_send_error)?;
            if chunk.is_empty() {
                continue;
            }
            limiter.acquire(chunk.len() as u64).await;
            file.write_all(&chunk).await.map_err(ProviderError::Io)?;
            downloaded += chunk.len() as u64;
            // Throttle UI updates to ~4 Hz; the orchestrator persists on its own clock.
            if last_emit.elapsed().as_millis() >= 250 {
                last_emit = std::time::Instant::now();
                progress(TransferProgress {
                    downloaded_bytes: downloaded,
                    total_bytes,
                });
            }
        }
        file.flush().await.map_err(ProviderError::Io)?;
        progress(TransferProgress {
            downloaded_bytes: downloaded,
            total_bytes,
        });

        Ok(TransferOutcome {
            bytes_written: downloaded,
            resumed_from,
            segment_snapshot: None,
        })
    }
}

fn map_send_error(e: reqwest::Error) -> ProviderError {
    if e.is_connect() {
        ProviderError::Network(format!("connect error: {e}"))
    } else if e.is_timeout() {
        ProviderError::Timeout(0)
    } else {
        ProviderError::Network(e.to_string())
    }
}

fn percent_decode_uri(input: &str) -> String {
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

// ---------------------------------------------------------------------------
// Segmented (multi-connection) transfer
// ---------------------------------------------------------------------------

/// Path of the per-segment side file for segment `index`.
fn segment_path(dest_part: &Path, index: u32) -> PathBuf {
    PathBuf::from(format!("{}.seg-{index:03}", dest_part.display()))
}

/// Snapshot the per-segment state from the side files *before* they are
/// merged away. Callers persist this so the task detail view can show how
/// the chunks were transferred.
fn snapshot_segments(
    dest_part: &Path,
    segments: &[SegmentRange],
) -> Vec<ariaflow_core::models::SegmentInfo> {
    use ariaflow_core::models::{SegmentInfo, SegmentStatus};
    segments
        .iter()
        .map(|seg| {
            let len = std::fs::metadata(segment_path(dest_part, seg.index))
                .map(|m| m.len())
                .unwrap_or(0)
                .min(seg.len());
            SegmentInfo {
                id: seg.index,
                start_offset: seg.start_offset,
                end_offset: seg.end_offset,
                downloaded_size: len,
                status: SegmentStatus::Completed,
                retry_count: 0,
            }
        })
        .collect()
}

/// Merge ordered segment side files into `dest_part` (truncating it), then
/// remove the side files. Streams so multi-GB files never sit in memory.
async fn merge_segment_files(
    segments: &[SegmentRange],
    dest_part: &Path,
) -> Result<(), ProviderError> {
    if let Some(parent) = dest_part.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(ProviderError::Io)?;
    }
    let mut out = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest_part)
        .await
        .map_err(ProviderError::Io)?;
    for seg in segments {
        let seg_file = segment_path(dest_part, seg.index);
        let mut input = tokio::fs::File::open(&seg_file)
            .await
            .map_err(ProviderError::Io)?;
        tokio::io::copy(&mut input, &mut out)
            .await
            .map_err(ProviderError::Io)?;
    }
    out.flush().await.map_err(ProviderError::Io)?;
    drop(out);
    let merged = tokio::fs::metadata(dest_part)
        .await
        .map_err(ProviderError::Io)?
        .len();
    let total: u64 = segments.iter().map(|s| s.len()).sum();
    if merged != total {
        return Err(ProviderError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("merged size {merged} != expected {total}"),
        )));
    }
    for seg in segments {
        let _ = tokio::fs::remove_file(segment_path(dest_part, seg.index)).await;
    }
    ariaflow_core::segment::remove_segment_manifest(dest_part);
    Ok(())
}

impl HttpProvider {
    /// Downloads `url` in parallel Range requests, one per planned segment.
    ///
    /// Layout: bytes live in `<dest_part>.seg-NNN` side files while in flight
    /// (each resumable independently); on success they are merged into
    /// `dest_part` and removed. A crash after the last segment completes but
    /// before the merge leaves the side files behind — the next call detects
    /// that and merges without touching the network.
    async fn download_segmented(
        url: &str,
        options: &ProviderOptions,
        dest_part: &Path,
        total: u64,
        progress: Arc<dyn Fn(TransferProgress) + Send + Sync>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<TransferOutcome, ProviderError> {
        let segments = plan_segments(
            total,
            options.max_connections,
            options.min_segment_size.max(1),
        );
        if segments.len() <= 1 {
            // Degenerate plan; fall back to the single-stream path by letting
            // the caller re-enter with connections=1.
            debug!("segmented plan degenerated to one segment; using single stream");
            let mut single = options.clone();
            single.max_connections = 1;
            let provider = HttpProvider::new();
            return DownloadProvider::download(
                &provider,
                url,
                &single,
                dest_part,
                progress,
                should_abort,
            )
            .await;
        }

        // Fast path: every segment is already fully on disk (crash between the
        // last segment and the merge) — merge without the network.
        let seg_lens: Vec<u64> = segments
            .iter()
            .map(|s| {
                std::fs::metadata(segment_path(dest_part, s.index))
                    .map(|m| m.len())
                    .unwrap_or(0)
            })
            .collect();
        let resumed_from: u64 = seg_lens
            .iter()
            .zip(&segments)
            .map(|(l, s)| (*l).min(s.len()))
            .sum();
        if seg_lens.iter().zip(&segments).all(|(l, s)| *l >= s.len()) {
            tracing::info!("all segments already on disk; merging without network");
            let snapshot = snapshot_segments(dest_part, &segments);
            merge_segment_files(&segments, dest_part).await?;
            progress(TransferProgress {
                downloaded_bytes: total,
                total_bytes: Some(total),
            });
            return Ok(TransferOutcome {
                bytes_written: total,
                resumed_from,
                segment_snapshot: Some(snapshot),
            });
        }

        if let Err(e) = ariaflow_core::segment::write_segment_manifest(dest_part, &segments) {
            tracing::warn!(
                "failed to write segment manifest {}: {e}",
                dest_part.display()
            );
        }
        let downloaded = Arc::new(AtomicU64::new(resumed_from));
        let cancel = Arc::new(AtomicBool::new(false));
        let limiter = resolve_limiter(options);

        // Progress aggregator: report the sum of per-segment counters ~4 Hz.
        let progress_task = {
            let downloaded = Arc::clone(&downloaded);
            let progress = Arc::clone(&progress);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    progress(TransferProgress {
                        downloaded_bytes: downloaded.load(Ordering::Relaxed).min(total),
                        total_bytes: Some(total),
                    });
                }
            })
        };

        let mut handles = Vec::with_capacity(segments.len());
        for seg in segments.iter().copied() {
            let options = options.clone();
            let seg_file = segment_path(dest_part, seg.index);
            let downloaded = Arc::clone(&downloaded);
            let cancel = Arc::clone(&cancel);
            let limiter = Arc::clone(&limiter);
            let should_abort = Arc::clone(&should_abort);
            let url = url.to_string();
            handles.push(tokio::spawn(async move {
                Self::download_segment(
                    &url,
                    &options,
                    seg,
                    &seg_file,
                    downloaded,
                    cancel,
                    limiter,
                    should_abort,
                )
                .await
            }));
        }

        let mut first_err: Option<ProviderError> = None;
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    cancel.store(true, Ordering::Relaxed);
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
                Err(join_err) => {
                    cancel.store(true, Ordering::Relaxed);
                    if first_err.is_none() {
                        first_err = Some(ProviderError::Network(format!(
                            "segment worker crashed: {join_err}"
                        )));
                    }
                }
            }
        }
        progress_task.abort();

        if let Some(e) = first_err {
            // Side files remain for the next attempt.
            return Err(e);
        }

        let snapshot = snapshot_segments(dest_part, &segments);
        merge_segment_files(&segments, dest_part).await?;
        progress(TransferProgress {
            downloaded_bytes: total,
            total_bytes: Some(total),
        });
        Ok(TransferOutcome {
            bytes_written: total,
            resumed_from,
            segment_snapshot: Some(snapshot),
        })
    }

    /// One segment of a segmented download. Resumes from the side file length.
    #[allow(clippy::too_many_arguments)]
    async fn download_segment(
        url: &str,
        options: &ProviderOptions,
        seg: SegmentRange,
        seg_file: &Path,
        downloaded: Arc<AtomicU64>,
        cancel: Arc<AtomicBool>,
        limiter: Arc<RateLimiter>,
        should_abort: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(), ProviderError> {
        let existing = tokio::fs::metadata(seg_file)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if existing >= seg.len() {
            return Ok(()); // already complete from a previous attempt
        }
        let start = seg.start_offset + existing;
        let client = Self::build_client(options)?;
        let mut req = client.get(url);
        req = Self::apply_headers(req, options);
        req = req.header("Range", format!("bytes={start}-{}", seg.end_offset));
        let resp = req.send().await.map_err(map_send_error)?;
        let status = resp.status();
        if status != StatusCode::PARTIAL_CONTENT {
            return Err(ProviderError::BadStatus {
                status: status.as_u16(),
                message: format!(
                    "segment {} request rejected by server (expected 206)",
                    seg.index
                ),
            });
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(seg_file)
            .await
            .map_err(ProviderError::Io)?;
        let mut file = file;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if should_abort() || cancel.load(Ordering::Relaxed) {
                file.flush().await.ok();
                return Err(ProviderError::Aborted);
            }
            let chunk = chunk.map_err(map_send_error)?;
            if chunk.is_empty() {
                continue;
            }
            limiter.acquire(chunk.len() as u64).await;
            file.write_all(&chunk).await.map_err(ProviderError::Io)?;
            downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        }
        file.flush().await.map_err(ProviderError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_filename_from_content_disposition() {
        assert_eq!(
            HttpProvider::parse_content_disposition("attachment; filename=\"report.pdf\""),
            Some("report.pdf".to_string())
        );
        assert_eq!(
            HttpProvider::parse_content_disposition("attachment; filename*=UTF-8''%E4%B8%AD.pdf"),
            Some("中.pdf".to_string())
        );
        assert_eq!(HttpProvider::parse_content_disposition("inline"), None);
    }

    #[test]
    fn percent_decode_uri_roundtrip() {
        assert_eq!(percent_decode_uri("a%20b"), "a b");
        assert_eq!(percent_decode_uri("100%"), "100%");
    }

    #[tokio::test]
    async fn merge_segment_files_concatenates_in_order_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!(
            "ariaflow-seg-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("file.bin.ariaflow-part");
        let segments = vec![
            SegmentRange {
                index: 0,
                start_offset: 0,
                end_offset: 3,
            },
            SegmentRange {
                index: 1,
                start_offset: 4,
                end_offset: 7,
            },
            SegmentRange {
                index: 2,
                start_offset: 8,
                end_offset: 9,
            },
        ];
        std::fs::write(segment_path(&part, 0), b"abcd").unwrap();
        std::fs::write(segment_path(&part, 1), b"efgh").unwrap();
        std::fs::write(segment_path(&part, 2), b"ij").unwrap();

        merge_segment_files(&segments, &part).await.unwrap();

        assert_eq!(std::fs::read(&part).unwrap(), b"abcdefghij");
        for seg in &segments {
            assert!(
                !segment_path(&part, seg.index).exists(),
                "seg side file removed"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn merge_segment_files_detects_size_mismatch() {
        let dir = std::env::temp_dir().join(format!(
            "ariaflow-seg-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("file.bin.ariaflow-part");
        let segments = vec![SegmentRange {
            index: 0,
            start_offset: 0,
            end_offset: 9,
        }];
        std::fs::write(segment_path(&part, 0), b"tiny").unwrap();

        let err = merge_segment_files(&segments, &part).await.unwrap_err();
        assert!(matches!(err, ProviderError::Io(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
