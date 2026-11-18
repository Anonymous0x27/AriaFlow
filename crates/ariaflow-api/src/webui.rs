//! Serving the web version of the desktop app at `/`.
//!
//! Two backends, selected at startup:
//!
//! - [`WebUi::Embedded`] — the production path: the contents of
//!   `apps/desktop/dist` embedded into the `ariaflowd` binary with
//!   [`rust_embed`] (`web/` next to settings, `assets/*` with immutable
//!   hashing, SPA fallback to `index.html`). Debug builds read the same folder
//!   from disk at runtime so `cargo check/test/clippy` work without `npm run
//!   build`; release builds embed the bytes (run `apps/desktop` build or
//!   `scripts/build.sh` first — a release build without `dist/` fails to
//!   compile, by design).
//! - [`WebUi::Dir`] — explicit `--web-dir` override: serve a directory from
//!   disk instead (useful to iterate on the frontend without rebuilding).
//! - [`WebUi::Missing`] — neither embedded assets nor a valid `--web-dir`:
//!   serve a small explanatory placeholder page.
//!
//! The router additionally gates the whole surface behind
//! `features.web` (toggleable at runtime via `PUT /api/v1/settings`).

use axum::body::Body;
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use std::path::{Path, PathBuf};

/// The desktop web build, embedded into this binary.
///
/// In debug builds rust-embed reads files lazily from disk (relative to the
/// crate's manifest dir at compile time), so no `dist/` is required to
/// compile/test; in release builds the bytes are baked into `ariaflowd`.
#[derive(RustEmbed)]
#[folder = "../../apps/desktop/dist"]
struct EmbeddedWeb;

/// Which web UI the daemon serves at `/`.
#[derive(Debug, Clone, Default)]
pub enum WebUi {
    /// Serve this directory from disk (explicit `--web-dir`).
    Dir(PathBuf),
    /// Serve the build artifact embedded at compile time.
    Embedded,
    /// No web UI available; serve the placeholder page.
    #[default]
    Missing,
}

/// True when the embedded bundle contains an `index.html`.
pub fn embedded_available() -> bool {
    EmbeddedWeb::get("index.html").is_some()
}

/// Resolve the web UI for `--web-dir` / default startup.
///
/// Explicit `--web-dir` wins (must contain `index.html`, else `Missing`).
/// Otherwise use the embedded bundle when available, else `Missing`.
pub fn resolve_web_ui(explicit: Option<&Path>) -> WebUi {
    if let Some(dir) = explicit {
        return if dir.join("index.html").is_file() {
            tracing::info!(dir = %dir.display(), "serving web UI from --web-dir");
            WebUi::Dir(dir.to_path_buf())
        } else {
            tracing::warn!(dir = %dir.display(), "--web-dir has no index.html; serving placeholder page");
            WebUi::Missing
        };
    }
    if embedded_available() {
        tracing::info!("serving embedded web UI (rust-embed, from apps/desktop/dist)");
        WebUi::Embedded
    } else {
        tracing::info!(
            "no embedded web UI (apps/desktop/dist missing at build time); serving placeholder page"
        );
        WebUi::Missing
    }
}

fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn cache_control(path: &str) -> &'static str {
    // Vite emits content-hashed files under assets/ — safe to cache forever.
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn etag_hex(bytes: &[u8]) -> String {
    // FNV-1a 128-bit-ish: cheap, stable for cache validation (not security).
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for &b in bytes {
        h1 = (h1 ^ b as u64).wrapping_mul(0x100000001b3);
        h2 = (h2 ^ b as u64).wrapping_mul(0x9e3779b97f4a7c15);
    }
    format!("\"{:016x}{:016x}\"", h1, h2)
}

struct Asset {
    body: Vec<u8>,
    mime: &'static str,
    etag: String,
}

fn lookup(web: &WebUi, rel: &str) -> Option<Asset> {
    match web {
        WebUi::Embedded => EmbeddedWeb::get(rel).map(|f| {
            let etag = etag_hex(&f.data);
            Asset {
                body: f.data.into_owned(),
                mime: content_type(rel),
                etag,
            }
        }),
        WebUi::Dir(dir) => {
            let candidate = dir.join(rel);
            // `rel` is sanitized (no `.`/`..` segments) by the caller; double-check containment.
            if !candidate.starts_with(dir) || !candidate.is_file() {
                return None;
            }
            let body = std::fs::read(&candidate).ok()?;
            Some(Asset {
                mime: content_type(rel),
                etag: etag_hex(&body),
                body,
            })
        }
        WebUi::Missing => None,
    }
}

/// Map a request path to a sanitized, project-relative asset key.
/// Returns `"index.html"` for `/`, `None` for anything with traversal segments.
fn normalize(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some("index.html".to_string());
    }
    let mut parts: Vec<&str> = Vec::new();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Some("index.html".to_string());
    }
    Some(parts.join("/"))
}

fn asset_response(asset: Asset, path: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, asset.mime)
        .header(header::ETAG, asset.etag)
        .header(header::CACHE_CONTROL, cache_control(path))
        .body(Body::from(asset.body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn looks_like_asset(rel: &str) -> bool {
    // Heuristic: a final segment containing a dot is a file, not an SPA route.
    rel.rsplit('/')
        .next()
        .map(|s| s.contains('.'))
        .unwrap_or(false)
}

fn etag_matches(if_none_match: Option<&str>, etag: &str) -> bool {
    let Some(value) = if_none_match else {
        return false;
    };
    value
        .split(',')
        .map(|t| t.trim())
        .any(|t| t == etag || t == "*" || t.strip_prefix("W/") == Some(etag))
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(
            serde_json::json!({ "error": { "code": "not_found", "message": "unknown path" } }),
        ),
    )
        .into_response()
}

/// Serve the web UI for `GET <raw_path>` with SPA fallback semantics.
///
/// - Exact asset match wins (with ETag + cache policy; `If-None-Match` → 304);
/// - Paths that look like files (`assets/x-<hash>.js`, `icon.png`) that miss
///   return a JSON 404 instead of the SPA shell;
/// - Other extensionless routes fall back to `index.html` (GET/HEAD only);
/// - Other methods get a JSON 404;
/// - `Missing` serves the caller-provided placeholder (handled there).
pub fn serve(
    web: &WebUi,
    raw_path: &str,
    method: &Method,
    if_none_match: Option<&str>,
) -> Response {
    let Some(rel) = normalize(raw_path) else {
        // Traversal attempt — do not fall back to index.html.
        return not_found();
    };
    if let Some(asset) = lookup(web, &rel) {
        if (method == Method::GET || method == Method::HEAD)
            && etag_matches(if_none_match, &asset.etag)
        {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &asset.etag)
                .header(header::CACHE_CONTROL, cache_control(&rel))
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        return asset_response(asset, &rel);
    }
    if !(method == Method::GET || method == Method::HEAD) || looks_like_asset(&rel) {
        return not_found();
    }
    // SPA fallback.
    match lookup(web, "index.html") {
        Some(asset) => asset_response(asset, "index.html"),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Shown when the web build is neither embedded nor available on disk.
const WEB_UI_PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>AriaFlow</title>
<style>
  body { font-family: system-ui, sans-serif; max-width: 42rem; margin: 8vh auto; padding: 0 1.5rem; color: #1c2430; }
  code, pre { background: #f1f3f5; border-radius: 6px; }
  code { padding: 0.15em 0.35em; }
  pre { padding: 0.75rem 1rem; overflow-x: auto; }
</style>
</head>
<body>
<h1>AriaFlow daemon is running</h1>
<p>The <code>/api/v1</code> endpoints (and <code>/mcp</code>) are available on this
listener, but this <code>ariaflowd</code> binary was built without the web UI.</p>
<p>To bundle the web version of the desktop app into the binary:</p>
<pre>cd apps/desktop && npm ci && npm run build
scripts/build.sh   # or: cargo build --release -p ariaflow-daemon</pre>
<p>…or run with <code>--web-dir /path/to/apps/desktop/dist</code> to serve it from disk.
You can also manage your downloads with the Tauri desktop app, the <code>ariaflow</code> CLI,
the TUI (<code>ariaflow --tui</code>) or any MCP client.</p>
</body>
</html>"#;

pub fn placeholder() -> axum::response::Html<&'static str> {
    axum::response::Html(WEB_UI_PLACEHOLDER)
}
