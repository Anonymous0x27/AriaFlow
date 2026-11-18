//! Resume-safety validation.
//!
//! Before a partially-downloaded file is resumed, we verify that the remote
//! resource is (very likely) the same as when the download started, using
//! ETag / Last-Modified / total-size comparison — never size alone.

use ariaflow_protocol::RemoteFileInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeDecision {
    /// Remote metadata matches what we recorded; resuming is safe.
    SafeToResume,
    /// Remote file changed (etag/last-modified/size mismatch); restart from scratch.
    RemoteChanged,
    /// Remote metadata unavailable and sizes unknown; treat cautiously.
    Unverifiable,
}

/// What we persisted about the remote resource when the task was created.
#[derive(Debug, Clone)]
pub struct PersistedRemoteState {
    pub total_size: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

pub fn validate_resume(
    persisted: &PersistedRemoteState,
    remote: &RemoteFileInfo,
) -> ResumeDecision {
    let mut saw_marker = false;
    if let (Some(p), Some(r)) = (&persisted.etag, &remote.etag) {
        saw_marker = true;
        if p != r {
            return ResumeDecision::RemoteChanged;
        }
    }
    if let (Some(p), Some(r)) = (&persisted.last_modified, &remote.last_modified) {
        saw_marker = true;
        if p != r {
            return ResumeDecision::RemoteChanged;
        }
    }
    if let (Some(p), Some(r)) = (persisted.total_size, remote.total_size) {
        saw_marker = true;
        if p != r {
            return ResumeDecision::RemoteChanged;
        }
    }
    if saw_marker {
        ResumeDecision::SafeToResume
    } else {
        ResumeDecision::Unverifiable
    }
}

/// Builds the `If-Range` header value for conditional range requests (RFC 7233 §3.2).
/// Prefers ETag (as a strong validator), falls back to Last-Modified.
pub fn if_range_value(remote: &RemoteFileInfo) -> Option<String> {
    remote.etag.clone().or_else(|| remote.last_modified.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(size: Option<u64>, etag: Option<&str>, lm: Option<&str>) -> RemoteFileInfo {
        RemoteFileInfo {
            final_url: "https://example.com/f".into(),
            total_size: size,
            accept_ranges: true,
            etag: etag.map(|s| s.to_string()),
            last_modified: lm.map(|s| s.to_string()),
            content_type: None,
            filename_hint: None,
            headers: vec![],
        }
    }

    #[test]
    fn identical_etag_resumes() {
        let p = PersistedRemoteState {
            total_size: Some(100),
            etag: Some("\"abc\"".into()),
            last_modified: None,
        };
        assert_eq!(
            validate_resume(&p, &remote(Some(100), Some("\"abc\""), None)),
            ResumeDecision::SafeToResume
        );
    }

    #[test]
    fn changed_etag_restarts() {
        let p = PersistedRemoteState {
            total_size: Some(100),
            etag: Some("\"abc\"".into()),
            last_modified: None,
        };
        assert_eq!(
            validate_resume(&p, &remote(Some(100), Some("\"xyz\""), None)),
            ResumeDecision::RemoteChanged
        );
    }

    #[test]
    fn changed_size_restarts() {
        let p = PersistedRemoteState {
            total_size: Some(100),
            etag: None,
            last_modified: None,
        };
        assert_eq!(
            validate_resume(&p, &remote(Some(200), None, None)),
            ResumeDecision::RemoteChanged
        );
    }

    #[test]
    fn no_markers_is_unverifiable() {
        let p = PersistedRemoteState {
            total_size: None,
            etag: None,
            last_modified: None,
        };
        assert_eq!(
            validate_resume(&p, &remote(None, None, None)),
            ResumeDecision::Unverifiable
        );
    }
}
