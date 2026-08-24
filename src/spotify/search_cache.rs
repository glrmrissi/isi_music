use anyhow::Context;
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::types::{AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, TrackSummary};

pub(super) const MAX_SEARCH_CACHE_ENTRIES: usize = 32;

#[derive(Clone)]
pub(super) struct SearchCache {
    pub(super) store: Arc<RwLock<HashMap<String, (Instant, FullSearchResults)>>>,
    pub(super) ttl: Duration,
    pub(super) conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
}

impl SearchCache {
    pub(super) fn new(ttl_seconds: u64) -> anyhow::Result<Self> {
        let ttl = Duration::from_secs(ttl_seconds);

        let conn = if cfg!(test) {
            rusqlite::Connection::open_in_memory()
        } else {
            let db_path = crate::config::get_local_db_path();
            rusqlite::Connection::open(&db_path)
        }
        .context("failed to open search cache db")?;

        let init_sql = if cfg!(test) {
            "CREATE TABLE IF NOT EXISTS search_cache (
                 key      TEXT PRIMARY KEY,
                 data     TEXT NOT NULL,
                 saved_at INTEGER NOT NULL
             );"
        } else {
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=1000;
             CREATE TABLE IF NOT EXISTS search_cache (
                 key      TEXT PRIMARY KEY,
                 data     TEXT NOT NULL,
                 saved_at INTEGER NOT NULL
             );"
        };
        conn.execute_batch(init_sql)
            .context("failed to init search cache schema")?;

        let preloaded = Self::load_from_db_sync(&conn, ttl).unwrap_or_else(|e| {
            warn!("Search cache: could not load from disk: {e}");
            HashMap::new()
        });

        Ok(Self {
            store: Arc::new(RwLock::new(preloaded)),
            ttl,
            conn: Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    fn unix_now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    fn load_from_db_sync(
        conn: &rusqlite::Connection,
        ttl: Duration,
    ) -> anyhow::Result<HashMap<String, (Instant, FullSearchResults)>> {
        let ttl_secs = ttl.as_secs() as i64;
        let now = Self::unix_now();

        conn.execute(
            "DELETE FROM search_cache WHERE (? - saved_at) >= ?",
            params![now, ttl_secs],
        )?;

        let mut stmt = conn.prepare(
            "SELECT key, data FROM search_cache
             ORDER BY saved_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![MAX_SEARCH_CACHE_ENTRIES as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            let (key, data) = row;
            if let Ok(cached) = serde_json::from_str::<CachedSearch>(&data) {
                map.insert(key, (Instant::now(), FullSearchResults::from(cached)));
            }
        }
        info!("Search cache: loaded {} entries from disk", map.len());
        Ok(map)
    }

    pub(super) async fn get(&self, key: &str) -> Option<FullSearchResults> {
        let mut guard = self.store.write().await;
        let expired = guard
            .get(key)
            .map(|(ts, _)| ts.elapsed() >= self.ttl)
            .unwrap_or(false);
        if expired {
            guard.remove(key);
            return None;
        }
        guard.get(key).map(|(_, results)| results.clone())
    }

    pub(super) async fn insert(&self, key: String, results: FullSearchResults) {
        let mut guard = self.store.write().await;
        guard.retain(|_, (ts, _)| ts.elapsed() < self.ttl);
        if guard.len() >= MAX_SEARCH_CACHE_ENTRIES && !guard.contains_key(&key) {
            let oldest_key = guard
                .iter()
                .min_by_key(|(_, (ts, _))| *ts)
                .map(|(key, _)| key.clone());
            if let Some(oldest_key) = oldest_key {
                guard.remove(&oldest_key);
            }
        }
        guard.insert(key.clone(), (Instant::now(), results.clone()));
        drop(guard);

        let conn = self.conn.clone();
        let now = Self::unix_now();
        let cached: CachedSearch = results.into();
        tokio::task::spawn_blocking(move || {
            let Ok(data) = serde_json::to_string(&cached) else {
                return;
            };
            let Ok(conn) = conn.lock() else {
                return;
            };
            let _ = conn.execute(
                "INSERT OR REPLACE INTO search_cache (key, data, saved_at) VALUES (?1, ?2, ?3)",
                params![key, data, now],
            );
            let _ = conn.execute(
                "DELETE FROM search_cache
                 WHERE key NOT IN (
                     SELECT key FROM search_cache ORDER BY saved_at DESC LIMIT ?1
                 )",
                params![MAX_SEARCH_CACHE_ENTRIES as i64],
            );
        });
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CachedSearch {
    tracks: Vec<CachedTrack>,
    artists: Vec<CachedArtist>,
    albums: Vec<CachedAlbum>,
    playlists: Vec<CachedPlaylist>,
    tracks_total: u32,
    artists_total: u32,
    albums_total: u32,
    playlists_total: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CachedTrack {
    pub(super) name: String,
    pub(super) artist: String,
    pub(super) album: String,
    pub(super) duration_ms: u64,
    pub(super) uri: String,
    pub(super) cover_path: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CachedArtist {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) uri: String,
    pub(super) genres: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CachedAlbum {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) artist: String,
    pub(super) uri: String,
    pub(super) total_tracks: u32,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CachedPlaylist {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) uri: String,
    pub(super) total_tracks: u32,
    pub(super) art_url: Option<String>,
}

impl From<FullSearchResults> for CachedSearch {
    fn from(r: FullSearchResults) -> Self {
        Self {
            tracks: r
                .tracks
                .into_iter()
                .map(|t| CachedTrack {
                    name: t.name,
                    artist: t.artist,
                    album: t.album,
                    duration_ms: t.duration_ms,
                    uri: t.uri,
                    cover_path: t.cover_path,
                })
                .collect(),
            artists: r
                .artists
                .into_iter()
                .map(|a| CachedArtist {
                    id: a.id,
                    name: a.name,
                    uri: a.uri,
                    genres: a.genres,
                })
                .collect(),
            albums: r
                .albums
                .into_iter()
                .map(|a| CachedAlbum {
                    id: a.id,
                    name: a.name,
                    artist: a.artist,
                    uri: a.uri,
                    total_tracks: a.total_tracks,
                })
                .collect(),
            playlists: r
                .playlists
                .into_iter()
                .map(|p| CachedPlaylist {
                    id: p.id,
                    name: p.name,
                    uri: p.uri,
                    total_tracks: p.total_tracks,
                    art_url: p.art_url,
                })
                .collect(),
            tracks_total: r.tracks_total,
            artists_total: r.artists_total,
            albums_total: r.albums_total,
            playlists_total: r.playlists_total,
        }
    }
}

impl From<CachedSearch> for FullSearchResults {
    fn from(c: CachedSearch) -> Self {
        Self {
            tracks: c
                .tracks
                .into_iter()
                .map(|t| TrackSummary {
                    name: t.name,
                    artist: t.artist,
                    album: t.album,
                    duration_ms: t.duration_ms,
                    uri: t.uri,
                    cover_path: t.cover_path,
                    added_at: None,
                })
                .collect(),
            artists: c
                .artists
                .into_iter()
                .map(|a| ArtistSummary {
                    id: a.id,
                    name: a.name,
                    uri: a.uri,
                    genres: a.genres,
                })
                .collect(),
            albums: c
                .albums
                .into_iter()
                .map(|a| AlbumSummary {
                    id: a.id,
                    name: a.name,
                    artist: a.artist,
                    uri: a.uri,
                    total_tracks: a.total_tracks,
                })
                .collect(),
            playlists: c
                .playlists
                .into_iter()
                .map(|p| PlaylistSummary {
                    id: p.id,
                    name: p.name,
                    uri: p.uri,
                    total_tracks: p.total_tracks,
                    art_url: p.art_url,
                })
                .collect(),
            tracks_total: c.tracks_total,
            artists_total: c.artists_total,
            albums_total: c.albums_total,
            playlists_total: c.playlists_total,
        }
    }
}
