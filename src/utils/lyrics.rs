use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::oneshot;
use crate::utils::debug_overlay::{DebugOverlay, LogLevel};
use tracing::{info, warn};

const LYRICS_CACHE_EXPIRY_DAYS: i64 = 60;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LyricsData {
    pub lines: Vec<LyricLine>,
    pub is_synced: bool,
}

impl LyricsData {
    pub fn active_idx(&self, progress_ms: u64) -> Option<usize> {
        if !self.is_synced || self.lines.is_empty() {
            return None;
        }
        self.lines.iter().rposition(|l| l.time_ms <= progress_ms)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

pub struct LyricsCache {
    conn: Connection,
}

impl LyricsCache {
    pub fn open(db_path: &PathBuf) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS lyrics_cache (
                 uri TEXT PRIMARY KEY,
                 lyrics_json TEXT NOT NULL,
                 is_synced INTEGER NOT NULL DEFAULT 0,
                 saved_at INTEGER NOT NULL
             );",
        )?;

        let expiry = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs() as i64
            - (LYRICS_CACHE_EXPIRY_DAYS * 86400);

        let _ = conn.execute(
            "DELETE FROM lyrics_cache WHERE saved_at < ?1",
            params![expiry],
        );

        Ok(Self { conn })
    }

    pub fn get(&self, uri: &str) -> Option<LyricsData> {
        self.conn
            .query_row(
                "SELECT lyrics_json FROM lyrics_cache WHERE uri = ?1",
                params![uri],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
    }

    pub fn save(&self, uri: &str, data: &LyricsData, debug_overlay: &Arc<DebugOverlay>) {
        let json = match serde_json::to_string(data) {
            Ok(j) => j,
            Err(e) => {
                debug_overlay.log(LogLevel::Warn, format!("lyrics: failed to serialize: {e}"));
                warn!("lyrics: failed to serialize: {e}");
                return;
            }
        };

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO lyrics_cache
             (uri, lyrics_json, is_synced, saved_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![uri, json, data.is_synced as i32, now],
        ) {
            debug_overlay.log(LogLevel::Warn, format!("lyrics: failed to save cache: {e}"));
            warn!("lyrics: failed to save cache: {e}");
        } else {
            debug_overlay.log(LogLevel::Info, format!("lyrics: saved to cache -> {}", uri));
            info!("lyrics: saved to cache -> {}", uri);
        }
    }
}

fn normalize_search_query(text: &str) -> String {
    text
        .split(&['(', '[', '-', '|'][..])
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
            let cs: u64 = parts[2].parse().ok()?;
            let ms = if parts[2].len() == 1 { cs * 100 } else { cs * 10 };
            Some(min * 60_000 + sec * 1_000 + ms)
        }
        _ => None,
    }
}

fn parse_plain(text: &str) -> LyricsData {
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

async fn fetch_lyrics(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
) -> Option<LyricsData> {
    let normalized_title = normalize_search_query(title);
    let normalized_artist = normalize_search_query(artist);

    if let Some(lyrics) = fetch_from_lrclib(http, &normalized_title, &normalized_artist, debug_overlay).await {
        return Some(lyrics);
    }

    if let Some(lyrics) = fetch_from_musixmatch(http, &normalized_title, &normalized_artist, debug_overlay).await {
        return Some(lyrics);
    }

    info!("lyrics: all synced APIs failed, trying fallback -> lyrics.ovh");
    debug_overlay.log(
        LogLevel::Info,
        "lyrics: all synced APIs failed, trying fallback -> lyrics.ovh",
    );
    fetch_from_ovh(http, &normalized_title, &normalized_artist, debug_overlay).await
}

async fn fetch_from_lrclib(
    http: &reqwest::Client,
    track: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
) -> Option<LyricsData> {
    let url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        urlencoding::encode(artist),
        urlencoding::encode(track)
    );

    info!("lyrics: fetching from lrclib -> {} - {}", artist, track);
    debug_overlay.log(
        LogLevel::Info,
        format!("lyrics: fetching from lrclib -> {} - {}", artist, track),
    );

    for attempt in 1..=2 {
        let resp = match tokio::time::timeout(
            Duration::from_secs(10),
            http.get(&url)
                .header("User-Agent", "isi-music/0.1[](https://github.com/glrmrissi/isi-music)")
                .send(),
        )
        .await
        {
            Ok(Ok(r)) if r.status().is_success() => r,
            Ok(Ok(r)) => {
                if attempt == 2 {
                    debug_overlay.log(
                        LogLevel::Warn,
                        format!("lyrics: lrclib returned status {} (final)", r.status()),
                    );
                }
                continue;
            }
            Ok(Err(e)) => {
                if attempt == 2 {
                    debug_overlay.log(LogLevel::Warn, format!("lyrics: lrclib request failed: {e}"));
                }
                continue;
            }
            Err(_) => {
                if attempt == 2 {
                    debug_overlay.log(LogLevel::Warn, "lyrics: lrclib request timed out");
                }
                continue;
            }
        };

        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                if attempt == 2 {
                    debug_overlay.log(LogLevel::Warn, format!("lyrics: json parse error from lrclib: {e}"));
                }
                continue;
            }
        };

        if let Some(lrc) = json["syncedLyrics"].as_str().filter(|s| !s.is_empty()) {
            let parsed = parse_lrc(lrc);
            if !parsed.is_empty() {
                debug_overlay.log(
                    LogLevel::Info,
                    format!("lyrics: synced lyrics found from lrclib ({} lines)", parsed.lines.len()),
                );
                info!("lyrics: synced lyrics found ({} lines)", parsed.lines.len());
                return Some(parsed);
            }
        }

        if let Some(plain) = json["plainLyrics"].as_str().filter(|s| !s.is_empty()) {
            let parsed = parse_plain(plain);
            debug_overlay.log(
                LogLevel::Info,
                "lyrics: plain lyrics found from lrclib".to_string(),
            );
            info!("lyrics: plain lyrics found from lrclib");
            return Some(parsed);
        }

        return None;
    }

    None
}

#[derive(Debug, Deserialize)]
struct MusixmatchResponse {
    message: MusixmatchMessage,
}

#[derive(Debug, Deserialize)]
struct MusixmatchMessage {
    body: MusixmatchBody,
}

#[derive(Debug, Deserialize)]
struct MusixmatchBody {
    #[serde(default)]
    track_list: Vec<MusixmatchTrack>,
}

#[derive(Debug, Deserialize)]
struct MusixmatchTrack {
    track: TrackData,
}

#[derive(Debug, Deserialize)]
struct TrackData {
    track_id: i32,
    track_name: String,
    artist_name: String,
    has_lyrics: bool,
}

async fn fetch_from_musixmatch(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
) -> Option<LyricsData> {
    debug_overlay.log(
        LogLevel::Info,
        format!("lyrics: fetching from musixmatch -> {} - {}", artist, title),
    );

    let search_url = format!(
        "https://api.musixmatch.com/ws/1.1/track.search?q_track={}&q_artist={}&f_has_lyrics=true&apikey={}",
        urlencoding::encode(title),
        urlencoding::encode(artist),
        "c38d4b3f87bd1dac158c1537cf3e5e28"
    );

    let search_resp = match tokio::time::timeout(Duration::from_secs(5), http.get(&search_url).send()).await {
        Ok(Ok(r)) if r.status().is_success() => r,
        Ok(Ok(r)) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: musixmatch search returned status {}", r.status()),
            );
            return None;
        }
        Ok(Err(e)) => {
            debug_overlay.log(LogLevel::Warn, format!("lyrics: musixmatch request failed: {e}"));
            return None;
        }
        Err(_) => {
            debug_overlay.log(LogLevel::Warn, "lyrics: musixmatch request timed out");
            return None;
        }
    };

    let search_json: MusixmatchResponse = match search_resp.json().await {
        Ok(j) => j,
        Err(e) => {
            debug_overlay.log(LogLevel::Warn, format!("lyrics: musixmatch json parse error: {e}"));
            return None;
        }
    };

    let track = search_json.message.body.track_list.first()?;
    if !track.track.has_lyrics {
        debug_overlay.log(
            LogLevel::Warn,
            format!("lyrics: track {} has no lyrics on musixmatch", track.track.track_name),
        );
        return None;
    }

    let track_id = track.track.track_id;
    let lyrics_url = format!(
        "https://api.musixmatch.com/ws/1.1/track.lyrics.get?track_id={}&apikey={}",
        track_id,
        "c38d4b3f87bd1dac158c1537cf3e5e28"
    );

    let lyrics_resp = match tokio::time::timeout(Duration::from_secs(5), http.get(&lyrics_url).send()).await {
        Ok(Ok(r)) if r.status().is_success() => r,
        _ => return None,
    };

    let lyrics_json: serde_json::Value = match lyrics_resp.json().await {
        Ok(j) => j,
        Err(_) => return None,
    };

    if let Some(lyrics_text) = lyrics_json["message"]["body"]["lyrics"]["lyrics_body"].as_str() {
        let cleaned = lyrics_text
            .lines()
            .filter(|l| !l.contains("****") && !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        if !cleaned.is_empty() {
            let parsed = parse_plain(&cleaned);
            debug_overlay.log(
                LogLevel::Info,
                format!("lyrics: found from musixmatch ({} lines)", parsed.lines.len()),
            );
            return Some(parsed);
        }
    }

    None
}

async fn fetch_from_ovh(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
) -> Option<LyricsData> {
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );

    info!("lyrics: fetching from lyrics.ovh -> {} - {}", artist, title);
    debug_overlay.log(
        LogLevel::Info,
        format!("lyrics: fetching from lyrics.ovh -> {} - {}", artist, title),
    );

    let resp = match tokio::time::timeout(Duration::from_secs(6), http.get(&url).send()).await {
        Ok(Ok(r)) if r.status().is_success() => r,
        Ok(Ok(r)) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: lyrics.ovh returned status {}", r.status()),
            );
            return None;
        }
        Ok(Err(e)) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: lyrics.ovh request failed: {e}"),
            );
            return None;
        }
        Err(_) => {
            debug_overlay.log(LogLevel::Warn, "lyrics: lyrics.ovh request timed out");
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            debug_overlay.log(LogLevel::Warn, format!("lyrics: json parse error from lyrics.ovh: {e}"));
            return None;
        }
    };

    let plain = match json["lyrics"].as_str() {
        Some(l) if !l.is_empty() => l,
        _ => {
            debug_overlay.log(
                LogLevel::Warn,
                "lyrics: lyrics.ovh returned empty or no lyrics field",
            );
            return None;
        }
    };

    let parsed = parse_plain(plain);
    if !parsed.is_empty() {
        debug_overlay.log(
            LogLevel::Info,
            format!(
                "lyrics: plain lyrics found from lyrics.ovh ({} lines)",
                parsed.lines.len()
            ),
        );
        info!(
            "lyrics: plain lyrics found from lyrics.ovh ({} lines)",
            parsed.lines.len()
        );
        Some(parsed)
    } else {
        debug_overlay.log(
            LogLevel::Warn,
            "lyrics: received empty lyrics from lyrics.ovh",
        );
        None
    }
}

#[derive(Default)]
struct HandleInner {
    last_uri: String,
    pending: Option<oneshot::Receiver<Option<LyricsData>>>,
}

#[derive(Clone)]
pub struct LyricsHandle {
    inner: Arc<Mutex<HandleInner>>,
    cache: Arc<Mutex<LyricsCache>>,
    http: reqwest::Client,
    debug_overlay: Arc<DebugOverlay>,
}

impl LyricsHandle {
    pub fn new(db_path: PathBuf, http: reqwest::Client, debug_overlay: Arc<DebugOverlay>) -> Result<Self> {
        let cache = LyricsCache::open(&db_path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(HandleInner::default())),
            cache: Arc::new(Mutex::new(cache)),
            http,
            debug_overlay,
        })
    }

    pub fn request(&self, title: &str, artist: &str, uri: &str) {
        let mut inner = self.inner.lock().unwrap();
        if inner.last_uri == uri {
            return;
        }

        inner.last_uri = uri.to_string();
        inner.pending = None;

        if let Ok(cache) = self.cache.lock() {
            if let Some(cached) = cache.get(uri) {
                self.debug_overlay.log(
                    LogLevel::Info,
                    format!("lyrics: found in cache -> {}", uri),
                );
                let (tx, rx) = oneshot::channel();
                let _ = tx.send(Some(cached));
                inner.pending = Some(rx);
                return;
            }
        }

        let (tx, rx) = oneshot::channel();
        inner.pending = Some(rx);

        let http = self.http.clone();
        let cache = Arc::clone(&self.cache);
        let title = title.to_string();
        let artist = artist.to_string();
        let uri = uri.to_string();
        let debug_overlay = self.debug_overlay.clone();

        tokio::spawn(async move {
            let result = fetch_lyrics(&http, &title, &artist, &debug_overlay).await;
            if let Some(ref data) = result {
                if let Ok(c) = cache.lock() {
                    c.save(&uri, data, &debug_overlay);
                }
            } else {
                debug_overlay.log(
                    LogLevel::Warn,
                    format!("lyrics: could not fetch lyrics for {} - {}", artist, title),
                );
            }
            let _ = tx.send(result);
        });
    }

    pub fn poll(&self) -> Option<LyricsData> {
        let mut inner = self.inner.lock().unwrap();
        let rx = inner.pending.as_mut()?;

        match rx.try_recv() {
            Ok(result) => {
                inner.pending = None;
                result
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(_) => {
                inner.pending = None;
                None
            }
        }
    }

    pub fn is_loading(&self) -> bool {
        self.inner.lock().unwrap().pending.is_some()
    }
}