use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::task::spawn_blocking;
use tracing::{info, warn};

use crate::config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub search_cache_entries: usize,
    pub library_cache_entries: usize,
    pub lyrics_cache_entries: usize,
    pub search_cache_size: u64,
    pub library_cache_size: u64,
    pub lyrics_cache_size: u64,
    pub last_cleanup: Option<u64>, // Unix timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryCache {
    pub liked: Vec<LikedTrack>,
    pub playlists: HashMap<String, Vec<TrackSummary>>,
    pub albums: HashMap<String, Vec<TrackSummary>>,
    pub artists: HashMap<String, Vec<TrackSummary>>,
    pub shows: HashMap<String, Vec<TrackSummary>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LikedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u32,
    pub cover_path: Option<String>,
    pub saved_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSummary {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u32,
    pub cover_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSearch {
    pub tracks: Vec<CachedTrack>,
    pub artists: Vec<CachedArtist>,
    pub albums: Vec<CachedAlbum>,
    pub playlists: Vec<CachedPlaylist>,
    pub saved_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u32,
    pub cover_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedArtist {
    pub id: String,
    pub name: String,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAlbum {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub uri: String,
    pub total_tracks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPlaylist {
    pub id: String,
    pub name: String,
    pub uri: String,
    pub total_tracks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedLyrics {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub lyrics: LyricsData,
    pub saved_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricsData {
    pub is_synced: bool,
    pub lines: Vec<LyricLine>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub time: Option<i64>,
    pub text: String,
}

pub struct CacheManager {
    db_path: String,
    search_cache: Arc<RwLock<SearchCache>>,
    library_cache: Arc<Mutex<LibraryCache>>,
    lyrics_cache: Arc<RwLock<LyricsCache>>,
    options: CacheOptions,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CacheOptions {
    pub enabled: bool,
    pub auto_cleanup: bool,
    pub max_size_mb: u64,
    pub cleanup_interval_hours: u32,
    pub keep_days: u32,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_cleanup: true,
            max_size_mb: 500,
            cleanup_interval_hours: 24,
            keep_days: 60,
        }
    }
}

struct SearchCache {
    store: HashMap<String, (u64, CachedSearch)>, // key -> (timestamp, data)
}

struct LyricsCache {
    store: HashMap<String, CachedLyrics>,
}

impl CacheManager {
    pub fn new() -> Self {
        let db_path = config::get_local_db_path();
        Self::new_with_path(&db_path)
    }

    pub fn new_with_path(db_path: &str) -> Self {
        let options = CacheOptions::default();

        Self {
            db_path: db_path.to_string(),
            search_cache: Arc::new(RwLock::new(SearchCache {
                store: HashMap::new(),
            })),
            library_cache: Arc::new(Mutex::new(LibraryCache {
                liked: Vec::new(),
                playlists: HashMap::new(),
                albums: HashMap::new(),
                artists: HashMap::new(),
                shows: HashMap::new(),
            })),
            lyrics_cache: Arc::new(RwLock::new(LyricsCache {
                store: HashMap::new(),
            })),
            options,
        }
    }

    fn unix_now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub async fn clear_search(&self) -> Result<()> {
        let mut search_guard = self.search_cache.write().await;
        search_guard.store.clear();

        let db_path = self.db_path.clone();
        spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("DELETE FROM search_cache", [])
                .unwrap_or_else(|e| {
                    warn!("Failed to clear search cache: {e}");
                    0
                });
        });

        info!("Search cache cleared");
        Ok(())
    }

    pub async fn clear_library(&self) -> Result<()> {
        let mut lib_guard = self.library_cache.lock().unwrap();
        lib_guard.liked.clear();
        lib_guard.playlists.clear();
        lib_guard.albums.clear();
        lib_guard.artists.clear();
        lib_guard.shows.clear();

        let db_path = self.db_path.clone();
        spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("DELETE FROM library_cache", [])
                .unwrap_or_else(|e| {
                    warn!("Failed to clear library cache: {e}");
                    0
                });
        });

        info!("Library cache cleared");
        Ok(())
    }

    pub async fn clear_lyrics(&self) -> Result<()> {
        let mut lyrics_guard = self.lyrics_cache.write().await;
        lyrics_guard.store.clear();

        let db_path = self.db_path.clone();
        spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("DELETE FROM lyrics_cache", [])
                .unwrap_or_else(|e| {
                    warn!("Failed to clear lyrics cache: {e}");
                    0
                });
        });

        info!("Lyrics cache cleared");
        Ok(())
    }

    pub async fn clear_all(&self) -> Result<()> {
        self.clear_search().await?;
        self.clear_library().await?;
        self.clear_lyrics().await?;

        info!("All caches cleared");
        Ok(())
    }

    pub async fn get_stats(&self) -> CacheStats {
        let search_guard = self.search_cache.read().await;
        let lyrics_guard = self.lyrics_cache.read().await;
        let lib_guard = self.library_cache.lock().unwrap();

        let now = Self::unix_now();
        let keep_seconds = self.options.keep_days * 24 * 3600;

        let search_entries = search_guard
            .store
            .values()
            .filter(|(ts, _)| now - (*ts as u64) < keep_seconds.into())
            .count();

        let lyrics_entries = lyrics_guard
            .store
            .values()
            .filter(|l| now - l.saved_at < keep_seconds.into())
            .count();

        let library_entries = lib_guard.liked.len()
            + lib_guard.playlists.values().map(|v| v.len()).sum::<usize>()
            + lib_guard.albums.values().map(|v| v.len()).sum::<usize>()
            + lib_guard.artists.values().map(|v| v.len()).sum::<usize>()
            + lib_guard.shows.values().map(|v| v.len()).sum::<usize>();

        CacheStats {
            search_cache_entries: search_entries,
            library_cache_entries: library_entries,
            lyrics_cache_entries: lyrics_entries,
            search_cache_size: 0,
            library_cache_size: 0,
            lyrics_cache_size: 0,
            last_cleanup: None,
        }
    }

    pub async fn cleanup_expired(&self) -> Result<()> {
        let now = Self::unix_now();
        let keep_seconds = self.options.keep_days * 24 * 3600;

        // Clean search cache
        {
            let mut search_guard = self.search_cache.write().await;
            search_guard
                .store
                .retain(|_, (ts, _)| now - (*ts as u64) < keep_seconds.into());
        }

        // Clean lyrics cache
        {
            let mut lyrics_guard = self.lyrics_cache.write().await;
            lyrics_guard
                .store
                .retain(|_, l| now - l.saved_at < keep_seconds.into());
        }

        info!("Cache cleanup completed");
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/utils/cache.rs"]
mod tests;
