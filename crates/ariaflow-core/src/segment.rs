//! Segment planning for multi-connection downloads.
//!
//! Pure, deterministic logic — unit-tested independently of the network.

/// One inclusive byte range of a remote file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentRange {
    pub index: u32,
    pub start_offset: u64,
    pub end_offset: u64,
}

impl SegmentRange {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> u64 {
        self.end_offset.saturating_sub(self.start_offset) + 1
    }
}

/// Number of segments to actually use for a file of `total_size` bytes.
///
/// Rules:
/// - never exceed `max_connections`
/// - each segment is at least `min_segment_size` (except possibly the remainder distribution)
/// - at least one segment for non-empty files
pub fn connection_count(total_size: u64, max_connections: u16, min_segment_size: u64) -> u16 {
    if total_size == 0 {
        return 1;
    }
    let max_conn = max_connections.max(1) as u64;
    let by_size = (total_size / min_segment_size.max(1)).max(1);
    max_conn.min(by_size).min(u16::MAX as u64) as u16
}

/// Splits `[0, total_size)` into balanced inclusive ranges.
pub fn plan_segments(
    total_size: u64,
    max_connections: u16,
    min_segment_size: u64,
) -> Vec<SegmentRange> {
    if total_size == 0 {
        return vec![SegmentRange {
            index: 0,
            start_offset: 0,
            end_offset: 0,
        }];
    }
    let n = connection_count(total_size, max_connections, min_segment_size) as u64;
    let base = total_size / n;
    let remainder = total_size % n;
    let mut ranges = Vec::with_capacity(n as usize);
    let mut start = 0u64;
    for i in 0..n {
        // Spread the remainder one byte at a time across the first segments.
        let extra = if i < remainder { 1 } else { 0 };
        let len = base + extra;
        let end = start + len - 1;
        ranges.push(SegmentRange {
            index: i as u32,
            start_offset: start,
            end_offset: end,
        });
        start = end + 1;
    }
    ranges
}

/// Splits an incomplete range into `into` roughly equal sub-ranges (work stealing).
///
/// `completed_bytes` is how many leading bytes of `range` are already on disk.
pub fn split_remaining(range: SegmentRange, completed_bytes: u64, into: u32) -> Vec<SegmentRange> {
    let remaining_start = range.start_offset + completed_bytes.min(range.len());
    if remaining_start >= range.end_offset || into <= 1 {
        return vec![SegmentRange {
            index: range.index,
            start_offset: remaining_start,
            end_offset: range.end_offset,
        }];
    }
    let remaining = range.end_offset - remaining_start + 1;
    let into = into.max(1) as u64;
    let base = remaining / into;
    let rem = remaining % into;
    let mut out = Vec::with_capacity(into as usize);
    let mut start = remaining_start;
    for i in 0..into {
        let len = base + if i < rem { 1 } else { 0 };
        if len == 0 {
            continue;
        }
        let end = start + len - 1;
        out.push(SegmentRange {
            index: range.index * 1000 + i as u32,
            start_offset: start,
            end_offset: end,
        });
        start = end + 1;
    }
    out
}

/// Path of the manifest the engine writes next to the part file at the start
/// of a segmented transfer. The plan depends on the runtime connection budget,
/// so it must be captured on disk rather than re-derived later.
pub fn manifest_path(part: &std::path::Path) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}.segments", part.display()))
}

/// Persist the segment plan as JSON next to the part file.
pub fn write_segment_manifest(
    part: &std::path::Path,
    segments: &[SegmentRange],
) -> std::io::Result<()> {
    let data = serde_json::to_vec(segments).map_err(std::io::Error::other)?;
    std::fs::write(manifest_path(part), data)
}

/// Read back the plan; `None` when missing or unreadable.
pub fn read_segment_manifest(part: &std::path::Path) -> Option<Vec<SegmentRange>> {
    let data = std::fs::read(manifest_path(part)).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Best-effort removal (merge finished, task deleted, ...).
pub fn remove_segment_manifest(part: &std::path::Path) {
    let _ = std::fs::remove_file(manifest_path(part));
}

/// Build a *live* per-segment snapshot from `<part>.seg-NNN` side files.
///
/// Returns `None` when the download does not look segmented on disk (missing
/// part path, no side files, or a degenerate single-segment plan) so callers
/// can fall back to the persisted DB rows.
///
/// Side files keep growing as segments download, so this reflects real
/// progress. Statuses: `completed` when a side file reached its planned
/// length, `downloading` when it has bytes but is incomplete, `pending` for
/// empty files.
pub fn live_segment_snapshot(
    save_path: &std::path::Path,
    total_size: u64,
    max_connections: u16,
    min_segment_size: u64,
) -> Option<Vec<crate::models::SegmentInfo>> {
    use crate::models::{SegmentInfo, SegmentStatus};
    let part = std::path::PathBuf::from(format!("{}.ariaflow-part", save_path.display()));
    // Prefer the exact plan the engine used; fall back to re-deriving it.
    let plan = match read_segment_manifest(&part) {
        Some(plan) => plan,
        None => plan_segments(total_size, max_connections, min_segment_size.max(1)),
    };
    if plan.len() <= 1 {
        return None;
    }
    // Any side file present? If none exists we know nothing live about this task.
    let mut found_any = false;
    let mut out = Vec::with_capacity(plan.len());
    for seg in plan {
        let seg_file = std::path::PathBuf::from(format!("{}.seg-{:03}", part.display(), seg.index));
        let len = std::fs::metadata(&seg_file).map(|m| m.len()).unwrap_or(0);
        if len > 0 {
            found_any = true;
        }
        let len = len.min(seg.len());
        let status = if len >= seg.len() {
            SegmentStatus::Completed
        } else if len > 0 {
            SegmentStatus::Downloading
        } else {
            SegmentStatus::Pending
        };
        out.push(SegmentInfo {
            id: seg.index,
            start_offset: seg.start_offset,
            end_offset: seg.end_offset,
            downloaded_size: len,
            status,
            retry_count: 0,
        });
    }
    found_any.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_balanced_split() {
        let ranges = plan_segments(1003, 4, 100);
        assert_eq!(ranges.len(), 4);
        assert_eq!(ranges[0].start_offset, 0);
        assert_eq!(ranges[3].end_offset, 1002);
        let total: u64 = ranges.iter().map(|r| r.len()).sum();
        assert_eq!(total, 1003);
        // remainder bytes are spread over the first segments
        assert_eq!(ranges[0].len() - ranges[3].len(), 1);
        // exact split when evenly divisible
        let even = plan_segments(1000, 4, 100);
        assert!(even.iter().all(|r| r.len() == 250));
    }

    #[test]
    fn respects_min_segment_size() {
        // 10 MiB file with 1 MiB min segment and 16 max connections -> 10 segments
        let ranges = plan_segments(10 * 1024 * 1024, 16, 1024 * 1024);
        assert_eq!(ranges.len(), 10);
    }

    #[test]
    fn single_connection_for_small_files() {
        let ranges = plan_segments(512, 16, 1024 * 1024);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].len(), 512);
    }

    #[test]
    fn split_remaining_covers_tail() {
        let range = SegmentRange {
            index: 3,
            start_offset: 100,
            end_offset: 199,
        };
        let parts = split_remaining(range, 40, 3);
        let covered: u64 = parts.iter().map(|p| p.len()).sum();
        assert_eq!(covered, 60);
        assert_eq!(parts[0].start_offset, 140);
        assert_eq!(parts.last().unwrap().end_offset, 199);
    }
}
