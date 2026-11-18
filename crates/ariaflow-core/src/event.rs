//! Event definitions and a lightweight broadcast event bus.
//!
//! All frontends (CLI/TUI/API/MCP/GUI) consume these events instead of polling
//! internal state. `EventBus` wraps `tokio::sync::broadcast`, so slow consumers
//! simply lag (receivers get `RecvError::Lagged`) rather than blocking producers.

use crate::ids::DownloadId;
use crate::status::DownloadStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadEvent {
    DownloadCreated {
        id: DownloadId,
    },
    DownloadStarted {
        id: DownloadId,
    },
    DownloadProgress {
        id: DownloadId,
        downloaded_size: u64,
        total_size: Option<u64>,
        speed_bytes_per_sec: u64,
        eta_seconds: Option<u64>,
    },
    DownloadPaused {
        id: DownloadId,
    },
    DownloadResumed {
        id: DownloadId,
    },
    DownloadCompleted {
        id: DownloadId,
        total_size: Option<u64>,
        average_speed_bytes_per_sec: u64,
    },
    DownloadFailed {
        id: DownloadId,
        error: String,
    },
    DownloadCancelled {
        id: DownloadId,
    },
    DownloadRemoved {
        id: DownloadId,
    },
    /// Coarse state change not covered by a dedicated variant above.
    StatusChanged {
        id: DownloadId,
        from: DownloadStatus,
        to: DownloadStatus,
    },
}

impl DownloadEvent {
    /// Stable event type string for SSE/webhook style consumers.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::DownloadCreated { .. } => "download_created",
            Self::DownloadStarted { .. } => "download_started",
            Self::DownloadProgress { .. } => "download_progress",
            Self::DownloadPaused { .. } => "download_paused",
            Self::DownloadResumed { .. } => "download_resumed",
            Self::DownloadCompleted { .. } => "download_completed",
            Self::DownloadFailed { .. } => "download_failed",
            Self::DownloadCancelled { .. } => "download_cancelled",
            Self::DownloadRemoved { .. } => "download_removed",
            Self::StatusChanged { .. } => "status_changed",
        }
    }
}

/// Cloneable broadcast handle. Sending never fails even with zero subscribers.
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<DownloadEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DownloadEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, event: DownloadEvent) {
        // No active subscribers is a normal condition, not an error.
        let _ = self.tx.send(event);
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}
