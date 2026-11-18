//! # AriaFlow Engine
//!
//! Independent download engine. Not bound to any GUI.
//!
//! Provides the transport layer for every supported protocol behind the shared
//! [`ariaflow_protocol::DownloadProvider`] interface:
//!
//! - `http` — HTTP/HTTPS: probe (HEAD/Range-GET), redirect-following metadata capture,
//!   proxies, custom headers/cookies, byte-accurate Range resume, token-bucket throttle
//! - `ftp` — FTP/explicit-FTPS: SIZE/MDTM probe, REST/RETR resume, passive mode
//! - `sftp` — SSH/SFTP: stat probe, password/agent/key auth chain, seek resume
//! - `metalink` — Metalink 3/4: mirror selection by preference, delegates the payload
//!   to the HTTP provider; manifest checksums flow into `x-checksum-*` probe headers
//! - `torrent` — BitTorrent magnet links and http(s) `.torrent` URLs (librqbit):
//!   piece-level resume via a staging directory next to the `.ariaflow-part` file
//! - `resume` / `segment` / `speed` / `throttle` — shared primitives (resume validation,
//!   segmentation plan, sliding-window speed meter, blocking+async rate limiter)
//!
//! The orchestration loop (state machine, persistence, crash recovery, checksum
//! verification) lives in `ariaflowd` (`crates/ariaflow-daemon/src/orchestrator.rs`),
//! which is the composition root; this crate never touches the database.

pub mod ftp;
pub mod http;
pub mod metalink;
pub mod resume;
pub mod sftp;
pub mod speed;
pub mod throttle;
pub mod torrent;

pub use ariaflow_core::segment::{connection_count, plan_segments, split_remaining, SegmentRange};
pub use ftp::FtpProvider;
pub use http::HttpProvider;
pub use metalink::MetalinkProvider;
pub use resume::{if_range_value, validate_resume, PersistedRemoteState, ResumeDecision};
pub use sftp::SftpProvider;
pub use speed::SpeedTracker;
pub use throttle::RateLimiter;
pub use torrent::TorrentProvider;
