//! Axum router for the AriaFlow Web API.

use crate::dto::{
    ApiErrorBody, ApiErrorDetail, BatchItemError, BatchRequest, BatchResponse, HistoryQuery,
    ListQuery, ProbeRequest, RemoveQuery, StatusResponse, API_VERSION,
};
use crate::rate_limit::RateLimiter;
use ariaflow_core::{
    AriaFlowError, DownloadEvent, DownloadFilter, DownloadId, DownloadService, DownloadStatus,
    EventBus, NewDownloadRequest, UpdateSettingsRequest,
};
use axum::body::Body;
use axum::extract::Request;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
use axum::http::{header, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::features::{self, Feature, Features};
use crate::webui;
pub use crate::webui::WebUi;

/// Shared state handed to every handler.
#[derive(Clone)]
pub struct ApiState {
    pub service: Arc<dyn DownloadService>,
    pub events: EventBus,
    /// `None` disables auth (loopback development default). Set this in production.
    pub token: Option<String>,
    pub rate_limiter: Arc<RateLimiter>,
    pub started_at: Instant,
    /// Optional transport provider for the /api/v1/probe diagnostic endpoint.
    pub provider: Option<Arc<dyn ariaflow_protocol::DownloadProvider>>,
    /// Live feature switches (web API / MCP / web UI). Toggled at runtime by
    /// `PUT /api/v1/settings`; see [`crate::features`].
    pub features: Features,
    /// Which web UI to serve at `/` (embedded assets by default).
    pub web_ui: WebUi,
}

impl ApiState {
    pub fn new(service: Arc<dyn DownloadService>, events: EventBus, token: Option<String>) -> Self {
        Self {
            service,
            events,
            token,
            rate_limiter: Arc::new(RateLimiter::new()),
            started_at: Instant::now(),
            provider: None,
            features: Features::default(),
            web_ui: WebUi::default(),
        }
    }

    pub fn with_provider(mut self, provider: Arc<dyn ariaflow_protocol::DownloadProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }

    pub fn with_web_ui(mut self, web_ui: WebUi) -> Self {
        self.web_ui = web_ui;
        self
    }
}

fn error_status(err: &AriaFlowError) -> StatusCode {
    match err {
        AriaFlowError::NotFound(_) => StatusCode::NOT_FOUND,
        AriaFlowError::InvalidRequest(_)
        | AriaFlowError::InvalidTransition { .. }
        | AriaFlowError::Url(_) => StatusCode::BAD_REQUEST,
        AriaFlowError::Unauthorized => StatusCode::UNAUTHORIZED,
        AriaFlowError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        AriaFlowError::Verification(_) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn error_response(err: AriaFlowError) -> Response {
    let status = error_status(&err);
    // Never echo secrets; the message comes from our own error types which
    // do not embed tokens/cookies.
    let body = ApiErrorBody {
        error: ApiErrorDetail {
            code: err.code().to_string(),
            message: err.to_string(),
        },
    };
    (status, Json(body)).into_response()
}

/// Constant-time equality to limit token comparison side channels.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn auth_middleware(State(state): State<ApiState>, req: Request, next: Next) -> Response {
    // Local health probes and OpenAPI doc stay unauthenticated when no token is set.
    let token = match &state.token {
        Some(t) => t,
        None => return next.run(req).await,
    };
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let presented = header.strip_prefix("Bearer ").unwrap_or("");
    if ct_eq(presented.as_bytes(), token.as_bytes()) {
        next.run(req).await
    } else {
        error_response(AriaFlowError::Unauthorized)
    }
}

async fn rate_limit_middleware(
    State(state): State<ApiState>,
    req: Request,
    next: Next,
) -> Response {
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    if state.rate_limiter.allow(ip) {
        next.run(req).await
    } else {
        error_response(AriaFlowError::RateLimited)
    }
}

async fn status(State(state): State<ApiState>) -> Response {
    match state.service.counts().await {
        Ok(counts) => (
            StatusCode::OK,
            Json(StatusResponse {
                product: "AriaFlow".to_string(),
                version: API_VERSION.to_string(),
                uptime_secs: state.started_at.elapsed().as_secs(),
                counts,
            }),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": { "code": "internal", "message": "failed to collect status" } })),
        )
            .into_response(),
    }
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn list_downloads(
    State(state): State<ApiState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ariaflow_core::Download>>, Response> {
    let filter: DownloadFilter = q.into_filter();
    state
        .service
        .list_downloads(filter)
        .await
        .map(Json)
        .map_err(error_response)
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn get_download(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<ariaflow_core::Download>, Response> {
    let id: DownloadId = id.parse().map_err(error_response)?;
    match state.service.get_download(id).await {
        Ok(Some(dl)) => Ok(Json(dl)),
        Ok(None) => Err(error_response(AriaFlowError::NotFound(id.to_string()))),
        Err(e) => Err(error_response(e)),
    }
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn add_download(
    State(state): State<ApiState>,
    Json(request): Json<NewDownloadRequest>,
) -> Result<(StatusCode, Json<ariaflow_core::Download>), Response> {
    state
        .service
        .add_download(request)
        .await
        .map(|dl| (StatusCode::CREATED, Json(dl)))
        .map_err(error_response)
}

macro_rules! id_action {
    ($name:ident, $method:ident) => {
        async fn $name(State(state): State<ApiState>, Path(id): Path<String>) -> Response {
            let id = match id.parse::<DownloadId>() {
                Ok(id) => id,
                Err(e) => return error_response(e),
            };
            match state.service.$method(id).await {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(e) => error_response(e),
            }
        }
    };
}

id_action!(pause_download_handler, pause_download);
id_action!(resume_download_handler, resume_download);
id_action!(cancel_download_handler, cancel_download);
id_action!(retry_download_handler, retry_download);

async fn delete_download(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(q): Query<RemoveQuery>,
) -> Response {
    let id = match id.parse::<DownloadId>() {
        Ok(id) => id,
        Err(e) => return error_response(e),
    };
    match state
        .service
        .remove_download(id, q.delete_file.unwrap_or(false))
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityRequest {
    pub priority: i32,
}

async fn set_priority(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(body): Json<PriorityRequest>,
) -> Response {
    let id = match id.parse::<DownloadId>() {
        Ok(id) => id,
        Err(e) => return error_response(e),
    };
    match state.service.set_priority(id, body.priority).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

/// Deserialises a JSON `null` as `Some(None)` and a number as `Some(Some(n))`.
/// An *absent* key yields `None`, which the handler rejects — the caller must
/// choose explicitly between "set" (number) and "clear" (null).
fn deserialize_explicit_nullable_u64<'de, D>(d: D) -> Result<Option<Option<u64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u64>::deserialize(d)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitRateRequest {
    /// Per-task cap in bytes/sec. `0` disables the cap explicitly; `null`
    /// clears the per-task override (the global `perTaskSpeedLimit` applies).
    #[serde(default, deserialize_with = "deserialize_explicit_nullable_u64")]
    pub limit_rate_bytes_per_sec: Option<Option<u64>>,
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn set_limit_rate(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(body): Json<LimitRateRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    let id = id.parse::<DownloadId>().map_err(error_response)?;
    let limit = match body.limit_rate_bytes_per_sec {
        Some(v) => v,
        None => {
            return Err(error_response(AriaFlowError::InvalidRequest(
                "limitRateBytesPerSec is required (a number, or null to clear)".to_string(),
            )))
        }
    };
    state
        .service
        .set_limit_rate(id, limit)
        .await
        .map_err(error_response)?;
    // Hot-apply: the engine reads the per-task cap when it starts a transfer,
    // so an in-flight download cycles pause→resume to pick up the new cap.
    let restarted = match state.service.get_download(id).await {
        Ok(Some(dl)) => {
            let pausable = matches!(
                dl.status,
                DownloadStatus::Queued
                    | DownloadStatus::Connecting
                    | DownloadStatus::Downloading
                    | DownloadStatus::Waiting
                    | DownloadStatus::Retrying
            );
            if pausable {
                match state.service.pause_download(id).await {
                    Ok(()) => state.service.resume_download(id).await.is_ok(),
                    Err(_) => false,
                }
            } else {
                false
            }
        }
        _ => false,
    };
    Ok(Json(json!({
        "id": id.to_string(),
        "limitRateBytesPerSec": limit,
        "restarted": restarted,
    })))
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn batch(
    State(state): State<ApiState>,
    Json(body): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, Response> {
    let svc = state.service.clone();
    let (affected, ids, errors): (u32, Vec<String>, Vec<BatchItemError>) =
        match body.action.as_str() {
            "pauseAll" => (
                svc.pause_all().await.map_err(error_response)?,
                vec![],
                vec![],
            ),
            "resumeAll" => (
                svc.resume_all().await.map_err(error_response)?,
                vec![],
                vec![],
            ),
            "cancelAll" => (
                svc.cancel_all().await.map_err(error_response)?,
                vec![],
                vec![],
            ),
            "removeCompleted" => (
                svc.remove_completed().await.map_err(error_response)?,
                vec![],
                vec![],
            ),
            "addMany" => {
                // Best effort: create what we can, report per-item failures so an
                // IDM-style bulk import of 500 links doesn't abort on one bad line.
                let mut ids = Vec::new();
                let mut errors = Vec::new();
                // Build the (index, url, request) triples from whichever form was sent.
                let items: Vec<(u32, String, NewDownloadRequest)> = match body.tasks {
                    Some(tasks) => {
                        if tasks.is_empty() {
                            return Err(error_response(AriaFlowError::InvalidRequest(
                                "addMany requires a non-empty `tasks` array".into(),
                            )));
                        }
                        tasks
                            .into_iter()
                            .enumerate()
                            .map(|(i, t)| (i as u32, t.url.clone(), t))
                            .collect()
                    }
                    None => {
                        let urls = body.urls.unwrap_or_default();
                        if urls.is_empty() {
                            return Err(error_response(AriaFlowError::InvalidRequest(
                                "addMany requires a non-empty `urls` or `tasks` array".into(),
                            )));
                        }
                        urls.into_iter()
                            .enumerate()
                            .map(|(i, u)| (i as u32, u.clone(), NewDownloadRequest::new(u)))
                            .collect()
                    }
                };
                for (index, url, req) in items {
                    match svc.add_download(req).await {
                        Ok(dl) => ids.push(dl.id.to_string()),
                        Err(e) => errors.push(BatchItemError {
                            index,
                            url,
                            message: e.to_string(),
                        }),
                    }
                }
                (ids.len() as u32, ids, errors)
            }
            other => {
                return Err(error_response(AriaFlowError::InvalidRequest(format!(
                    "unknown batch action: {other}"
                ))))
            }
        };
    Ok(Json(BatchResponse {
        action: body.action,
        affected,
        ids,
        errors,
    }))
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn get_settings(
    State(state): State<ApiState>,
) -> Result<Json<ariaflow_core::GlobalSettings>, Response> {
    state
        .service
        .get_settings()
        .await
        .map(Json)
        .map_err(error_response)
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn put_settings(
    State(state): State<ApiState>,
    Json(patch): Json<UpdateSettingsRequest>,
) -> Result<Json<ariaflow_core::GlobalSettings>, Response> {
    // Bind addresses are boot-time values: reject a malformed apiBind here so a bad
    // value can never brick the next daemon start. (Empty is rejected because an
    // unset apiBind means "keep the default", never "no listener" — use 127.0.0.1 to
    // not expose anything.)
    let api_bind_field_set = patch.api_bind.is_some();
    if let Some(raw) = patch.api_bind.as_deref().map(str::trim) {
        if raw.is_empty() || raw.parse::<std::net::SocketAddr>().is_err() {
            return Err(error_response(
                ariaflow_core::AriaFlowError::InvalidRequest(format!(
                    "apiBind {raw:?} is not an ADDR:PORT socket address (e.g. \"0.0.0.0:8787\")"
                )),
            ));
        }
    }
    // Bind addresses are boot-time values: reject a malformed webUiBind here so a
    // bad value can never brick the next daemon start.
    let web_bind_field_set = patch.web_ui_bind.is_some();
    if let Some(raw) = patch.web_ui_bind.as_deref().map(str::trim) {
        if !raw.is_empty() && raw.parse::<std::net::SocketAddr>().is_err() {
            return Err(error_response(
                ariaflow_core::AriaFlowError::InvalidRequest(format!(
                    "webUiBind {raw:?} is not an ADDR:PORT socket address (e.g. \"0.0.0.0:9090\")"
                )),
            ));
        }
    }
    match state.service.update_settings(patch).await {
        Ok(settings) => {
            // Push the runtime feature switches immediately — no restart needed.
            state.features.set_all(
                settings.api_enabled,
                settings.mcp_enabled,
                settings.web_ui_enabled,
            );
            if api_bind_field_set {
                tracing::info!(
                    api_bind = %settings.api_bind,
                    "apiBind saved; it takes effect after the daemon restarts (--api-bind overrides)"
                );
            }
            if web_bind_field_set {
                tracing::info!(
                    web_ui_bind = %settings.web_ui_bind,
                    "webUiBind saved; it takes effect after the daemon restarts (--web-bind overrides)"
                );
            }
            tracing::info!(
                api = settings.api_enabled,
                mcp = settings.mcp_enabled,
                web = settings.web_ui_enabled,
                "feature switches updated"
            );
            Ok(Json(settings))
        }
        Err(e) => Err(error_response(e)),
    }
}

#[allow(clippy::result_large_err)]
async fn list_history(
    State(state): State<ApiState>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<ariaflow_core::HistoryEntry>>, Response> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    state
        .service
        .list_history(limit)
        .await
        .map(Json)
        .map_err(error_response)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryDeleteQuery {
    /// Comma-separated download_history row ids; missing/empty clears all.
    ids: Option<String>,
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn delete_history(
    State(state): State<ApiState>,
    Query(q): Query<HistoryDeleteQuery>,
) -> Result<Json<serde_json::Value>, Response> {
    let deleted = match q.ids.as_deref().map(str::trim) {
        None | Some("") => state
            .service
            .clear_history()
            .await
            .map_err(error_response)?,
        Some(list) => {
            let mut ids: Vec<i64> = Vec::new();
            for part in list.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let id: i64 = part.parse().map_err(|_| {
                    error_response(AriaFlowError::InvalidRequest(format!(
                        "invalid history id {part:?}"
                    )))
                })?;
                ids.push(id);
            }
            state
                .service
                .delete_history(&ids)
                .await
                .map_err(error_response)?
        }
    };
    Ok(Json(json!({ "deleted": deleted })))
}

/// The built-in category classification rules (machine-readable export).
/// Static data, so it is public within the authenticated API surface.
async fn categories() -> Json<serde_json::Value> {
    Json(ariaflow_core::category::category_rules_json())
}

#[allow(clippy::result_large_err)]
async fn list_proxies(
    State(state): State<ApiState>,
) -> Result<Json<Vec<ariaflow_core::ProxyEntry>>, Response> {
    state
        .service
        .list_proxies()
        .await
        .map(Json)
        .map_err(error_response)
}

#[allow(clippy::result_large_err)]
async fn create_proxy(
    State(state): State<ApiState>,
    Json(input): Json<ariaflow_core::ProxyInput>,
) -> Result<(StatusCode, Json<ariaflow_core::ProxyEntry>), Response> {
    state
        .service
        .create_proxy(input)
        .await
        .map(|p| (StatusCode::CREATED, Json(p)))
        .map_err(error_response)
}

#[allow(clippy::result_large_err)]
async fn update_proxy(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(input): Json<ariaflow_core::ProxyInput>,
) -> Result<Json<ariaflow_core::ProxyEntry>, Response> {
    state
        .service
        .update_proxy(&id, input)
        .await
        .map(Json)
        .map_err(error_response)
}

async fn delete_proxy(State(state): State<ApiState>, Path(id): Path<String>) -> Response {
    match state.service.delete_proxy(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

// ---------- SSH certificates (metadata only — key material never leaves) ----------

#[allow(clippy::result_large_err)]
async fn list_ssh_certificates(
    State(state): State<ApiState>,
) -> Result<Json<Vec<ariaflow_core::SshCertificateEntry>>, Response> {
    state
        .service
        .list_ssh_certificates()
        .await
        .map(Json)
        .map_err(error_response)
}

#[allow(clippy::result_large_err)]
async fn create_ssh_certificate(
    State(state): State<ApiState>,
    Json(input): Json<ariaflow_core::SshCertificateInput>,
) -> Result<(StatusCode, Json<ariaflow_core::SshCertificateEntry>), Response> {
    state
        .service
        .create_ssh_certificate(input)
        .await
        .map(|c| (StatusCode::CREATED, Json(c)))
        .map_err(error_response)
}

#[allow(clippy::result_large_err)]
async fn update_ssh_certificate(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(input): Json<ariaflow_core::SshCertificateInput>,
) -> Result<Json<ariaflow_core::SshCertificateEntry>, Response> {
    state
        .service
        .update_ssh_certificate(&id, input)
        .await
        .map(Json)
        .map_err(error_response)
}

async fn delete_ssh_certificate(State(state): State<ApiState>, Path(id): Path<String>) -> Response {
    match state.service.delete_ssh_certificate(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => error_response(e),
    }
}

// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn probe(
    State(state): State<ApiState>,
    Json(req): Json<ProbeRequest>,
) -> Result<(StatusCode, Json<ariaflow_protocol::RemoteFileInfo>), Response> {
    let provider = state.provider.clone().ok_or_else(|| {
        error_response(AriaFlowError::Protocol(
            "probe endpoint is not enabled on this daemon".to_string(),
        ))
    })?;
    // Probe honours the configured global User-Agent so URL inspection behaves
    // exactly like a download (many hosts reject obvious bot UAs).
    let user_agent = state
        .service
        .get_settings()
        .await
        .map(|s| s.user_agent)
        .ok()
        .filter(|ua| !ua.trim().is_empty())
        .unwrap_or_else(|| ariaflow_core::settings::DEFAULT_USER_AGENT.to_string());
    let options = ariaflow_protocol::ProviderOptions {
        headers: req.headers,
        user_agent: Some(user_agent),
        connect_timeout_secs: 30,
        read_timeout_secs: 60,
        ..Default::default()
    };
    match provider.probe(&req.url, &options).await {
        Ok(info) => Ok((StatusCode::OK, Json(info))),
        Err(ariaflow_protocol::ProviderError::BadStatus { status, message }) => Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": { "code": "bad_status", "message": format!("{status}: {message}") } })),
        )
            .into_response()),
        Err(e) => Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": { "code": "probe_failed", "message": e.to_string() } })),
        )
            .into_response()),
    }
}

async fn events_sse(
    State(state): State<ApiState>,
) -> Sse<
    impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(ev) => Some(ev),
        Err(_lagged) => {
            tracing::warn!("SSE client lagged; some progress events were dropped");
            None
        }
    });
    let stream = stream.map(|ev: DownloadEvent| {
        let kind = ev.kind().to_string();
        let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
        Ok(axum::response::sse::Event::default().event(kind).data(data))
    });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}

async fn openapi_json() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        include_str!("../assets/openapi.json"),
    )
}

/// JSON 404 for unmatched API/MCP paths so the SPA fallback never swallows them.
async fn api_not_found() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": { "code": "not_found", "message": "unknown API endpoint" } })),
    )
        .into_response()
}

/// 503 gate for the web API (mounted on the TCP listener only; the pipe is exempt).
macro_rules! api_gate {
    ($features:expr) => {{
        let gate_features = $features;
        middleware::from_fn(move |req: Request, next: axum::middleware::Next| {
            let features = gate_features.clone();
            async move {
                if features.get(Feature::Api) {
                    next.run(req).await
                } else {
                    features::disabled_json(Feature::Api)
                }
            }
        })
    }};
}

/// 503 gate for the MCP endpoint (applied on every listener).
macro_rules! mcp_gate {
    ($features:expr) => {{
        let gate_features = $features;
        middleware::from_fn(move |req: Request, next: axum::middleware::Next| {
            let features = gate_features.clone();
            async move {
                if features.get(Feature::Mcp) {
                    next.run(req).await
                } else {
                    features::disabled_json(Feature::Mcp)
                }
            }
        })
    }};
}

async fn web_fallback_response(state: ApiState, req: Request) -> Response {
    if !state.features.get(Feature::Web) {
        return features::disabled_html(Feature::Web);
    }
    match &state.web_ui {
        WebUi::Missing => webui::placeholder().into_response(),
        ui => webui::serve(
            ui,
            req.uri().path(),
            req.method(),
            req.headers()
                .get(axum::http::header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok()),
        ),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsListQuery {
    /// Absolute directory to list; empty/missing lists the user's home directory.
    path: Option<String>,
    show_hidden: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FsEntry {
    name: String,
    /// Absolute path of the entry (join of the canonical listed dir + name).
    path: String,
    /// "dir" | "file" | "other"
    kind: String,
    /// Bytes; only for regular files.
    size: Option<u64>,
    /// Seconds since the Unix epoch; null when unknown.
    modified_secs: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FsListResponse {
    /// Canonical absolute path that was listed.
    path: String,
    /// Canonical parent, null at the filesystem root.
    parent: Option<String>,
    is_root: bool,
    show_hidden: bool,
    /// True when more entries exist than were returned (capped at 5000).
    truncated: bool,
    entries: Vec<FsEntry>,
}

const FS_LIST_CAP: usize = 5000;

fn fs_error_response(status: StatusCode, code: &str, message: String) -> Response {
    (
        status,
        Json(ApiErrorBody {
            error: ApiErrorDetail {
                code: code.to_string(),
                message,
            },
        }),
    )
        .into_response()
}

/// Read-only directory listing backing the in-app folder picker. Runs under the
/// same auth/rate-limit/feature gates as the rest of the API; it exposes entry
/// metadata only (never file contents).
// axum handlers conventionally return `Response` on error; avoiding the large-Err variant
// would change every route signature for a local API — suppress the lint locally.
#[allow(clippy::result_large_err)]
async fn fs_list(Query(q): Query<FsListQuery>) -> Result<Json<FsListResponse>, Response> {
    let requested = q.path.unwrap_or_default().trim().to_string();
    let show_hidden = q.show_hidden.unwrap_or(false);
    let base = if requested.is_empty() {
        dirs::home_dir().ok_or_else(|| {
            fs_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "could not resolve the home directory; pass an explicit path".to_string(),
            )
        })?
    } else {
        std::path::PathBuf::from(requested)
    };
    let canonical = std::fs::canonicalize(&base).map_err(|e| {
        fs_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("cannot read directory {}: {e}", base.display()),
        )
    })?;
    if !canonical.is_dir() {
        return Err(fs_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("{} is not a directory", canonical.display()),
        ));
    }

    let mut entries: Vec<FsEntry> = Vec::new();
    let mut truncated = false;
    let read_dir = std::fs::read_dir(&canonical).map_err(|e| {
        fs_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("cannot list directory {}: {e}", canonical.display()),
        )
    })?;
    for entry in read_dir {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        // Follow symlinks for type detection; a broken link stays visible as "other".
        let md = std::fs::metadata(entry.path()).ok();
        let (kind, size, modified_secs) = match &md {
            Some(md) if md.is_dir() => ("dir".to_string(), None, modified_secs(md)),
            Some(md) if md.is_file() => ("file".to_string(), Some(md.len()), modified_secs(md)),
            Some(_) => ("other".to_string(), None, None),
            None => ("other".to_string(), None, None),
        };
        entries.push(FsEntry {
            path: canonical.join(&name).to_string_lossy().into_owned(),
            name,
            kind,
            size,
            modified_secs,
        });
        if entries.len() >= FS_LIST_CAP {
            truncated = true;
            break;
        }
    }
    fn kind_rank(kind: &str) -> u8 {
        match kind {
            "dir" => 0,
            "file" => 1,
            _ => 2,
        }
    }
    entries.sort_by(|a, b| {
        kind_rank(&a.kind)
            .cmp(&kind_rank(&b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let parent = canonical.parent().map(|p| p.to_string_lossy().into_owned());
    let is_root = parent.as_deref() == Some(canonical.to_string_lossy().as_ref())
        || canonical.parent().is_none();
    Ok(Json(FsListResponse {
        path: canonical.to_string_lossy().into_owned(),
        parent,
        is_root,
        show_hidden,
        truncated,
        entries,
    }))
}

// ---------- completed-file download / save-as ----------

/// POST body for `POST /api/v1/downloads/{id}/save-as`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAsRequest {
    /// Directory on the *daemon host* to copy the file into.
    pub dest_dir: String,
    /// Target file name; defaults to the task's file name. Never a path.
    pub filename: Option<String>,
    /// Overwrite an existing file with the same name (default false → 409).
    pub overwrite: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAsResponse {
    pub saved_to: String,
    pub size: u64,
}

/// Task lookup shared by the file endpoints: the task must exist, be completed
/// and its recorded file must still be a regular file on disk.
#[allow(clippy::result_large_err)]
async fn completed_task(state: &ApiState, id: &str) -> Result<ariaflow_core::Download, Response> {
    let id = id.parse::<DownloadId>().map_err(error_response)?;
    let dl = match state.service.get_download(id).await {
        Ok(Some(dl)) => dl,
        Ok(None) => return Err(error_response(AriaFlowError::NotFound(id.to_string()))),
        Err(e) => return Err(error_response(e)),
    };
    if dl.status != DownloadStatus::Completed {
        return Err(fs_error_response(
            StatusCode::CONFLICT,
            "not_completed",
            format!(
                "task {id} is {} — files can only be fetched once the download completed",
                dl.status.as_str()
            ),
        ));
    }
    Ok(dl)
}

/// History-row lookup shared by the history file endpoints: the entry must
/// exist and record a successful completion.
#[allow(clippy::result_large_err)]
async fn completed_history_entry(
    state: &ApiState,
    id: i64,
) -> Result<ariaflow_core::HistoryEntry, Response> {
    let entry = match state.service.get_history(id).await {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            return Err(fs_error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("history entry {id} not found"),
            ))
        }
        Err(e) => return Err(error_response(e)),
    };
    if entry.result != "completed" {
        return Err(fs_error_response(
            StatusCode::CONFLICT,
            "not_completed",
            format!(
                "history entry {id} is {} — files can only be fetched for completed downloads",
                entry.result
            ),
        ));
    }
    Ok(entry)
}

/// Resolve the on-disk path of a file recorded by the daemon, or a 404/409 response.
#[allow(clippy::result_large_err)]
async fn resolve_existing_file(
    path: &std::path::Path,
) -> Result<(std::path::PathBuf, u64), Response> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(meta) => meta,
        Err(_) => {
            return Err(fs_error_response(
                StatusCode::NOT_FOUND,
                "file_missing",
                format!("the file is missing on disk: {}", path.display()),
            ))
        }
    };
    if !meta.is_file() {
        return Err(fs_error_response(
            StatusCode::CONFLICT,
            "not_a_file",
            format!("path is not a regular file: {}", path.display()),
        ));
    }
    Ok((path.to_path_buf(), meta.len()))
}

/// Resolve the on-disk path for a completed task, or a 404/409 response.
#[allow(clippy::result_large_err)]
async fn resolve_task_file(
    dl: &ariaflow_core::Download,
) -> Result<(std::path::PathBuf, u64), Response> {
    resolve_existing_file(&dl.save_path).await
}

/// RFC 5987 Content-Disposition: ASCII fallback + UTF-8 filename*.
fn attachment_content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && c != '\\' && (c as u32) >= 0x20 && (c as u32) != 0x7f {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::with_capacity(filename.len() * 3);
    for byte in filename.as_bytes() {
        let is_unreserved = matches!(byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~');
        if is_unreserved {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// Parse a single HTTP `Range: bytes=…` request against a known total size.
/// Returns `(start, inclusive_end)`, or `None` if unsatisfiable/unsupported.
fn parse_range(header_value: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header_value.trim().strip_prefix("bytes=")?;
    // Single range only; multi-range requests fall back to a full-body 200.
    if spec.contains(',') {
        return None;
    }
    let (start_s, end_s) = spec.split_once('-')?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();
    if start_s.is_empty() {
        let suffix: u64 = end_s.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let n = suffix.min(total);
        Some((total - n, total - 1))
    } else {
        let start: u64 = start_s.parse().ok()?;
        if start >= total {
            return None;
        }
        let end: u64 = if end_s.is_empty() {
            total - 1
        } else {
            end_s.parse::<u64>().ok()?.min(total - 1)
        };
        if start > end {
            return None;
        }
        Some((start, end))
    }
}

fn attachment_file_name(recorded: &str, path: &std::path::Path) -> String {
    if recorded.trim().is_empty() {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".to_string())
    } else {
        recorded.to_string()
    }
}

/// Build the streaming response for a resolved local file (200 or 206).
async fn stream_file_response(
    path: &std::path::Path,
    total: u64,
    filename: &str,
    range: Option<String>,
) -> Response {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(e) => {
            return fs_error_response(
                StatusCode::NOT_FOUND,
                "file_missing",
                format!("cannot open {}: {e}", path.display()),
            )
        }
    };

    let disposition = attachment_content_disposition(filename);
    let base = Response::builder().header(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );

    match range {
        Some(spec) => match parse_range(&spec, total) {
            Some((start, end)) => {
                use tokio::io::{AsyncReadExt, AsyncSeekExt};
                let len = end - start + 1;
                let mut file = file;
                if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                    return fs_error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "seek_failed",
                        "failed to seek in the source file".to_string(),
                    );
                }
                let body = Body::from_stream(ReaderStream::new(file.take(len)));
                build_file_response(
                    base,
                    StatusCode::PARTIAL_CONTENT,
                    "application/octet-stream",
                    disposition,
                    len,
                    Some((start, end, total)),
                    body,
                )
            }
            None => Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                .body(Body::empty())
                .expect("static response"),
        },
        None => {
            let body = Body::from_stream(ReaderStream::new(file));
            build_file_response(
                base,
                StatusCode::OK,
                "application/octet-stream",
                disposition,
                total,
                None,
                body,
            )
        }
    }
}

fn build_file_response(
    base: axum::http::response::Builder,
    status: StatusCode,
    content_type: &'static str,
    disposition: String,
    len: u64,
    range: Option<(u64, u64, u64)>,
    body: Body,
) -> Response {
    let mut builder = base
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, len.to_string());
    if let Some((start, end, total)) = range {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }
    builder.body(body).unwrap_or_else(|e| {
        fs_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "response_build",
            e.to_string(),
        )
    })
}

/// `GET /api/v1/downloads/{id}/file` — stream the completed file as an
/// attachment. Supports a single `Range: bytes=…` request (206) so browsers
/// can resume / probe large files. Auth: the same bearer token as the rest
/// of the API (this leaks nothing beyond files of tasks the caller can see).
#[allow(clippy::result_large_err)]
async fn download_task_file(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, Response> {
    let dl = completed_task(&state, &id).await?;
    let (path, total) = resolve_task_file(&dl).await?;
    let filename = attachment_file_name(&dl.filename, &path);
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    Ok(stream_file_response(&path, total, &filename, range).await)
}

/// `GET /api/v1/history/{id}/file` — same streaming semantics keyed by a
/// `download_history` row id, so it also works after the task row was deleted.
#[allow(clippy::result_large_err)]
async fn download_history_file(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    headers: axum::http::HeaderMap,
) -> Result<Response, Response> {
    let entry = completed_history_entry(&state, id).await?;
    let (path, total) = resolve_existing_file(std::path::Path::new(&entry.save_path)).await?;
    let filename = attachment_file_name(&entry.filename, &path);
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    Ok(stream_file_response(&path, total, &filename, range).await)
}

/// Reject names that are not safe single file names.
fn validate_file_name(name: &str) -> Result<String, (StatusCode, &'static str, String)> {
    let trimmed = name.trim();
    let invalid = |msg: &str| Err((StatusCode::BAD_REQUEST, "invalid_filename", msg.to_string()));
    if trimmed.is_empty() {
        return invalid("filename must not be empty");
    }
    if trimmed == "." || trimmed == ".." {
        return invalid("filename must not be '.' or '..'");
    }
    if trimmed.len() > 255 {
        return invalid("filename is too long (max 255 bytes/chars)");
    }
    if trimmed
        .chars()
        .any(|c| matches!(c, '/' | '\\') || c.is_control())
    {
        return invalid("filename must not contain path separators or control characters");
    }
    Ok(trimmed.to_string())
}

/// Server-side copy of a local file into `req.dest_dir`. Shared by the
/// task and history save-as endpoints.
#[allow(clippy::result_large_err)]
async fn save_local_file_as(
    src_path: std::path::PathBuf,
    default_filename: &str,
    req: SaveAsRequest,
) -> Result<Json<SaveAsResponse>, Response> {
    let dest_dir = std::path::PathBuf::from(req.dest_dir.trim());
    if dest_dir.as_os_str().is_empty() {
        return Err(fs_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "destDir must not be empty".to_string(),
        ));
    }
    let dest_dir = std::fs::canonicalize(&dest_dir).map_err(|e| {
        fs_error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            format!(
                "cannot read destination directory {}: {e}",
                dest_dir.display()
            ),
        )
    })?;
    if !dest_dir.is_dir() {
        return Err(fs_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("{} is not a directory", dest_dir.display()),
        ));
    }

    let filename = validate_file_name(req.filename.as_deref().unwrap_or(default_filename))
        .map_err(|(status, code, message)| fs_error_response(status, code, message))?;

    let src_canonical = std::fs::canonicalize(&src_path).map_err(|e| {
        fs_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "canonicalize_failed",
            format!("cannot resolve {}: {e}", src_path.display()),
        )
    })?;
    let dest = dest_dir.join(&filename);
    // Refuse to copy a file onto itself (also via symlinked directories).
    let dest_canonical = if dest.exists() {
        std::fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone())
    } else {
        dest_dir.join(&filename)
    };
    if dest_canonical == src_canonical {
        return Err(fs_error_response(
            StatusCode::BAD_REQUEST,
            "same_file",
            "source and destination are the same file".to_string(),
        ));
    }
    let overwrite = req.overwrite.unwrap_or(false);
    if dest.exists() && !overwrite {
        return Err(fs_error_response(
            StatusCode::CONFLICT,
            "already_exists",
            format!(
                "{} already exists; pass overwrite=true to replace it",
                dest.display()
            ),
        ));
    }

    let src_for_copy = src_canonical.clone();
    let dest_for_copy = dest.clone();
    let size = tokio::task::spawn_blocking(move || std::fs::copy(&src_for_copy, &dest_for_copy))
        .await
        .map_err(|e| {
            fs_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "copy_failed",
                format!("copy task failed: {e}"),
            )
        })?
        .map_err(|e| {
            fs_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "copy_failed",
                format!("cannot copy to {}: {e}", dest.display()),
            )
        })?;

    Ok(Json(SaveAsResponse {
        saved_to: dest.to_string_lossy().into_owned(),
        size,
    }))
}

/// `POST /api/v1/downloads/{id}/save-as` — server-side copy of a completed
/// file into `destDir` (a directory on the daemon host). This is what the
/// web UI's "另存为…" and the MCP `ariaflow_save_download` tool use.
#[allow(clippy::result_large_err)]
async fn save_task_file_as(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<SaveAsRequest>,
) -> Result<Json<SaveAsResponse>, Response> {
    let dl = completed_task(&state, &id).await?;
    let (src_path, _) = resolve_task_file(&dl).await?;
    save_local_file_as(src_path, &dl.filename, req).await
}

/// `POST /api/v1/history/{id}/save-as` — history-scoped variant of the
/// save-as endpoint; same daemon-host copy semantics.
#[allow(clippy::result_large_err)]
async fn save_history_file_as(
    State(state): State<ApiState>,
    Path(id): Path<i64>,
    Json(req): Json<SaveAsRequest>,
) -> Result<Json<SaveAsResponse>, Response> {
    let entry = completed_history_entry(&state, id).await?;
    let (src_path, _) = resolve_existing_file(std::path::Path::new(&entry.save_path)).await?;
    save_local_file_as(src_path, &entry.filename, req).await
}

fn modified_secs(md: &std::fs::Metadata) -> Option<i64> {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Builds the router, mounting only the API. Exposed for integration tests.
pub fn router(state: ApiState) -> Router {
    router_with(state, None)
}

/// Builds the router with full feature gating — used for the TCP listener.
/// The pipe listener uses [`router_ipc`] instead (web API gate exempt).
///
/// The optional MCP router (conventionally serving `/mcp`) is mounted under
/// the same bearer-token semantics as the rest of the API, plus the MCP
/// feature gate. The web UI at `/` is gated by `state.features.web`; the
/// 503 page is served in-process when disabled.
pub fn router_with(state: ApiState, mcp: Option<Router>) -> Router {
    build_app(state, mcp, true)
}

/// Pipe router (Unix socket / named pipe): the local, trusted management
/// channel — the web API gate is NOT applied (so it can always be used to
/// re-enable the API), while the MCP and web gates still apply.
pub fn router_ipc(state: ApiState, mcp: Option<Router>) -> Router {
    build_app(state, mcp, false)
}

fn build_app(state: ApiState, mcp: Option<Router>, gate_api: bool) -> Router {
    let features = state.features.clone();
    let protected = Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/downloads", get(list_downloads).post(add_download))
        .route("/api/v1/downloads/batch", post(batch))
        .route(
            "/api/v1/downloads/{id}",
            get(get_download).delete(delete_download),
        )
        .route("/api/v1/downloads/{id}/pause", post(pause_download_handler))
        .route(
            "/api/v1/downloads/{id}/resume",
            post(resume_download_handler),
        )
        .route(
            "/api/v1/downloads/{id}/cancel",
            post(cancel_download_handler),
        )
        .route("/api/v1/downloads/{id}/retry", post(retry_download_handler))
        .route("/api/v1/downloads/{id}/priority", put(set_priority))
        .route("/api/v1/downloads/{id}/limit-rate", put(set_limit_rate))
        .route("/api/v1/downloads/{id}/file", get(download_task_file))
        .route("/api/v1/downloads/{id}/save-as", post(save_task_file_as))
        .route("/api/v1/settings", get(get_settings).put(put_settings))
        .route("/api/v1/history", get(list_history).delete(delete_history))
        .route("/api/v1/history/{id}/file", get(download_history_file))
        .route("/api/v1/history/{id}/save-as", post(save_history_file_as))
        .route("/api/v1/categories", get(categories))
        .route(
            "/api/v1/certificates",
            get(list_ssh_certificates).post(create_ssh_certificate),
        )
        .route(
            "/api/v1/certificates/{id}",
            put(update_ssh_certificate).delete(delete_ssh_certificate),
        )
        .route("/api/v1/proxies", get(list_proxies).post(create_proxy))
        .route(
            "/api/v1/proxies/{id}",
            put(update_proxy).delete(delete_proxy),
        )
        .route("/api/v1/probe", post(probe))
        .route("/api/v1/fs/list", get(fs_list))
        .route("/api/v1/events", get(events_sse))
        .route("/api/v1/openapi.json", get(openapi_json));
    let mut protected = protected.layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    protected = protected.layer(middleware::from_fn_with_state(
        state.clone(),
        rate_limit_middleware,
    ));
    if gate_api {
        // Outermost: reject disabled requests before auth/rate-limit work.
        protected = protected.layer(api_gate!(features.clone()));
    }

    let app = Router::new()
        .merge(protected)
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:5173".parse().unwrap(),
                    "http://127.0.0.1:5173".parse().unwrap(),
                    "http://localhost:4173".parse().unwrap(),
                    "http://127.0.0.1:4173".parse().unwrap(),
                    "https://tauri.localhost".parse().unwrap(),
                    "tauri://localhost".parse().unwrap(),
                ])
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .layer(TraceLayer::new_for_http())
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(state.clone());

    // The MCP router carries its own `()` state; wrap it in the same auth +
    // rate-limit middleware the REST API uses, now that both sides are `Router<()>`.
    let app = match mcp {
        Some(mcp_router) => app.merge(
            mcp_router
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                ))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    rate_limit_middleware,
                ))
                .layer(mcp_gate!(features.clone())),
        ),
        None => app,
    };

    // Unmatched API/MCP subpaths must answer with a JSON 404, never with the SPA.
    let mut api_catchall = Router::new()
        .route("/api", any(api_not_found))
        .route("/api/v1", any(api_not_found))
        .route("/api/{*path}", any(api_not_found))
        .route("/mcp/{*path}", any(api_not_found));
    if gate_api {
        api_catchall = api_catchall.layer(api_gate!(features.clone()));
    }
    let app = app.merge(api_catchall);

    // The web UI is gated in-process so it can be toggled without a restart.
    let web_state = state.clone();
    app.fallback(move |req: Request| {
        let state = web_state.clone();
        async move { web_fallback_response(state, req).await }
    })
}

/// Serve the same API over a platform IPC channel — Unix domain socket on
/// Unix-likes, named pipe on Windows — so local clients (the desktop shell)
/// never depend on TCP or on the web API surface.
///
/// Same router, same bearer-token semantics as [`serve`]; the pipe itself is
/// additionally protected by filesystem ownership (mode 0600 on Unix).
/// SSE streaming works; the request/response cycle is identical to TCP.
#[cfg(unix)]
pub async fn serve_ipc(
    state: ApiState,
    endpoint: &str,
    mcp: Option<Router>,
) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let path = std::path::Path::new(endpoint);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    // A stale socket from an unclean shutdown would block the bind.
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    tracing::info!(path = %path.display(), "AriaFlow IPC listening");
    axum::serve(listener, router_ipc(state, mcp))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// Windows named-pipe implementation, e.g. `\\.\pipe\ariaflowd`.
///
/// `axum::serve` needs a [`axum::serve::Listener`]; tokio exposes named pipes
/// through `NamedPipeServer`, so we adapt it here. One `Listen` instance is
/// always pre-created so sequential clients queue up instead of getting
/// broken-pipe errors between connections.
#[cfg(windows)]
struct NamedPipeListener {
    name: String,
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl NamedPipeListener {
    fn bind(name: &str) -> std::io::Result<Self> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(name)?;
        Ok(Self {
            name: name.to_string(),
            server,
        })
    }
}

#[cfg(windows)]
impl axum::serve::Listener for NamedPipeListener {
    type Io = tokio::net::windows::named_pipe::NamedPipeServer;
    type Addr = String;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        use tokio::net::windows::named_pipe::ServerOptions;
        loop {
            match self.server.connect().await {
                Ok(()) => {}
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                }
            }
            // Hand the connected instance to the caller and pre-create the
            // next one so a client that arrives right now still connects.
            match ServerOptions::new().create(&self.name) {
                Ok(next) => {
                    let connected = std::mem::replace(&mut self.server, next);
                    return (connected, self.name.clone());
                }
                Err(e) => panic!("failed to create next named-pipe instance: {e}"),
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.name.clone())
    }
}

#[cfg(windows)]
pub async fn serve_ipc(
    state: ApiState,
    endpoint: &str,
    mcp: Option<Router>,
) -> std::io::Result<()> {
    let listener = NamedPipeListener::bind(endpoint)?;
    tracing::info!(endpoint, "AriaFlow IPC listening (named pipe)");
    axum::serve(listener, router_ipc(state, mcp))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

#[cfg(not(any(unix, windows)))]
pub async fn serve_ipc(
    _state: ApiState,
    _endpoint: &str,
    _mcp: Option<Router>,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "IPC is only supported on Unix-like systems and Windows",
    ))
}

/// Convenience: serve with graceful shutdown on Ctrl-C / SIGTERM.
pub async fn serve(state: ApiState, bind: SocketAddr, mcp: Option<Router>) -> std::io::Result<()> {
    let app = router_with(state, mcp);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "AriaFlow API listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
