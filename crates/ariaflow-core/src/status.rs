//! Download task state machine.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a download task.
///
/// Transition overview:
/// `Pending -> Queued -> Connecting -> Downloading -> (Paused | Retrying | Merging | Verifying) -> Completed`
/// Any state may transition to `Failed` or `Cancelled` by user or system action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Pending,
    Queued,
    Connecting,
    Downloading,
    Paused,
    Waiting,
    Retrying,
    Completed,
    Failed,
    Cancelled,
    Merging,
    Verifying,
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Connecting => "connecting",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Waiting => "waiting",
            Self::Retrying => "retrying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Merging => "merging",
            Self::Verifying => "verifying",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "queued" => Self::Queued,
            "connecting" => Self::Connecting,
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "waiting" => Self::Waiting,
            "retrying" => Self::Retrying,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "merging" => Self::Merging,
            "verifying" => Self::Verifying,
            _ => return None,
        })
    }

    /// Terminal states no longer change without an explicit user action (retry/re-add).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Statuses in which the task holds (or may hold) network connections.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Downloading | Self::Merging | Self::Verifying
        )
    }

    /// Statuses that can be paused by the user.
    pub fn is_pausable(&self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Connecting | Self::Downloading | Self::Waiting | Self::Retrying
        )
    }

    /// Statuses from which a download can be resumed.
    pub fn is_resumable(&self) -> bool {
        matches!(self, Self::Paused | Self::Waiting | Self::Retrying)
    }

    /// Returns true if `to` is a legal next state for `from`.
    pub fn can_transition_to(&self, to: &DownloadStatus) -> bool {
        use DownloadStatus::*;
        if self == to {
            return true; // idempotent refresh
        }
        if matches!(to, Failed | Cancelled) {
            return !self.is_terminal();
        }
        if *to == Paused {
            return self.is_pausable();
        }
        match (self, to) {
            (Pending, Queued) => true,
            (Pending | Queued, Connecting) => true,
            (Connecting, Downloading | Paused | Waiting | Retrying) => true,
            (Downloading, Paused | Waiting | Retrying | Merging | Verifying | Completed) => true,
            (Paused, Queued | Connecting) => true,
            (Waiting, Queued | Connecting | Retrying) => true,
            (Retrying, Queued | Connecting) => true,
            (Merging, Verifying | Completed) => true,
            (Verifying, Completed | Merging) => true,
            (Failed | Cancelled, Queued | Pending) => true, // retry / re-download
            _ => false,
        }
    }
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_happy_path() {
        use DownloadStatus::*;
        let path = [
            Pending,
            Queued,
            Connecting,
            Downloading,
            Merging,
            Verifying,
            Completed,
        ];
        for pair in path.windows(2) {
            assert!(
                pair[0].can_transition_to(&pair[1]),
                "{:?} -> {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn completed_cannot_download() {
        assert!(!DownloadStatus::Completed.can_transition_to(&DownloadStatus::Downloading));
        assert!(!DownloadStatus::Completed.is_active());
        assert!(DownloadStatus::Completed.is_terminal());
    }

    #[test]
    fn failed_can_retry() {
        assert!(DownloadStatus::Failed.can_transition_to(&DownloadStatus::Queued));
    }

    #[test]
    fn str_roundtrip() {
        for s in [
            "pending",
            "queued",
            "connecting",
            "downloading",
            "paused",
            "waiting",
            "retrying",
            "completed",
            "failed",
            "cancelled",
            "merging",
            "verifying",
        ] {
            assert_eq!(DownloadStatus::parse(s).unwrap().as_str(), s);
        }
    }
}
