//! Request/response DTOs for the Web API and the Rust client.

use ariaflow_core::{DownloadCounts, DownloadFilter, DownloadStatus};
use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub product: String,
    pub version: String,
    pub uptime_secs: u64,
    pub counts: DownloadCounts,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// Comma-separated status values, e.g. `downloading,queued`
    pub status: Option<String>,
    pub category: Option<String>,
    pub q: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

impl ListQuery {
    pub fn into_filter(self) -> DownloadFilter {
        let statuses = self
            .status
            .map(|s| {
                s.split(',')
                    .filter_map(|p| DownloadStatus::parse(p.trim()))
                    .collect()
            })
            .unwrap_or_default();
        DownloadFilter {
            statuses,
            category: self.category,
            search: self.q,
            sort_by: None,
            sort_order: Default::default(),
            page: self.page,
            page_size: self.page_size,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequest {
    /// One of: addMany | pauseAll | resumeAll | cancelAll | removeCompleted
    pub action: String,
    /// addMany: plain URL list (backwards compatible).
    pub urls: Option<Vec<String>>,
    /// addMany: IDM-style batch import. Each item uses the same wire format as
    /// `POST /api/v1/downloads`; takes precedence over `urls` when present.
    pub tasks: Option<Vec<ariaflow_core::NewDownloadRequest>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItemError {
    /// Zero-based index into `tasks`/`urls`.
    pub index: u32,
    pub url: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    pub action: String,
    pub affected: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    /// Per-item failures from addMany (best effort; successful items are still created).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<BatchItemError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    /// Max entries to return (default 100, clamped to 1..=500).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyPathParams {
    pub id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveQuery {
    pub delete_file: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRequest {
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}
