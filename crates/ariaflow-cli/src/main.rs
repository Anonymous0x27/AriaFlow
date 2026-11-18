//! `ariaflow` — unified command-line client for AriaFlow.
//!
//! Talks to a running `ariaflowd` over the local pipe by default (Unix domain
//! socket / Windows named pipe), or over the web API with `--transport webapi`
//! (`--transport auto` falls back to the web API when the pipe is unreachable).
//! All machine-readable
//! output is available via `--json`; errors map to stable exit codes:
//! 0 success · 1 general error · 2 usage (clap) · 3 cannot reach daemon.

mod daemon_cmd;
mod tui;
mod util;

use ariaflow_api::client::{AriaFlowClient, ClientError};
use ariaflow_core::{DownloadFilter, DownloadId, NewDownloadRequest};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "ariaflow",
    version,
    about = "AriaFlow — fast, modern cross-platform download manager",
    long_about = "AriaFlow CLI. Talks to a running `ariaflowd` daemon: over the local pipe by default (Unix socket / named pipe),\nor over the web API with --transport webapi. The pipe is tried first in `auto` mode and the web API is used as a fallback.\n\nExit codes: 0=success, 1=error, 2=usage, 3=daemon unreachable."
)]
struct Cli {
    /// Machine-readable JSON output
    #[arg(long, global = true)]
    json: bool,
    /// Daemon API base URL (used by the web API transport, and as the automatic fallback)
    #[arg(
        long,
        global = true,
        env = "ARIAFLOW_API_URL",
        default_value = "http://127.0.0.1:8787"
    )]
    api_url: String,
    /// How to reach ariaflowd: auto (pipe first, fall back to web API), ipc (pipe only), webapi (HTTP only)
    #[arg(long, global = true, env = "ARIAFLOW_TRANSPORT", default_value = "auto", value_parser = ["auto", "ipc", "webapi"])]
    transport: String,
    /// Pipe endpoint (empty = platform default: Unix socket on Unix, named pipe on Windows)
    #[arg(long, global = true, env = "ARIAFLOW_IPC_PATH")]
    ipc_path: Option<String>,
    /// API bearer token
    #[arg(
        long,
        global = true,
        env = "ARIAFLOW_API_TOKEN",
        hide_env_values = true
    )]
    token: Option<String>,
    /// Launch the interactive terminal UI (full-screen; --api-url/--token apply as usual)
    #[arg(long, global = true)]
    tui: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show daemon status and aggregate counters
    Status,
    /// List download tasks
    List {
        /// Filter by comma-separated statuses, e.g. downloading,queued
        #[arg(long)]
        status: Option<String>,
        /// Filter by category key, e.g. documents
        #[arg(long)]
        category: Option<String>,
        /// Substring search on filename/URL
        #[arg(long)]
        search: Option<String>,
        /// Page number (1-based)
        #[arg(long)]
        page: Option<u32>,
        /// Page size
        #[arg(long)]
        page_size: Option<u32>,
    },
    /// Show detailed information about one task
    Info {
        /// Task id (full UUID)
        id: String,
    },
    /// Add a new download
    Add {
        /// URL to download
        url: String,
        /// Save directory (default: configured download directory)
        #[arg(long)]
        dir: Option<String>,
        /// Connections per task (1-64)
        #[arg(long)]
        connections: Option<u16>,
        /// Task priority (higher = sooner)
        #[arg(long)]
        priority: Option<i32>,
        /// Extra header, e.g. --header "Authorization: Bearer x" (repeatable)
        #[arg(long = "header")]
        headers: Vec<String>,
        /// Convenience: sets the Referer header
        #[arg(long)]
        referer: Option<String>,
        /// Bearer token for the download URL
        #[arg(long, hide_env_values = true)]
        bearer: Option<String>,
        /// Basic auth credentials as user:password
        #[arg(long, hide_env_values = true)]
        basic: Option<String>,
        /// Override the output filename
        #[arg(long)]
        filename: Option<String>,
        /// Add without starting immediately
        #[arg(long)]
        paused: bool,
        /// Per-task speed limit, e.g. 10M (enforced by the daemon engine)
        #[arg(long = "limit-rate")]
        limit_rate: Option<String>,
        /// Automatic retry attempts (stored; enforced by engine milestone)
        #[arg(long)]
        retry: Option<u32>,
        /// Start no earlier than this instant, e.g. 2026-11-13T02:00+08:00 or "tomorrow 02:00"
        #[arg(long)]
        schedule: Option<String>,
    },
    /// Pause a task (or every active task with --all)
    Pause {
        id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Resume a paused task (or all paused tasks with --all)
    Resume {
        id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Cancel a task (or all active tasks with --all)
    Cancel {
        id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Remove a task, or --completed to clear all completed tasks
    Remove {
        id: Option<String>,
        /// Remove all completed tasks
        #[arg(long)]
        completed: bool,
        /// Also delete the downloaded/partial file
        #[arg(long)]
        delete_file: bool,
    },
    /// Re-queue a failed/cancelled task
    Retry { id: String },
    /// Probe a URL (metadata only, nothing is downloaded)
    Probe {
        url: String,
        /// Extra header, repeatable (e.g. --header "Authorization: Bearer x")
        #[arg(long = "header")]
        headers: Vec<String>,
        /// Sets the Referer header
        #[arg(long)]
        referer: Option<String>,
    },
    /// Add many URLs from a text file (one URL per line, '#' comments allowed)
    BatchAdd {
        /// Path to the URL list file (use '-' for stdin)
        file: String,
        /// Save directory (default: configured download directory)
        #[arg(long)]
        dir: Option<String>,
        /// Connections per task (1-64)
        #[arg(long)]
        connections: Option<u16>,
        /// Task priority (higher = sooner)
        #[arg(long)]
        priority: Option<i32>,
        /// Extra header applied to every task, e.g. --header "Authorization: Bearer x" (repeatable)
        #[arg(long = "header")]
        headers: Vec<String>,
        /// Convenience: sets the Referer header on every task
        #[arg(long)]
        referer: Option<String>,
        /// Bearer token applied to every task
        #[arg(long, hide_env_values = true)]
        bearer: Option<String>,
        /// Basic auth credentials as user:password, applied to every task
        #[arg(long, hide_env_values = true)]
        basic: Option<String>,
        /// Add all tasks without starting immediately
        #[arg(long)]
        paused: bool,
        /// Per-task speed limit, e.g. 10M
        #[arg(long = "limit-rate")]
        limit_rate: Option<String>,
        /// Automatic retry attempts (stored; enforced by engine milestone)
        #[arg(long)]
        retry: Option<u32>,
        /// Start no earlier than this instant, e.g. 2026-11-13T02:00+08:00 or "tomorrow 02:00"
        #[arg(long)]
        schedule: Option<String>,
    },
    /// Show or change when tasks are allowed to start automatically
    Schedule {
        #[command(subcommand)]
        cmd: ScheduleCommand,
    },
    /// Show recent completion history (newest first)
    History {
        /// Maximum entries to show
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Manage named proxies
    Proxies {
        #[command(subcommand)]
        cmd: ProxyCommand,
    },
    /// Show or export the built-in task category rules
    Categories {
        #[command(subcommand)]
        cmd: CategoriesCommand,
    },
    /// Read or change configuration
    Config {
        #[command(subcommand)]
        cmd: ConfigCommand,
    },
    /// Control the background daemon
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCommand,
    },
}

#[derive(Subcommand)]
enum ProxyCommand {
    /// List configured proxies
    List,
    /// Add a proxy, e.g. ariaflow proxies add --name corp --kind http --address http://127.0.0.1:7890
    Add {
        #[arg(long)]
        name: String,
        /// http | https | socks5 | socks5h
        #[arg(long)]
        kind: String,
        /// e.g. http://127.0.0.1:7890
        #[arg(long)]
        address: String,
        #[arg(long)]
        username: Option<String>,
        #[arg(long, hide_env_values = true)]
        password: Option<String>,
        /// Hosts that bypass the proxy (glob, comma separated)
        #[arg(long)]
        bypass: Option<String>,
        /// Mark as the default proxy
        #[arg(long = "default")]
        is_default: bool,
    },
    /// Remove a proxy by id (or unambiguous id prefix)
    Remove { id: String },
    /// Mark a proxy as the default
    SetDefault { id: String },
}

#[derive(Subcommand)]
enum CategoriesCommand {
    /// Print the built-in category rules
    Show,
    /// Write the category rules JSON to a file (or stdout when omitted)
    Export {
        /// Destination file, e.g. categories.json
        file: Option<String>,
    },
}

#[derive(Subcommand)]
enum ScheduleCommand {
    /// Show the configured daily download window and any scheduled tasks
    Show,
    /// Set the daily download window (local time, 24h), e.g. `ariaflow schedule set 02:00 06:00`
    Set { start: String, end: String },
    /// Clear the daily download window
    Clear,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Print current settings (as seen by the daemon)
    Show,
    /// Set a setting key to a value, e.g. ariaflow config set download-dir ~/Downloads
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Start ariaflowd in the background
    Start {
        /// Path to the ariaflowd binary; use this when ariaflowd is not next to `ariaflow` or on PATH
        #[arg(long, value_name = "PATH")]
        bin: Option<std::path::PathBuf>,
    },
    /// Stop ariaflowd gracefully
    Stop,
    /// Restart ariaflowd
    Restart {
        /// Path to the ariaflowd binary; use this when ariaflowd is not next to `ariaflow` or on PATH
        #[arg(long, value_name = "PATH")]
        bin: Option<std::path::PathBuf>,
    },
    /// Check whether ariaflowd is running
    Status,
}

fn client(cli: &Cli) -> Result<AriaFlowClient, ClientError> {
    match cli.transport.as_str() {
        "ipc" => AriaFlowClient::ipc(cli.ipc_path.as_deref(), cli.token.clone()),
        "webapi" => AriaFlowClient::web_api(&cli.api_url, cli.token.clone()),
        // auto (default): prefer the local pipe, silently fall back to the web API.
        _ => AriaFlowClient::auto(cli.ipc_path.as_deref(), &cli.api_url, cli.token.clone()),
    }
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

fn fmt_client_error(e: ClientError) -> (String, u8) {
    match e {
        ClientError::Connection(msg) => (
            format!("cannot reach AriaFlow daemon ({msg}).\nStart it with: ariaflow daemon start"),
            3,
        ),
        ClientError::Api {
            status,
            code,
            message,
        } => (format!("API error {status} [{code}]: {message}"), 1),
        ClientError::Decode(msg) => (format!("could not decode daemon response: {msg}"), 1),
    }
}

/// Parses `user:password` (splits on the first ':'), hiding the password from
/// any error messages that might be printed.
fn parse_basic_pair(raw: &str) -> Result<(String, String), (String, u8)> {
    match raw.split_once(':') {
        Some((u, p)) if !u.is_empty() && !p.is_empty() => Ok((u.to_string(), p.to_string())),
        _ => Err((
            "--basic expects user:password (password may itself contain ':')".to_string(),
            2,
        )),
    }
}

fn parse_headers(raw: &[String], referer: &Option<String>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = raw
        .iter()
        .filter_map(|h| {
            let (name, value) = h.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    if let Some(r) = referer {
        out.push(("Referer".to_string(), r.clone()));
    }
    out
}

/// Resolves a task id argument to a full DownloadId.
/// Accepts a full UUID, or an unambiguous prefix (>= 4 chars) resolved against the daemon's task list.
async fn resolve_id(c: &AriaFlowClient, raw: &str) -> Result<DownloadId, (String, u8)> {
    if let Ok(id) = raw.parse::<DownloadId>() {
        return Ok(id);
    }
    if raw.len() >= 4 {
        let items = c
            .list(&DownloadFilter::unfiltered())
            .await
            .map_err(fmt_client_error)?;
        let matches: Vec<_> = items
            .iter()
            .filter(|d| d.id.to_string().starts_with(raw))
            .collect();
        match matches.len() {
            1 => return Ok(matches[0].id),
            0 => return Err((format!("no task matches id prefix {raw:?}"), 1)),
            n => {
                return Err((
                    format!("id prefix {raw:?} is ambiguous ({n} matches); use more characters"),
                    2,
                ))
            }
        }
    }
    Err((format!("invalid download id: {raw:?}"), 2))
}

fn normalize_config_key(key: &str) -> Option<String> {
    Some(match key {
        "download-dir" | "download_dir" | "downloadDir" => "downloadDir".into(),
        "max-concurrent" | "max_concurrent" | "maxConcurrentTasks" => "maxConcurrentTasks".into(),
        "max-connections" | "max_connections" | "maxConnectionsPerTask" => {
            "maxConnectionsPerTask".into()
        }
        "max-total-connections" | "maxTotalConnections" => "maxTotalConnections".into(),
        "global-speed-limit" | "globalSpeedLimit" | "speed-limit" => "globalSpeedLimit".into(),
        "per-task-speed-limit" | "perTaskSpeedLimit" => "perTaskSpeedLimit".into(),
        "retry-count" | "retryCount" => "retryCount".into(),
        "min-segment-size" | "minSegmentSize" => "minSegmentSize".into(),
        "connect-timeout" | "connectTimeoutSecs" => "connectTimeoutSecs".into(),
        "read-timeout" | "readTimeoutSecs" => "readTimeoutSecs".into(),
        "api-enabled" | "apiEnabled" => "apiEnabled".into(),
        "api-bind" | "apiBind" => "apiBind".into(),
        "mcp-enabled" | "mcpEnabled" => "mcpEnabled".into(),
        "web-ui-enabled" | "webUiEnabled" => "webUiEnabled".into(),
        "web-ui-bind" | "webUiBind" => "webUiBind".into(),
        "user-agent" | "userAgent" => "userAgent".into(),
        "auto-quit-daemon" | "autoQuitDaemon" => "autoQuitDaemon".into(),
        "download-window-start" | "downloadWindowStartMinute" => "downloadWindowStartMinute".into(),
        "download-window-end" | "downloadWindowEndMinute" => "downloadWindowEndMinute".into(),
        "on-download-complete-command" | "onDownloadCompleteCommand" | "on-complete-command" => {
            "onDownloadCompleteCommand".into()
        }
        "on-download-error-command" | "onDownloadErrorCommand" | "on-error-command" => {
            "onDownloadErrorCommand".into()
        }
        _ => return None,
    })
}

fn config_patch(key: &str, value: &str) -> Result<Value, String> {
    let normalized = normalize_config_key(key).ok_or_else(|| {
        format!("unknown setting key: {key:?}.\nKnown keys: download-dir, max-concurrent, max-connections, max-total-connections, global-speed-limit, per-task-speed-limit, retry-count, min-segment-size, connect-timeout, read-timeout, api-enabled, api-bind, mcp-enabled, web-ui-enabled, web-ui-bind, user-agent, auto-quit-daemon, download-window-start, download-window-end, on-download-complete-command, on-download-error-command")
    })?;
    let v: Value = match normalized.as_str() {
        "downloadDir" => Value::String(util::expand_tilde(value).to_string_lossy().into_owned()),
        "downloadWindowStartMinute" | "downloadWindowEndMinute" => {
            // Accept either "HH:MM" (local) or a bare minute count.
            if value.contains(':') {
                json!(util::parse_window_hhmm(value)?)
            } else {
                let n: u32 = value
                    .parse()
                    .map_err(|_| format!("{normalized} expects HH:MM or minutes, got {value:?}"))?;
                if n >= 1440 {
                    return Err(format!("{normalized} must be < 1440 minutes, got {n}"));
                }
                json!(n)
            }
        }
        "apiEnabled" | "mcpEnabled" | "webUiEnabled" | "autoQuitDaemon" => {
            match value.to_ascii_lowercase().as_str() {
                "true" | "1" | "on" | "yes" => Value::Bool(true),
                "false" | "0" | "off" | "no" => Value::Bool(false),
                _ => {
                    return Err(format!(
                        "{normalized} expects a boolean (true/false), got {value:?}"
                    ))
                }
            }
        }
        "apiBind"
        | "webUiBind"
        | "userAgent"
        | "onDownloadCompleteCommand"
        | "onDownloadErrorCommand" => {
            // Empty string clears the hook.
            Value::String(value.to_string())
        }
        numeric => {
            let bytes = if numeric.contains("SpeedLimit") || numeric == "minSegmentSize" {
                Some(util::parse_bytes(value)?)
            } else {
                None
            };
            match bytes {
                Some(n) => json!(n),
                None => Value::from(
                    value
                        .parse::<u64>()
                        .map_err(|_| format!("{normalized} expects a number, got {value:?}"))?,
                ),
            }
        }
    };
    Ok(json!({ normalized: v }))
}

/// Resolves a proxy id argument (full id or unambiguous prefix) against the daemon's proxy list.
async fn resolve_proxy_id(c: &AriaFlowClient, raw: &str) -> Result<String, (String, u8)> {
    let all = c.list_proxies().await.map_err(fmt_client_error)?;
    if all.iter().any(|p| p.id == raw) {
        return Ok(raw.to_string());
    }
    let matches: Vec<_> = all.iter().filter(|p| p.id.starts_with(raw)).collect();
    match matches.len() {
        1 => Ok(matches[0].id.clone()),
        0 => Err((format!("no proxy matches id prefix {raw:?}"), 1)),
        n => Err((format!("id prefix {raw:?} is ambiguous ({n} proxies)"), 2)),
    }
}

fn render_download_row(d: &ariaflow_core::Download) -> Vec<String> {
    vec![
        d.id.to_string()[..8].to_string(),
        d.filename.clone(),
        d.status.to_string(),
        format!("{:.1}%", d.progress),
        d.total_size
            .map(util::human_bytes)
            .unwrap_or_else(|| "?".into()),
        util::human_speed(d.speed_bytes_per_sec),
        util::human_eta(d.eta_seconds()),
    ]
}

async fn run(cli: Cli) -> Result<i32, (String, u8)> {
    if cli.tui {
        let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
        return match tui::run(c).await {
            Ok(()) => Ok(0),
            Err(e) => Err((format!("terminal UI error: {e}"), 1)),
        };
    }
    let Some(command) = &cli.command else {
        return Err((
            "missing subcommand (see `ariaflow --help`), or pass --tui for the interactive UI"
                .to_string(),
            2,
        ));
    };
    match command {
        Command::Status => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let status = c.status().await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&status).unwrap_or_default());
            } else {
                let cts = &status.counts;
                println!(
                    "AriaFlow {} — daemon up {}s",
                    status.version, status.uptime_secs
                );
                util::print_table(
                    &[
                        "total",
                        "downloading",
                        "waiting",
                        "paused",
                        "completed",
                        "failed",
                        "cancelled",
                    ],
                    &[vec![
                        cts.total.to_string(),
                        cts.downloading.to_string(),
                        cts.waiting.to_string(),
                        cts.paused.to_string(),
                        cts.completed.to_string(),
                        cts.failed.to_string(),
                        cts.cancelled.to_string(),
                    ]],
                );
                println!(
                    "downloaded: {}   current speed: {}",
                    util::human_bytes(cts.total_downloaded_bytes),
                    util::human_speed(cts.global_speed_bytes_per_sec)
                );
            }
            Ok(0)
        }
        Command::List {
            status,
            category,
            search,
            page,
            page_size,
        } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let filter = DownloadFilter {
                statuses: status
                    .as_deref()
                    .map(|s| {
                        s.split(',')
                            .filter_map(|p| ariaflow_core::DownloadStatus::parse(p.trim()))
                            .collect()
                    })
                    .unwrap_or_default(),
                category: category.clone(),
                search: search.clone(),
                sort_by: None,
                sort_order: Default::default(),
                page: *page,
                page_size: *page_size,
            };
            let items = c.list(&filter).await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&items).unwrap_or_default());
            } else if items.is_empty() {
                println!("No downloads. Add one with: ariaflow add <URL>");
            } else {
                util::print_table(
                    &[
                        "ID", "Filename", "Status", "Progress", "Size", "Speed", "ETA",
                    ],
                    &items.iter().map(render_download_row).collect::<Vec<_>>(),
                );
            }
            Ok(0)
        }
        Command::Info { id } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let id = resolve_id(&c, id).await?;
            let dl = c.get(id).await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&dl).unwrap_or_default());
                return Ok(0);
            }
            println!("ID:          {}", dl.id);
            println!("URL:         {}", dl.url);
            if let Some(u) = &dl.final_url {
                println!("Final URL:   {u}");
            }
            println!("Filename:    {}", dl.filename);
            println!("Save path:   {}", dl.save_path.display());
            println!("Status:      {}", dl.status);
            println!(
                "Progress:    {:.2}%  ({}/{})",
                dl.progress,
                util::human_bytes(dl.downloaded_size),
                dl.total_size
                    .map(util::human_bytes)
                    .unwrap_or_else(|| "?".into())
            );
            println!(
                "Speed:       {}   avg {}",
                util::human_speed(dl.speed_bytes_per_sec),
                util::human_speed(dl.average_speed_bytes_per_sec)
            );
            println!("ETA:         {}", util::human_eta(dl.eta_seconds()));
            println!(
                "Priority:    {}   Connections: {}/{}",
                dl.priority, dl.current_connections, dl.max_connections
            );
            println!("Category:    {}", dl.category);
            if let Some(etag) = &dl.etag {
                println!("ETag:        {etag}");
            }
            if let Some(lm) = &dl.last_modified {
                println!("Last-Mod:    {lm}");
            }
            if let Some(cs) = &dl.checksum {
                println!("Checksum:    {} {}", cs.algorithm.as_str(), cs.digest);
            }
            if !dl.headers.is_empty() {
                println!("Headers:");
                for (k, v) in &dl.headers {
                    let v_redacted = if k.eq_ignore_ascii_case("authorization")
                        || k.eq_ignore_ascii_case("cookie")
                    {
                        "<redacted>".to_string()
                    } else {
                        v.clone()
                    };
                    println!("  {k}: {v_redacted}");
                }
            }
            if let Some(start) = dl.scheduled_start_at {
                println!("Scheduled:   {}", start.with_timezone(&chrono::Local));
            }
            if let Some(err) = &dl.error_message {
                println!("Error:       {err}");
            }
            if !dl.segments.is_empty() {
                println!("Segments:    {}", dl.segments.len());
                for s in &dl.segments {
                    println!(
                        "  #{:<3} [{:>12}..{:<12}] {:>6.2}%  {:?}",
                        s.id,
                        s.start_offset,
                        s.end_offset,
                        if s.total_bytes() == 0 {
                            0.0
                        } else {
                            s.downloaded_size as f64 / s.total_bytes() as f64 * 100.0
                        },
                        s.status
                    );
                }
            }
            Ok(0)
        }
        Command::Add {
            url,
            dir,
            connections,
            priority,
            headers,
            referer,
            bearer,
            basic,
            filename,
            paused,
            limit_rate,
            retry,
            schedule,
        } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let mut req = NewDownloadRequest::new(url.clone());
            req.save_dir = dir.as_deref().map(util::expand_tilde);
            req.max_connections = *connections;
            req.priority = *priority;
            req.headers = parse_headers(headers, referer);
            req.filename = filename.clone();
            req.start_paused = *paused;
            req.limit_rate_bytes_per_sec = limit_rate
                .as_deref()
                .map(util::parse_bytes)
                .transpose()
                .map_err(|e| (e, 2))?;
            req.max_retries = *retry;
            req.scheduled_start_at = schedule
                .as_deref()
                .map(util::parse_schedule_instant)
                .transpose()
                .map_err(|e| (e, 2))?;
            if let Some(token) = bearer {
                req.auth = Some(ariaflow_core::AuthConfig::Bearer {
                    token: token.clone(),
                });
            }
            if let Some(pair) = basic {
                let (username, password) = parse_basic_pair(pair)?;
                req.auth = Some(ariaflow_core::AuthConfig::Basic { username, password });
            }
            let dl = c.add(&req).await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&dl).unwrap_or_default());
            } else {
                println!("added {}  id={}", dl.filename, dl.id);
                println!("  url:  {}", dl.url);
                println!("  path: {}", dl.save_path.display());
                println!("  status: {}", dl.status);
            }
            Ok(0)
        }
        Command::Pause { id, all } => single_or_all(&cli, id, *all, "pause", "pauseAll").await,
        Command::Resume { id, all } => single_or_all(&cli, id, *all, "resume", "resumeAll").await,
        Command::Cancel { id, all } => single_or_all(&cli, id, *all, "cancel", "cancelAll").await,
        Command::Remove {
            id,
            completed,
            delete_file,
        } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            if *completed {
                let res = c
                    .batch("removeCompleted", None)
                    .await
                    .map_err(fmt_client_error)?;
                report_batch(&cli, &res.action, res.affected);
                return Ok(0);
            }
            let id = id
                .as_deref()
                .ok_or_else(|| ("provide a task id or --completed".to_string(), 2u8))?;
            let id = resolve_id(&c, id).await?;
            c.remove(id, *delete_file).await.map_err(fmt_client_error)?;
            println!("removed {id}");
            Ok(0)
        }
        Command::Retry { id } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let id = resolve_id(&c, id).await?;
            c.retry(id).await.map_err(fmt_client_error)?;
            println!("requeued {id}");
            Ok(0)
        }
        Command::BatchAdd {
            file,
            dir,
            connections,
            priority,
            headers,
            referer,
            bearer,
            basic,
            paused,
            limit_rate,
            retry,
            schedule,
        } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let content = if file == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| (format!("failed to read stdin: {e}"), 1))?;
                buf
            } else {
                std::fs::read_to_string(file)
                    .map_err(|e| (format!("cannot read {file}: {e}"), 1))?
            };
            let urls: Vec<String> = content
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect();
            if urls.is_empty() {
                return Err(("no URLs found in input".to_string(), 2));
            }
            // Shared per-task options: every URL in the file gets the same
            // headers/auth/limits, mirroring the single `add` command.
            let parsed_headers = parse_headers(headers, referer);
            let auth = if let Some(token) = bearer {
                Some(ariaflow_core::AuthConfig::Bearer {
                    token: token.clone(),
                })
            } else if let Some(pair) = basic {
                let (username, password) = parse_basic_pair(pair)?;
                Some(ariaflow_core::AuthConfig::Basic { username, password })
            } else {
                None
            };
            let save_dir = dir.as_deref().map(util::expand_tilde);
            let limit_rate_bytes = limit_rate
                .as_deref()
                .map(util::parse_bytes)
                .transpose()
                .map_err(|e| (e, 2))?;
            let scheduled_start = schedule
                .as_deref()
                .map(util::parse_schedule_instant)
                .transpose()
                .map_err(|e| (e, 2))?;
            let tasks: Vec<ariaflow_core::NewDownloadRequest> = urls
                .into_iter()
                .map(|url| {
                    let mut req = NewDownloadRequest::new(url);
                    req.save_dir = save_dir.clone();
                    req.max_connections = *connections;
                    req.priority = *priority;
                    req.headers = parsed_headers.clone();
                    req.auth = auth.clone();
                    req.start_paused = *paused;
                    req.limit_rate_bytes_per_sec = limit_rate_bytes;
                    req.max_retries = *retry;
                    req.scheduled_start_at = scheduled_start;
                    req
                })
                .collect();
            let res = c.batch_add(tasks).await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&res).unwrap_or_default());
            } else {
                println!("added {} task(s)", res.affected);
                if !res.errors.is_empty() {
                    println!("{} item(s) failed:", res.errors.len());
                    for item in &res.errors {
                        println!("  • [{}] {} — {}", item.index, item.url, item.message);
                    }
                }
            }
            Ok(0)
        }
        Command::Probe {
            url,
            headers,
            referer,
        } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let headers = parse_headers(headers, referer);
            let info = c.probe(url, headers).await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&info).unwrap_or_default());
            } else {
                println!("final URL:    {}", info.final_url);
                println!(
                    "size:         {}",
                    info.total_size
                        .map(util::human_bytes)
                        .unwrap_or_else(|| "unknown".into())
                );
                println!(
                    "accept-ranges:{}",
                    if info.accept_ranges { " yes" } else { " no" }
                );
                if let Some(etag) = &info.etag {
                    println!("etag:         {etag}");
                }
                if let Some(lm) = &info.last_modified {
                    println!("last-modified:{lm}");
                }
                if let Some(ct) = &info.content_type {
                    println!("content-type: {ct}");
                }
                if let Some(name) = &info.filename_hint {
                    println!("filename:     {name}");
                }
            }
            Ok(0)
        }
        Command::Schedule { cmd } => {
            match cmd {
                ScheduleCommand::Show => {
                    let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                    let settings = c.settings().await.map_err(fmt_client_error)?;
                    let start = settings.download_window_start_minute;
                    let end = settings.download_window_end_minute;
                    if cli.json {
                        print_json(&serde_json::json!({
                            "downloadWindowStartMinute": start,
                            "downloadWindowEndMinute": end,
                        }));
                    } else {
                        match (start, end) {
                            (Some(s), Some(e)) => println!(
                                "download window: {} - {} (local)",
                                util::format_window_hhmm(s),
                                util::format_window_hhmm(e)
                            ),
                            _ => println!(
                                "download window: not configured (downloads start immediately)"
                            ),
                        }
                    }
                    Ok(0)
                }
                ScheduleCommand::Set { start, end } => {
                    let start_min = util::parse_window_hhmm(start).map_err(|e| (e, 2))?;
                    let end_min = util::parse_window_hhmm(end).map_err(|e| (e, 2))?;
                    if start_min == end_min {
                        return Err(("window start and end must differ (a zero-length window would never run)".into(), 2));
                    }
                    let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                    let patch = serde_json::json!({
                        "downloadWindowStartMinute": start_min,
                        "downloadWindowEndMinute": end_min,
                    });
                    let updated = c.update_settings(&patch).await.map_err(fmt_client_error)?;
                    if cli.json {
                        print_json(&serde_json::to_value(&updated).unwrap_or_default());
                    } else {
                        println!(
                        "download window set: {} - {} (local); automatic starts outside the window are deferred",
                        util::format_window_hhmm(start_min),
                        util::format_window_hhmm(end_min)
                    );
                    }
                    Ok(0)
                }
                ScheduleCommand::Clear => {
                    let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                    let patch = serde_json::json!({
                        "downloadWindowStartMinute": null,
                        "downloadWindowEndMinute": null,
                    });
                    let updated = c.update_settings(&patch).await.map_err(fmt_client_error)?;
                    if cli.json {
                        print_json(&serde_json::to_value(&updated).unwrap_or_default());
                    } else {
                        println!("download window cleared; downloads start immediately");
                    }
                    Ok(0)
                }
            }
        }
        Command::History { limit } => {
            let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
            let entries = c.history(*limit).await.map_err(fmt_client_error)?;
            if cli.json {
                print_json(&serde_json::to_value(&entries).unwrap_or_default());
            } else if entries.is_empty() {
                println!("No history yet.");
            } else {
                util::print_table(
                    &["Filename", "Result", "Size", "Avg speed", "Completed"],
                    &entries
                        .iter()
                        .map(|e| {
                            vec![
                                e.filename.clone(),
                                e.result.clone(),
                                e.total_size
                                    .map(util::human_bytes)
                                    .unwrap_or_else(|| "?".into()),
                                e.average_speed
                                    .map(util::human_speed)
                                    .unwrap_or_else(|| "?".into()),
                                e.completed_at.clone(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                );
            }
            Ok(0)
        }
        Command::Proxies { cmd } => match cmd {
            ProxyCommand::List => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let proxies = c.list_proxies().await.map_err(fmt_client_error)?;
                if cli.json {
                    print_json(&serde_json::to_value(&proxies).unwrap_or_default());
                } else if proxies.is_empty() {
                    println!("No proxies configured. Add one with: ariaflow proxies add --name <name> --kind http --address <url>");
                } else {
                    util::print_table(
                        &["ID", "Name", "Kind", "Address", "Default"],
                        &proxies
                            .iter()
                            .map(|p| {
                                vec![
                                    p.id[..8.min(p.id.len())].to_string(),
                                    p.name.clone(),
                                    p.kind.clone(),
                                    p.address.clone(),
                                    if p.is_default {
                                        "*".into()
                                    } else {
                                        String::new()
                                    },
                                ]
                            })
                            .collect::<Vec<_>>(),
                    );
                }
                Ok(0)
            }
            ProxyCommand::Add {
                name,
                kind,
                address,
                username,
                password,
                bypass,
                is_default,
            } => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let input = ariaflow_core::ProxyInput {
                    name: name.clone(),
                    kind: kind.clone(),
                    address: address.clone(),
                    username: username.clone(),
                    password: password.clone(),
                    bypass: bypass.clone(),
                    is_default: *is_default,
                };
                let entry = c.create_proxy(&input).await.map_err(fmt_client_error)?;
                if cli.json {
                    print_json(&serde_json::to_value(&entry).unwrap_or_default());
                } else {
                    println!(
                        "added proxy {} ({} {})",
                        entry.name, entry.kind, entry.address
                    );
                }
                Ok(0)
            }
            ProxyCommand::Remove { id } => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let id = resolve_proxy_id(&c, id).await?;
                c.delete_proxy(&id).await.map_err(fmt_client_error)?;
                println!("removed proxy {id}");
                Ok(0)
            }
            ProxyCommand::SetDefault { id } => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let id = resolve_proxy_id(&c, id).await?;
                let proxies = c.list_proxies().await.map_err(fmt_client_error)?;
                let entry = proxies
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| (format!("proxy {id} vanished during update"), 1))?;
                let input = ariaflow_core::ProxyInput {
                    name: entry.name,
                    kind: entry.kind,
                    address: entry.address,
                    username: entry.username,
                    password: entry.password,
                    bypass: entry.bypass,
                    is_default: true,
                };
                let updated = c
                    .update_proxy(&id, &input)
                    .await
                    .map_err(fmt_client_error)?;
                println!("proxy {} is now the default", updated.name);
                Ok(0)
            }
        },
        Command::Categories { cmd } => match cmd {
            CategoriesCommand::Show => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let rules = c.categories().await.map_err(fmt_client_error)?;
                print_json(&rules);
                Ok(0)
            }
            CategoriesCommand::Export { file } => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let rules = c.categories().await.map_err(fmt_client_error)?;
                let pretty =
                    serde_json::to_string_pretty(&rules).map_err(|e| (e.to_string(), 1))?;
                match file {
                    Some(path) => {
                        std::fs::write(path, &pretty)
                            .map_err(|e| (format!("cannot write {path}: {e}"), 1))?;
                        if !cli.json {
                            println!("category rules written to {path}");
                        }
                    }
                    None => println!("{pretty}"),
                }
                Ok(0)
            }
        },
        Command::Config { cmd } => match cmd {
            ConfigCommand::Show => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let settings = c.settings().await.map_err(fmt_client_error)?;
                print_json(&serde_json::to_value(&settings).unwrap_or_default());
                Ok(0)
            }
            ConfigCommand::Set { key, value } => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                let patch = config_patch(key, value).map_err(|e| (e, 2))?;
                let updated = c.update_settings(&patch).await.map_err(fmt_client_error)?;
                if cli.json {
                    print_json(&serde_json::to_value(&updated).unwrap_or_default());
                } else {
                    println!("updated {key}");
                }
                Ok(0)
            }
        },
        Command::Daemon { cmd } => match cmd {
            DaemonCommand::Start { bin } => daemon_cmd::start(bin.as_deref())
                .map(|_| 0)
                .map_err(|e| (e, 1)),
            DaemonCommand::Stop => daemon_cmd::stop().map(|_| 0).map_err(|e| (e, 1)),
            DaemonCommand::Restart { bin } => daemon_cmd::restart(bin.as_deref())
                .map(|_| 0)
                .map_err(|e| (e, 1)),
            DaemonCommand::Status => {
                let c = client(&cli).map_err(|e| (e.to_string(), 1))?;
                match c.status().await {
                    Ok(status) => {
                        if cli.json {
                            print_json(&serde_json::json!({
                                "running": true,
                                "version": status.version,
                                "uptimeSecs": status.uptime_secs,
                                "apiUrl": cli.api_url,
                            }));
                        } else {
                            println!(
                                "ariaflowd: running  version={}  uptime={}s  api={}",
                                status.version, status.uptime_secs, cli.api_url
                            );
                        }
                        Ok(0)
                    }
                    Err(_) => {
                        if cli.json {
                            print_json(
                                &serde_json::json!({ "running": false, "apiUrl": cli.api_url }),
                            );
                        } else {
                            println!("ariaflowd: NOT reachable at {}", cli.api_url);
                        }
                        Ok(3)
                    }
                }
            }
        },
    }
}

async fn single_or_all(
    cli: &Cli,
    id: &Option<String>,
    all: bool,
    single_action: &str,
    batch_action: &str,
) -> Result<i32, (String, u8)> {
    let c = client(cli).map_err(|e| (e.to_string(), 1))?;
    if all {
        let res = c
            .batch(batch_action, None)
            .await
            .map_err(fmt_client_error)?;
        report_batch(cli, &res.action, res.affected);
        return Ok(0);
    }
    let id = id.as_deref().ok_or_else(|| {
        (
            format!("provide a task id or --all (for {single_action})"),
            2u8,
        )
    })?;
    let id = resolve_id(&c, id).await?;
    match single_action {
        "pause" => c.pause(id).await.map_err(fmt_client_error)?,
        "resume" => c.resume(id).await.map_err(fmt_client_error)?,
        "cancel" => c.cancel(id).await.map_err(fmt_client_error)?,
        other => return Err((format!("internal: unknown action {other}"), 1)),
    }
    let done_word = match single_action {
        "pause" => "paused",
        "resume" => "resumed",
        "cancel" => "cancelled",
        other => other,
    };
    println!("{done_word} {id}");
    Ok(0)
}

fn report_batch(cli: &Cli, action: &str, affected: u32) {
    if cli.json {
        print_json(&serde_json::json!({ "action": action, "affected": affected }));
    } else {
        println!("{action}: {affected} task(s) affected");
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => ExitCode::from(code as u8),
        Err((msg, code)) => {
            eprintln!("error: {msg}");
            ExitCode::from(code)
        }
    }
}
