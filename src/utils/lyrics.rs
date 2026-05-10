use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tracing::warn;

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
        self.lines
            .iter()
            .rposition(|l| l.time_ms <= progress_ms)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

pub fn parse_lrc(lrc: &str) -> LyricsData {
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut has_timestamps = false;

    for raw in lrc.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
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

        if timestamps.is_empty() {
            continue;
        }

        has_timestamps = true;
        for ms in timestamps {
            lines.push(LyricLine {
                time_ms: ms,
                text: text.clone(),
            });
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
    let colon = s.find(':')?;
    let mm: u64 = s[..colon].trim().parse().ok()?;

    let rest = &s[colon + 1..];
    let (ss_str, cs): (&str, u64) = if let Some(dot) = rest.find('.') {
        let cs_str = &rest[dot + 1..];
        let cs: u64 = cs_str.parse().unwrap_or(0);
        let cs_ms = match cs_str.len() {
            1 => cs * 100,
            2 => cs * 10,
            3 => cs,
            _ => cs * 10,
        };
        (&rest[..dot], cs_ms)
    } else if let Some(c2) = rest.find(':') {
        let cs: u64 = rest[c2 + 1..].trim().parse().unwrap_or(0);
        (&rest[..c2], cs * 10)
    } else {
        (rest, 0)
    };

    let ss: u64 = ss_str.trim().parse().ok()?;
    Some(mm * 60_000 + ss * 1_000 + cs)
}

fn parse_plain(text: &str) -> LyricsData {
    let lines = text
        .lines()
        .map(|l| LyricLine {
            time_ms: 0,
            text: l.to_string(),
        })
        .filter(|l| !l.text.trim().is_empty())
        .collect();
    LyricsData {
        lines,
        is_synced: false,
    }
}

pub struct LyricsCache {
    conn: Connection,
}

impl LyricsCache {
    pub fn open(path: &PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS lyrics_cache (
                uri         TEXT    PRIMARY KEY,
                lyrics_json TEXT    NOT NULL,
                is_synced   INTEGER DEFAULT 0,
                saved_at    INTEGER NOT NULL
            );",
        )?;
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

    pub fn save(&self, uri: &str, data: &LyricsData) {
        let json = match serde_json::to_string(data) {
            Ok(j) => j,
            Err(e) => {
                warn!("lyrics: failed to serialize: {e}");
                return;
            }
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO lyrics_cache (uri, lyrics_json, is_synced, saved_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![uri, json, data.is_synced as i32, now],
        ) {
            warn!("lyrics: cache write failed: {e}");
        }
    }
}

async fn fetch_from_lrclib(
    http: &reqwest::Client,
    track: &str,
    artist: &str,
) -> Option<LyricsData> {
    let url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        urlencoding::encode(artist),
        urlencoding::encode(track)
    );

    let resp = http
        .get(&url)
        .header("User-Agent", "isi-music/0.1 (https://github.com/your/repo)")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let json: serde_json::Value = resp.json().await.ok()?;

    if let Some(lrc) = json["syncedLyrics"].as_str().filter(|s| !s.is_empty()) {
        let parsed = parse_lrc(lrc);
        if !parsed.is_empty() {
            return Some(parsed);
        }
    }

    if let Some(plain) = json["plainLyrics"].as_str().filter(|s| !s.is_empty()) {
        return Some(parse_plain(plain));
    }

    None
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
}

impl LyricsHandle {
    pub fn new(db_path: PathBuf, http: reqwest::Client) -> Result<Self> {
        let cache = LyricsCache::open(&db_path)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(HandleInner::default())),
            cache: Arc::new(Mutex::new(cache)),
            http,
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

        tokio::spawn(async move {
            let result = fetch_from_lrclib(&http, &title, &artist).await;
            if let Some(ref data) = result {
                if let Ok(c) = cache.lock() {
                    c.save(&uri, data);
                }
            }
            let _ = tx.send(result);
        });
    }

    pub fn take(&self) -> Option<LyricsData> {
        let mut inner = self.inner.lock().unwrap();
        let rx = inner.pending.as_mut()?;
        match rx.try_recv() {
            Ok(result) => {
                inner.pending = None;
                result
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => {
                inner.pending = None;
                None
            }
        }
    }

    pub fn is_loading(&self) -> bool {
        self.inner.lock().unwrap().pending.is_some()
    }
}