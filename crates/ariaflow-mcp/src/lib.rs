//! # AriaFlow MCP Server
//!
//! Exposes AriaFlow to AI agents over the Model Context Protocol.
//!
//! Two transports:
//! - **stdio** (default): newline-delimited JSON-RPC 2.0 on stdin/stdout
//! - **HTTP** (`--http <addr>`): POST `/mcp` with a JSON-RPC body
//!
//! Design rules:
//! - the MCP server never touches SQLite; every tool call goes through the Web API,
//!   which enforces the same auth, validation and rate limiting as any other client
//! - sensitive values (Authorization/Cookie headers) are redacted in tool output
//! - destructive actions (file deletion) require an explicit `confirm: true` argument

use ariaflow_api::client::{AriaFlowClient, ClientError};
use ariaflow_core::{Download, DownloadFilter, DownloadId, NewDownloadRequest};
use base64::Engine as _;
use serde_json::{json, Map, Value};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "ariaflow-mcp";

async fn mcp_post(
    axum::extract::State(client): axum::extract::State<AriaFlowClient>,
    axum::Json(message): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    let response = handle_message(&client, &message)
        .await
        .unwrap_or(serde_json::Value::Null);
    axum::Json(response)
}

async fn info() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "name": SERVER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PROTOCOL_VERSION,
        "transport": "http",
        "endpoint": "/mcp"
    }))
}

/// Axum router exposing the MCP endpoint at `POST /mcp` (JSON body in, JSON-RPC out)
/// plus `GET /mcp` returning server info. Mounted by `ariaflowd` on its main
/// listener (inheriting the same bearer-token semantics as the rest of the API)
/// and by `ariaflow-mcp --http` on its own port.
pub fn router(client: AriaFlowClient) -> axum::Router {
    use axum::routing::get;
    use axum::Router;

    let app: Router<AriaFlowClient> = Router::new().route("/mcp", get(info).post(mcp_post));
    app.with_state(client)
}

fn redact_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(k, v)| {
            if k.eq_ignore_ascii_case("authorization") || k.eq_ignore_ascii_case("cookie") {
                (k.clone(), "<redacted>".to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

fn download_json(d: &Download) -> Value {
    let mut v = serde_json::to_value(d).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut v {
        if let Some(headers) = map.get("headers").and_then(|h| h.as_array()).cloned() {
            let pairs: Vec<(String, String)> = headers
                .into_iter()
                .filter_map(|item| {
                    let arr = item.as_array()?;
                    Some((
                        arr.first()?.as_str()?.to_string(),
                        arr.get(1)?.as_str()?.to_string(),
                    ))
                })
                .collect();
            map.insert(
                "headers".to_string(),
                serde_json::to_value(redact_headers(&pairs)).unwrap_or(Value::Null),
            );
        }
    }
    v
}

/// JSON Schema definitions of the 12 AriaFlow tools.
pub fn tool_definitions() -> Vec<Value> {
    let id_prop = json!({ "type": "string", "description": "Task id (UUID) of the download" });
    vec![
        json!({
            "name": "ariaflow_status",
            "description": "Get daemon status and aggregate counters (totals, active, speed)",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "ariaflow_list_downloads",
            "description": "List download tasks, optionally filtered by status/category/search text",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "Comma separated statuses, e.g. 'downloading,queued'" },
                    "category": { "type": "string", "description": "Category key, e.g. documents, videos, archives" },
                    "search": { "type": "string", "description": "Substring match on filename or URL" },
                    "page": { "type": "integer", "minimum": 1 },
                    "pageSize": { "type": "integer", "minimum": 1, "maximum": 1000 }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ariaflow_add_download",
            "description": "Add a new download task. URL must use an allowed scheme: http, https, ftp, ftps, sftp, magnet (BitTorrent), .torrent file URLs (http/https), metalink.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "format": "uri" },
                    "filename": { "type": "string" },
                    "saveDir": { "type": "string", "description": "Absolute save directory" },
                    "connections": { "type": "integer", "minimum": 1, "maximum": 64 },
                    "priority": { "type": "integer" },
                    "headers": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Extra HTTP headers" }
                },
                "required": ["url"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ariaflow_pause_download",
            "description": "Pause an active task",
            "inputSchema": { "type": "object", "properties": { "id": id_prop }, "required": ["id"], "additionalProperties": false }
        }),
        json!({
            "name": "ariaflow_resume_download",
            "description": "Resume a paused task",
            "inputSchema": { "type": "object", "properties": { "id": id_prop }, "required": ["id"], "additionalProperties": false }
        }),
        json!({
            "name": "ariaflow_cancel_download",
            "description": "Cancel a task (keeps partial files)",
            "inputSchema": { "type": "object", "properties": { "id": id_prop }, "required": ["id"], "additionalProperties": false }
        }),
        json!({
            "name": "ariaflow_remove_download",
            "description": "Remove a task from the list. Set deleteFile=true to also delete files on disk, which REQUIRES confirm=true.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id_prop,
                    "deleteFile": { "type": "boolean", "default": false },
                    "confirm": { "type": "boolean", "default": false, "description": "Must be true when deleteFile is true" }
                },
                "required": ["id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ariaflow_get_download_info",
            "description": "Get full details of one task including segment progress (sensitive headers redacted)",
            "inputSchema": { "type": "object", "properties": { "id": id_prop }, "required": ["id"], "additionalProperties": false }
        }),
        json!({
            "name": "ariaflow_get_settings",
            "description": "Get the current global AriaFlow settings",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "ariaflow_update_settings",
            "description": "Patch global settings; only supplied fields are changed. Numeric speeds are in bytes/sec (0 = unlimited).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "downloadDir": { "type": "string" },
                    "maxConcurrentTasks": { "type": "integer", "minimum": 1, "maximum": 64 },
                    "maxConnectionsPerTask": { "type": "integer", "minimum": 1, "maximum": 64 },
                    "globalSpeedLimit": { "type": "integer", "minimum": 0 },
                    "perTaskSpeedLimit": { "type": "integer", "minimum": 0 },
                    "retryCount": { "type": "integer", "minimum": 0, "maximum": 100 },
                    "apiEnabled": { "type": "boolean" },
                    "mcpEnabled": { "type": "boolean" },
                    "webUiEnabled": { "type": "boolean" },
                    "autoQuitDaemon": { "type": "boolean", "description": "Desktop shell preference: when the AriaFlow desktop app quits, also shut down ariaflowd. Default false." },
                    "webUiBind": {
                        "type": "string",
                        "description": "ADDR:PORT for the standalone web listener (page+API same-origin), e.g. 0.0.0.0:9090. Empty string disables. Saved now; takes effect after the daemon restarts (--web-bind overrides)."
                    },
                    "apiBind": {
                        "type": "string",
                        "description": "ADDR:PORT for the primary HTTP API listener, e.g. 0.0.0.0:8787 for LAN access. Saved now; takes effect after the daemon restarts (--api-bind / ARIAFLOW_API_BIND overrides). Non-loopback binds should be paired with a --token."
                    },
                    "onDownloadCompleteCommand": { "type": "string", "description": "Shell command run when a task completes (post-processing hook). Empty string clears." },
                    "onDownloadErrorCommand": { "type": "string", "description": "Shell command run when a task permanently fails. Empty string clears." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ariaflow_get_download_file",
            "description": "Fetch a COMPLETED download's file bytes, returned as base64. Intended for small files (configs, texts, small archives): default cap 1 MiB, hard cap 10 MiB. For larger files use ariaflow_save_download (daemon-side copy to a local path) — bytes never transit this tool.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id (UUID) of the completed download" },
                    "maxBytes": { "type": "integer", "minimum": 1, "maximum": 10485760, "description": "Maximum file size to return (default 1048576 = 1 MiB, hard cap 10485760 = 10 MiB)" }
                },
                "required": ["id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ariaflow_save_download",
            "description": "Copy a COMPLETED download's file to a directory ON THE DAEMON HOST (the machine ariaflowd runs on). Use this for large files: bytes are copied server-side and never transit MCP. Returns { savedTo, size }.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Task id (UUID) of the completed download" },
                    "destDir": { "type": "string", "description": "Absolute path of the destination directory on the daemon host" },
                    "filename": { "type": "string", "description": "Target file name (default: the task's file name). Must not contain path separators" },
                    "overwrite": { "type": "boolean", "description": "Overwrite an existing file with the same name (default false; refused with an error otherwise)" }
                },
                "required": ["id", "destDir"],
                "additionalProperties": false
            }
        }),
    ]
}

fn get_str<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn parse_id(args: &Map<String, Value>) -> Result<DownloadId, String> {
    let raw =
        get_str(args, "id").ok_or_else(|| "missing required string argument: id".to_string())?;
    raw.parse::<DownloadId>()
        .map_err(|e| format!("invalid id: {e}"))
}

async fn call_tool(client: &AriaFlowClient, name: &str, args: &Value) -> Result<Value, String> {
    let empty = Map::new();
    let args = args.as_object().unwrap_or(&empty);
    match name {
        "ariaflow_status" => {
            let status = client.status().await.map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(status).unwrap_or(Value::Null))
        }
        "ariaflow_list_downloads" => {
            let statuses = get_str(args, "status")
                .map(|s| {
                    s.split(',')
                        .filter_map(|p| ariaflow_core::DownloadStatus::parse(p.trim()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let filter = DownloadFilter {
                statuses,
                category: get_str(args, "category").map(str::to_string),
                search: get_str(args, "search").map(str::to_string),
                sort_by: None,
                sort_order: Default::default(),
                page: args.get("page").and_then(|v| v.as_u64()).map(|v| v as u32),
                page_size: args
                    .get("pageSize")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
            };
            let items = client.list(&filter).await.map_err(|e| e.to_string())?;
            Ok(json!(items.iter().map(download_json).collect::<Vec<_>>()))
        }
        "ariaflow_add_download" => {
            let url = get_str(args, "url")
                .ok_or_else(|| "missing required string argument: url".to_string())?;
            let mut req = NewDownloadRequest::new(url.to_string());
            req.filename = get_str(args, "filename").map(str::to_string);
            req.save_dir = get_str(args, "saveDir").map(std::path::PathBuf::from);
            req.max_connections = args
                .get("connections")
                .and_then(|v| v.as_u64())
                .map(|v| v as u16);
            req.priority = args
                .get("priority")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
            if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
                req.headers = headers
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect();
            }
            let dl = client.add(&req).await.map_err(|e| e.to_string())?;
            Ok(download_json(&dl))
        }
        "ariaflow_pause_download" => {
            let id = parse_id(args)?;
            client.pause(id).await.map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "id": id.to_string(), "action": "paused" }))
        }
        "ariaflow_resume_download" => {
            let id = parse_id(args)?;
            client.resume(id).await.map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "id": id.to_string(), "action": "resumed" }))
        }
        "ariaflow_cancel_download" => {
            let id = parse_id(args)?;
            client.cancel(id).await.map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "id": id.to_string(), "action": "cancelled" }))
        }
        "ariaflow_remove_download" => {
            let id = parse_id(args)?;
            let delete_file = args
                .get("deleteFile")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let confirm = args
                .get("confirm")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if delete_file && !confirm {
                return Err(
                    "deleteFile=true requires confirm=true. This will permanently delete files on disk."
                        .to_string(),
                );
            }
            client
                .remove(id, delete_file)
                .await
                .map_err(|e| e.to_string())?;
            Ok(
                json!({ "ok": true, "id": id.to_string(), "action": "removed", "deleteFile": delete_file }),
            )
        }
        "ariaflow_get_download_info" => {
            let id = parse_id(args)?;
            let dl = client.get(id).await.map_err(|e| e.to_string())?;
            Ok(download_json(&dl))
        }
        "ariaflow_get_download_file" => {
            const DEFAULT_MAX: u64 = 1024 * 1024; // 1 MiB
            const HARD_MAX: u64 = 10 * 1024 * 1024; // 10 MiB
            let id = parse_id(args)?;
            let max_bytes = args
                .get("maxBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_MAX)
                .min(HARD_MAX);
            let dl = client.get(id).await.map_err(|e| e.to_string())?;
            if dl.status != ariaflow_core::DownloadStatus::Completed {
                return Err(format!(
                    "task is {} — files can only be fetched once the download completed",
                    dl.status.as_str()
                ));
            }
            let known_size = dl.total_size.unwrap_or(0);
            if known_size > 0 && known_size > max_bytes {
                return Err(format!(
                    "file is {known_size} bytes, larger than maxBytes={max_bytes}; use ariaflow_save_download to copy it to a local path, or raise maxBytes (hard cap {HARD_MAX})"
                ));
            }
            let bytes = client
                .download_file_bytes(id)
                .await
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > max_bytes {
                return Err(format!(
                    "file is {} bytes, larger than maxBytes={max_bytes}; use ariaflow_save_download to copy it to a local path, or raise maxBytes (hard cap {HARD_MAX})",
                    bytes.len()
                ));
            }
            Ok(json!({
                "id": id.to_string(),
                "filename": dl.filename,
                "size": bytes.len(),
                "encoding": "base64",
                "content": base64::engine::general_purpose::STANDARD.encode(&bytes),
            }))
        }
        "ariaflow_save_download" => {
            let id = parse_id(args)?;
            let dest_dir = get_str(args, "destDir")
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .ok_or_else(|| "missing required string argument: destDir".to_string())?
                .to_string();
            let filename = get_str(args, "filename").map(str::to_string);
            let overwrite = args
                .get("overwrite")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let result = client
                .save_download_as(id, &dest_dir, filename.as_deref(), overwrite)
                .await
                .map_err(|e| e.to_string())?;
            Ok(result)
        }
        "ariaflow_get_settings" => {
            let settings = client.settings().await.map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(settings).unwrap_or(Value::Null))
        }
        "ariaflow_update_settings" => {
            let mut patch = Map::new();
            for key in [
                "downloadDir",
                "maxConcurrentTasks",
                "maxConnectionsPerTask",
                "globalSpeedLimit",
                "perTaskSpeedLimit",
                "retryCount",
                "apiEnabled",
                "mcpEnabled",
                "webUiEnabled",
                "autoQuitDaemon",
                "webUiBind",
                "apiBind",
                "onDownloadCompleteCommand",
                "onDownloadErrorCommand",
            ] {
                if let Some(v) = args.get(key) {
                    patch.insert(key.to_string(), v.clone());
                }
            }
            if patch.is_empty() {
                return Err("no updatable settings supplied".to_string());
            }
            let updated = client
                .update_settings(&Value::Object(patch))
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(updated).unwrap_or(Value::Null))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn tool_result_success(payload: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap_or_default() }],
        "isError": false
    })
}

fn tool_result_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": format!("error: {message}") }],
        "isError": true
    })
}

/// Handles one JSON-RPC message. Returns `Some(response)` for requests, `None` for notifications.
pub async fn handle_message(client: &AriaFlowClient, message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");

    // Notifications never get a response.
    let is_notification = id.is_none();

    let result: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
        })),
        "notifications/initialized" | "notifications/cancelled" => {
            return None;
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = message.get("params").cloned().unwrap_or(json!({}));
            let name = params
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            if name.is_empty() {
                Err((-32602, "missing tool name".to_string()))
            } else {
                match call_tool(client, &name, &arguments).await {
                    Ok(payload) => Ok(tool_result_success(payload)),
                    Err(e) => Ok(tool_result_error(&e)),
                }
            }
        }
        other => Err((-32601, format!("method not found: {other}"))),
    };

    if is_notification {
        return None;
    }
    let id = id.unwrap_or(Value::Null);
    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

/// Maps client errors to a readable one-line string for MCP logs (never to stdout).
pub fn describe_client_error(e: &ClientError) -> String {
    match e {
        ClientError::Connection(msg) => format!("cannot reach AriaFlow daemon: {msg}"),
        ClientError::Api {
            status,
            code,
            message,
        } => format!("API error {status} [{code}]: {message}"),
        ClientError::Decode(msg) => format!("decode error: {msg}"),
    }
}
