use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::spawn_blocking;
use tracing::{info, warn};

use crate::config;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheStats {
    pub search_cache_entries: usize,
    pub library_cache_entries: usize,
    pub lyrics_cache_entries: usize,
    pub search_cache_size: u64,
    pub library_cache_size: u64,
    pub lyrics_cache_size: u64,
    pub last_cleanup: Option<u64>,
}

pub struct CacheManager {
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
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

impl CacheManager {
    pub fn new() -> anyhow::Result<Self> {
        let db_path = config::get_local_db_path();
        Self::new_with_path(&db_path)
    }

    pub fn new_with_path(db_path: &str) -> anyhow::Result<Self> {
        let options = CacheOptions::default();
        let conn = rusqlite::Connection::open(db_path)
            .with_context(|| format!("failed to open cache db at {}", db_path))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )
        .context("failed to set pragmas")?;

        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
            options,
        })
    }

    pub async fn clear_search(&self) -> Result<()> {
        let conn = self.conn.clone();
        spawn_blocking(move || {
            let Ok(conn) = conn.lock() else { return };
            let _ = conn
                .execute("DELETE FROM search_cache", [])
                .unwrap_or_else(|e| {
                    warn!("Failed to clear search cache: {e}");
                    0
                });
        });

        info!("Search cache cleared");
        Ok(())
    }

    pub async fn clear_library(&self) -> Result<()> {
        let conn = self.conn.clone();
        spawn_blocking(move || {
            let Ok(conn) = conn.lock() else { return };
            let _ = conn
                .execute("DELETE FROM library_cache", [])
                .unwrap_or_else(|e| {
                    warn!("Failed to clear library cache: {e}");
                    0
                });
        });

        info!("Library cache cleared");
        Ok(())
    }

    pub async fn clear_lyrics(&self) -> Result<()> {
        let conn = self.conn.clone();
        spawn_blocking(move || {
            let Ok(conn) = conn.lock() else { return };
            let _ = conn
                .execute("DELETE FROM lyrics_cache", [])
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
        let conn = self.conn.clone();
        spawn_blocking(move || {
            let Ok(conn) = conn.lock() else {
                return CacheStats {
                    search_cache_entries: 0,
                    library_cache_entries: 0,
                    lyrics_cache_entries: 0,
                    search_cache_size: 0,
                    library_cache_size: 0,
                    lyrics_cache_size: 0,
                    last_cleanup: None,
                };
            };

            let search_entries: usize = conn
                .query_row("SELECT COUNT(*) FROM search_cache", [], |r| r.get(0))
                .unwrap_or(0);

            let library_entries: usize = conn
                .query_row("SELECT COUNT(*) FROM library_cache", [], |r| r.get(0))
                .unwrap_or(0);

            let lyrics_entries: usize = conn
                .query_row("SELECT COUNT(*) FROM lyrics_cache", [], |r| r.get(0))
                .unwrap_or(0);

            CacheStats {
                search_cache_entries: search_entries,
                library_cache_entries: library_entries,
                lyrics_cache_entries: lyrics_entries,
                search_cache_size: 0,
                library_cache_size: 0,
                lyrics_cache_size: 0,
                last_cleanup: None,
            }
        })
        .await
        .unwrap_or(CacheStats {
            search_cache_entries: 0,
            library_cache_entries: 0,
            lyrics_cache_entries: 0,
            search_cache_size: 0,
            library_cache_size: 0,
            lyrics_cache_size: 0,
            last_cleanup: None,
        })
    }

    pub async fn cleanup_expired(&self) -> Result<()> {
        let keep_seconds = self.options.keep_days * 24 * 3600;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let conn = self.conn.clone();
        spawn_blocking(move || {
            let Ok(conn) = conn.lock() else { return };
            let _ = conn.execute(
                "DELETE FROM search_cache WHERE (?1 - saved_at) >= ?2",
                rusqlite::params![now as i64, keep_seconds as i64],
            );
            let _ = conn.execute(
                "DELETE FROM lyrics_cache WHERE (?1 - saved_at) >= ?2",
                rusqlite::params![now as i64, keep_seconds as i64],
            );
        });

        info!("Cache cleanup completed");
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/utils/cache.rs"]
mod tests;
