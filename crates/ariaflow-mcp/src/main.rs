//! `ariaflow-mcp` — MCP server for AI agents to control AriaFlow.
//!
//! Default transport: stdio (newline-delimited JSON-RPC 2.0), suitable for
//! `claude_desktop_config.json`-style MCP client configuration.
//! `--http 127.0.0.1:8788` exposes the same JSON-RPC endpoint over HTTP POST /mcp.
//!
//! Environment:
//! - ARIAFLOW_API_URL       (default http://127.0.0.1:8787)
//! - ARIAFLOW_API_TOKEN     (optional bearer token)
//! - ARIAFLOW_TRANSPORT     (auto | ipc | webapi; default auto = pipe first)
//! - ARIAFLOW_IPC_PATH      (pipe endpoint; empty = platform default)
//!
//! Note: the daemon itself also serves MCP on its main listener at `/mcp`,
//! so this binary is only needed for stdio-style clients.

use ariaflow_api::client::AriaFlowClient;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Parser)]
#[command(
    name = "ariaflow-mcp",
    version,
    about = "AriaFlow MCP server (AI agent integration)"
)]
struct McpArgs {
    /// Expose JSON-RPC over HTTP at this address instead of stdio
    #[arg(long)]
    http: Option<std::net::SocketAddr>,

    /// AriaFlow daemon API base URL (web API transport / auto fallback)
    #[arg(
        long,
        env = "ARIAFLOW_API_URL",
        default_value = "http://127.0.0.1:8787"
    )]
    api_url: String,

    /// How to reach ariaflowd: auto (pipe first, web API fallback), ipc, webapi
    #[arg(long, env = "ARIAFLOW_TRANSPORT", default_value = "auto", value_parser = ["auto", "ipc", "webapi"])]
    transport: String,

    /// Pipe endpoint (empty = platform default)
    #[arg(long, env = "ARIAFLOW_IPC_PATH")]
    ipc_path: Option<String>,

    /// Bearer token for the daemon API
    #[arg(long, env = "ARIAFLOW_API_TOKEN", hide_env_values = true)]
    token: Option<String>,
}

fn build_client(args: &McpArgs) -> AriaFlowClient {
    let result = match args.transport.as_str() {
        "ipc" => AriaFlowClient::ipc(args.ipc_path.as_deref(), args.token.clone()),
        "webapi" => AriaFlowClient::web_api(&args.api_url, args.token.clone()),
        _ => AriaFlowClient::auto(args.ipc_path.as_deref(), &args.api_url, args.token.clone()),
    };
    match result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ariaflow-mcp: cannot configure transport: {e}");
            std::process::exit(2);
        }
    }
}

async fn stdio_loop(client: AriaFlowClient) -> std::io::Result<()> {
    // IMPORTANT: stdout carries only JSON-RPC frames; logs go to stderr.
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let message: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": serde_json::Value::Null,
                    "error": { "code": -32700, "message": format!("parse error: {e}") }
                });
                stdout
                    .write_all(serde_json::to_string(&err)?.as_bytes())
                    .await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };
        if let Some(response) = ariaflow_mcp::handle_message(&client, &message).await {
            stdout
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

async fn http_loop(client: AriaFlowClient, addr: std::net::SocketAddr) -> std::io::Result<()> {
    use axum::extract::State;
    use axum::routing::{get, post};
    use axum::{Json, Router};

    async fn mcp_post(
        State(client): State<AriaFlowClient>,
        Json(message): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        // Notifications receive an empty 202-ish payload; JSON-RPC tolerates null here.
        let response = ariaflow_mcp::handle_message(&client, &message)
            .await
            .unwrap_or(serde_json::Value::Null);
        Json(response)
    }

    async fn info() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "name": "ariaflow-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "transport": "http",
            "endpoint": "/mcp"
        }))
    }

    let app = Router::new()
        .route("/", get(info))
        .route("/mcp", post(mcp_post))
        .with_state(client);

    eprintln!("ariaflow-mcp: HTTP transport listening on http://{addr}/mcp");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ARIAFLOW_MCP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = McpArgs::parse();
    let client = build_client(&args);
    match args.http {
        Some(addr) => http_loop(client, addr).await,
        None => stdio_loop(client).await,
    }
}
