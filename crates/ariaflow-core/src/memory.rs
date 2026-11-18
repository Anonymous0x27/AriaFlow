//! In-memory `DownloadService` implementation.
//!
//! Used for development, tests and TUI/CLI bring-up before the storage-backed
//! daemon service is wired in. It enforces the real state machine and event
//! emission but never touches the network or the filesystem (except `delete_file`).

use crate::category::{classify_by_filename, filename_from_url};
use crate::error::{AriaFlowError, Result};
use crate::event::{DownloadEvent, EventBus};
use crate::filename::{safe_save_path, sanitize_filename};
use crate::filter::DownloadFilter;
use crate::ids::DownloadId;
use crate::models::{Download, NewDownloadRequest, ProxyEntry, ProxyInput};
use crate::service::{apply_filter_in_memory, DownloadCounts, DownloadService};
use crate::settings::{GlobalSettings, UpdateSettingsRequest};
use crate::status::DownloadStatus;
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct InMemoryDownloadService {
    inner: Mutex<HashMap<DownloadId, Download>>,
    settings: Mutex<GlobalSettings>,
    proxies: Mutex<Vec<ProxyEntry>>,
    bus: EventBus,
}

impl InMemoryDownloadService {
    pub fn new() -> Self {
        Self::with_bus(EventBus::default())
    }

    pub fn with_bus(bus: EventBus) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            settings: Mutex::new(GlobalSettings::default()),
            proxies: Mutex::new(Vec::new()),
            bus,
        }
    }

    pub fn bus(&self) -> EventBus {
        self.bus.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<DownloadId, Download>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn transition(dl: &mut Download, to: DownloadStatus) -> Result<()> {
        if !dl.status.can_transition_to(&to) {
            return Err(AriaFlowError::InvalidTransition {
                from: dl.status.to_string(),
                to: to.to_string(),
            });
        }
        dl.status = to;
        Ok(())
    }
}

impl Default for InMemoryDownloadService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DownloadService for InMemoryDownloadService {
    async fn add_download(&self, request: NewDownloadRequest) -> Result<Download> {
        let parsed = url::Url::parse(&request.url)?;
        if !matches!(
            parsed.scheme(),
            "http" | "https" | "ftp" | "ftps" | "sftp" | "ssh" | "magnet" | "metalink" | "file"
        ) {
            return Err(AriaFlowError::InvalidRequest(format!(
                "unsupported URL scheme: {}",
                parsed.scheme()
            )));
        }
        let settings = self.settings.lock().unwrap().clone();
        let raw_name = request
            .filename
            .clone()
            .or_else(|| filename_from_url(&parsed))
            .unwrap_or_else(|| "download".to_string());
        let filename = sanitize_filename(&raw_name);
        let save_dir = request
            .save_dir
            .clone()
            .unwrap_or_else(|| settings.download_dir.clone());
        let save_path = safe_save_path(&save_dir, &filename);

        let now = Utc::now();
        let initial_status = if request.start_paused {
            DownloadStatus::Pending
        } else {
            DownloadStatus::Queued
        };
        let dl = Download {
            id: DownloadId::new(),
            url: request.url.clone(),
            final_url: None,
            filename: filename.clone(),
            save_path,
            total_size: None,
            downloaded_size: 0,
            status: initial_status,
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
        };
        let mut map = self.lock();
        map.insert(dl.id, dl.clone());
        drop(map);
        self.bus
            .publish(DownloadEvent::DownloadCreated { id: dl.id });
        Ok(dl)
    }

    async fn pause_download(&self, id: DownloadId) -> Result<()> {
        let mut map = self.lock();
        let dl = map
            .get_mut(&id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        Self::transition(dl, DownloadStatus::Paused)?;
        dl.speed_bytes_per_sec = 0;
        dl.current_connections = 0;
        self.bus.publish(DownloadEvent::DownloadPaused { id });
        Ok(())
    }

    async fn resume_download(&self, id: DownloadId) -> Result<()> {
        let mut map = self.lock();
        let dl = map
            .get_mut(&id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        if !dl.status.is_resumable() && dl.status != DownloadStatus::Pending {
            return Err(AriaFlowError::InvalidTransition {
                from: dl.status.to_string(),
                to: DownloadStatus::Queued.to_string(),
            });
        }
        Self::transition(dl, DownloadStatus::Queued)?;
        self.bus.publish(DownloadEvent::DownloadResumed { id });
        Ok(())
    }

    async fn cancel_download(&self, id: DownloadId) -> Result<()> {
        let mut map = self.lock();
        let dl = map
            .get_mut(&id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        Self::transition(dl, DownloadStatus::Cancelled)?;
        dl.speed_bytes_per_sec = 0;
        dl.current_connections = 0;
        self.bus.publish(DownloadEvent::DownloadCancelled { id });
        Ok(())
    }

    async fn remove_download(&self, id: DownloadId, _delete_file: bool) -> Result<()> {
        let mut map = self.lock();
        if map.remove(&id).is_none() {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        self.bus.publish(DownloadEvent::DownloadRemoved { id });
        Ok(())
    }

    async fn retry_download(&self, id: DownloadId) -> Result<()> {
        let mut map = self.lock();
        let dl = map
            .get_mut(&id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        Self::transition(dl, DownloadStatus::Queued)?;
        dl.retry_count += 1;
        dl.error_message = None;
        dl.progress = 0.0;
        dl.downloaded_size = 0;
        self.bus.publish(DownloadEvent::DownloadCreated { id });
        Ok(())
    }

    async fn get_download(&self, id: DownloadId) -> Result<Option<Download>> {
        Ok(self.lock().get(&id).cloned())
    }

    async fn list_downloads(&self, filter: DownloadFilter) -> Result<Vec<Download>> {
        let mut items: Vec<Download> = self.lock().values().cloned().collect();
        apply_filter_in_memory(&mut items, &filter);
        Ok(items)
    }

    async fn counts(&self) -> Result<DownloadCounts> {
        let map = self.lock();
        let mut c = DownloadCounts {
            total: map.len() as u64,
            ..DownloadCounts::default()
        };
        for d in map.values() {
            match d.status {
                DownloadStatus::Downloading
                | DownloadStatus::Connecting
                | DownloadStatus::Merging
                | DownloadStatus::Verifying => {
                    c.downloading += 1;
                    c.global_speed_bytes_per_sec += d.speed_bytes_per_sec;
                }
                DownloadStatus::Queued
                | DownloadStatus::Pending
                | DownloadStatus::Retrying
                | DownloadStatus::Waiting => c.waiting += 1,
                DownloadStatus::Paused => c.paused += 1,
                DownloadStatus::Completed => c.completed += 1,
                DownloadStatus::Failed => c.failed += 1,
                DownloadStatus::Cancelled => c.cancelled += 1,
            }
            c.total_downloaded_bytes += d.downloaded_size;
        }
        Ok(c)
    }

    async fn set_limit_rate(&self, id: DownloadId, limit: Option<u64>) -> Result<()> {
        let mut map = self.lock();
        let dl = map
            .get_mut(&id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        dl.limit_rate_bytes_per_sec = limit;
        Ok(())
    }

    async fn set_priority(&self, id: DownloadId, priority: i32) -> Result<()> {
        let mut map = self.lock();
        let dl = map
            .get_mut(&id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        dl.priority = priority;
        Ok(())
    }

    async fn get_settings(&self) -> Result<GlobalSettings> {
        Ok(self.settings.lock().unwrap().clone())
    }

    async fn update_settings(&self, patch: UpdateSettingsRequest) -> Result<GlobalSettings> {
        let mut guard = self.settings.lock().unwrap();
        guard.apply_patch(&patch);
        guard.validate()?;
        Ok(guard.clone())
    }

    async fn list_proxies(&self) -> Result<Vec<ProxyEntry>> {
        let guard = self.proxies.lock().unwrap_or_else(|p| p.into_inner());
        let mut proxies = guard.clone();
        proxies.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        Ok(proxies)
    }

    async fn create_proxy(&self, input: ProxyInput) -> Result<ProxyEntry> {
        let input = input.validated()?;
        let mut guard = self.proxies.lock().unwrap_or_else(|p| p.into_inner());
        let entry = ProxyEntry {
            id: crate::ids::DownloadId::new().to_string(),
            name: input.name,
            kind: input.kind,
            address: input.address,
            username: input.username,
            password: input.password,
            bypass: input.bypass,
            is_default: input.is_default,
            created_at: Some(Utc::now().to_rfc3339()),
        };
        if entry.is_default {
            for p in guard.iter_mut() {
                p.is_default = false;
            }
        }
        guard.push(entry.clone());
        Ok(entry)
    }

    async fn update_proxy(&self, id: &str, input: ProxyInput) -> Result<ProxyEntry> {
        let input = input.validated()?;
        let mut guard = self.proxies.lock().unwrap_or_else(|p| p.into_inner());
        let entry = guard
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;
        entry.name = input.name;
        entry.kind = input.kind;
        entry.address = input.address;
        entry.username = input.username;
        entry.password = input.password;
        entry.bypass = input.bypass;
        entry.is_default = input.is_default;
        let updated = entry.clone();
        if updated.is_default {
            for p in guard.iter_mut() {
                if p.id != id {
                    p.is_default = false;
                }
            }
        }
        Ok(updated)
    }

    async fn delete_proxy(&self, id: &str) -> Result<()> {
        let mut guard = self.proxies.lock().unwrap_or_else(|p| p.into_inner());
        let before = guard.len();
        guard.retain(|p| p.id != id);
        if guard.len() == before {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        Ok(())
    }
}
