//! Transport-aware Rust client for the AriaFlow daemon.
//! Used by the CLI, the TUI, the MCP server and the browser native host.
//!
//! Two channels carry the same HTTP wire protocol:
//!
//! - [`Transport::WebApi`] — HTTP over TCP (the Web API, default `http://127.0.0.1:8787`)
//! - [`Transport::Ipc`] — HTTP/1.1 over the local pipe (Unix domain socket on
//!   Unix, a named pipe on Windows)
//!
//! [`Transport::Auto`] (the CLI default) prefers the pipe and silently falls
//! back to the Web API on *connect-class* failures only — a request that
//! reached the daemon is never replayed behind the user's back.

use crate::dto::{BatchResponse, StatusResponse};
use crate::ipc;
use ariaflow_core::{Download, DownloadFilter, DownloadId, GlobalSettings, NewDownloadRequest};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("API error ({status}) [{code}]: {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("failed to decode response: {0}")]
    Decode(String),
}

pub type ClientResult<T> = std::result::Result<T, ClientError>;

/// Which channel a client talks to the daemon over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// HTTP over TCP (the Web API).
    WebApi { base_url: String },
    /// HTTP/1.1 over the local pipe (Unix domain socket / named pipe).
    Ipc { endpoint: String },
    /// Prefer the pipe; fall back to the Web API on connect-class failures.
    Auto {
        endpoint: String,
        fallback_base_url: String,
    },
}

/// Builds `/api/v1/downloads?...` with properly percent-encoded query parameters.
fn list_query_path(filter: &DownloadFilter) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    if !filter.statuses.is_empty() {
        let joined = filter
            .statuses
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(",");
        serializer.append_pair("status", &joined);
    }
    if let Some(cat) = &filter.category {
        serializer.append_pair("category", cat);
    }
    if let Some(q) = &filter.search {
        serializer.append_pair("q", q);
    }
    if let Some(p) = filter.page {
        serializer.append_pair("page", &p.to_string());
    }
    if let Some(ps) = filter.page_size {
        serializer.append_pair("pageSize", &ps.to_string());
    }
    let query = serializer.finish();
    if query.is_empty() {
        "/api/v1/downloads".to_string()
    } else {
        format!("/api/v1/downloads?{query}")
    }
}

impl Transport {
    /// Describe the transport for diagnostics, e.g. in error messages.
    fn describe(&self) -> String {
        match self {
            Transport::WebApi { base_url } => format!("web API {base_url}"),
            Transport::Ipc { endpoint } => format!("pipe {endpoint}"),
            Transport::Auto {
                endpoint,
                fallback_base_url,
            } => format!("pipe {endpoint} (fallback web API {fallback_base_url})"),
        }
    }
}

#[derive(Clone)]
pub struct AriaFlowClient {
    transport: Transport,
    token: Option<String>,
    http: reqwest::Client,
}

impl AriaFlowClient {
    /// Backwards-compatible constructor: plain Web API client at `base_url`.
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> ClientResult<Self> {
        Self::web_api(base_url, token)
    }

    /// Web API transport at `base_url` (e.g. `http://127.0.0.1:8787`).
    pub fn web_api(base_url: impl Into<String>, token: Option<String>) -> ClientResult<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        url::Url::parse(&base_url)
            .map_err(|e| ClientError::Connection(format!("invalid base url: {e}")))?;
        Ok(Self::with_transport(Transport::WebApi { base_url }, token))
    }

    /// Pipe transport. `endpoint` of `None` selects the platform default
    /// (Unix: `$XDG_RUNTIME_DIR/ariaflowd.sock` or data-dir fallback;
    /// Windows: `\\.\pipe\ariaflowd`).
    pub fn ipc(endpoint: Option<&str>, token: Option<String>) -> ClientResult<Self> {
        let endpoint = endpoint
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .unwrap_or_else(ipc::default_endpoint);
        Ok(Self::with_transport(Transport::Ipc { endpoint }, token))
    }

    /// Pipe-first transport that transparently retries over the Web API when
    /// the pipe cannot be reached at all. Non-connect errors (HTTP error
    /// status, malformed response) are never retried elsewhere.
    pub fn auto(
        endpoint: Option<&str>,
        api_url: impl Into<String>,
        token: Option<String>,
    ) -> ClientResult<Self> {
        let fallback_base_url = api_url.into().trim_end_matches('/').to_string();
        url::Url::parse(&fallback_base_url)
            .map_err(|e| ClientError::Connection(format!("invalid base url: {e}")))?;
        let endpoint = endpoint
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .unwrap_or_else(ipc::default_endpoint);
        Ok(Self::with_transport(
            Transport::Auto {
                endpoint,
                fallback_base_url,
            },
            token,
        ))
    }

    fn with_transport(transport: Transport, token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client builder cannot fail with default configuration");
        Self {
            transport,
            token,
            http,
        }
    }

    fn url(&self, path: &str) -> String {
        let base = match &self.transport {
            Transport::WebApi { base_url } => base_url,
            Transport::Ipc { .. } => "http://localhost",
            Transport::Auto {
                fallback_base_url, ..
            } => fallback_base_url,
        };
        format!("{base}{path}")
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    /// Single request entry point shared by both transports.
    /// Returns `(status, raw body)`; error mapping happens in `raw_json`/`raw_unit`.
    async fn raw_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<(u16, Vec<u8>), ClientError> {
        match &self.transport {
            Transport::WebApi { .. } => self.http_request(method, &self.url(path), body).await,
            Transport::Ipc { endpoint } => self.ipc_request(method, path, body, endpoint).await,
            Transport::Auto {
                endpoint,
                fallback_base_url,
            } => match self.ipc_request(method, path, body, endpoint).await {
                Err(ClientError::Connection(_)) => {
                    let url = format!("{fallback_base_url}{path}");
                    self.http_request(method, &url, body).await.map_err(|e| {
                        ClientError::Connection(format!(
                            "pipe {endpoint} unavailable; web API {fallback_base_url} {e}"
                        ))
                    })
                }
                other => other,
            },
        }
    }

    async fn http_request(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<(u16, Vec<u8>), ClientError> {
        let builder = match method {
            "GET" => self.http.get(url),
            "POST" => self.http.post(url),
            "PUT" => self.http.put(url),
            "DELETE" => self.http.delete(url),
            "PATCH" => self.http.patch(url),
            other => {
                return Err(ClientError::Connection(format!(
                    "unsupported HTTP method {other:?}"
                )))
            }
        };
        let builder = builder.header("accept", "application/json");
        let builder = self.authed(builder);
        let builder = match body {
            Some(bytes) => builder
                .header("content-type", "application/json")
                .body(bytes.to_vec()),
            None => builder,
        };
        let res = builder
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;
        let status = res.status().as_u16();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| ClientError::Decode(e.to_string()))?;
        Ok((status, bytes.to_vec()))
    }

    async fn ipc_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        endpoint: &str,
    ) -> Result<(u16, Vec<u8>), ClientError> {
        match ipc::request(
            method,
            path,
            body,
            self.token.as_deref(),
            endpoint,
            ipc::DEFAULT_TIMEOUT,
        )
        .await
        {
            Ok(res) => Ok((res.status, res.body)),
            Err(ipc::IpcError::Connect(m)) => Err(ClientError::Connection(m)),
            Err(ipc::IpcError::Protocol(m)) => Err(ClientError::Decode(m)),
        }
    }

    /// Decode an error body (`{"error": {"code", "message"}}`) into a ClientError::Api.
    fn api_error(status: u16, body: &[u8]) -> ClientError {
        let parsed: Option<serde_json::Value> = serde_json::from_slice(body).ok();
        let (code, message) = parsed
            .as_ref()
            .and_then(|v| v.get("error"))
            .map(|e| {
                (
                    e.get("code")
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or_else(|| std::str::from_utf8(body).unwrap_or("<non-utf8 body>"))
                        .to_string(),
                )
            })
            .unwrap_or_else(|| {
                (
                    "http_error".to_string(),
                    String::from_utf8_lossy(body).into_owned(),
                )
            });
        ClientError::Api {
            status,
            code,
            message,
        }
    }

    /// Perform a request expecting a JSON body of type `T`.
    async fn raw_json<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> ClientResult<T> {
        let body_bytes = body
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|e| ClientError::Decode(e.to_string()))?;
        let (status, bytes) = self
            .raw_request(method, path, body_bytes.as_deref())
            .await?;
        if !(200..300).contains(&status) {
            return Err(Self::api_error(status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            ClientError::Decode(format!(
                "invalid JSON from {}: {e}",
                self.transport.describe()
            ))
        })
    }

    /// Perform a request with no meaningful response body (2xx/204 are success).
    async fn raw_unit(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> ClientResult<()> {
        let body_bytes = body
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|e| ClientError::Decode(e.to_string()))?;
        let (status, bytes) = self
            .raw_request(method, path, body_bytes.as_deref())
            .await?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(Self::api_error(status, &bytes))
    }

    pub async fn status(&self) -> ClientResult<StatusResponse> {
        self.raw_json("GET", "/api/v1/status", None).await
    }

    /// Lists downloads. Server-side filtering supports statuses/category/search/paging via query string.
    pub async fn list(&self, filter: &DownloadFilter) -> ClientResult<Vec<Download>> {
        let path = list_query_path(filter);
        self.raw_json("GET", &path, None).await
    }

    pub async fn get(&self, id: DownloadId) -> ClientResult<Download> {
        self.raw_json("GET", &format!("/api/v1/downloads/{id}"), None)
            .await
    }

    pub async fn add(&self, request: &NewDownloadRequest) -> ClientResult<Download> {
        self.raw_json(
            "POST",
            "/api/v1/downloads",
            Some(serde_json::to_value(request).map_err(|e| ClientError::Decode(e.to_string()))?),
        )
        .await
    }

    async fn id_action(&self, id: DownloadId, action: &str) -> ClientResult<()> {
        self.raw_unit("POST", &format!("/api/v1/downloads/{id}/{action}"), None)
            .await
    }

    pub async fn pause(&self, id: DownloadId) -> ClientResult<()> {
        self.id_action(id, "pause").await
    }
    pub async fn resume(&self, id: DownloadId) -> ClientResult<()> {
        self.id_action(id, "resume").await
    }
    pub async fn cancel(&self, id: DownloadId) -> ClientResult<()> {
        self.id_action(id, "cancel").await
    }
    pub async fn retry(&self, id: DownloadId) -> ClientResult<()> {
        self.id_action(id, "retry").await
    }

    pub async fn remove(&self, id: DownloadId, delete_file: bool) -> ClientResult<()> {
        self.raw_unit(
            "DELETE",
            &format!("/api/v1/downloads/{id}?deleteFile={delete_file}"),
            None,
        )
        .await
    }

    pub async fn set_priority(&self, id: DownloadId, priority: i32) -> ClientResult<()> {
        self.raw_unit(
            "PUT",
            &format!("/api/v1/downloads/{id}/priority"),
            Some(serde_json::json!({ "priority": priority })),
        )
        .await
    }

    pub async fn batch(
        &self,
        action: &str,
        urls: Option<Vec<String>>,
    ) -> ClientResult<BatchResponse> {
        self.raw_json(
            "POST",
            "/api/v1/downloads/batch",
            Some(serde_json::json!({ "action": action, "urls": urls })),
        )
        .await
    }

    /// Creates many tasks, each with its own options (IDM-style batch import).
    /// Best effort: the daemon reports per-item failures in the response `errors` list.
    pub async fn batch_add(
        &self,
        tasks: Vec<ariaflow_core::NewDownloadRequest>,
    ) -> ClientResult<BatchResponse> {
        self.raw_json(
            "POST",
            "/api/v1/downloads/batch",
            Some(serde_json::json!({ "action": "addMany", "tasks": tasks })),
        )
        .await
    }

    /// Probes a URL via the daemon (HEAD with ranged-GET fallback) and returns remote metadata.
    pub async fn probe(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
    ) -> ClientResult<ariaflow_protocol::RemoteFileInfo> {
        self.raw_json(
            "POST",
            "/api/v1/probe",
            Some(serde_json::json!({ "url": url, "headers": headers })),
        )
        .await
    }

    pub async fn settings(&self) -> ClientResult<GlobalSettings> {
        self.raw_json("GET", "/api/v1/settings", None).await
    }

    pub async fn update_settings<T: Serialize>(&self, patch: &T) -> ClientResult<GlobalSettings> {
        self.raw_json(
            "PUT",
            "/api/v1/settings",
            Some(serde_json::to_value(patch).map_err(|e| ClientError::Decode(e.to_string()))?),
        )
        .await
    }

    /// SSE endpoint URL for consumers that open a long-lived GET themselves (browsers).
    /// Over a pipe-only daemon (`--no-api`) no HTTP endpoint exists; the URL is then
    /// a best-effort hint at the Web API default.
    pub fn events_url(&self) -> String {
        match &self.transport {
            Transport::WebApi { base_url }
            | Transport::Auto {
                fallback_base_url: base_url,
                ..
            } => {
                format!("{base_url}/api/v1/events")
            }
            Transport::Ipc { .. } => "http://127.0.0.1:8787/api/v1/events".to_string(),
        }
    }

    pub async fn history(&self, limit: u32) -> ClientResult<Vec<ariaflow_core::HistoryEntry>> {
        self.raw_json("GET", &format!("/api/v1/history?limit={limit}"), None)
            .await
    }

    /// Fetch a completed task's file bytes (for small files; the tool layer
    /// caps what it accepts). The HTTP client timeout is 30s, so callers that
    /// may move large files should prefer [`Self::save_download_as`].
    pub async fn download_file_bytes(&self, id: DownloadId) -> ClientResult<Vec<u8>> {
        let (status, bytes) = self
            .raw_request("GET", &format!("/api/v1/downloads/{id}/file"), None)
            .await?;
        if (200..300).contains(&status) {
            Ok(bytes)
        } else {
            Err(Self::api_error(status, &bytes))
        }
    }

    /// Server-side copy of a completed task's file into `dest_dir` (a
    /// directory on the daemon host). Returns the daemon's response JSON
    /// (`{ savedTo, size }`).
    pub async fn save_download_as(
        &self,
        id: DownloadId,
        dest_dir: &str,
        filename: Option<&str>,
        overwrite: bool,
    ) -> ClientResult<serde_json::Value> {
        let mut body = serde_json::json!({ "destDir": dest_dir, "overwrite": overwrite });
        if let Some(name) = filename {
            body["filename"] = serde_json::Value::String(name.to_string());
        }
        self.raw_json(
            "POST",
            &format!("/api/v1/downloads/{id}/save-as"),
            Some(body),
        )
        .await
    }

    pub async fn categories(&self) -> ClientResult<serde_json::Value> {
        self.raw_json("GET", "/api/v1/categories", None).await
    }

    pub async fn list_proxies(&self) -> ClientResult<Vec<ariaflow_core::ProxyEntry>> {
        self.raw_json("GET", "/api/v1/proxies", None).await
    }

    pub async fn create_proxy(
        &self,
        input: &ariaflow_core::ProxyInput,
    ) -> ClientResult<ariaflow_core::ProxyEntry> {
        self.raw_json(
            "POST",
            "/api/v1/proxies",
            Some(serde_json::to_value(input).map_err(|e| ClientError::Decode(e.to_string()))?),
        )
        .await
    }

    pub async fn update_proxy(
        &self,
        id: &str,
        input: &ariaflow_core::ProxyInput,
    ) -> ClientResult<ariaflow_core::ProxyEntry> {
        self.raw_json(
            "PUT",
            &format!("/api/v1/proxies/{id}"),
            Some(serde_json::to_value(input).map_err(|e| ClientError::Decode(e.to_string()))?),
        )
        .await
    }

    pub async fn delete_proxy(&self, id: &str) -> ClientResult<()> {
        self.raw_unit("DELETE", &format!("/api/v1/proxies/{id}"), None)
            .await
    }
}
