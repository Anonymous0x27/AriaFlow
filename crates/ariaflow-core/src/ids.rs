//! Strongly-typed identifiers used across AriaFlow.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Unique identifier of a download task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(transparent)]
pub struct DownloadId(Uuid);

impl DownloadId {
    /// Generates a random new download id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for DownloadId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DownloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for DownloadId {
    type Err = crate::AriaFlowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Accept both full UUIDs and unambiguous prefixes (>= 4 chars) for CLI convenience.
        if let Ok(u) = Uuid::parse_str(s) {
            return Ok(Self(u));
        }
        Err(crate::AriaFlowError::InvalidRequest(format!(
            "invalid download id: {s:?} (expected a full UUID)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_display_parse() {
        let id = DownloadId::new();
        let parsed: DownloadId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn rejects_garbage() {
        assert!("not-a-uuid".parse::<DownloadId>().is_err());
    }
}
