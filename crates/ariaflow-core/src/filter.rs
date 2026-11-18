//! Filtering, sorting and pagination for download queries.

use crate::status::DownloadStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortField {
    CreatedAt,
    CompletedAt,
    Filename,
    Size,
    Progress,
    Priority,
    Speed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    #[default]
    Desc,
    Asc,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFilter {
    /// Match any of these statuses; empty = all.
    pub statuses: Vec<DownloadStatus>,
    /// Match a single category key, e.g. "documents", "videos".
    pub category: Option<String>,
    /// Case-insensitive substring match on filename / url.
    pub search: Option<String>,
    pub sort_by: Option<SortField>,
    pub sort_order: SortOrder,
    /// 1-based page number.
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

impl DownloadFilter {
    pub const fn unfiltered() -> Self {
        const ORDER: SortOrder = SortOrder::Desc;
        Self {
            statuses: Vec::new(),
            category: None,
            search: None,
            sort_by: None,
            sort_order: ORDER,
            page: None,
            page_size: None,
        }
    }

    pub fn page(mut self, page: u32, page_size: u32) -> Self {
        self.page = Some(page);
        self.page_size = Some(page_size);
        self
    }
}
