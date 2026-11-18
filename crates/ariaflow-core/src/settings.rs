//! Global user-facing settings (persisted by the storage layer).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default User-Agent for outgoing downloads — Linux desktop Chrome. Many
/// file hosts serve different (or no) bytes to obvious bot UAs; the previous
/// Windows-NT Chrome default turned out to attract 403s from some hosts, so
/// the initial value now matches the seeded list below.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36";

/// User-Agents seeded on first run (and for settings written before the
/// `userAgents` field existed). The download dialogs let the user pick one
/// of these or type any value freely; the settings page manages the list.
pub const DEFAULT_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36 Edg/157.0.0.0",
    "Mozilla/5.0 (X11; Linux x86_64; rv:160.0) Gecko/20100101 Firefox/160.0",
    "curl/8.23.0",
    "Wget/1.25.0",
    "Wget2/2.3",
];

fn default_user_agents() -> Vec<String> {
    DEFAULT_USER_AGENTS.iter().map(|s| s.to_string()).collect()
}

/// Max entries kept in the User-Agent list (config hygiene, not a hard API limit).
pub const MAX_USER_AGENTS: usize = 64;
/// Max length of a single User-Agent entry.
pub const MAX_USER_AGENT_LEN: usize = 512;

fn normalize_user_agents(list: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.clone()))
        .take(MAX_USER_AGENTS)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    pub download_dir: PathBuf,
    /// Max simultaneously active downloads.
    pub max_concurrent_tasks: u16,
    /// Default per-task connection count.
    pub max_connections_per_task: u16,
    /// Hard cap on total connections across all tasks. 0 = unlimited.
    pub max_total_connections: u16,
    /// Global speed limit in bytes/sec. 0 = unlimited.
    pub global_speed_limit: u64,
    /// Per-task speed limit in bytes/sec. 0 = unlimited.
    pub per_task_speed_limit: u64,
    /// Automatic retry attempts for failed downloads.
    pub retry_count: u32,
    /// Segment splitting threshold: files smaller than this use a single connection.
    pub min_segment_size: u64,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: u64,
    /// Idle/read timeout in seconds.
    pub read_timeout_secs: u64,
    /// Whether the web API is enabled.
    pub api_enabled: bool,
    /// Bind address of the web API. Defaults to loopback only.
    pub api_bind: String,
    /// Bind address of the optional standalone web listener (the desktop web
    /// UI + same-origin `/api/v1`), e.g. `0.0.0.0:9090` for LAN browsers.
    /// Empty = not bound. Read once at daemon startup (restart required);
    /// `--web-bind` overrides this value for the boot. Empty string clears.
    #[serde(default)]
    pub web_ui_bind: String,
    /// Whether the MCP endpoint (`/mcp`) is enabled. Toggleable at runtime without restart.
    pub mcp_enabled: bool,
    /// Whether the web page (`/`) is enabled. Toggleable at runtime without restart.
    #[serde(default = "default_true")]
    pub web_ui_enabled: bool,
    /// User-Agent header used for downloads.
    pub user_agent: String,
    /// Managed list of User-Agents offered in the download dialogs ("输入或选择").
    /// Seeded with [`DEFAULT_USER_AGENTS`] on first run; settings written before
    /// this field existed also pick up the seed (empty list = user cleared it).
    #[serde(default = "default_user_agents")]
    pub user_agents: Vec<String>,
    /// Desktop shell preference: when the AriaFlow desktop app quits (tray
    /// menu → 退出), also shut down ariaflowd. Off by default — quitting the
    /// UI leaves the daemon running in the background. Stored server-side so
    /// the whole UI reads the same value.
    #[serde(default)]
    pub auto_quit_daemon: bool,
    /// Daily download window start, minutes after local midnight. `None` disables the window.
    #[serde(default)]
    pub download_window_start_minute: Option<u32>,
    /// Daily download window end (exclusive), minutes after local midnight.
    #[serde(default)]
    pub download_window_end_minute: Option<u32>,
    /// IDM-style post-processing hook: shell command run when a task completes
    /// (e.g. `notify-send "Done"`; empty/absent disables). Arbitrary code by design — set it yourself.
    #[serde(default)]
    pub on_download_complete_command: Option<String>,
    /// Same hook, run when a task permanently fails.
    #[serde(default)]
    pub on_download_error_command: Option<String>,
    /// Show paused tasks in the desktop overview's "in progress" section.
    /// Persisted server-side so the whole UI (daemon-served web page included)
    /// honours it.
    #[serde(default)]
    pub overview_show_paused: bool,
    /// Show newly created (pending/queued/waiting/retrying) tasks in the
    /// overview's in-progress section. Defaults to true so a freshly added
    /// task is visible right away; persisted server-side like overviewShowPaused.
    #[serde(default = "default_true")]
    pub overview_show_new_tasks: bool,
    /// Stat cards hidden from the overview page. Empty = show everything
    /// (the pre-0.1 default). Unknown ids are dropped when patched.
    #[serde(default)]
    pub overview_hidden_cards: Vec<String>,
    /// Default for the "also delete the downloaded file(s)" checkbox shown in
    /// every remove-task confirmation dialog. Clients read this as the initial
    /// checkbox state; the user can still override per deletion.
    #[serde(default)]
    pub delete_files_on_remove: bool,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            download_dir: crate::paths::default_download_dir(),
            max_concurrent_tasks: 5,
            max_connections_per_task: 16,
            max_total_connections: 64,
            global_speed_limit: 0,
            per_task_speed_limit: 0,
            retry_count: 5,
            min_segment_size: 1024 * 1024, // 1 MiB
            connect_timeout_secs: 30,
            read_timeout_secs: 60,
            api_enabled: true,
            api_bind: "127.0.0.1:8787".to_string(),
            web_ui_bind: String::new(),
            mcp_enabled: true,
            web_ui_enabled: true,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            user_agents: default_user_agents(),
            auto_quit_daemon: false,
            download_window_start_minute: None,
            download_window_end_minute: None,
            on_download_complete_command: None,
            on_download_error_command: None,
            overview_show_paused: false,
            overview_show_new_tasks: true,
            overview_hidden_cards: Vec::new(),
            delete_files_on_remove: false,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Card ids the overview page knows about; unknown ids in a settings patch are ignored.
pub const KNOWN_OVERVIEW_CARDS: &[&str] = &[
    "downloading",
    "waiting",
    "paused",
    "completed",
    "failed",
    "currentSpeed",
    "totalDownloaded",
];

fn normalize_hidden_cards(cards: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    cards
        .into_iter()
        .filter(|c| KNOWN_OVERVIEW_CARDS.contains(&c.as_str()))
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

/// Deserialises a JSON `null` as `Some(None)` and a number as `Some(Some(n))`.
/// Combined with `#[serde(default)]`, an *absent* key yields `None`, which the
/// patch logic treats as "leave unchanged". This is what lets clients clear a
/// previously-set window by explicitly sending `null`.
fn deserialize_explicit_nullable<'de, D>(d: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u32>::deserialize(d)?))
}

/// Partial patch for settings; `None` fields are left unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    pub download_dir: Option<PathBuf>,
    pub max_concurrent_tasks: Option<u16>,
    pub max_connections_per_task: Option<u16>,
    pub max_total_connections: Option<u16>,
    pub global_speed_limit: Option<u64>,
    pub per_task_speed_limit: Option<u64>,
    pub retry_count: Option<u32>,
    pub min_segment_size: Option<u64>,
    pub connect_timeout_secs: Option<u64>,
    pub read_timeout_secs: Option<u64>,
    pub api_enabled: Option<bool>,
    pub api_bind: Option<String>,
    pub web_ui_bind: Option<String>,
    pub mcp_enabled: Option<bool>,
    pub web_ui_enabled: Option<bool>,
    pub user_agent: Option<String>,
    /// Absent = unchanged; `[]` clears the list; entries are trimmed, deduped, capped.
    #[serde(default)]
    pub user_agents: Option<Vec<String>>,
    pub auto_quit_daemon: Option<bool>,
    /// Merge-patch semantics: absent = unchanged, explicit `null` = clear, number = set.
    #[serde(default, deserialize_with = "deserialize_explicit_nullable")]
    pub download_window_start_minute: Option<Option<u32>>,
    #[serde(default, deserialize_with = "deserialize_explicit_nullable")]
    pub download_window_end_minute: Option<Option<u32>>,
    /// Absent = unchanged; `""` clears; any other value replaces.
    #[serde(default)]
    pub on_download_complete_command: Option<String>,
    #[serde(default)]
    pub on_download_error_command: Option<String>,
    pub overview_show_paused: Option<bool>,
    pub overview_show_new_tasks: Option<bool>,
    pub overview_hidden_cards: Option<Vec<String>>,
    #[serde(default)]
    pub delete_files_on_remove: Option<bool>,
}

impl GlobalSettings {
    /// Validates scheduling-related fields after a patch is applied.
    /// Returns an error for out-of-range minute values.
    pub fn validate(&self) -> crate::Result<()> {
        if self.user_agents.len() > MAX_USER_AGENTS {
            return Err(crate::AriaFlowError::InvalidRequest(format!(
                "userAgents must contain at most {MAX_USER_AGENTS} entries, got {}",
                self.user_agents.len()
            )));
        }
        for (i, ua) in self.user_agents.iter().enumerate() {
            if ua.len() > MAX_USER_AGENT_LEN {
                return Err(crate::AriaFlowError::InvalidRequest(format!(
                    "userAgents[{i}] is too long ({} bytes, max {MAX_USER_AGENT_LEN})",
                    ua.len()
                )));
            }
            if ua.chars().any(|c| c.is_control()) {
                return Err(crate::AriaFlowError::InvalidRequest(format!(
                    "userAgents[{i}] must not contain control characters"
                )));
            }
        }
        for (name, v) in [
            (
                "onDownloadCompleteCommand",
                &self.on_download_complete_command,
            ),
            ("onDownloadErrorCommand", &self.on_download_error_command),
        ] {
            if let Some(cmd) = v {
                if cmd.len() > 1024 {
                    return Err(crate::AriaFlowError::InvalidRequest(format!(
                        "{name} must be at most 1024 bytes, got {}",
                        cmd.len()
                    )));
                }
                if cmd.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
                    return Err(crate::AriaFlowError::InvalidRequest(format!(
                        "{name} must be a single line"
                    )));
                }
            }
        }
        for (name, v) in [
            (
                "downloadWindowStartMinute",
                self.download_window_start_minute,
            ),
            ("downloadWindowEndMinute", self.download_window_end_minute),
        ] {
            if let Some(m) = v {
                if m >= 24 * 60 {
                    return Err(crate::AriaFlowError::InvalidRequest(format!(
                        "{name} must be < 1440 (minutes after local midnight), got {m}"
                    )));
                }
            }
        }
        if let (Some(start), Some(end)) = (
            self.download_window_start_minute,
            self.download_window_end_minute,
        ) {
            if start == end {
                return Err(crate::AriaFlowError::InvalidRequest(
                    "download window start and end must differ (a zero-length window would never run)".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn apply_patch(&mut self, patch: &UpdateSettingsRequest) {
        if let Some(v) = &patch.download_dir {
            self.download_dir = v.clone();
        }
        if let Some(v) = patch.max_concurrent_tasks {
            self.max_concurrent_tasks = v;
        }
        if let Some(v) = patch.max_connections_per_task {
            self.max_connections_per_task = v;
        }
        if let Some(v) = patch.max_total_connections {
            self.max_total_connections = v;
        }
        if let Some(v) = patch.global_speed_limit {
            self.global_speed_limit = v;
        }
        if let Some(v) = patch.per_task_speed_limit {
            self.per_task_speed_limit = v;
        }
        if let Some(v) = patch.retry_count {
            self.retry_count = v;
        }
        if let Some(v) = patch.min_segment_size {
            self.min_segment_size = v;
        }
        if let Some(v) = patch.connect_timeout_secs {
            self.connect_timeout_secs = v;
        }
        if let Some(v) = patch.read_timeout_secs {
            self.read_timeout_secs = v;
        }
        if let Some(v) = patch.api_enabled {
            self.api_enabled = v;
        }
        if let Some(v) = &patch.api_bind {
            self.api_bind = v.clone();
        }
        if let Some(v) = &patch.web_ui_bind {
            self.web_ui_bind = v.trim().to_string();
        }
        if let Some(v) = patch.mcp_enabled {
            self.mcp_enabled = v;
        }
        if let Some(v) = patch.web_ui_enabled {
            self.web_ui_enabled = v;
        }
        if let Some(v) = &patch.user_agent {
            self.user_agent = v.clone();
        }
        if let Some(v) = &patch.user_agents {
            self.user_agents = normalize_user_agents(v.clone());
        }
        if let Some(v) = patch.auto_quit_daemon {
            self.auto_quit_daemon = v;
        }
        if let Some(v) = &patch.download_window_start_minute {
            self.download_window_start_minute = *v;
        }
        if let Some(v) = &patch.download_window_end_minute {
            self.download_window_end_minute = *v;
        }
        if let Some(v) = &patch.on_download_complete_command {
            let trimmed = v.trim();
            self.on_download_complete_command = (!trimmed.is_empty()).then(|| v.clone());
        }
        if let Some(v) = &patch.on_download_error_command {
            let trimmed = v.trim();
            self.on_download_error_command = (!trimmed.is_empty()).then(|| v.clone());
        }
        if let Some(v) = patch.overview_show_paused {
            self.overview_show_paused = v;
        }
        if let Some(v) = patch.overview_show_new_tasks {
            self.overview_show_new_tasks = v;
        }
        if let Some(ref v) = patch.overview_hidden_cards {
            self.overview_hidden_cards = normalize_hidden_cards(v.clone());
        }
        if let Some(v) = patch.delete_files_on_remove {
            self.delete_files_on_remove = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_window_keys_leave_settings_unchanged() {
        let patch: UpdateSettingsRequest = serde_json::from_str("{}").unwrap();
        let mut s = GlobalSettings {
            download_window_start_minute: Some(60),
            download_window_end_minute: Some(120),
            ..GlobalSettings::default()
        };
        s.apply_patch(&patch);
        assert_eq!(s.download_window_start_minute, Some(60));
        assert_eq!(s.download_window_end_minute, Some(120));
    }

    #[test]
    fn explicit_null_clears_window() {
        let patch: UpdateSettingsRequest = serde_json::from_str(
            r#"{"downloadWindowStartMinute":null,"downloadWindowEndMinute":null}"#,
        )
        .unwrap();
        let mut s = GlobalSettings {
            download_window_start_minute: Some(60),
            download_window_end_minute: Some(120),
            ..GlobalSettings::default()
        };
        s.apply_patch(&patch);
        assert_eq!(s.download_window_start_minute, None);
        assert_eq!(s.download_window_end_minute, None);
    }

    #[test]
    fn numbers_set_window() {
        let patch: UpdateSettingsRequest = serde_json::from_str(
            r#"{"downloadWindowStartMinute":120,"downloadWindowEndMinute":360}"#,
        )
        .unwrap();
        let mut s = GlobalSettings::default();
        s.apply_patch(&patch);
        assert_eq!(s.download_window_start_minute, Some(120));
        assert_eq!(s.download_window_end_minute, Some(360));
    }

    #[test]
    fn hidden_cards_patch_filters_unknown_and_dedupes() {
        let patch: UpdateSettingsRequest = serde_json::from_str(
            r#"{"overviewHiddenCards":["waiting","bogus","waiting","failed","CURRENTSPEED"]}"#,
        )
        .unwrap();
        let mut s = GlobalSettings::default();
        assert!(s.overview_hidden_cards.is_empty());
        s.apply_patch(&patch);
        assert_eq!(s.overview_hidden_cards, vec!["waiting", "failed"]);
    }

    #[test]
    fn hidden_cards_absent_patch_leaves_settings_unchanged() {
        let patch: UpdateSettingsRequest = serde_json::from_str("{}").unwrap();
        let mut s = GlobalSettings {
            overview_hidden_cards: vec!["paused".to_string()],
            ..GlobalSettings::default()
        };
        s.apply_patch(&patch);
        assert_eq!(s.overview_hidden_cards, vec!["paused"]);
    }

    #[test]
    fn auto_quit_daemon_defaults_false_and_patches() {
        let mut s = GlobalSettings::default();
        assert!(!s.auto_quit_daemon);
        let patch: UpdateSettingsRequest =
            serde_json::from_str(r#"{"autoQuitDaemon":true}"#).unwrap();
        s.apply_patch(&patch);
        assert!(s.auto_quit_daemon);
        // absent key leaves the value untouched
        let patch: UpdateSettingsRequest = serde_json::from_str("{}").unwrap();
        s.apply_patch(&patch);
        assert!(s.auto_quit_daemon);
        let patch: UpdateSettingsRequest =
            serde_json::from_str(r#"{"autoQuitDaemon":false}"#).unwrap();
        s.apply_patch(&patch);
        assert!(!s.auto_quit_daemon);
    }

    #[test]
    fn default_user_agent_is_linux_chrome() {
        let s = GlobalSettings::default();
        assert_eq!(s.user_agent, DEFAULT_USER_AGENT);
        assert!(s.user_agent.contains("X11; Linux x86_64"));
        // The previously seeded Windows UA must not come back.
        assert!(!s.user_agent.contains("Windows NT 10.0"));
    }

    #[test]
    fn default_user_agents_seeds_six_entries() {
        let s = GlobalSettings::default();
        assert_eq!(s.user_agents.len(), 6);
        assert_eq!(s.user_agents, DEFAULT_USER_AGENTS);
        assert!(s.user_agents.iter().any(|u| u == "curl/8.23.0"));
        assert!(s.user_agents.iter().any(|u| u == "Wget/1.25.0"));
        assert!(s.user_agents.iter().any(|u| u == "Wget2/2.3"));
        assert!(s
            .user_agents
            .iter()
            .any(|u| u.contains("Firefox/160.0")));
        assert!(s.user_agents.iter().any(|u| u.contains("Edg/157.0.0.0")));
    }

    #[test]
    fn settings_json_without_user_agents_seeds_defaults() {
        // Simulates settings persisted before the `userAgents` field existed.
        let mut v = serde_json::to_value(GlobalSettings::default()).unwrap();
        v.as_object_mut().unwrap().remove("userAgents");
        let s: GlobalSettings = serde_json::from_value(v).unwrap();
        assert_eq!(s.user_agents, DEFAULT_USER_AGENTS);
        // An explicit empty list is a meaningful value (user cleared it) — kept.
        let v = serde_json::json!({ "userAgents": [] });
        let mut merged = serde_json::to_value(GlobalSettings::default()).unwrap();
        merged.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
        let s: GlobalSettings = serde_json::from_value(merged).unwrap();
        assert!(s.user_agents.is_empty());
    }

    #[test]
    fn patch_normalizes_and_dedupes_user_agents() {
        let patch: UpdateSettingsRequest = serde_json::from_str(
            r#"{"userAgents":["  curl/8.23.0  ","wget/1.25.0","curl/8.23.0","   ","Wget2/2.3"]}"#,
        )
        .unwrap();
        let mut s = GlobalSettings::default();
        s.apply_patch(&patch);
        assert_eq!(s.user_agents, vec!["curl/8.23.0", "wget/1.25.0", "Wget2/2.3"]);
        s.validate().unwrap();
    }

    #[test]
    fn patch_can_clear_user_agents_and_empty_is_valid() {
        let patch: UpdateSettingsRequest = serde_json::from_str(r#"{"userAgents":[]}"#).unwrap();
        let mut s = GlobalSettings::default();
        s.apply_patch(&patch);
        assert!(s.user_agents.is_empty());
        s.validate().unwrap();
    }

    #[test]
    fn validate_rejects_bad_user_agents() {
        let mut s = GlobalSettings::default();
        s.user_agents = vec!["bad\nagent".to_string()];
        assert!(s.validate().is_err());
        s.user_agents = vec!["x".repeat(MAX_USER_AGENT_LEN + 1)];
        assert!(s.validate().is_err());
        s.user_agents = vec!["fine".to_string(); MAX_USER_AGENTS + 1];
        assert!(s.validate().is_err());
    }

    #[test]
    fn out_of_range_window_is_rejected() {
        let s = GlobalSettings {
            download_window_start_minute: Some(2000),
            ..GlobalSettings::default()
        };
        assert!(s.validate().is_err());
        let s = GlobalSettings {
            download_window_start_minute: Some(60),
            download_window_end_minute: Some(60),
            ..GlobalSettings::default()
        };
        assert!(s.validate().is_err());
    }
}
