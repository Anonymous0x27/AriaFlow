//! Runtime feature switches for the daemon's HTTP-facing surfaces:
//! the web API (`/api/v1/*`), the MCP endpoint (`/mcp`) and the web page (`/`).
//!
//! Semantics (see docs/API.md):
//!
//! - The switches live in `GlobalSettings` (`apiEnabled`, `mcpEnabled`, `webUiEnabled`)
//!   and are pushed into these atomics by `PUT /api/v1/settings` — they take
//!   effect **immediately, without a restart**.
//! - The local pipe (Unix socket / named pipe) is the trusted management
//!   channel and is *not* gated by `apiEnabled`: desktop clients connected
//!   over the pipe can always read/write settings and flip the switches back.
//!   `mcpEnabled` and `webUiEnabled` apply on every listener.
//! - When a feature is disabled its surface answers `503` with
//!   `{"error": {"code": "feature_disabled", …}}` (HTTP API / MCP) or a small
//!   explanation page (web UI). Long-lived SSE connections established while a
//!   feature was enabled are not torn down; new requests are rejected.
//! - `--no-api` remains a hard switch (the TCP listener is not bound at all);
//!   these runtime switches govern surfaces on bound listeners.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Which daemon surface a switch controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `/api/v1/*` — the web API (not applied to the local pipe).
    Api,
    /// `/mcp` — the MCP JSON-RPC endpoint (applied on every listener).
    Mcp,
    /// `/` — the web page (applied on every listener).
    Web,
}

impl Feature {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Api => "web API",
            Self::Mcp => "MCP endpoint",
            Self::Web => "web UI",
        }
    }
}

/// Cheap-to-clone handle to the live feature flags (shared with the router).
#[derive(Debug, Clone)]
pub struct Features {
    inner: Arc<FeatureFlags>,
}

#[derive(Debug)]
struct FeatureFlags {
    api: AtomicBool,
    mcp: AtomicBool,
    web: AtomicBool,
}

impl Features {
    pub fn new(api: bool, mcp: bool, web: bool) -> Self {
        Self {
            inner: Arc::new(FeatureFlags {
                api: AtomicBool::new(api),
                mcp: AtomicBool::new(mcp),
                web: AtomicBool::new(web),
            }),
        }
    }

    pub fn get(&self, feature: Feature) -> bool {
        let slot = match feature {
            Feature::Api => &self.inner.api,
            Feature::Mcp => &self.inner.mcp,
            Feature::Web => &self.inner.web,
        };
        slot.load(Ordering::Relaxed)
    }

    pub fn set(&self, feature: Feature, enabled: bool) {
        let slot = match feature {
            Feature::Api => &self.inner.api,
            Feature::Mcp => &self.inner.mcp,
            Feature::Web => &self.inner.web,
        };
        slot.store(enabled, Ordering::Relaxed);
    }

    /// Applies all three switches at once (used after settings are persisted).
    pub fn set_all(&self, api: bool, mcp: bool, web: bool) {
        self.set(Feature::Api, api);
        self.set(Feature::Mcp, mcp);
        self.set(Feature::Web, web);
    }
}

impl Default for Features {
    fn default() -> Self {
        Self::new(true, true, true)
    }
}

/// JSON 503 body used by the HTTP API and MCP surfaces when a feature is off.
pub fn disabled_json(feature: Feature) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let message = match feature {
        Feature::Api => {
            "the web API is disabled; re-enable it with `ariaflow config set api-enabled true` \
             or from the desktop app over the local pipe (effective immediately, no restart)"
        }
        Feature::Mcp => {
            "the MCP endpoint is disabled; re-enable it with `ariaflow config set mcp-enabled true` \
             or from the desktop app (effective immediately, no restart)"
        }
        Feature::Web => {
            "the web UI is disabled; re-enable it with `ariaflow config set web-ui-enabled true` \
             or from the desktop app (effective immediately, no restart)"
        }
    };
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "error": {
                "code": "feature_disabled",
                "message": message,
            }
        })),
    )
        .into_response()
}

/// Minimal HTML 503 page for the web UI surface.
pub fn disabled_html(feature: Feature) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>AriaFlow — disabled</title>
<style>body{font-family:system-ui,sans-serif;max-width:40rem;margin:12vh auto;padding:0 1.5rem;color:#1c2430}code{background:#f1f3f5;border-radius:6px;padding:.1em .35em}</style>
</head>
<body>
<h1>AriaFlow web UI is disabled</h1>
<p>The daemon is running and downloads are unaffected; only the <code>/</code> page is off.</p>
<p>Re-enable it with <code>ariaflow config set web-ui-enabled true</code> (over the local pipe), or from
the desktop app's Settings page — it takes effect immediately, no restart required.</p>
</body>
</html>"#;
    let _ = feature;
    (StatusCode::SERVICE_UNAVAILABLE, axum::response::Html(PAGE)).into_response()
}
