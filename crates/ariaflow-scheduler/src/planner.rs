//! Pure scheduling decisions.

use ariaflow_core::{Download, DownloadId};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    pub max_concurrent_tasks: u16,
    pub max_connections_per_task: u16,
    pub max_total_connections: u16,
    /// 0 = unlimited
    pub global_speed_limit: u64,
    /// 0 = unlimited
    pub per_task_speed_limit: u64,
    /// Automatic retries per task.
    pub retry_count: u32,
    /// Base of the exponential backoff ladder, in seconds.
    pub retry_backoff_base_secs: u64,
    /// Cap for the backoff ladder, in seconds.
    pub retry_backoff_max_secs: u64,
    /// Optional daily window during which downloads may run.
    pub allowed_time_window: Option<crate::window::TimeWindow>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 5,
            max_connections_per_task: 16,
            max_total_connections: 64,
            global_speed_limit: 0,
            per_task_speed_limit: 0,
            retry_count: 5,
            retry_backoff_base_secs: 2,
            retry_backoff_max_secs: 300,
            allowed_time_window: None,
        }
    }
}

/// How many tasks to launch given `slots` free task slots.
///
/// Selection order: higher `priority` first, ties broken by earlier `created_at`.
pub fn pick_tasks<'a>(
    candidates: impl Iterator<Item = &'a Download>,
    slots: usize,
) -> Vec<DownloadId> {
    let mut eligible: Vec<&Download> = candidates.collect();
    eligible.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
    eligible.into_iter().take(slots).map(|d| d.id).collect()
}

/// Connection budget for one task, respecting both per-task and global ceilings.
pub fn effective_max_connections(
    task_preference: u16,
    global_per_task_cap: u16,
    remaining_global_connections: u16,
) -> u16 {
    task_preference
        .min(global_per_task_cap)
        .min(remaining_global_connections)
        .max(1)
}

/// Exponential backoff: base * 2^attempt, clamped to `max_secs`. Attempt 0 -> base.
pub fn retry_backoff(attempt: u32, base_secs: u64, max_secs: u64) -> Duration {
    let shift = attempt.min(20); // avoid overflow
    let secs = base_secs
        .saturating_mul(1u64 << shift)
        .min(max_secs.max(base_secs));
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariaflow_core::{Download, DownloadStatus, DownloadSummary};
    use chrono::Utc;

    fn dl(priority: i32, created_offset_secs: i64) -> Download {
        let now = Utc::now();
        Download {
            id: DownloadId::new(),
            url: "https://example.com/f".into(),
            final_url: None,
            filename: "f".into(),
            save_path: "/tmp/f".into(),
            total_size: None,
            downloaded_size: 0,
            status: DownloadStatus::Queued,
            speed_bytes_per_sec: 0,
            average_speed_bytes_per_sec: 0,
            progress: 0.0,
            priority,
            retry_count: 0,
            max_connections: 16,
            current_connections: 0,
            category: "other".into(),
            etag: None,
            last_modified: None,
            checksum: None,
            proxy: None,
            auth: None,
            headers: vec![],
            create_subdirs: false,
            created_at: now + chrono::Duration::seconds(created_offset_secs),
            started_at: None,
            completed_at: None,
            error_message: None,
            segments: vec![],
            limit_rate_bytes_per_sec: None,
            max_retries: None,
            scheduled_start_at: None,
            user_agent: None,
            ssh_certificate_id: None,
        }
    }

    #[test]
    fn picks_by_priority_then_age() {
        let a = dl(0, -100);
        let b = dl(10, -1);
        let c = dl(10, -50);
        let d = dl(-5, -999);
        let items = [a.clone(), b.clone(), c.clone(), d.clone()];
        let picked = pick_tasks(items.iter(), 3);
        assert_eq!(picked, vec![c.id, b.id, a.id]);
    }

    #[test]
    fn connection_budget_respects_all_caps() {
        assert_eq!(effective_max_connections(16, 8, 64), 8);
        assert_eq!(effective_max_connections(16, 64, 4), 4);
        assert_eq!(effective_max_connections(2, 16, 64), 2);
        assert_eq!(effective_max_connections(0, 16, 64), 1);
    }

    #[test]
    fn backoff_ladder() {
        assert_eq!(retry_backoff(0, 2, 300), Duration::from_secs(2));
        assert_eq!(retry_backoff(3, 2, 300), Duration::from_secs(16));
        assert_eq!(retry_backoff(30, 2, 300), Duration::from_secs(300));
    }

    #[test]
    fn summary_type_is_usable_here() {
        let d = dl(1, 0);
        let s = DownloadSummary::from(&d);
        assert_eq!(s.priority, 1);
    }
}
