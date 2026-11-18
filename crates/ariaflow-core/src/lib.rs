//! # AriaFlow Core
//!
//! Core domain models, the unified [`DownloadService`] business interface and the
//! [`EventBus`] used to push download events to every frontend
//! (CLI / TUI / Web API / MCP / Tauri desktop).
//!
//! This crate intentionally has **no** dependency on any GUI framework, the network
//! or SQLite. It defines the vocabulary the rest of the workspace speaks.

pub mod category;
pub mod checksum;
pub mod error;
pub mod event;
pub mod filename;
pub mod filter;
pub mod ids;
pub mod memory;
pub mod models;
pub mod paths;
pub mod scheduling;
pub mod segment;
pub mod service;
pub mod settings;
pub mod status;

pub use checksum::{Checksum, ChecksumAlgorithm};
pub use error::{AriaFlowError, Result};
pub use event::{DownloadEvent, EventBus};
pub use filter::{DownloadFilter, SortField, SortOrder};
pub use ids::DownloadId;
pub use memory::InMemoryDownloadService;
pub use models::{
    AuthConfig, Download, DownloadSummary, NewDownloadRequest, ProxyConfig, ProxyKind, SegmentInfo,
    SegmentStatus,
};
pub use models::{HistoryEntry, ProxyEntry, ProxyInput, SshCertificateEntry, SshCertificateInput};
pub use scheduling::{in_window, next_window_start, task_start_permitted, window_bounds, Window};
pub use service::{DownloadCounts, DownloadService};
pub use settings::{GlobalSettings, UpdateSettingsRequest};
pub use status::DownloadStatus;
