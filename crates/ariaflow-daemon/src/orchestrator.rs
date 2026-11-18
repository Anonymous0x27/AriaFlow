//! The AriaFlow download orchestrator.
//!
//! This is the loop that turns persisted `queued` tasks into bytes on disk:
//!
//! ```text
//!   Queued ──▶ Connecting ──▶ Downloading ──▶ Merging ──▶ Verifying ──▶ Completed
//!                    │              │              │            │
//!                    └─▶ Failed/Retrying/Paused    └── Paused/Cancelled (user abort)
//! ```
//!
//! Design:
//! - The orchestrator is the only code that executes transfers. Clients (GUI,
//!   CLI, TUI, API, MCP) never download bytes themselves; they flip task state
//!   in the database, and the event watcher here trips the in-flight transfer.
//! - Tasks are picked in scheduler order (priority desc, then oldest first)
//!   and bounded by `max_concurrent_tasks` from the global settings.
//! - Partial bytes live in `<save_path>.ariaflow-part`; crash recovery flips
//!   interrupted tasks back to `queued` at startup and the providers resume
//!   from the partial file (HTTP Range, FTP REST, SFTP seek, BT piece resume).
//! - Resume safety: a stored ETag that changed, or a partial file bigger than
//!   the probed size, invalidates the partial instead of appending garbage.
//! - Checksums: `task.expected_checksum` wins; otherwise an `x-checksum-*`
//!   header discovered during the probe is used (Metalink manifests flow
//!   through this path automatically).

use ariaflow_core::{
    task_start_permitted, Checksum, ChecksumAlgorithm, Download, DownloadEvent, DownloadFilter,
    DownloadId, DownloadService, DownloadStatus,
};
use ariaflow_engine::SpeedTracker;
use ariaflow_protocol::{ProviderOptions, ProviderRegistry, TransferProgress};
use ariaflow_scheduler::{effective_max_connections, pick_tasks, retry_backoff};
use ariaflow_storage::SqliteDownloadService;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TICK: Duration = Duration::from_millis(750);
const PROGRESS_FLUSH: Duration = Duration::from_millis(500);
/// Base/cap for the in-memory retry backoff ladder (seconds).
const RETRY_BACKOFF_BASE_SECS: u64 = 2;
const RETRY_BACKOFF_MAX_SECS: u64 = 300;

/// Shared abort registry: task id → abort flag. Inserted synchronously in
/// `tick` before a worker is spawned so a task is never executed twice.
#[derive(Clone, Default)]
struct AbortRegistry {
    inner: Arc<Mutex<HashMap<DownloadId, Arc<AtomicBool>>>>,
}

impl AbortRegistry {
    /// Always registers a fresh, unset flag and replaces any stale entry.
    ///
    /// A paused run leaves its flag set to `true`; if the next run for the same
    /// id reused that entry (the old code's `or_insert_with`), the resumed
    /// transfer would inherit `abort == true`, download zero bytes and strand
    /// the task in `downloading` forever. Replacing unconditionally is safe:
    /// the old worker keeps its own clone of the stale (set) flag, and the new
    /// worker starts from a clean slate.
    fn insert(&self, id: DownloadId) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.inner.lock().unwrap().insert(id, Arc::clone(&flag));
        flag
    }
    fn abort(&self, id: DownloadId) {
        if let Some(flag) = self.inner.lock().unwrap().get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
    fn remove(&self, id: DownloadId) {
        self.inner.lock().unwrap().remove(&id);
    }
    fn active_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
    fn active_ids(&self) -> Vec<DownloadId> {
        self.inner.lock().unwrap().keys().copied().collect()
    }
}

pub struct Orchestrator {
    service: Arc<SqliteDownloadService>,
    registry: Arc<ProviderRegistry>,
    aborts: AbortRegistry,
    /// Shared aggregate limiter for the global speed cap; rebuilt when the
    /// configured rate changes. Tasks already running keep the bucket they
    /// cloned; new tasks pick up the new rate.
    global_limiter: std::sync::Mutex<Option<(u64, Arc<ariaflow_protocol::throttle::RateLimiter>)>>,
}

impl Orchestrator {
    pub fn new(service: Arc<SqliteDownloadService>, registry: Arc<ProviderRegistry>) -> Arc<Self> {
        Arc::new(Self {
            service,
            registry,
            aborts: AbortRegistry::default(),
            global_limiter: std::sync::Mutex::new(None),
        })
    }

    /// Spawns the main loop and the event watcher. Returns immediately.
    pub fn spawn(self: &Arc<Self>) {
        // Watcher: user actions (pause/cancel/remove) travel over the event bus;
        // trip the matching in-flight transfer flag.
        let watcher = Arc::clone(self);
        tokio::spawn(async move {
            watcher.watch_events().await;
        });
        let runner = Arc::clone(self);
        tokio::spawn(async move {
            runner.run().await;
        });
    }

    async fn run(self: Arc<Self>) {
        match self.service.engine_reset_interrupted().await {
            Ok(n) if n > 0 => {
                tracing::info!(count = n, "recovered interrupted transfers; re-queued")
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "crash recovery sweep failed"),
        }
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::warn!(error = %e, "orchestrator tick failed");
            }
        }
    }

    /// Trips abort flags for in-flight tasks whose database status no longer
    /// reflects an active transfer (belt-and-braces behind the event watcher).
    async fn watchdog(&self) {
        for id in self.aborts.active_ids() {
            match self.service.get_download(id).await {
                Ok(Some(dl)) => {
                    if matches!(
                        dl.status,
                        DownloadStatus::Paused | DownloadStatus::Cancelled | DownloadStatus::Failed
                    ) {
                        self.aborts.abort(id);
                    }
                }
                Ok(None) => self.aborts.abort(id), // removed from the database
                Err(_) => {}
            }
        }
    }

    async fn tick(self: &Arc<Self>) -> ariaflow_core::Result<()> {
        self.watchdog().await;
        let settings = self.service.get_settings().await?;
        let active = self.aborts.active_count();
        let slots = (settings.max_concurrent_tasks as usize).saturating_sub(active);
        if slots == 0 {
            return Ok(());
        }
        let queued = self
            .service
            .list_downloads(DownloadFilter {
                statuses: vec![DownloadStatus::Queued],
                ..DownloadFilter::unfiltered()
            })
            .await?;
        let now_local = chrono::Local::now();
        let mut deferred = 0usize;
        let eligible: Vec<Download> = queued
            .into_iter()
            .filter(|dl| {
                let ok = task_start_permitted(dl, &settings, now_local);
                if !ok {
                    deferred += 1;
                }
                ok
            })
            .collect();
        if deferred > 0 {
            tracing::debug!(deferred, "queued tasks held back by scheduling rules");
        }
        let picked = pick_tasks(eligible.iter(), slots);
        for id in picked {
            // Register before spawning so the next tick cannot double-launch it.
            self.aborts.insert(id);
            let worker = Arc::clone(self);
            tokio::spawn(async move {
                worker.run_task(id).await;
            });
        }
        Ok(())
    }

    async fn watch_events(self: Arc<Self>) {
        let mut rx = self.service.bus().subscribe();
        loop {
            match rx.recv().await {
                Ok(DownloadEvent::DownloadPaused { id })
                | Ok(DownloadEvent::DownloadCancelled { id })
                | Ok(DownloadEvent::DownloadRemoved { id }) => self.aborts.abort(id),
                Ok(DownloadEvent::DownloadFailed { id, .. }) => self.aborts.abort(id),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!(skipped = n, "orchestrator event watcher lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    async fn run_task(self: Arc<Self>, id: DownloadId) {
        let result = self.execute(id).await;
        match result {
            Ok(()) | Err(Stage::Aborted) => {}
            Err(Stage::InvalidTransition) => {
                // The user changed the task state concurrently; leave it alone.
                tracing::debug!(task = %id, "task state changed underneath us; stopping");
            }
            Err(Stage::Failed(message)) => self.handle_failure(id, &message).await,
            Err(Stage::FailedPermanent(message)) => self.fail_permanently(id, &message).await,
        }
        self.aborts.remove(id);
    }

    /// Terminal failure path: mark `Failed`, archive history, notify, run the
    /// error hook. Shared by the retry-budget-exhausted branch and by
    /// permanent errors that must not be retried at all.
    async fn fail_permanently(&self, id: DownloadId, message: &str) {
        let dl = match self.service.get_download(id).await {
            Ok(Some(d)) => d,
            _ => return,
        };
        let settings = self.service.get_settings().await.unwrap_or_default();
        let _ = self
            .service
            .engine_transition(&id, DownloadStatus::Failed, Some(message))
            .await;
        let _ = self.service.engine_idle(&id).await;
        let _ = self
            .service
            .engine_record_history(&id, "failed", Some(message))
            .await;
        self.service.bus().publish(DownloadEvent::DownloadFailed {
            id,
            error: message.to_string(),
        });
        spawn_hook(
            settings.on_download_error_command.as_deref(),
            "failed",
            &dl,
            Some(message),
        );
    }

    async fn handle_failure(&self, id: DownloadId, message: &str) {
        let dl = match self.service.get_download(id).await {
            Ok(Some(d)) => d,
            _ => return,
        };
        let settings = self.service.get_settings().await.unwrap_or_default();
        let max_retries = dl.max_retries.unwrap_or(settings.retry_count);
        let attempt = dl.retry_count + 1;
        if attempt > max_retries {
            self.fail_permanently(id, message).await;
            return;
        }
        // Persist the Retrying state and schedule the requeue outside this task.
        let new_attempt = self.service.engine_bump_retry(&id).await.unwrap_or(attempt);
        let _ = self
            .service
            .engine_transition(&id, DownloadStatus::Retrying, Some(message))
            .await;
        let _ = self.service.engine_idle(&id).await;
        let delay = retry_backoff(
            new_attempt.saturating_sub(1),
            RETRY_BACKOFF_BASE_SECS,
            RETRY_BACKOFF_MAX_SECS,
        );
        tracing::info!(task = %id, attempt = new_attempt, delay_secs = delay.as_secs(), "scheduling retry");
        let service = Arc::clone(&self.service);
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Ok(true) = service.engine_requeue_if_retrying(&id).await {
                tracing::info!(task = %id, "task re-queued for retry");
            }
        });
    }

    async fn execute(&self, id: DownloadId) -> Result<(), Stage> {
        let dl = self
            .service
            .get_download(id)
            .await
            .map_err(|e| Stage::Failed(storage_msg(&e)))?
            .ok_or(Stage::Aborted)?; // removed before we started
        if dl.status != DownloadStatus::Queued {
            return Ok(()); // user changed it after the pick; nothing to do
        }
        let abort = self.aborts.insert(id);

        self.service
            .engine_transition(&id, DownloadStatus::Connecting, None)
            .await
            .map_err(map_state_err)?;

        let settings = self
            .service
            .get_settings()
            .await
            .map_err(|e| Stage::Failed(storage_msg(&e)))?;

        let provider = self
            .registry
            .provider_for(&dl.url)
            .map_err(|e| Stage::Failed(format!("no provider registered for this URL: {e}")))?;

        // Per-task cap: explicit task value wins, then the global per-task
        // default from settings. The *global* cap is separate: it paces the
        // aggregate of all tasks through a shared token bucket (aria2-style
        // --max-overall-download-limit).
        let per_task_limit = dl
            .limit_rate_bytes_per_sec
            .unwrap_or(settings.per_task_speed_limit);
        let shared_limiter = if settings.global_speed_limit > 0 {
            let mut guard = self
                .global_limiter
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let limiter = match guard.as_ref() {
                Some((rate, limiter)) if *rate == settings.global_speed_limit => {
                    Arc::clone(limiter)
                }
                _ => {
                    let limiter = Arc::new(ariaflow_protocol::throttle::RateLimiter::new(
                        settings.global_speed_limit,
                    ));
                    *guard = Some((settings.global_speed_limit, Arc::clone(&limiter)));
                    limiter
                }
            };
            Some(limiter)
        } else {
            None
        };
        // Segment planning: per-task preference bounded by the per-task cap and
        // the global connection budget (0 = unlimited -> saturate at u16::MAX).
        let remaining_global = if settings.max_total_connections == 0 {
            u16::MAX
        } else {
            settings.max_total_connections
        };
        let max_connections = effective_max_connections(
            dl.max_connections.max(1),
            settings.max_connections_per_task.max(1),
            remaining_global,
        );

        // IDM-style per-task authentication: translate the stored AuthConfig
        // into an Authorization header (unless the user already set one via
        // custom headers). Basic credentials are base64(user:password).
        let mut option_headers: Vec<(String, String)> =
            std::iter::once(("accept".to_string(), "*/*".to_string()))
                .chain(dl.headers.iter().map(|(k, v)| (k.clone(), v.clone())))
                .collect();
        if !option_headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        {
            match &dl.auth {
                Some(ariaflow_core::AuthConfig::Basic { username, password }) => {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    option_headers.push((
                        "authorization".to_string(),
                        format!(
                            "Basic {}",
                            STANDARD.encode(format!("{username}:{password}"))
                        ),
                    ));
                }
                Some(ariaflow_core::AuthConfig::Bearer { token }) => {
                    option_headers.push(("authorization".to_string(), format!("Bearer {token}")));
                }
                Some(ariaflow_core::AuthConfig::None) | None => {}
            }
        }

        // Certificate authentication (ssh/sftp): resolve the selected
        // certificate to key material here, in memory, right before the
        // transfer. A missing/unreadable certificate fails the task with a
        // permanent error — retrying cannot make it appear.
        let ssh_certificate = match &dl.ssh_certificate_id {
            Some(certificate_id) => {
                match self
                    .service
                    .ssh_certificate_material(certificate_id)
                    .await
                {
                    Ok(Some((name, private_key, passphrase))) => {
                        Some(ariaflow_protocol::SshCertificateAuth {
                            name,
                            private_key,
                            passphrase,
                        })
                    }
                    Ok(None) => {
                        return Err(Stage::FailedPermanent(format!(
                            "configured ssh certificate {certificate_id} no longer exists"
                        )));
                    }
                    Err(e) => {
                        return Err(Stage::Failed(format!(
                            "failed to load ssh certificate {certificate_id}: {e}"
                        )));
                    }
                }
            }
            None => None,
        };

        let options = ProviderOptions {
            headers: option_headers,
            proxy: dl.proxy.clone(),
            user_agent: Some(
                dl.user_agent
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| settings.user_agent.clone()),
            ),
            connect_timeout_secs: settings.connect_timeout_secs,
            read_timeout_secs: settings.read_timeout_secs,
            danger_accept_invalid_certs: false,
            extra_ca_certs: Vec::new(),
            rate_limit_bytes_per_sec: per_task_limit,
            max_connections,
            min_segment_size: settings.min_segment_size,
            ssh_certificate,
            shared_limiter,
        };

        // ---- Probe ----------------------------------------------------------
        let info = match provider.probe(&dl.url, &options).await {
            Ok(info) => info,
            Err(e) => {
                let message = if permanent_probe_error(&e) {
                    format!("probe failed: {e}")
                } else {
                    format!("probe failed (will retry): {e}")
                };
                return Err(if permanent_probe_error(&e) {
                    Stage::FailedPermanent(message)
                } else {
                    Stage::Failed(message)
                });
            }
        };
        let final_url = Some(info.final_url.clone());
        self.service
            .engine_record_probe(
                &id,
                final_url.as_deref(),
                info.total_size,
                info.etag.as_deref(),
                info.last_modified.as_deref(),
            )
            .await
            .map_err(|e| Stage::Failed(storage_msg(&e)))?;

        // Resume safety: a changed ETag or an impossible partial size invalidates
        // whatever is on disk instead of appending to a stale prefix.
        let part_path = partial_path(&dl);
        if let Ok(meta) = tokio::fs::metadata(&part_path).await {
            let stale = match (&dl.etag, &info.etag) {
                (Some(old), Some(new)) if old != new => true,
                _ => match info.total_size {
                    Some(total) => meta.len() > total,
                    None => false,
                },
            };
            if stale {
                tracing::warn!(task = %id, "partial file invalidated (etag/size changed); restarting");
                remove_part_artifacts(&part_path).await;
                let _ = self.service.engine_idle(&id).await;
            }
        }
        // Crash between "transfer finished" and "merged": the partial file is
        // already byte-complete — skip the network entirely.
        let part_len = tokio::fs::metadata(&part_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let part_already_complete = matches!(info.total_size, Some(t) if t > 0 && part_len == t);

        // ---- Download -------------------------------------------------------
        self.service
            .engine_transition(&id, DownloadStatus::Downloading, None)
            .await
            .map_err(map_state_err)?;
        self.service
            .bus()
            .publish(DownloadEvent::DownloadStarted { id });

        let (mut outcome, final_downloaded, final_total) = if part_already_complete {
            tracing::info!(task = %id, "partial file already complete; skipping network transfer");
            let avg_speed = average_speed(dl.started_at.as_ref(), part_len);
            let _ = self
                .service
                .engine_record_progress(&id, part_len, info.total_size, 0, avg_speed, 100.0)
                .await;
            (
                ariaflow_protocol::TransferOutcome {
                    bytes_written: part_len,
                    resumed_from: part_len,
                    segment_snapshot: None,
                },
                part_len,
                info.total_size,
            )
        } else {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<TransferProgress>();
            let forwarder = spawn_progress_forwarder(self.service.clone(), id, info.total_size, rx);

            let progress_cb: Arc<dyn Fn(TransferProgress) + Send + Sync> = Arc::new({
                let tx = tx.clone();
                move |p| {
                    let _ = tx.send(p);
                }
            });
            let abort_flag = Arc::clone(&abort);
            let should_abort: Arc<dyn Fn() -> bool + Send + Sync> =
                Arc::new(move || abort_flag.load(Ordering::Relaxed));
            if let Some(parent) = dl.save_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            let raw_outcome = provider
                .download(&dl.url, &options, &part_path, progress_cb, should_abort)
                .await;
            // `download` consumed (and dropped) progress_cb; dropping the original
            // handle too closes the channel so the forwarder can finish.
            drop(tx);
            let (final_downloaded, final_total) = forwarder.await.unwrap_or((0, info.total_size));

            let outcome = match raw_outcome {
                Ok(o) => o,
                Err(ariaflow_protocol::ProviderError::Aborted) => {
                    let _ = self.service.engine_idle(&id).await;
                    return Err(Stage::Aborted);
                }
                Err(e) => return Err(Stage::Failed(format!("transfer failed: {e}"))),
            };
            (outcome, final_downloaded, final_total)
        };

        if abort.load(Ordering::Relaxed) {
            // Aborted right at the finish line; the partial stays resumable.
            let _ = self.service.engine_idle(&id).await;
            return Err(Stage::Aborted);
        }

        // ---- Merge (rename .ariaflow-part into place) -----------------------
        self.service
            .engine_transition(&id, DownloadStatus::Merging, None)
            .await
            .map_err(map_state_err)?;
        if let Err(e) = finalize_artifact(&part_path, &dl.save_path).await {
            return Err(Stage::Failed(format!(
                "failed to move {} into place: {e}",
                part_path.display()
            )));
        }

        // ---- Verify (checksum) ----------------------------------------------
        let expected_checksum = dl.checksum.clone().or_else(|| {
            Checksum::from_headers(info.headers.iter().map(|(k, v)| (k.clone(), v.clone())))
        });
        match expected_checksum {
            Some(expected) => {
                self.service
                    .engine_transition(&id, DownloadStatus::Verifying, None)
                    .await
                    .map_err(map_state_err)?;
                let actual = hash_file(&dl.save_path, expected.algorithm).await;
                match actual {
                    Ok(digest) if digest.eq_ignore_ascii_case(&expected.digest) => {}
                    Ok(digest) => {
                        return Err(Stage::Failed(format!(
                            "checksum mismatch ({}): expected {}, got {}",
                            expected.algorithm.as_str(),
                            expected.digest,
                            digest
                        )));
                    }
                    Err(e) => {
                        return Err(Stage::Failed(format!(
                            "checksum verification failed to read {}: {e}",
                            dl.save_path.display()
                        )));
                    }
                }
                // Verifying -> Completed
                self.service
                    .engine_transition(&id, DownloadStatus::Completed, None)
                    .await
                    .map_err(map_state_err)?;
            }
            None => {
                // Merging -> Completed
                self.service
                    .engine_transition(&id, DownloadStatus::Completed, None)
                    .await
                    .map_err(map_state_err)?;
            }
        }

        if let Some(segments) = outcome.segment_snapshot.take() {
            // Persist the final per-chunk state for the task detail view.
            if let Err(e) = self.service.replace_segments(id, segments).await {
                tracing::warn!(task = %id, "failed to persist segment snapshot: {e}");
            }
        }
        let avg_speed = average_speed(dl.started_at.as_ref(), outcome.bytes_written);
        let final_total = final_total.or(Some(outcome.bytes_written));
        self.service
            .engine_finish(
                &id,
                outcome.bytes_written.max(final_downloaded),
                final_total,
                avg_speed,
            )
            .await
            .map_err(|e| Stage::Failed(storage_msg(&e)))?;
        let _ = self
            .service
            .engine_record_history(&id, "completed", None)
            .await;
        if let Ok(settings) = self.service.get_settings().await {
            spawn_hook(
                settings.on_download_complete_command.as_deref(),
                "completed",
                &dl,
                None,
            );
        }
        Ok(())
    }
}

/// IDM-style post-processing hook (a.k.a. aria2 `on-download-complete` /
/// JDownloader event scripts / IDM's "antivirus check").
///
/// Runs a user-configured shell command after a task reaches a terminal
/// state. Fire-and-forget: stdio is detached (so a chatty hook can never
/// wedge the daemon), with a hard 10-minute cap. Environment:
/// `ARIAFLOW_RESULT`, `ARIAFLOW_ID`, `ARIAFLOW_URL`, `ARIAFLOW_FINAL_URL`,
/// `ARIAFLOW_FILE`, `ARIAFLOW_FILENAME`, `ARIAFLOW_TOTAL_BYTES`, `ARIAFLOW_ERROR`.
fn spawn_hook(command: Option<&str>, result: &'static str, dl: &Download, error: Option<&str>) {
    let Some(command) = command.map(str::trim).filter(|c| !c.is_empty()) else {
        return;
    };
    let command = command.to_string();
    let envs: Vec<(String, String)> = vec![
        ("ARIAFLOW_RESULT".to_string(), result.to_string()),
        ("ARIAFLOW_ID".to_string(), dl.id.to_string()),
        ("ARIAFLOW_URL".to_string(), dl.url.clone()),
        (
            "ARIAFLOW_FINAL_URL".to_string(),
            dl.final_url.clone().unwrap_or_else(|| dl.url.clone()),
        ),
        (
            "ARIAFLOW_FILE".to_string(),
            dl.save_path.display().to_string(),
        ),
        ("ARIAFLOW_FILENAME".to_string(), dl.filename.clone()),
        (
            "ARIAFLOW_TOTAL_BYTES".to_string(),
            dl.total_size.map(|n| n.to_string()).unwrap_or_default(),
        ),
        (
            "ARIAFLOW_ERROR".to_string(),
            error.unwrap_or_default().to_string(),
        ),
    ];
    tracing::info!(task = %dl.id, result, "running post-download hook");
    tokio::spawn(async move {
        let mut cmd = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(&command);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c").arg(&command);
            c
        };
        cmd.envs(envs)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                tracing::warn!(command = %command, error = %e, "post-download hook failed to spawn");
                return;
            }
        };
        let waited = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            child.wait_with_output(),
        )
        .await;
        match waited {
            Ok(Ok(output)) => {
                tracing::info!(code = ?output.status.code(), "post-download hook exited")
            }
            Ok(Err(e)) => tracing::warn!(error = %e, "post-download hook wait failed"),
            Err(_) => tracing::warn!("post-download hook killed after 10 minutes"),
        }
    });
}

/// Remove `<part>` and every `<part>.seg-NNN` side file (segmented transfers).
async fn remove_part_artifacts(part_path: &std::path::Path) {
    let _ = tokio::fs::remove_file(part_path).await;
    let _ = tokio::fs::remove_file(ariaflow_core::segment::manifest_path(part_path)).await;
    if let Some(name) = part_path.file_name().and_then(|n| n.to_str()) {
        if let Some(parent) = part_path.parent() {
            let prefix = format!("{name}.seg-");
            if let Ok(mut entries) = tokio::fs::read_dir(parent).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if entry
                        .file_name()
                        .to_str()
                        .map(|n| n.starts_with(&prefix))
                        .unwrap_or(false)
                    {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }
}

fn partial_path(dl: &Download) -> PathBuf {
    PathBuf::from(format!("{}.ariaflow-part", dl.save_path.display()))
}

/// Rename `part` onto `final_path`, falling back to copy+delete across filesystems.
async fn finalize_artifact(
    part: &std::path::Path,
    final_path: &std::path::Path,
) -> std::io::Result<()> {
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    match tokio::fs::rename(part, final_path).await {
        Ok(()) => Ok(()),
        // `rename` fails across filesystems (EXDEV / ERROR_NOT_SAME_DEVICE) and in
        // some directory-rename edge cases; fall back to copy + remove. The copy
        // path surfaces any real error.
        Err(_) => {
            if part.is_dir() {
                copy_dir(part, final_path)?;
                std::fs::remove_dir_all(part)?;
            } else {
                tokio::fs::copy(part, final_path).await?;
                tokio::fs::remove_file(part).await?;
            }
            Ok(())
        }
    }
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn permanent_probe_error(e: &ariaflow_protocol::ProviderError) -> bool {
    use ariaflow_protocol::ProviderError::*;
    match e {
        BadStatus { status, .. } => (400..500).contains(status) && *status != 408 && *status != 429,
        Unsupported(_) | Authentication(_) => true,
        _ => false,
    }
}

fn storage_msg(e: &ariaflow_core::AriaFlowError) -> String {
    e.to_string()
}

fn map_state_err(e: ariaflow_core::AriaFlowError) -> Stage {
    match e {
        ariaflow_core::AriaFlowError::InvalidTransition { .. } => Stage::InvalidTransition,
        other => Stage::Failed(other.to_string()),
    }
}

fn average_speed(started_at: Option<&chrono::DateTime<chrono::Utc>>, bytes: u64) -> u64 {
    match started_at {
        Some(t) => {
            let secs = (chrono::Utc::now() - *t).num_milliseconds().max(1) as f64 / 1000.0;
            (bytes as f64 / secs) as u64
        }
        None => 0,
    }
}

/// Forwards provider progress into the database + event bus, throttled to ~2 Hz.
/// Returns the last observed (downloaded_bytes, total_bytes) when the provider drops
/// its progress handle.
fn spawn_progress_forwarder(
    service: Arc<SqliteDownloadService>,
    id: DownloadId,
    total_hint: Option<u64>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransferProgress>,
) -> tokio::task::JoinHandle<(u64, Option<u64>)> {
    tokio::spawn(async move {
        let mut tracker = SpeedTracker::new();
        let mut last_flush = std::time::Instant::now() - PROGRESS_FLUSH;
        let mut last: Option<TransferProgress> = None;
        while let Some(p) = rx.recv().await {
            tracker.record(p.downloaded_bytes);
            last = Some(p);
            if last_flush.elapsed() >= PROGRESS_FLUSH {
                last_flush = std::time::Instant::now();
                let total = p.total_bytes.or(total_hint);
                let speed = tracker.instant_speed();
                let avg = tracker.average_speed(p.downloaded_bytes);
                let progress = match total {
                    Some(t) if t > 0 => (p.downloaded_bytes.min(t) as f64 / t as f64) * 100.0,
                    _ => 0.0,
                };
                let _ = service
                    .engine_record_progress(&id, p.downloaded_bytes, total, speed, avg, progress)
                    .await;
            }
        }
        match last {
            Some(p) => {
                let total = p.total_bytes.or(total_hint);
                let speed = tracker.instant_speed();
                let avg = tracker.average_speed(p.downloaded_bytes);
                let progress = match total {
                    Some(t) if t > 0 => (p.downloaded_bytes.min(t) as f64 / t as f64) * 100.0,
                    _ => 0.0,
                };
                let _ = service
                    .engine_record_progress(&id, p.downloaded_bytes, total, speed, avg, progress)
                    .await;
                (p.downloaded_bytes, total)
            }
            None => (0, total_hint),
        }
    })
}

/// Streaming file hash for checksum verification.
async fn hash_file(
    path: &std::path::Path,
    algorithm: ChecksumAlgorithm,
) -> std::io::Result<String> {
    use sha2::Digest;
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 1024 * 1024];
    let mut md5 = md5::Md5::new();
    let mut sha1 = sha1::Sha1::new();
    let mut sha256 = sha2::Sha256::new();
    let mut sha512 = sha2::Sha512::new();
    let mut blake3 = blake3::Hasher::new();
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        match algorithm {
            ChecksumAlgorithm::Md5 => md5.update(chunk),
            ChecksumAlgorithm::Sha1 => sha1.update(chunk),
            ChecksumAlgorithm::Sha256 => sha256.update(chunk),
            ChecksumAlgorithm::Sha512 => sha512.update(chunk),
            ChecksumAlgorithm::Blake3 => {
                blake3.update(chunk);
            }
        }
    }
    Ok(match algorithm {
        ChecksumAlgorithm::Md5 => format!("{:x}", md5.finalize()),
        ChecksumAlgorithm::Sha1 => format!("{:x}", sha1.finalize()),
        ChecksumAlgorithm::Sha256 => format!("{:x}", sha256.finalize()),
        ChecksumAlgorithm::Sha512 => format!("{:x}", sha512.finalize()),
        ChecksumAlgorithm::Blake3 => blake3.finalize().to_hex().to_string(),
    })
}

/// Why a task stopped. `Aborted` covers user pause/cancel/removal — the database
/// is already consistent in those cases, so the worker stays silent.
enum Stage {
    Aborted,
    InvalidTransition,
    Failed(String),
    /// No point retrying (e.g. authentication rejected, certificate missing,
    /// permanent HTTP 4xx): fails the task immediately instead of burning
    /// through the retry budget.
    FailedPermanent(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_replaces_stale_abort_flag() {
        let registry = AbortRegistry::default();
        let id = DownloadId::new();

        // First (paused) run registers its flag and gets aborted by the user.
        let first = registry.insert(id);
        registry.abort(id);
        assert!(first.load(Ordering::Relaxed));

        // The resume API re-queues the task and the next worker registers its
        // flag *before* the paused worker's epilogue (`remove`) has run. The
        // resumed run must start from a clean, unset flag.
        let second = registry.insert(id);
        assert!(
            !second.load(Ordering::Relaxed),
            "resumed run must not inherit the paused run's abort flag"
        );
        // The paused worker still holds its own clone — the pending pause is
        // not lost for that worker.
        assert!(first.load(Ordering::Relaxed));
    }

    #[test]
    fn remove_drops_the_entry() {
        let registry = AbortRegistry::default();
        let id = DownloadId::new();
        let _ = registry.insert(id);
        registry.remove(id);
        assert_eq!(registry.active_count(), 0);
    }
}
