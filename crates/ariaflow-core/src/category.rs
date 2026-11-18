//! Built-in download categories and extension-based classification.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinCategory {
    pub key: &'static str,
    pub label: &'static str,
    pub label_en: &'static str,
    pub extensions: &'static [&'static str],
}

pub const BUILTIN_CATEGORIES: &[BuiltinCategory] = &[
    BuiltinCategory {
        key: "documents",
        label: "文档",
        label_en: "Documents",
        extensions: &[
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "txt", "md",
            "rtf", "epub", "mobi", "csv", "key", "numbers", "pages",
        ],
    },
    BuiltinCategory {
        key: "images",
        label: "图片",
        label_en: "Images",
        extensions: &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "ico", "tiff", "heic", "psd", "ai",
            "eps",
        ],
    },
    BuiltinCategory {
        key: "videos",
        label: "视频",
        label_en: "Videos",
        extensions: &[
            "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp", "ts",
            "m2ts",
        ],
    },
    BuiltinCategory {
        key: "audio",
        label: "音频",
        label_en: "Audio",
        extensions: &[
            "mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus", "aiff", "ape", "alac",
        ],
    },
    BuiltinCategory {
        key: "archives",
        label: "压缩文件",
        label_en: "Archives",
        extensions: &[
            "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "iso", "dmg", "cab", "arj", "lzh",
        ],
    },
    BuiltinCategory {
        key: "installers",
        label: "软件安装包",
        label_en: "Installers",
        extensions: &[
            "exe", "msi", "msix", "appx", "deb", "rpm", "apk", "app", "pkg", "run", "bin", "jar",
            "dmg", "snap",
        ],
    },
];

/// Classifies a file name into a built-in category key. Falls back to `"other"`.
pub fn classify_by_filename(filename: &str) -> &'static str {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext {
        Some(ref ext) => BUILTIN_CATEGORIES
            .iter()
            .find(|c| c.extensions.contains(&ext.as_str()))
            .map(|c| c.key)
            .unwrap_or("other"),
        None => "other",
    }
}

/// Guesses a filename from the URL path when no filename is provided explicitly.
pub fn filename_from_url(url: &url::Url) -> Option<String> {
    let last = url.path_segments()?.rfind(|s| !s.is_empty())?;
    let name = percent_decode(last);
    if name.is_empty() {
        return None;
    }
    Some(name)
}

fn percent_decode(input: &str) -> String {
    // Minimal percent-decoder (no external dep): handles %XX and leaves '+' untouched.
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 {
            let h = bytes.get(i + 1).copied().and_then(hex_val);
            let l = bytes.get(i + 2).copied().and_then(hex_val);
            if let (Some(h), Some(l)) = (h, l) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The built-in classification rules as a versioned, machine-readable document.
/// Used by `GET /api/v1/categories` and `ariaflow categories export`.
pub fn category_rules_json() -> serde_json::Value {
    let categories: Vec<serde_json::Value> = BUILTIN_CATEGORIES
        .iter()
        .map(|c| {
            serde_json::json!({
                "key": c.key,
                "label": c.label,
                "labelEn": c.label_en,
                "extensions": c.extensions,
            })
        })
        .collect();
    serde_json::json!({
        "version": 1,
        "categories": categories,
        "fallback": "other",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_extensions() {
        assert_eq!(classify_by_filename("report.PDF"), "documents");
        assert_eq!(classify_by_filename("photo.jpeg"), "images");
        assert_eq!(classify_by_filename("movie.mkv"), "videos");
        assert_eq!(classify_by_filename("song.flac"), "audio");
        assert_eq!(classify_by_filename("archive.7z"), "archives");
        assert_eq!(classify_by_filename("setup.exe"), "installers");
        assert_eq!(classify_by_filename("file.unknownext"), "other");
        assert_eq!(classify_by_filename("no_extension"), "other");
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
    }

    #[test]
    fn filename_from_url_path() {
        let u = url::Url::parse("https://example.com/files/some%20file.zip?dl=1").unwrap();
        assert_eq!(filename_from_url(&u).as_deref(), Some("some file.zip"));
    }
}
