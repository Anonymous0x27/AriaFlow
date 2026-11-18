//! Filename safety: path traversal protection, reserved-name handling, collision avoidance.

use std::path::{Path, PathBuf};

/// Windows reserved device names that cannot be used as filenames.
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitises an untrusted (e.g. server-provided or URL-derived) file name.
///
/// - strips path separators and parent-directory components
/// - removes control characters and trims trailing dots/spaces (Windows)
/// - avoids Windows reserved device names
/// - caps length to 200 bytes (keeping the extension)
/// - returns `"download"` if nothing usable remains
pub fn sanitize_filename(raw: &str) -> String {
    // Take only the final component-ish part; strip separators and drive letters.
    let without_separators: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = without_separators
        .trim()
        .trim_matches('.')
        .trim()
        .to_string();

    // A name with no alphanumeric content (e.g. "///" -> "___") is not usable.
    let mut name = if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_alphanumeric()) {
        "download".to_string()
    } else {
        trimmed
    };

    // Windows reserved names (with or without extension).
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        name = format!("_{name}");
    }

    // Length cap, preserving the extension.
    const MAX_BYTES: usize = 200;
    if name.len() > MAX_BYTES {
        let path = Path::new(&name);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("download")
            .to_string();
        let ext_len = if ext.is_empty() { 0 } else { ext.len() + 1 };
        let keep = MAX_BYTES.saturating_sub(ext_len).max(1);
        let mut new_stem = stem.clone();
        while new_stem.len() > keep {
            new_stem.pop();
        }
        while !new_stem.is_char_boundary(new_stem.len()) {
            new_stem.pop();
        }
        name = if ext.is_empty() {
            new_stem
        } else {
            format!("{new_stem}.{ext}")
        };
    }
    name
}

/// If `path` exists, returns a deduplicated variant `name (n).ext` in the same directory.
pub fn dedupe_path(path: &Path) -> PathBuf {
    dedupe_path_reserved(path, &|_| false)
}

/// Like [`dedupe_path`], but additionally treats `reserved(candidate) == true` as occupied.
/// Callers use this to avoid collisions with in-flight tasks whose files do not exist yet.
pub fn dedupe_path_reserved(path: &Path, reserved: &dyn Fn(&Path) -> bool) -> PathBuf {
    if !path.exists() && !reserved(path) {
        return path.to_path_buf();
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for n in 1..10000u32 {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() && !reserved(&candidate) {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{}", uuid::Uuid::new_v4().simple(), ext))
}

/// Resolves `save_dir` against `filename`, protecting against directory escape.
///
/// The returned path is guaranteed to be a direct child of `save_dir` with a sanitised name.
pub fn safe_save_path(save_dir: &Path, filename: &str) -> PathBuf {
    let clean = sanitize_filename(filename);
    save_dir.join(clean)
}

/// Strips a trailing `.torrent` extension (case-insensitive).
///
/// Used for http(s) URLs that point at a `.torrent` manifest: the downloaded
/// artifact is the torrent's payload, not the manifest itself, so the payload
/// should not inherit the manifest's file name.
pub fn strip_torrent_suffix(name: &str) -> String {
    const SUFFIX: &str = ".torrent";
    if name.len() > SUFFIX.len() && name[name.len() - SUFFIX.len()..].eq_ignore_ascii_case(SUFFIX) {
        return name[..name.len() - SUFFIX.len()].to_string();
    }
    if name.eq_ignore_ascii_case(SUFFIX) {
        // A bare ".torrent" would sanitize to nothing; keep something readable.
        return "torrent".to_string();
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitises_traversal() {
        let cleaned = sanitize_filename("../../etc/passwd");
        assert!(!cleaned.contains('/'));
        assert!(!cleaned.contains('\\'));
        assert!(!cleaned.starts_with('.'));
        assert!(!cleaned.is_empty());
    }

    #[test]
    fn sanitises_reserved_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(sanitize_filename("com1"), "_com1");
    }

    #[test]
    fn sanitises_empty() {
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("///"), "download");
    }

    #[test]
    fn dedupe_nonexistent_unchanged() {
        let p = dedupe_path(Path::new("/nonexistent-dir-xyz/file.zip"));
        assert_eq!(p, Path::new("/nonexistent-dir-xyz/file.zip"));
    }

    #[test]
    fn safe_path_stays_inside_dir() {
        let p = safe_save_path(Path::new("/tmp/dl"), "../evil.txt");
        assert!(p.starts_with("/tmp/dl"));
    }

    #[test]
    fn strip_torrent_suffix_cases() {
        assert_eq!(strip_torrent_suffix("ubuntu.iso.torrent"), "ubuntu.iso");
        assert_eq!(strip_torrent_suffix("Foo.TORRENT"), "Foo");
        assert_eq!(strip_torrent_suffix("file.Torrent"), "file");
        assert_eq!(strip_torrent_suffix("no-suffix"), "no-suffix");
        assert_eq!(strip_torrent_suffix(".torrent"), "torrent");
        assert_eq!(strip_torrent_suffix("a.torrentx"), "a.torrentx");
    }
}
