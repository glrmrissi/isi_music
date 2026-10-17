use std::io::Write;
use std::path::{Path, PathBuf};

use super::parse::parse_lrc;
use super::types::LyricsData;

fn uri_hash(uri: &str) -> String {
    format!("{:x}", md5::compute(uri.as_bytes()))
}

pub(super) fn lyrics_cache_path(uri: &str) -> Option<PathBuf> {
    let dir = crate::config::lyrics_cache_dir().ok()?;
    Some(dir.join(format!("{}.lrc", uri_hash(uri))))
}

pub(super) fn local_lrc_path(uri: &str) -> Option<PathBuf> {
    let path = if let Some(s) = uri.strip_prefix("file://") {
        PathBuf::from(s)
    } else if uri.starts_with("file:") {
        PathBuf::from(uri.strip_prefix("file:")?)
    } else if !uri.starts_with("spotify:")
        && !uri.starts_with("http:")
        && !uri.starts_with("https:")
    {
        PathBuf::from(uri)
    } else {
        return None;
    };

    // Try /music/song.mp3 -> /music/song.lrc
    let stem = path.file_stem()?;
    let mut lrc = path.clone();
    lrc.set_file_name(stem);
    lrc.set_extension("lrc");
    Some(lrc)
}

pub(super) fn load_lrc_file(path: &Path) -> Option<LyricsData> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(parse_lrc(&text))
}

fn format_lrc(data: &LyricsData) -> String {
    if data.is_synced && !data.lines.is_empty() {
        data.lines
            .iter()
            .map(|l| {
                let total_ms = l.time_ms;
                let min = total_ms / 60_000;
                let sec = (total_ms % 60_000) / 1000;
                let cs = (total_ms % 1000) / 10;
                format!("[{:02}:{:02}.{:02}]{}", min, sec, cs, l.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        data.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(super) fn save_lrc_file(path: &Path, data: &LyricsData) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = f.write_all(format_lrc(data).as_bytes());
    }
}
