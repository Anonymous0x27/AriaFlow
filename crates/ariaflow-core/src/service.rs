//! The unified business interface every AriaFlow frontend talks to.

use crate::error::Result;
use crate::filter::{DownloadFilter, SortField, SortOrder};
use crate::ids::DownloadId;
use crate::models::{
    Download, HistoryEntry, NewDownloadRequest, ProxyEntry, ProxyInput, SshCertificateEntry,
    SshCertificateInput,
};
use crate::settings::{GlobalSettings, UpdateSettingsRequest};
use crate::status::DownloadStatus;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Aggregate counters for dashboards and `ariaflow status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCounts {
    pub total: u64,
    pub downloading: u64,
    pub waiting: u64,
    pub paused: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub total_downloaded_bytes: u64,
    pub global_speed_bytes_per_sec: u64,
}

/// Unified business interface. Implemented by the storage-backed daemon service,
/// the in-memory development service, and ultimately the full engine-backed service.
#[async_trait]
pub trait DownloadService: Send + Sync {
    async fn add_download(&self, request: NewDownloadRequest) -> Result<Download>;

    async fn pause_download(&self, id: DownloadId) -> Result<()>;

    async fn resume_download(&self, id: DownloadId) -> Result<()>;

    async fn cancel_download(&self, id: DownloadId) -> Result<()>;

    /// Removes the task record; `delete_file` also deletes partial/complete files on disk.
    async fn remove_download(&self, id: DownloadId, delete_file: bool) -> Result<()>;

    async fn retry_download(&self, id: DownloadId) -> Result<()>;

    async fn get_download(&self, id: DownloadId) -> Result<Option<Download>>;

    async fn list_downloads(&self, filter: DownloadFilter) -> Result<Vec<Download>>;

    async fn counts(&self) -> Result<DownloadCounts>;

    async fn set_priority(&self, id: DownloadId, priority: i32) -> Result<()>;

    /// Sets (or clears, when `limit` is `None`) the per-task speed cap in
    /// bytes/sec. `None` = inherit the global `perTaskSpeedLimit`.
    async fn set_limit_rate(&self, id: DownloadId, limit: Option<u64>) -> Result<()>;

    /// Persists the final per-segment state of a segmented transfer.
    /// Storage-backed services implement this; the in-memory service ignores it.
    async fn replace_segments(
        &self,
        _id: DownloadId,
        _segments: Vec<crate::models::SegmentInfo>,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_settings(&self) -> Result<GlobalSettings>;

    async fn update_settings(&self, patch: UpdateSettingsRequest) -> Result<GlobalSettings>;

    /// Recent terminal records (completed/failed/cancelled), newest first.
    /// In-memory service returns an empty list.
    async fn list_history(&self, _limit: u32) -> Result<Vec<HistoryEntry>> {
        Ok(Vec::new())
    }

    /// Fetch one history record by row id. `Ok(None)` when absent.
    /// In-memory service returns None.
    async fn get_history(&self, _id: i64) -> Result<Option<HistoryEntry>> {
        Ok(None)
    }

    /// Delete specific history records by row id. Returns the number deleted.
    /// Storage-backed daemons implement this; other services return 0.
    async fn delete_history(&self, _ids: &[i64]) -> Result<u64> {
        Ok(0)
    }

    /// Delete every history record. Returns the number deleted.
    async fn clear_history(&self) -> Result<u64> {
        Ok(0)
    }

    async fn list_proxies(&self) -> Result<Vec<ProxyEntry>> {
        Ok(Vec::new())
    }

    async fn create_proxy(&self, _input: ProxyInput) -> Result<ProxyEntry> {
        Err(crate::AriaFlowError::InvalidRequest(
            "proxy management requires the storage-backed daemon".to_string(),
        ))
    }

    async fn update_proxy(&self, _id: &str, _input: ProxyInput) -> Result<ProxyEntry> {
        Err(crate::AriaFlowError::InvalidRequest(
            "proxy management requires the storage-backed daemon".to_string(),
        ))
    }

    async fn delete_proxy(&self, _id: &str) -> Result<()> {
        Err(crate::AriaFlowError::InvalidRequest(
            "proxy management requires the storage-backed daemon".to_string(),
        ))
    }

    // ---- SSH certificates (for ssh/sftp certificate authentication) ----

    async fn list_ssh_certificates(&self) -> Result<Vec<SshCertificateEntry>> {
        Ok(Vec::new())
    }

    async fn create_ssh_certificate(
        &self,
        _input: SshCertificateInput,
    ) -> Result<SshCertificateEntry> {
        Err(crate::AriaFlowError::InvalidRequest(
            "certificate management requires the storage-backed daemon".to_string(),
        ))
    }

    async fn update_ssh_certificate(
        &self,
        _id: &str,
        _input: SshCertificateInput,
    ) -> Result<SshCertificateEntry> {
        Err(crate::AriaFlowError::InvalidRequest(
            "certificate management requires the storage-backed daemon".to_string(),
        ))
    }

    async fn delete_ssh_certificate(&self, _id: &str) -> Result<()> {
        Err(crate::AriaFlowError::InvalidRequest(
            "certificate management requires the storage-backed daemon".to_string(),
        ))
    }

    // ---- provided batch helpers -------------------------------------------

    async fn pause_all(&self) -> Result<u32> {
        let active = self
            .list_downloads(DownloadFilter {
                statuses: vec![
                    DownloadStatus::Pending,
                    DownloadStatus::Queued,
                    DownloadStatus::Connecting,
                    DownloadStatus::Downloading,
                    DownloadStatus::Retrying,
                    DownloadStatus::Waiting,
                ],
                ..DownloadFilter::unfiltered()
            })
            .await?;
        let mut n = 0;
        for d in active {
            if self.pause_download(d.id).await.is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    async fn resume_all(&self) -> Result<u32> {
        let paused = self
            .list_downloads(DownloadFilter {
                statuses: vec![DownloadStatus::Paused],
                ..DownloadFilter::unfiltered()
            })
            .await?;
        let mut n = 0;
        for d in paused {
            if self.resume_download(d.id).await.is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    async fn cancel_all(&self) -> Result<u32> {
        let active = self
            .list_downloads(DownloadFilter {
                statuses: vec![
                    DownloadStatus::Pending,
                    DownloadStatus::Queued,
                    DownloadStatus::Connecting,
                    DownloadStatus::Downloading,
                    DownloadStatus::Retrying,
                    DownloadStatus::Waiting,
                ],
                ..DownloadFilter::unfiltered()
            })
            .await?;
        let mut n = 0;
        for d in active {
            if self.cancel_download(d.id).await.is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    async fn remove_completed(&self) -> Result<u32> {
        let done = self
            .list_downloads(DownloadFilter {
                statuses: vec![DownloadStatus::Completed],
                ..DownloadFilter::unfiltered()
            })
            .await?;
        let mut n = 0;
        for d in done {
            if self.remove_download(d.id, false).await.is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }
}

/// Applies `filter` in memory (shared by services without full SQL filtering).
pub fn apply_filter_in_memory(items: &mut Vec<Download>, filter: &DownloadFilter) {
    if !filter.statuses.is_empty() {
        items.retain(|d| filter.statuses.contains(&d.status));
    }
    if let Some(cat) = &filter.category {
        items.retain(|d| d.category == *cat);
    }
    if let Some(search) = &filter.search {
        let needle = search.to_lowercase();
        items.retain(|d| {
            d.filename.to_lowercase().contains(&needle) || d.url.to_lowercase().contains(&needle)
        });
    }
    match filter.sort_by {
        Some(SortField::CreatedAt) | None => items.sort_by_key(|d| d.created_at),
        Some(SortField::CompletedAt) => items.sort_by_key(|d| d.completed_at),
        Some(SortField::Filename) => items.sort_by_key(|d| d.filename.clone()),
        Some(SortField::Size) => items.sort_by_key(|d| d.total_size),
        Some(SortField::Progress) => items.sort_by(|a, b| {
            a.progress
                .partial_cmp(&b.progress)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        Some(SortField::Priority) => items.sort_by_key(|d| d.priority),
        Some(SortField::Speed) => items.sort_by_key(|d| d.speed_bytes_per_sec),
    }
    if matches!(filter.sort_order, SortOrder::Desc) {
        items.reverse();
    }
    if let (Some(page), Some(page_size)) = (filter.page, filter.page_size) {
        let page = page.max(1) as usize;
        let page_size = page_size.clamp(1, 1000) as usize;
        let skip = (page - 1) * page_size;
        let drained = items.drain(..items.len().min(skip)).count();
        let _ = drained;
        items.truncate(page_size);
    }
}
