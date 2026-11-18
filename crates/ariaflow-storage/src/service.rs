//! `SqliteDownloadService` — persistent implementation of the unified business interface.
//!
//! Tasks are persisted immediately; state transitions are validated against the
//! same state machine the in-memory service uses. The network orchestration loop
//! (engine) plugs in at `ariaflow-daemon` and flips tasks through
//! `Connecting -> Downloading -> ... -> Completed`.

use crate::repository::{
    DownloadRepository, HistoryRepository, ProxyRepository, SettingsRepository,
    SshCertificateRepository,
};
use crate::Database;
use ariaflow_core::event::DownloadEvent;
use ariaflow_core::filter::DownloadFilter;
use ariaflow_core::ids::DownloadId;
use ariaflow_core::models::Download;
use ariaflow_core::service::{apply_filter_in_memory, DownloadCounts, DownloadService};
use ariaflow_core::settings::{GlobalSettings, UpdateSettingsRequest};
use ariaflow_core::status::DownloadStatus;
use ariaflow_core::{AriaFlowError, EventBus, NewDownloadRequest, Result};
use async_trait::async_trait;

const GLOBAL_SETTINGS_KEY: &str = "global";

pub struct SqliteDownloadService {
    repo: DownloadRepository,
    history: HistoryRepository,
    settings: SettingsRepository,
    proxies: ProxyRepository,
    ssh_certificates: SshCertificateRepository,
    bus: EventBus,
}

impl SqliteDownloadService {
    pub fn new(db: Database) -> Self {
        crate::service::ensure_global_settings(&db);
        Self {
            repo: DownloadRepository::new(db.clone()),
            history: HistoryRepository::new(db.clone()),
            settings: SettingsRepository::new(db.clone()),
            proxies: ProxyRepository::new(db.clone()),
            ssh_certificates: SshCertificateRepository::new(db),
            bus: EventBus::default(),
        }
    }

    pub fn bus(&self) -> EventBus {
        self.bus.clone()
    }

    /// Private key material for an `ssh_certificates` row — for the engine
    /// only. Never exposed through any API surface.
    pub async fn ssh_certificate_material(
        &self,
        certificate_id: &str,
    ) -> Result<Option<(String, String, Option<String>)>> {
        self.ssh_certificates.material(certificate_id).await
    }

    /// Per-segment state for a single-task view: a live snapshot derived from
    /// `.seg-NNN` side files while a segmented transfer is in flight
    /// (or paused), otherwise the rows persisted at completion.
    async fn live_or_persisted_segments(
        &self,
        dl: &Download,
    ) -> Result<Vec<ariaflow_core::models::SegmentInfo>> {
        if let Some(total) = dl.total_size {
            let settings = Self::load_settings_inner(&self.settings).await;
            if let Some(live) = ariaflow_core::segment::live_segment_snapshot(
                &dl.save_path,
                total,
                dl.max_connections,
                settings.min_segment_size,
            ) {
                return Ok(live);
            }
        }
        self.repo.load_segments(&dl.id).await
    }

    fn transition_to(&self, dl: &Download, to: DownloadStatus) -> Result<()> {
        if !dl.status.can_transition_to(&to) {
            return Err(AriaFlowError::InvalidTransition {
                from: dl.status.to_string(),
                to: to.to_string(),
            });
        }
        Ok(())
    }

    async fn set_status(
        &self,
        id: &DownloadId,
        to: DownloadStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.repo.update_status(id, to, error).await?;
        Ok(())
    }

    /// Flips a task to `to` through the shared state machine, then persists the
    /// new status (and merges `error` as error_message when provided; the
    /// COALESCE keeps the existing message when `error` is None).
    ///
    /// Returns `InvalidTransition` without touching the database when the task is
    /// already in a state from which `to` is unreachable (e.g. the user paused the
    /// task while it was connecting); callers treat that as a silent stop signal.
    pub async fn engine_transition(
        &self,
        id: &DownloadId,
        to: DownloadStatus,
        error: Option<&str>,
    ) -> Result<()> {
        let dl = self
            .repo
            .get(id)
            .await?
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        self.transition_to(&dl, to)?;
        self.set_status(id, to, error).await
    }

    pub async fn engine_record_probe(
        &self,
        id: &DownloadId,
        final_url: Option<&str>,
        total_size: Option<u64>,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<()> {
        self.repo
            .set_probe(id, final_url, total_size, etag, last_modified)
            .await?;
        Ok(())
    }

    pub async fn engine_record_progress(
        &self,
        id: &DownloadId,
        downloaded_size: u64,
        total_size: Option<u64>,
        speed_bytes_per_sec: u64,
        average_speed_bytes_per_sec: u64,
        progress: f64,
    ) -> Result<()> {
        self.repo
            .update_progress(
                id,
                downloaded_size,
                total_size,
                speed_bytes_per_sec,
                average_speed_bytes_per_sec,
                progress,
            )
            .await?;
        let eta_seconds = match total_size {
            Some(t) if speed_bytes_per_sec > 0 && t > downloaded_size => {
                Some((t - downloaded_size) / speed_bytes_per_sec)
            }
            _ => None,
        };
        self.bus.publish(DownloadEvent::DownloadProgress {
            id: *id,
            downloaded_size,
            total_size,
            speed_bytes_per_sec,
            eta_seconds,
        });
        Ok(())
    }

    pub async fn engine_finish(
        &self,
        id: &DownloadId,
        downloaded_size: u64,
        total_size: Option<u64>,
        average_speed_bytes_per_sec: u64,
    ) -> Result<()> {
        self.repo
            .finish_download(id, downloaded_size, average_speed_bytes_per_sec)
            .await?;
        self.bus.publish(DownloadEvent::DownloadCompleted {
            id: *id,
            total_size,
            average_speed_bytes_per_sec,
        });
        Ok(())
    }

    /// Zeroes live speed/connection counters without changing status.
    pub async fn engine_idle(&self, id: &DownloadId) -> Result<()> {
        self.repo.mark_idle(id).await?;
        Ok(())
    }

    pub async fn engine_bump_retry(&self, id: &DownloadId) -> Result<u32> {
        Ok(self.repo.bump_retry(id).await?.unwrap_or(u32::MAX))
    }

    pub async fn engine_requeue_if_retrying(&self, id: &DownloadId) -> Result<bool> {
        self.repo.requeue_if_retrying(id).await
    }

    /// Crash recovery; returns how many interrupted tasks were sent back to `queued`.
    pub async fn engine_reset_interrupted(&self) -> Result<u64> {
        self.repo.reset_interrupted_transfers().await
    }

    pub async fn engine_record_history(
        &self,
        id: &DownloadId,
        result: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        if let Some(dl) = self.repo.get(id).await? {
            self.history.record(&dl, result, error_message).await?;
        }
        Ok(())
    }

    pub async fn load_settings_inner(settings: &SettingsRepository) -> GlobalSettings {
        match settings.get_json(GLOBAL_SETTINGS_KEY).await {
            Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
            _ => GlobalSettings::default(),
        }
    }
}

fn ensure_global_settings(db: &Database) {
    // Best-effort eager seed; real write happens on first update.
    let _ = db;
}

#[async_trait]
impl DownloadService for SqliteDownloadService {
    async fn add_download(&self, request: NewDownloadRequest) -> Result<Download> {
        let settings = Self::load_settings_inner(&self.settings).await;
        let mut dl = Download::from_request(&request, &settings)?;
        // IDM-style "never overwrite": pick `name (n).ext` when the target exists on
        // disk or is claimed by another live task, so two downloads of the same file
        // can never clobber each other (or your existing files).
        let reserved: std::collections::HashSet<std::path::PathBuf> = self
            .repo
            .list()
            .await
            .map(|items| {
                items
                    .into_iter()
                    .filter(|d| !d.status.is_terminal())
                    .map(|d| d.save_path)
                    .collect()
            })
            .unwrap_or_default();
        let deduped =
            ariaflow_core::filename::dedupe_path_reserved(&dl.save_path, &|p| reserved.contains(p));
        if deduped != dl.save_path {
            if let Some(name) = deduped.file_name().and_then(|n| n.to_str()) {
                dl.filename = name.to_string();
                dl.category = ariaflow_core::category::classify_by_filename(name).to_string();
            }
            dl.save_path = deduped;
        }
        self.repo.insert(&dl).await?;
        let _ = self
            .repo
            .record_event(
                Some(&dl.id),
                "download_created",
                serde_json::json!({ "url": dl.url }),
            )
            .await;
        self.bus
            .publish(DownloadEvent::DownloadCreated { id: dl.id });
        Ok(dl)
    }

    async fn pause_download(&self, id: DownloadId) -> Result<()> {
        let dl = self
            .repo
            .get(&id)
            .await?
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        if !dl.status.is_pausable() {
            return Err(AriaFlowError::InvalidTransition {
                from: dl.status.to_string(),
                to: DownloadStatus::Paused.to_string(),
            });
        }
        self.transition_to(&dl, DownloadStatus::Paused)?;
        self.set_status(&id, DownloadStatus::Paused, None).await?;
        let _ = self
            .repo
            .record_event(Some(&id), "download_paused", serde_json::json!({}))
            .await;
        self.bus.publish(DownloadEvent::DownloadPaused { id });
        Ok(())
    }

    async fn resume_download(&self, id: DownloadId) -> Result<()> {
        let dl = self
            .repo
            .get(&id)
            .await?
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        if !dl.status.is_resumable() && dl.status != DownloadStatus::Pending {
            return Err(AriaFlowError::InvalidTransition {
                from: dl.status.to_string(),
                to: DownloadStatus::Queued.to_string(),
            });
        }
        self.set_status(&id, DownloadStatus::Queued, None).await?;
        self.bus.publish(DownloadEvent::DownloadResumed { id });
        Ok(())
    }

    async fn cancel_download(&self, id: DownloadId) -> Result<()> {
        let dl = self
            .repo
            .get(&id)
            .await?
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        self.transition_to(&dl, DownloadStatus::Cancelled)?;
        self.set_status(&id, DownloadStatus::Cancelled, None)
            .await?;
        let _ = self.history.record(&dl, "cancelled", None).await;
        self.bus.publish(DownloadEvent::DownloadCancelled { id });
        Ok(())
    }

    async fn remove_download(&self, id: DownloadId, delete_file: bool) -> Result<()> {
        let dl = self
            .repo
            .get(&id)
            .await?
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        if delete_file {
            // Remove the partial file (and segment side files) plus the
            // final target, best effort.
            let partial =
                std::path::PathBuf::from(format!("{}.ariaflow-part", dl.save_path.display()));
            let _ = std::fs::remove_file(&partial);
            let _ = std::fs::remove_file(ariaflow_core::segment::manifest_path(&partial));
            if let Some(name) = partial.file_name().and_then(|n| n.to_str()) {
                let prefix = format!("{name}.seg-");
                if let Some(parent) = partial.parent() {
                    if let Ok(entries) = std::fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            if entry
                                .file_name()
                                .to_str()
                                .is_some_and(|n| n.starts_with(&prefix))
                            {
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                }
            }
            if dl.status == DownloadStatus::Completed {
                let _ = std::fs::remove_file(&dl.save_path);
            }
        }
        self.repo.delete(&id).await?;
        self.bus.publish(DownloadEvent::DownloadRemoved { id });
        Ok(())
    }

    async fn retry_download(&self, id: DownloadId) -> Result<()> {
        let dl = self
            .repo
            .get(&id)
            .await?
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        if !matches!(
            dl.status,
            DownloadStatus::Failed | DownloadStatus::Cancelled
        ) {
            return Err(AriaFlowError::InvalidTransition {
                from: dl.status.to_string(),
                to: DownloadStatus::Queued.to_string(),
            });
        }
        self.repo.reset_for_retry(&id).await?;
        self.bus.publish(DownloadEvent::DownloadCreated { id });
        Ok(())
    }

    async fn get_download(&self, id: DownloadId) -> Result<Option<Download>> {
        let mut dl = self.repo.get(&id).await?;
        if let Some(dl) = dl.as_mut() {
            dl.segments = self.live_or_persisted_segments(dl).await?;
        }
        Ok(dl)
    }

    async fn list_downloads(&self, filter: DownloadFilter) -> Result<Vec<Download>> {
        let mut items = self.repo.list().await?;
        apply_filter_in_memory(&mut items, &filter);
        Ok(items)
    }

    async fn counts(&self) -> Result<DownloadCounts> {
        self.repo.counts().await
    }

    async fn set_priority(&self, id: DownloadId, priority: i32) -> Result<()> {
        if !self.repo.update_priority(&id, priority).await? {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        Ok(())
    }

    async fn set_limit_rate(&self, id: DownloadId, limit: Option<u64>) -> Result<()> {
        if !self.repo.update_limit_rate(&id, limit).await? {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        Ok(())
    }

    async fn replace_segments(
        &self,
        id: DownloadId,
        segments: Vec<ariaflow_core::models::SegmentInfo>,
    ) -> Result<()> {
        self.repo.replace_segments(&id, &segments).await
    }

    async fn get_settings(&self) -> Result<GlobalSettings> {
        Ok(Self::load_settings_inner(&self.settings).await)
    }

    async fn list_history(&self, limit: u32) -> Result<Vec<ariaflow_core::HistoryEntry>> {
        let limit = limit.clamp(1, 500);
        self.history.recent(limit).await
    }

    async fn get_history(&self, id: i64) -> Result<Option<ariaflow_core::HistoryEntry>> {
        self.history.get(id).await
    }

    async fn delete_history(&self, ids: &[i64]) -> Result<u64> {
        self.history.delete_ids(ids).await
    }

    async fn clear_history(&self) -> Result<u64> {
        self.history.clear().await
    }

    async fn list_proxies(&self) -> Result<Vec<ariaflow_core::ProxyEntry>> {
        self.proxies.list().await
    }

    async fn create_proxy(
        &self,
        input: ariaflow_core::ProxyInput,
    ) -> Result<ariaflow_core::ProxyEntry> {
        self.proxies.insert(&input.validated()?).await
    }

    async fn update_proxy(
        &self,
        id: &str,
        input: ariaflow_core::ProxyInput,
    ) -> Result<ariaflow_core::ProxyEntry> {
        self.proxies.update(id, &input.validated()?).await
    }

    async fn delete_proxy(&self, id: &str) -> Result<()> {
        self.proxies.delete(id).await
    }

    async fn list_ssh_certificates(&self) -> Result<Vec<ariaflow_core::SshCertificateEntry>> {
        self.ssh_certificates.list().await
    }

    async fn create_ssh_certificate(
        &self,
        input: ariaflow_core::SshCertificateInput,
    ) -> Result<ariaflow_core::SshCertificateEntry> {
        self.ssh_certificates.insert(&input).await
    }

    async fn update_ssh_certificate(
        &self,
        id: &str,
        input: ariaflow_core::SshCertificateInput,
    ) -> Result<ariaflow_core::SshCertificateEntry> {
        self.ssh_certificates.update(id, &input).await
    }

    async fn delete_ssh_certificate(&self, id: &str) -> Result<()> {
        self.ssh_certificates.delete(id).await
    }

    async fn update_settings(&self, patch: UpdateSettingsRequest) -> Result<GlobalSettings> {
        let mut current = Self::load_settings_inner(&self.settings).await;
        current.apply_patch(&patch);
        current.validate()?;
        let serialized = serde_json::to_string(&current)?;
        self.settings
            .set_json(GLOBAL_SETTINGS_KEY, &serialized)
            .await?;
        Ok(current)
    }
}
