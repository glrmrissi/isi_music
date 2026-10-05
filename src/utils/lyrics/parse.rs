use super::types::{LyricLine, LyricsData};

pub(super) fn normalize_search_query(text: &str) -> String {
    text.split(&['(', '[', '-', '|'][..])
        .next()
        .unwrap_or(text)
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_lrc(lrc: &str) -> LyricsData {
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut has_timestamps = false;

    for raw in lrc.lines() {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }

        let mut rest = raw;
        let mut timestamps: Vec<u64> = Vec::new();

        while rest.starts_with('[') {
            let end = match rest.find(']') {
                Some(i) => i,
                None => break,
            };
            let tag = &rest[1..end];
            rest = &rest[end + 1..];

            if let Some(ms) = parse_timestamp(tag) {
                timestamps.push(ms);
            }
        }

        let text = rest.trim().to_string();
        if text.is_empty() {
            continue;
        }

        if !timestamps.is_empty() {
            has_timestamps = true;
            for ms in timestamps {
                lines.push(LyricLine {
                    time_ms: ms,
                    text: text.clone(),
                });
            }
        }
    }

    lines.sort_by_key(|l| l.time_ms);

    if has_timestamps && !lines.is_empty() {
        LyricsData {
            lines,
            is_synced: true,
        }
    } else {
        parse_plain(lrc)
    }
}

fn parse_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    let parts: Vec<&str> = s.split([':', '.']).collect();

    match parts.len() {
        2 => {
            let min: u64 = parts[0].parse().ok()?;
            let sec: u64 = parts[1].parse().ok()?;
            Some(min * 60_000 + sec * 1_000)
        }
        3 => {
            let min: u64 = parts[0].parse().ok()?;
            let sec: u64 = parts[1].parse().ok()?;
            let frac_str = parts[2];
            let frac_val: u64 = frac_str.parse().ok()?;
            let ms = match frac_str.len() {
                1 => frac_val * 100,
                2 => frac_val * 10,
                3 => frac_val,
                _ => frac_str[..3].parse().ok()?,
            };
            Some(min * 60_000 + sec * 1_000 + ms)
        }
        _ => None,
    }
}

pub(super) fn parse_plain(text: &str) -> LyricsData {
    let lines = text
        .lines()
        .map(|l| LyricLine {
            time_ms: 0,
            text: l.trim().to_string(),
        })
        .filter(|l| !l.text.is_empty())
        .collect();

    LyricsData {
        lines,
        is_synced: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timestamp_formats() {
        assert_eq!(parse_timestamp("01:23"), Some(83_000));
        assert_eq!(parse_timestamp("01:23.4"), Some(83_400));
        assert_eq!(parse_timestamp("01:23.45"), Some(83_450));
        assert_eq!(parse_timestamp("01:23.456"), Some(83_456));
        assert_eq!(parse_timestamp("00:00.000"), Some(0));
        assert_eq!(parse_timestamp("invalid"), None);
    }
}
