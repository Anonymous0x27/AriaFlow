//! Shared helpers: human-friendly formatting and parsing.

pub fn parse_bytes(input: &str) -> Result<u64, String> {
    let s = input.trim().to_ascii_uppercase().replace(' ', "");
    let (num, mult) = split_suffix(&s);
    let value: f64 = num
        .parse()
        .map_err(|_| format!("invalid size: {input:?}"))?;
    if value < 0.0 {
        return Err(format!("size must not be negative: {input:?}"));
    }
    Ok((value * mult as f64) as u64)
}

fn split_suffix(s: &str) -> (&str, u64) {
    for (suffix, mult) in [
        ("TIB", 1u64 << 40),
        ("GIB", 1 << 30),
        ("MIB", 1 << 20),
        ("KIB", 1 << 10),
    ] {
        if let Some(n) = s.strip_suffix(suffix) {
            return (n, mult);
        }
    }
    for (suffix, mult) in [
        ("TB", 1u64 << 40),
        ("GB", 1 << 30),
        ("MB", 1 << 20),
        ("KB", 1 << 10),
        ("T", 1 << 40),
        ("G", 1 << 30),
        ("M", 1 << 20),
        ("K", 1 << 10),
    ] {
        if let Some(n) = s.strip_suffix(suffix) {
            return (n, mult);
        }
    }
    (s, 1)
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn human_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", human_bytes(bytes_per_sec))
}

pub fn human_eta(secs: Option<u64>) -> String {
    match secs {
        None => "--".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        Some(s) => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

/// Expands a leading `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = dirs_home() {
            return home;
        }
    }
    std::path::PathBuf::from(path)
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
}

/// Prints a simple aligned table. Columns widths computed from content (ASCII).
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(cell.chars().count());
            }
        }
    }
    let fmt_line = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    let header_cells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    println!("{}", fmt_line(&header_cells));
    println!(
        "{}",
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("  ")
    );
    for row in rows {
        println!("{}", fmt_line(row));
    }
}

/// Parses a user-supplied instant for `--schedule`.
///
/// Accepts RFC 3339 (e.g. `2026-11-13T02:00:00+08:00`), `YYYY-MM-DD HH:MM`,
/// `YYYY-MM-DD HH:MM:SS`, `tomorrow HH:MM`, or `HH:MM` (today, or tomorrow if
/// that time has already passed today). Naive forms are interpreted in local
/// time. Returns seconds-precision UTC.
pub fn parse_schedule_instant(input: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    use chrono::{DateTime, Duration, Local, NaiveDateTime};

    let s = input.trim();

    // RFC 3339 with offset.
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }

    let now = Local::now();

    // Bare clock time ("02:30", "2:30"): today, or tomorrow if already past.
    if let Ok(t) = chrono::NaiveTime::parse_from_str(s, "%H:%M") {
        let today = now.date_naive();
        let naive = today.and_time(t);
        let candidate = local_single(naive)?;
        if candidate >= now {
            return Ok(candidate.with_timezone(&chrono::Utc));
        }
        let tomorrow = today + Duration::days(1);
        return Ok(local_single(tomorrow.and_time(t))?.with_timezone(&chrono::Utc));
    }

    // "tomorrow HH:MM" or "tomorrow HH:MM:SS".
    if let Some(rest) = s.strip_prefix("tomorrow") {
        let rest = rest.trim();
        for (fmt, len) in [("%H:%M", 5), ("%H:%M:%S", 8)] {
            if rest.len() == len {
                if let Ok(t) = chrono::NaiveTime::parse_from_str(rest, fmt) {
                    let day = now.date_naive() + Duration::days(1);
                    return Ok(local_single(day.and_time(t))?.with_timezone(&chrono::Utc));
                }
            }
        }
        return Err(format!(
            "unrecognised schedule time {input:?}; expected e.g. 'tomorrow 02:00'"
        ));
    }

    // Naive "YYYY-MM-DD HH:MM[:SS]" or "YYYY-MM-DDTHH:MM[:SS]": local time.
    for fmt in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(local_single(naive)?.with_timezone(&chrono::Utc));
        }
    }

    Err(format!(
        "unrecognised schedule time {input:?}; expected RFC 3339 (2026-11-13T02:00+08:00), 'YYYY-MM-DD HH:MM', 'tomorrow HH:MM', or 'HH:MM'"
    ))
}

/// Resolves a naive local datetime to a single instant, choosing the earliest
/// on DST "fall back" ambiguity and rejecting nonexistent times on "spring forward".
fn local_single(naive: chrono::NaiveDateTime) -> Result<chrono::DateTime<chrono::Local>, String> {
    use chrono::{Local, TimeZone};
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Ok(dt),
        chrono::LocalResult::Ambiguous(dt, _) => Ok(dt),
        chrono::LocalResult::None => Err(format!(
            "local time {naive} does not exist (DST transition); pick a different time"
        )),
    }
}

/// Parses "HH:MM" (24h) into minutes after local midnight.
pub fn parse_window_hhmm(input: &str) -> Result<u32, String> {
    let s = input.trim();
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| format!("invalid time {input:?}; expected HH:MM in 24h format"))?;
    let hour: u32 = h
        .parse()
        .map_err(|_| format!("invalid hour in {input:?}; expected HH:MM in 24h format"))?;
    let minute: u32 = m
        .parse()
        .map_err(|_| format!("invalid minute in {input:?}; expected HH:MM in 24h format"))?;
    if hour >= 24 || minute >= 60 {
        return Err(format!(
            "time {input:?} out of range; expected HH:MM in 24h format"
        ));
    }
    Ok(hour * 60 + minute)
}

/// Formats minutes after midnight as "HH:MM".
pub fn format_window_hhmm(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}
