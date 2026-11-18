//! Graceful daemon shutdown for the desktop shell's "auto quit ariaflowd"
//! option (Settings → 守护进程 → “自动退出 ariaflowd”, stored in the daemon's
//! global settings as `autoQuitDaemon`).
//!
//! Mirrors `ariaflow daemon stop`: read `<data_dir>/ariaflowd.pid`, SIGTERM
//! (Unix) / taskkill (Windows), wait briefly, then drop a stale pid file.
//! Best-effort by design — quitting the app must never wedge on a wedged
//! daemon (the caller decides how long to wait / whether to proceed).

use std::path::PathBuf;
use std::time::Duration;

/// `<data_dir>/ariaflow` where ariaflow-core keeps `ariaflowd.pid`
/// (Linux `~/.local/share/ariaflow`, macOS `~/Library/Application Support/ariaflow`,
/// Windows `%APPDATA%\ariaflow`).
fn data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("ariaflow"))
        .unwrap_or_else(|| PathBuf::from("./ariaflow-data"))
}

fn pid_file_path() -> PathBuf {
    data_dir().join("ariaflowd.pid")
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // kill(pid, 0): existence + permission check, no signal delivered
    let rc = unsafe { kill(pid as i32, 0) };
    rc == 0
}

#[cfg(not(unix))]
fn process_alive(pid: u32) -> bool {
    // tasklist is available on all supported Windows versions
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output();
    output
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(unix)]
fn signal_term(pid: u32) -> Result<(), String> {
    const SIGTERM: i32 = 15;
    let rc = unsafe { kill(pid as i32, SIGTERM) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "failed to signal pid {pid}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn signal_term(pid: u32) -> Result<(), String> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .status()
        .map_err(|e| format!("taskkill failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill exited with {status}"))
    }
}

/// Read the pid file, if any. `Ok(None)` means “no daemon recorded”.
fn read_daemon_pid() -> Result<Option<u32>, String> {
    let path = pid_file_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => raw
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| format!("invalid pid file {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_file_path());
}

/// Non-blocking graceful stop: resolve the pid and send SIGTERM, then return
/// immediately WITHOUT waiting for the daemon to exit. This is what the tray
/// "退出" flow uses so the UI can exit instantly while ariaflowd shuts itself
/// down in the background (it persists state on SIGTERM like any other path).
pub fn signal_daemon_stop() -> Result<String, String> {
    let pid = match read_daemon_pid()? {
        None => return Ok("ariaflowd not running (no pid file)".to_string()),
        Some(pid) => pid,
    };
    if !process_alive(pid) {
        remove_pid_file();
        return Ok(format!(
            "ariaflowd not running (removed stale pid file for pid {pid})"
        ));
    }
    signal_term(pid)?;
    Ok(format!("ariaflowd (pid {pid}) signalled to stop"))
}

/// Shuts ariaflowd down gracefully and waits up to 3s for the process to exit.
/// Returns a human-readable note on success; an `Err` only when we know the
/// daemon is (believed) running but did not stop in time.
pub async fn quit_daemon() -> Result<String, String> {
    let pid = match read_daemon_pid()? {
        None => return Ok("ariaflowd not running (no pid file)".to_string()),
        Some(pid) => pid,
    };
    if !process_alive(pid) {
        remove_pid_file();
        return Ok(format!(
            "ariaflowd not running (removed stale pid file for pid {pid})"
        ));
    }
    signal_term(pid)?;
    // The daemon persists its state and exits on SIGTERM; give it up to 3s.
    for _ in 0..30 {
        if !process_alive(pid) {
            remove_pid_file();
            return Ok(format!("ariaflowd (pid {pid}) stopped"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "ariaflowd (pid {pid}) did not exit within 3s; inspect the ariaflowd log"
    ))
}
