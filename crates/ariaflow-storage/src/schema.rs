//! Embedded, ordered migrations. Each runs exactly once, inside a transaction.

/// (version, name, sql) — versions must be strictly increasing.
pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "init",
        r#"
CREATE TABLE IF NOT EXISTS downloads (
    id                  TEXT PRIMARY KEY,
    url                 TEXT NOT NULL,
    final_url           TEXT,
    filename            TEXT NOT NULL,
    save_path           TEXT NOT NULL,
    total_size          INTEGER,
    downloaded_size     INTEGER NOT NULL DEFAULT 0,
    status              TEXT NOT NULL DEFAULT 'queued',
    progress            REAL NOT NULL DEFAULT 0.0,
    speed               INTEGER NOT NULL DEFAULT 0,
    average_speed       INTEGER NOT NULL DEFAULT 0,
    priority            INTEGER NOT NULL DEFAULT 0,
    retry_count         INTEGER NOT NULL DEFAULT 0,
    max_connections     INTEGER NOT NULL DEFAULT 16,
    current_connections INTEGER NOT NULL DEFAULT 0,
    category            TEXT NOT NULL DEFAULT 'other',
    etag                TEXT,
    last_modified       TEXT,
    checksum_json       TEXT,
    proxy_json          TEXT,
    error_message       TEXT,
    create_subdirs      INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL,
    started_at          TEXT,
    completed_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_downloads_status    ON downloads(status);
CREATE INDEX IF NOT EXISTS idx_downloads_category  ON downloads(category);
CREATE INDEX IF NOT EXISTS idx_downloads_created   ON downloads(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_downloads_priority  ON downloads(priority DESC);

CREATE TABLE IF NOT EXISTS segments (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    download_id     TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    segment_index   INTEGER NOT NULL,
    start_offset    INTEGER NOT NULL,
    end_offset      INTEGER NOT NULL,
    downloaded_size INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'pending',
    retry_count     INTEGER NOT NULL DEFAULT 0,
    UNIQUE(download_id, segment_index)
);
CREATE INDEX IF NOT EXISTS idx_segments_download ON segments(download_id);

CREATE TABLE IF NOT EXISTS categories (
    key         TEXT PRIMARY KEY,
    label       TEXT NOT NULL,
    extensions  TEXT NOT NULL DEFAULT '[]',
    is_builtin  INTEGER NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO categories(key, label, extensions, is_builtin, sort_order) VALUES
    ('documents',  '文档',       '["pdf","doc","docx","xls","xlsx","ppt","pptx","txt","md","epub"]', 1, 10),
    ('images',     '图片',       '["jpg","jpeg","png","gif","webp","svg","bmp"]',                  1, 20),
    ('videos',     '视频',       '["mp4","mkv","avi","mov","webm","flv"]',                        1, 30),
    ('audio',      '音频',       '["mp3","flac","wav","aac","ogg","m4a","opus"]',                 1, 40),
    ('archives',   '压缩文件',    '["zip","rar","7z","tar","gz","bz2","xz","zst","iso"]',           1, 50),
    ('installers', '软件安装包',  '["exe","msi","deb","rpm","apk","dmg","pkg","run"]',             1, 60),
    ('other',      '其他',       '[]',                                                        1, 999);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS download_headers (
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (download_id, name)
);

CREATE TABLE IF NOT EXISTS download_history (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    url             TEXT NOT NULL,
    filename        TEXT NOT NULL,
    save_path       TEXT NOT NULL,
    total_size      INTEGER,
    average_speed   INTEGER,
    result          TEXT NOT NULL,
    error_message   TEXT,
    started_at      TEXT,
    completed_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_history_completed ON download_history(completed_at DESC);
CREATE INDEX IF NOT EXISTS idx_history_result    ON download_history(result);

CREATE TABLE IF NOT EXISTS proxies (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    address    TEXT NOT NULL,
    username   TEXT,
    password   TEXT,
    bypass     TEXT,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS api_tokens (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    scopes       TEXT NOT NULL DEFAULT '["*"]',
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT,
    revoked_at   TEXT
);

CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    download_id TEXT,
    event_type  TEXT NOT NULL,
    payload     TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_events_download ON events(download_id);
CREATE INDEX IF NOT EXISTS idx_events_created  ON events(created_at DESC);
"#,
    ),
    (
        2,
        "task-limits",
        r#"
ALTER TABLE downloads ADD COLUMN limit_rate_bytes_per_sec INTEGER;
ALTER TABLE downloads ADD COLUMN max_retries INTEGER;
"#,
    ),
    (
        3,
        "scheduled-start",
        r#"
ALTER TABLE downloads ADD COLUMN scheduled_start_at TEXT;
"#,
    ),
    (
        4,
        "task-auth",
        r#"
ALTER TABLE downloads ADD COLUMN auth_json TEXT;
"#,
    ),
    (
        5,
        "certificates-and-user-agent",
        r#"
ALTER TABLE downloads ADD COLUMN user_agent TEXT;
ALTER TABLE downloads ADD COLUMN ssh_certificate_id TEXT;
CREATE TABLE IF NOT EXISTS ssh_certificates (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    private_key TEXT NOT NULL,
    passphrase  TEXT,
    algorithm   TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_ssh_certificates_name ON ssh_certificates(name);
"#,
    ),
];
