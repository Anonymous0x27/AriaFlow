//! `ariaflow daemon start|stop|restart|status`
//!
//! `start` locates the `ariaflowd` binary (an explicit `--bin` path when given,
//! otherwise the sibling of the current executable, falling back to PATH) and
//! spawns it detached with logs under the AriaFlow log directory. `stop`
//! signals the pid from the pid file. `status` reports whether a daemon
//! answers on the API endpoint.

use std::path::PathBuf;
use std::process::Stdio;

const PID_ENV: &str = "ARIAFLOW_PID_FILE";

fn pid_path() -> PathBuf {
    if let Ok(p) = std::env::var(PID_ENV) {
        return PathBuf::from(p);
    }
    ariaflow_core::paths::pid_file_path()
}

/// Locates the `ariaflowd` binary.
///
/// An explicit `--bin` path always wins: it must exist and be a file, otherwise
/// the caller reports the error instead of silently falling back. Without it we
/// look next to the current executable first, then on PATH.
fn find_ariaflowd(explicit: Option<&std::path::Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(format!("--bin: {} is not a file", path.display()))
        };
    }
    // 1) sibling of current exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(if cfg!(windows) {
                "ariaflowd.exe"
            } else {
                "ariaflowd"
            });
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    // 2) PATH lookup
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(if cfg!(windows) {
                "ariaflowd.exe"
            } else {
                "ariaflowd"
            });
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err("ariaflowd binary not found next to `ariaflow` or on PATH. Build the workspace first, or pass --bin <path>.".to_string())
}

fn log_dir() -> PathBuf {
    ariaflow_core::paths::log_dir()
}

fn read_pid() -> Option<u32> {
    std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0): existence + permission check, no signal delivered
        unsafe { libc_kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        // tasklist is available on all supported Windows versions
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output();
        output
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn setsid() -> i32;
}
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    kill(pid, sig)
}

pub fn start(bin: Option<&std::path::Path>) -> Result<(), String> {
    if let Some(pid) = read_pid() {
        if process_alive(pid) {
            return Err(format!(
                "ariaflowd appears to be running already (pid {pid})"
            ));
        }
    }
    let bin = find_ariaflowd(bin)?;
    spawn_daemon(&bin)
}

fn spawn_daemon(bin: &std::path::Path) -> Result<(), String> {
    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| format!("cannot create log dir {}: {e}", log_dir.display()))?;
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("ariaflowd.log"))
        .map_err(|e| e.to_string())?;
    let stderr = stdout.try_clone().map_err(|e| e.to_string())?;

    let mut cmd = std::process::Command::new(bin);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Detach into a new session so the daemon survives the terminal that started it.
        unsafe {
            cmd.pre_exec(|| {
                setsid();
                Ok(())
            });
        }
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;
    println!("ariaflowd started (pid {})", child.id());
    println!("logs: {}", log_dir.join("ariaflowd.log").display());
    Ok(())
}

pub fn stop() -> Result<(), String> {
    let pid = read_pid().ok_or_else(|| "no pid file found; is ariaflowd running?".to_string())?;
    #[cfg(unix)]
    {
        const SIGTERM: i32 = 15;
        let rc = unsafe { libc_kill(pid as i32, SIGTERM) };
        if rc != 0 {
            return Err(format!(
                "failed to signal pid {pid} (errno {})",
                std::io::Error::last_os_error()
            ));
        }
    }
    #[cfg(not(unix))]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .status()
            .map_err(|e| format!("taskkill failed: {e}"))?;
        if !status.success() {
            return Err(format!("taskkill exited with {status}"));
        }
    }
    // Wait up to 10s for graceful exit.
    for _ in 0..100 {
        if !process_alive(pid) {
            let _ = std::fs::remove_file(pid_path());
            println!("ariaflowd (pid {pid}) stopped");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!(
        "ariaflowd (pid {pid}) did not exit within 10s; inspect logs"
    ))
}

pub fn restart(bin: Option<&std::path::Path>) -> Result<(), String> {
    // Resolve (and validate) the binary FIRST, so a bad --bin fails without
    // taking down the running daemon.
    let bin = find_ariaflowd(bin)?;
    match stop() {
        Ok(()) => {}
        Err(e) => eprintln!("note: {e} (continuing with start)"),
    }
    if let Some(pid) = read_pid() {
        if process_alive(pid) {
            // stop() already waited; this is defensive only.
            return Err(format!("ariaflowd (pid {pid}) is still running"));
        }
    }
    spawn_daemon(&bin)
}
