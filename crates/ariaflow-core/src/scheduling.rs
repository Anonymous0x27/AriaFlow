//! Pure scheduling helpers shared by the daemon orchestrator and tests.
//!
//! Two user-visible concepts live here:
//!
//! * [`Download::scheduled_start_at`](crate::models::Download::scheduled_start_at):
//!   a per-task "not before" instant. The task stays queued but is not eligible
//!   for execution until then.
//! * A daily download window on the global settings: between
//!   `download_window_start_minute` (inclusive) and `download_window_end_minute`
//!   (exclusive), minutes after local midnight. When a window is configured,
//!   automatic starts outside it are deferred; tasks resume at the next window
//!   boundary. Manual actions (pause/resume/cancel) are never blocked.

use crate::models::Download;
use crate::settings::GlobalSettings;
use chrono::{DateTime, Local, TimeZone};

/// A validated daily window in minutes after local midnight.
///
/// `start < end` means a same-day window (e.g. 02:00–06:00). `start > end`
/// wraps across midnight (e.g. 22:00–06:00). Equal boundaries are treated as
/// "disabled" by [`window_bounds`] because a zero-length window would never
/// admit anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start_minute: u32,
    pub end_minute: u32,
}

/// Validates the configured window, if any. Returns `None` when no usable
/// window is configured (missing, out of range, or zero-length).
pub fn window_bounds(settings: &GlobalSettings) -> Option<Window> {
    let start = settings.download_window_start_minute?;
    let end = settings.download_window_end_minute?;
    if start >= 24 * 60 || end >= 24 * 60 {
        return None;
    }
    if start == end {
        return None;
    }
    Some(Window {
        start_minute: start,
        end_minute: end,
    })
}

/// Minutes after local midnight for `at`.
fn minute_of_day(at: DateTime<Local>) -> u32 {
    use chrono::Timelike;
    at.hour() * 60 + at.minute()
}

/// Whether `at` falls inside the window.
pub fn in_window(window: &Window, at: DateTime<Local>) -> bool {
    let minute = minute_of_day(at);
    if window.start_minute < window.end_minute {
        minute >= window.start_minute && minute < window.end_minute
    } else {
        // Wraps midnight.
        minute >= window.start_minute || minute < window.end_minute
    }
}

/// The next local instant at which the window opens, strictly after `at`.
/// Returns `None` only when local time conversion fails (shouldn't happen with
/// a valid `at`).
pub fn next_window_start(window: &Window, at: DateTime<Local>) -> Option<DateTime<Local>> {
    for day_offset in 0..=1 {
        let day = at.date_naive() + chrono::Duration::days(day_offset);
        let naive = day.and_hms_opt(window.start_minute / 60, window.start_minute % 60, 0)?;
        let candidate = match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(c) => c,
            // On a DST "spring forward" gap, 02:30 may not exist; fall back to
            // the first valid instant of that minute instead of failing open.
            chrono::LocalResult::Ambiguous(c, _) => c,
            chrono::LocalResult::None => {
                match Local.from_local_datetime(&(naive + chrono::Duration::hours(1))) {
                    chrono::LocalResult::Single(c) => c,
                    chrono::LocalResult::Ambiguous(c, _) => c,
                    chrono::LocalResult::None => continue,
                }
            }
        };
        if candidate > at {
            return Some(candidate);
        }
    }
    None
}

/// Whether an otherwise-queued task is allowed to start now.
///
/// Two gates apply, both global-settings/scheduling driven:
/// * per-task scheduled start: `now < scheduled_start_at` blocks the task.
/// * daily window: when configured and `now` is outside it, automatic starts
///   are deferred.
pub fn task_start_permitted(
    download: &Download,
    settings: &GlobalSettings,
    now: DateTime<Local>,
) -> bool {
    if let Some(start) = download.scheduled_start_at {
        if now.with_timezone(&chrono::Utc) < start {
            return false;
        }
    }
    match window_bounds(settings) {
        Some(window) => in_window(&window, now),
        None => true,
    }
}

/// Human-readable reason a queued task is not starting now; used for logs.
pub fn deferred_reason(download: &Download, settings: &GlobalSettings) -> Option<String> {
    if let Some(start) = download.scheduled_start_at {
        let now = chrono::Utc::now();
        if now < start {
            return Some(format!(
                "scheduled to start at {}",
                start.with_timezone(&Local).format("%Y-%m-%d %H:%M %z")
            ));
        }
    }
    window_bounds(settings).map(|w| {
        format!(
            "outside download window ({:02}:{:02}-{:02}:{:02} local)",
            w.start_minute / 60,
            w.start_minute % 60,
            w.end_minute / 60,
            w.end_minute % 60
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn settings_with_window(start: Option<u32>, end: Option<u32>) -> GlobalSettings {
        GlobalSettings {
            download_window_start_minute: start,
            download_window_end_minute: end,
            ..GlobalSettings::default()
        }
    }

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("test local time must be unambiguous")
    }

    #[test]
    fn no_window_configured_means_always_open() {
        let s = settings_with_window(None, None);
        assert!(window_bounds(&s).is_none());
        assert!(in_window(
            &Window {
                start_minute: 120,
                end_minute: 360
            },
            local(2026, 11, 12, 3, 0)
        ));
    }

    #[test]
    fn zero_length_or_out_of_range_window_is_disabled() {
        assert!(window_bounds(&settings_with_window(Some(120), Some(120))).is_none());
        assert!(window_bounds(&settings_with_window(Some(1440), Some(100))).is_none());
        assert!(window_bounds(&settings_with_window(Some(0), Some(1440))).is_none());
    }

    #[test]
    fn same_day_window_bounds() {
        let w = Window {
            start_minute: 120,
            end_minute: 360,
        }; // 02:00 - 06:00
        assert!(in_window(&w, local(2026, 11, 12, 2, 0)));
        assert!(in_window(&w, local(2026, 11, 12, 5, 59)));
        assert!(!in_window(&w, local(2026, 11, 12, 6, 0)));
        assert!(!in_window(&w, local(2026, 11, 12, 1, 59)));
    }

    #[test]
    fn wrapping_window_bounds() {
        let w = Window {
            start_minute: 22 * 60,
            end_minute: 6 * 60,
        }; // 22:00 - 06:00
        assert!(in_window(&w, local(2026, 11, 12, 23, 0)));
        assert!(in_window(&w, local(2026, 11, 12, 5, 59)));
        assert!(!in_window(&w, local(2026, 11, 12, 12, 0)));
        assert!(!in_window(&w, local(2026, 11, 12, 6, 0)));
    }

    #[test]
    fn next_window_start_same_day() {
        let w = Window {
            start_minute: 120,
            end_minute: 360,
        };
        let at = local(2026, 11, 12, 7, 30);
        let next = next_window_start(&w, at).unwrap();
        assert_eq!(next, local(2026, 11, 13, 2, 0));
    }

    #[test]
    fn next_window_start_wrapping() {
        let w = Window {
            start_minute: 22 * 60,
            end_minute: 6 * 60,
        };
        let at = local(2026, 11, 12, 12, 0);
        let next = next_window_start(&w, at).unwrap();
        assert_eq!(next, local(2026, 11, 12, 22, 0));
    }

    fn dummy_download(scheduled: Option<DateTime<Utc>>) -> Download {
        let mut d = Download::from_request(
            &crate::models::NewDownloadRequest::new("https://example.com/f.bin"),
            &GlobalSettings::default(),
        )
        .unwrap();
        d.scheduled_start_at = scheduled;
        d
    }

    #[test]
    fn scheduled_start_blocks_until_instant() {
        let future = chrono::Utc::now() + chrono::Duration::hours(2);
        let dl = dummy_download(Some(future));
        let now = Local::now();
        assert!(!task_start_permitted(&dl, &GlobalSettings::default(), now));
        let dl = dummy_download(Some(chrono::Utc::now() - chrono::Duration::hours(1)));
        assert!(task_start_permitted(
            &dl,
            &GlobalSettings::default(),
            Local::now()
        ));
    }
}
