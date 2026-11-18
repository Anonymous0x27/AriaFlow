//! `ariaflow-native-host` — Browser Native Messaging host for AriaFlow.
//!
//! Protocol (Chrome/Firefox Native Messaging): each frame is a 4-byte little-endian
//! length followed by a UTF-8 JSON message. Browsers launch this binary via the
//! registered host manifest; it forwards download requests to the AriaFlow daemon
//! over the local HTTP API. The database is never touched by the browser or this host.
//!
//! Supported messages (browser -> host):
//! {
//!   "type": "add_download",
//!   "url": "https://…",
//!   "filename": "optional.bin",
//!   "headers": { "Referer": "…", … },   // optional
//!   "saveDir": "/optional/abs/dir"      // optional
//! }
//! { "type": "ping" }
//!
//! Host -> browser responses:
//! { "ok": true,  "id": "<uuid>", "action": "added" }
//! { "ok": false, "error": "message" }

use ariaflow_api::client::AriaFlowClient;
use ariaflow_core::NewDownloadRequest;
use serde_json::{json, Value};
use std::io::{Read, Write};

const MAX_FRAME: u32 = 1024 * 1024; // 1 MiB hard cap

fn read_frame(stdin: &mut impl Read) -> std::io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match stdin.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame too large: {len} bytes"),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    stdin.read_exact(&mut buf)?;
    Ok(Some(buf))
}

fn write_frame(stdout: &mut impl Write, body: &[u8]) -> std::io::Result<()> {
    let len = (body.len() as u32).to_le_bytes();
    stdout.write_all(&len)?;
    stdout.write_all(body)?;
    stdout.flush()
}

fn handle_message(client: &AriaFlowClient, message: &Value) -> Value {
    match message.get("type").and_then(|t| t.as_str()) {
        Some("ping") => {
            json!({ "ok": true, "action": "pong", "version": env!("CARGO_PKG_VERSION") })
        }
        Some("add_download") => {
            let Some(url) = message.get("url").and_then(|u| u.as_str()) else {
                return json!({ "ok": false, "error": "missing url" });
            };
            let mut req = NewDownloadRequest::new(url.to_string());
            req.filename = message
                .get("filename")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            req.save_dir = message
                .get("saveDir")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            if let Some(headers) = message.get("headers").and_then(|h| h.as_object()) {
                req.headers = headers
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect();
            }
            // The client is async; block on it with a minimal runtime per message.
            let handle = tokio::runtime::Handle::current();
            match tokio::task::block_in_place(|| handle.block_on(client.add(&req))) {
                Ok(dl) => {
                    json!({ "ok": true, "action": "added", "id": dl.id.to_string(), "filename": dl.filename, "savePath": dl.save_path })
                }
                Err(e) => json!({ "ok": false, "error": e.to_string() }),
            }
        }
        Some(other) => json!({ "ok": false, "error": format!("unknown message type: {other}") }),
        None => json!({ "ok": false, "error": "missing message type" }),
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> std::io::Result<()> {
    // IMPORTANT: stdout carries only native-messaging frames; logs go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ARIAFLOW_HOST_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    // 默认管道优先（可用 ARIAFLOW_TRANSPORT=webapi 强制走 HTTP）
    // pipe first by default; set ARIAFLOW_TRANSPORT=webapi to force the HTTP API
    let api_url =
        std::env::var("ARIAFLOW_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".to_string());
    let ipc_path = std::env::var("ARIAFLOW_IPC_PATH").ok();
    let token = std::env::var("ARIAFLOW_API_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let transport = std::env::var("ARIAFLOW_TRANSPORT").unwrap_or_else(|_| "auto".to_string());
    let client = match transport.as_str() {
        "webapi" => AriaFlowClient::web_api(&api_url, token),
        "ipc" => AriaFlowClient::ipc(ipc_path.as_deref(), token),
        _ => AriaFlowClient::auto(ipc_path.as_deref(), &api_url, token),
    };
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ariaflow-native-host: invalid transport configuration: {e}");
            std::process::exit(2);
        }
    };

    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    while let Some(frame) = read_frame(&mut stdin)? {
        let message: Value = match serde_json::from_slice(&frame) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({ "ok": false, "error": format!("invalid JSON frame: {e}") });
                write_frame(&mut stdout, err.to_string().as_bytes())?;
                continue;
            }
        };
        let response = handle_message(&client, &message);
        write_frame(&mut stdout, response.to_string().as_bytes())?;
    }
    Ok(())
}
