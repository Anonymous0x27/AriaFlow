//! `ariaflowd` — AriaFlow headless background daemon.
//!
//! Runs the download service without any GUI. GUI, CLI, TUI, API and MCP are all
//! clients of this daemon. Downloads resume from SQLite after a crash or reboot.
//!
//! ```bash
//! ariaflowd                       # listen on 127.0.0.1:8787 + the platform pipe
//! ariaflowd --web-dir /path/to/dist   # serve the web app at / from disk instead of the embedded bundle
//! ariaflowd --no-ipc              # HTTP only
//! ariaflowd --no-api              # pipe only, no TCP listener
//! ariaflowd --no-mcp              # do not mount MCP at /mcp
//! ariaflowd --ipc /run/user/1000/ariaflowd.sock
//! ariaflowd --api-bind 0.0.0.0:8787 --token "$TOKEN"   # LAN exposure (explicit opt-in)
//! ```
//!
//! The web page is the same frontend as the Tauri desktop app, embedded into the
//! binary at compile time (rust-embed) and served at `/`; `--web-dir` overrides it
//! with a directory on disk. HTTP API, MCP and the web page can all be toggled at
//! runtime via `PUT /api/v1/settings` (no restart); the local pipe is never gated.
//! MCP JSON-RPC is served at `/mcp` (same bearer-token semantics as the REST API).

use ariaflow_api::client::AriaFlowClient;
use ariaflow_api::server::{serve, serve_ipc, ApiState};
use ariaflow_api::webui::{resolve_web_ui, WebUi};
use ariaflow_api::Features;
use ariaflow_core::paths;
use ariaflow_engine::{FtpProvider, HttpProvider, MetalinkProvider, SftpProvider, TorrentProvider};
use ariaflow_protocol::ProviderRegistry;
use ariaflow_storage::{Database, SqliteDownloadService};
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

mod orchestrator;
use orchestrator::Orchestrator;

#[derive(Parser)]
#[command(
    name = "ariaflowd",
    version,
    about = "AriaFlow headless daemon",
    long_about = "Runs the AriaFlow download service in the background.\nBinds to 127.0.0.1:8787 by default; exposing on a LAN/public interface requires an explicit --api-bind (or the persisted apiBind setting) and a bearer --token."
)]
struct DaemonArgs {
    /// API bind address, e.g. 127.0.0.1:8787 (default) or 0.0.0.0:8787.
    /// Overrides the persisted `apiBind` setting; the persisted setting is
    /// honoured on every start when this flag is absent (restart required).
    #[arg(long, env = "ARIAFLOW_API_BIND", value_name = "ADDR:PORT")]
    api_bind: Option<String>,

    /// Bearer token required for API access. Strongly recommended when api-bind is not loopback.
    #[arg(long, env = "ARIAFLOW_API_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Start the local IPC listener alongside the HTTP API (enabled by default).
    /// Optional path; omit to use the platform default
    /// (Unix: `$XDG_RUNTIME_DIR/ariaflowd.sock` or `<data-dir>/ariaflowd.sock`;
    ///  Windows: `\\.\pipe\ariaflowd`).
    #[arg(long, env = "ARIAFLOW_IPC", num_args = 0..=1, default_missing_value = "", value_name = "PATH")]
    ipc: Option<String>,

    /// Do not serve the local pipe (HTTP API only).
    #[arg(long)]
    no_ipc: bool,

    /// Do not serve HTTP; IPC-only mode (requires the pipe, i.e. not --no-ipc).
    #[arg(long)]
    no_api: bool,

    /// Serve the web version of the desktop app from this directory instead of
    /// the bundle embedded at compile time (must contain index.html).
    #[arg(long, env = "ARIAFLOW_WEB_DIR", value_name = "DIR")]
    web_dir: Option<PathBuf>,

    /// Additionally bind a standalone listener for the web UI + same-origin
    /// `/api/v1` (e.g. `0.0.0.0:9090`), meant for browsers on the LAN.
    /// Overrides the persisted `webUiBind` setting; restart required either
    /// way. Empty = disabled. Auth semantics match --api-bind.
    #[arg(long, env = "ARIAFLOW_WEB_BIND", value_name = "ADDR:PORT")]
    web_bind: Option<String>,

    /// Do not mount the MCP JSON-RPC endpoint at `/mcp`.
    #[arg(long, env = "ARIAFLOW_NO_MCP")]
    no_mcp: bool,

    /// Path of the SQLite database (default: platform data dir / ariaflow.db)
    #[arg(long, env = "ARIAFLOW_DB")]
    db: Option<PathBuf>,

    /// Only initialise the database and print effective configuration, then exit
    #[arg(long)]
    print_config: bool,
}

/// Resolve the effective api-bind address.
///
/// Precedence: explicit `--api-bind` / `ARIAFLOW_API_BIND` → persisted
/// `apiBind` setting → `127.0.0.1:8787`. An explicit value that fails to parse
/// aborts startup (the user just typed it); a malformed persisted value only
/// logs an error and falls back to the loopback default, so a stale setting
/// can never brick the daemon.
fn resolve_api_bind(flag: Option<&str>, persisted: &str) -> Result<SocketAddr, String> {
    let explicit = flag.map(str::trim).filter(|s| !s.is_empty());
    if let Some(raw) = explicit {
        return raw
            .parse::<SocketAddr>()
            .map_err(|e| format!("invalid --api-bind {raw:?}: {e}"));
    }
    let persisted = persisted.trim();
    if !persisted.is_empty() {
        match persisted.parse::<SocketAddr>() {
            Ok(addr) => return Ok(addr),
            Err(e) => tracing::error!(
                value = %persisted,
                error = %e,
                "ignoring persisted apiBind (not an ADDR:PORT socket address); falling back to 127.0.0.1:8787"
            ),
        }
    }
    Ok(SocketAddr::from(([127, 0, 0, 1], 8787)))
}

fn write_pid_file() -> Option<PathBuf> {
    let path = paths::pid_file_path();
    if let Err(e) = paths::ensure_dir(path.parent().unwrap_or_else(|| std::path::Path::new("."))) {
        tracing::warn!(error = %e, "could not create data dir for pid file");
        return None;
    }
    match std::fs::write(&path, std::process::id().to_string()) {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::warn!(error = %e, "could not write pid file {}", path.display());
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ARIAFLOW_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .init();

    let args = DaemonArgs::parse();
    let db_path = args.db.clone().unwrap_or_else(paths::database_path);
    tracing::info!(path = %db_path.display(), "opening AriaFlow database");
    let db = Database::connect(&db_path).await?;

    // The api-bind and the web listener default come from persisted settings,
    // so the service (and its settings row) must be available this early.
    let service = Arc::new(SqliteDownloadService::new(db));
    let events = service.bus();
    let boot_settings = ariaflow_core::DownloadService::get_settings(service.as_ref())
        .await
        .unwrap_or_default();

    // --api-bind (or ARIAFLOW_API_BIND) wins; otherwise the persisted
    // `apiBind` setting; otherwise the loopback default. A malformed persisted
    // value never bricks the daemon — it logs an error and falls back.
    let bind: SocketAddr = resolve_api_bind(args.api_bind.as_deref(), &boot_settings.api_bind)?;
    // IPC is enabled by default (platform pipe); --no-ipc turns it off,
    // --ipc [PATH] overrides the endpoint.
    let ipc_endpoint: Option<String> = if args.no_ipc {
        None
    } else {
        match args.ipc.as_deref() {
            None | Some("") => Some(paths::default_ipc_path()),
            Some(p) => Some(p.to_string()),
        }
    };
    let is_loopback = bind.ip().is_loopback();
    if !is_loopback && args.token.is_none() {
        tracing::warn!(
            "binding to a non-loopback address WITHOUT an API token. Anyone who can reach {bind} can control downloads. Set --token."
        );
    }

    // --web-bind (flag) wins over the persisted webUiBind setting.
    let web_bind_raw: Option<String> = match args
        .web_bind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(v) => Some(v.to_string()),
        None => {
            let v = boot_settings.web_ui_bind.trim();
            (!v.is_empty()).then(|| v.to_string())
        }
    };
    let web_bind: Option<SocketAddr> = web_bind_raw
        .as_deref()
        .map(str::parse::<SocketAddr>)
        .transpose()
        .map_err(|e| {
            format!("invalid web-bind {web_bind_raw:?}: {e} (want ADDR:PORT, e.g. 0.0.0.0:9090)")
        })?;
    let web_bind = web_bind.filter(|wb| {
        if !args.no_api && *wb == bind {
            tracing::warn!(
                "web-bind is the api-bind address; the api-bind listener already serves the web UI"
            );
            return false;
        }
        true
    });
    if let Some(wb) = web_bind {
        if !wb.ip().is_loopback() && args.token.is_none() {
            tracing::warn!("binding the web listener to a non-loopback address WITHOUT an API token. Anyone who can reach {wb} can control downloads. Set --token.");
        }
    }
    if args.no_api && ipc_endpoint.is_none() && web_bind.is_none() {
        return Err("--no-api requires the pipe listener (not --no-ipc) or --web-bind".into());
    }

    if args.print_config {
        println!("product:    AriaFlow {}", env!("CARGO_PKG_VERSION"));
        println!("db:         {}", db_path.display());
        println!("api_bind:   {bind}");
        println!(
            "token:      {}",
            if args.token.is_some() {
                "<set>"
            } else {
                "<none>"
            }
        );
        println!("loopback:   {is_loopback}");
        println!(
            "ipc:        {}",
            ipc_endpoint.as_deref().unwrap_or("<disabled>")
        );
        println!(
            "web_bind:   {}",
            web_bind
                .map(|wb| wb.to_string())
                .unwrap_or_else(|| "<disabled>".to_string())
        );
        println!("no_api:     {}", args.no_api);
        println!("no_ipc:     {}", args.no_ipc);
        println!("no_mcp:     {}", args.no_mcp);
        println!(
            "web_dir:    {}",
            args.web_dir
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<auto>".to_string())
        );
        return Ok(());
    }

    // Live feature switches: initialised from persisted settings; `PUT /api/v1/settings`
    // flips them at runtime (no restart). The pipe stays ungated by design.
    let features = Features::new(
        boot_settings.api_enabled,
        !args.no_mcp && boot_settings.mcp_enabled,
        boot_settings.web_ui_enabled,
    );

    let api_service: Arc<dyn ariaflow_core::DownloadService> = service.clone();

    // Transport registry: every protocol the engine speaks, dispatched by URL scheme.
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(HttpProvider::new()));
    providers.register(Arc::new(FtpProvider::new()));
    providers.register(Arc::new(SftpProvider::new()));
    providers.register(Arc::new(MetalinkProvider::new()));
    providers.register(Arc::new(TorrentProvider::new()));
    let providers = Arc::new(providers);

    // Start the download engine (crash recovery + queued-task loop).
    Orchestrator::new(service, providers.clone()).spawn();

    // ---------- web UI (desktop app served as a web page) ----------
    let web = resolve_web_ui(args.web_dir.as_deref());

    // ---------- MCP (mounted at /mcp on the same listener) ----------
    let mcp = if args.no_mcp {
        None
    } else {
        // MCP's tool calls loop back into the daemon. Prefer the pipe: it is
        // never gated, so MCP keeps working when the HTTP API is toggled off
        // at runtime. Fall back to the loopback HTTP API only when no pipe exists.
        let mcp_client = match &ipc_endpoint {
            Some(ep) => AriaFlowClient::ipc(Some(ep.as_str()), args.token.clone()),
            None => {
                let port = bind.port();
                AriaFlowClient::web_api(format!("http://127.0.0.1:{port}"), args.token.clone())
            }
        };
        match mcp_client {
            Ok(client) => Some(ariaflow_mcp::router(client)),
            Err(e) => {
                tracing::warn!(error = %e, "MCP endpoint disabled: could not build loopback client");
                None
            }
        }
    };

    let pid_file = write_pid_file();
    let state = ApiState::new(api_service, events, args.token.clone())
        .with_provider(providers)
        .with_features(features)
        .with_web_ui(web.clone());

    let web_desc: String = match &web {
        WebUi::Embedded => "embedded".to_string(),
        WebUi::Dir(dir) => format!("{}", dir.display()),
        WebUi::Missing => "not found".to_string(),
    };
    let mut listeners_banner = Vec::new();
    if !args.no_api {
        listeners_banner.push(format!("http://{bind}"));
    }
    if let Some(ep) = &ipc_endpoint {
        listeners_banner.push(ep.clone());
    }
    if !args.no_api {
        listeners_banner.push(format!("http://{bind}/ (web: {web_desc})"));
    }
    if let Some(wb) = web_bind {
        listeners_banner.push(format!("http://{wb}/ (web: {web_desc}, web-bind)"));
    }
    if mcp.is_some() && !args.no_api {
        listeners_banner.push(format!("http://{bind}/mcp (MCP)"));
    }
    if mcp.is_some() {
        if let Some(wb) = web_bind {
            listeners_banner.push(format!("http://{wb}/mcp (MCP)"));
        }
    }
    {
        let f = &state.features;
        tracing::info!(
            web_api = f.get(ariaflow_api::Feature::Api),
            mcp = f.get(ariaflow_api::Feature::Mcp),
            web_ui = f.get(ariaflow_api::Feature::Web),
            "feature switches (persisted in settings; PUT /api/v1/settings flips them at runtime)"
        );
    }
    tracing::info!(?listeners_banner, "AriaFlow daemon ready");
    println!(
        "AriaFlow daemon {} listening on {}",
        env!("CARGO_PKG_VERSION"),
        listeners_banner.join(", ")
    );

    // Serve all listeners together; if any dies, shut down (the rest are
    // aborted) so a half-broken daemon never lingers silently.
    let mcp_web = mcp.clone();
    let mcp_http = mcp;
    let mcp_ipc = if args.no_mcp {
        None
    } else {
        // The pipe serves the same router; give it an MCP mount that talks over the pipe.
        match AriaFlowClient::ipc(ipc_endpoint.as_deref(), args.token.clone()) {
            Ok(client) => Some(ariaflow_mcp::router(client)),
            Err(e) => {
                tracing::warn!(error = %e, "MCP endpoint disabled on the pipe listener");
                None
            }
        }
    };
    let mut listeners = tokio::task::JoinSet::new();
    if !args.no_api {
        let state = state.clone();
        listeners.spawn(async move { serve(state, bind, mcp_http).await });
    }
    if let Some(ep) = ipc_endpoint {
        let state = state.clone();
        listeners.spawn(async move { serve_ipc(state, &ep, mcp_ipc).await });
    }
    if let Some(wb) = web_bind {
        listeners.spawn(async move { serve(state, wb, mcp_web).await });
    } else {
        drop(state);
    }
    let result = match listeners.join_next().await {
        Some(Ok(outcome)) => outcome,
        Some(Err(e)) => Err(std::io::Error::other(format!("listener task failed: {e}"))),
        None => Ok(()),
    };
    listeners.abort_all();

    if let Some(pid) = pid_file {
        let _ = std::fs::remove_file(pid);
    }
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_api_bind;
    use std::net::SocketAddr;

    #[test]
    fn explicit_flag_wins_over_persisted() {
        let addr = resolve_api_bind(Some("0.0.0.0:8787"), "127.0.0.1:9999").unwrap();
        assert_eq!(addr, "0.0.0.0:8787".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn invalid_explicit_flag_is_rejected() {
        assert!(resolve_api_bind(Some("not-an-addr"), "127.0.0.1:9999").is_err());
    }

    #[test]
    fn persisted_value_is_used_when_flag_absent() {
        let addr = resolve_api_bind(None, "0.0.0.0:8787").unwrap();
        assert_eq!(addr, "0.0.0.0:8787".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn invalid_persisted_value_falls_back_to_loopback() {
        let addr = resolve_api_bind(None, "garbage").unwrap();
        assert_eq!(addr, "127.0.0.1:8787".parse::<SocketAddr>().unwrap());
        let addr = resolve_api_bind(None, "").unwrap();
        assert_eq!(addr, "127.0.0.1:8787".parse::<SocketAddr>().unwrap());
        let addr = resolve_api_bind(None, "   ").unwrap();
        assert_eq!(addr, "127.0.0.1:8787".parse::<SocketAddr>().unwrap());
    }
}
