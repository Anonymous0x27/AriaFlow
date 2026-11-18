//! # AriaFlow API
//!
//! - `server`: Axum-based Web API (status / downloads / settings / batch / SSE)
//! - `client`: transport-aware Rust client used by CLI, TUI, MCP server and browser native host
//!   (HTTP/Web API by default; HTTP-over-IPC pipe when the pipe endpoint is configured —
//!   the CLI selects it with `--transport`)
//! - `ipc`:    HTTP/1.1-over-pipe client (Unix domain socket / Windows named pipe)
//!
//! The API is versioned under `/api/v1` and defaults to loopback-only binding.

pub mod client;
pub mod dto;
pub mod features;
pub mod ipc;
pub mod rate_limit;
pub mod server;
pub mod webui;

pub use client::{AriaFlowClient, ClientError, ClientResult, Transport};
pub use dto::{BatchRequest, BatchResponse, ListQuery, StatusResponse};
pub use features::{Feature, Features};
pub use server::{router, router_ipc, router_with, serve, serve_ipc, ApiState, WebUi};
pub use webui::embedded_available;
