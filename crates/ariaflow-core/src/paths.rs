//! Platform-conformant configuration and data directories for AriaFlow.
//!
//! - Linux:   ~/.config/ariaflow , ~/.local/share/ariaflow   (or XDG_*)
//! - macOS:   ~/Library/Application Support/AriaFlow
//! - Windows: %APPDATA%\AriaFlow

use std::path::PathBuf;

/// Directory holding `ariaflow.db` and other persistent state.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("ariaflow"))
        .unwrap_or_else(|| PathBuf::from("./ariaflow-data"))
}

/// Directory holding configuration files (`config.toml`).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("ariaflow"))
        .unwrap_or_else(|| PathBuf::from("./ariaflow-config"))
}

/// Directory holding log files.
pub fn log_dir() -> PathBuf {
    dirs::state_dir()
        .map(|d| d.join("ariaflow").join("log"))
        .unwrap_or_else(|| data_dir().join("log"))
}

/// Full path of the primary SQLite database.
pub fn database_path() -> PathBuf {
    data_dir().join("ariaflow.db")
}

/// Default user download directory.
///
/// Falls back gracefully: XDG download dir → `~/Downloads` if it exists → home directory → `.`.
pub fn default_download_dir() -> PathBuf {
    if let Some(dir) = dirs::download_dir() {
        return dir;
    }
    if let Some(home) = dirs::home_dir() {
        let downloads = home.join("Downloads");
        if downloads.is_dir() {
            return downloads;
        }
        return home;
    }
    PathBuf::from(".")
}

/// Path of the daemon pid file.
pub fn pid_file_path() -> PathBuf {
    data_dir().join("ariaflowd.pid")
}

/// Ensures the directory exists.
pub fn ensure_dir(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Default IPC endpoint for `ariaflowd`.
///
/// - Unix:   `$XDG_RUNTIME_DIR/ariaflowd.sock` when set, otherwise
///   `<data_dir>/ariaflowd.sock` (e.g. `~/.local/share/ariaflow/ariaflowd.sock`)
/// - Windows: the named pipe `\\.\pipe\ariaflowd`
///
/// The desktop application and the browser native-host use this endpoint when
/// no explicit path is configured.
pub fn default_ipc_path() -> String {
    #[cfg(unix)]
    {
        if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
            let candidate = std::path::Path::new(&runtime).join("ariaflowd.sock");
            return candidate.to_string_lossy().into_owned();
        }
        data_dir()
            .join("ariaflowd.sock")
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(not(unix))]
    {
        r"\\.\pipe\ariaflowd".to_string()
    }
}
