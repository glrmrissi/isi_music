use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
    cleanup_running: Arc<AtomicBool>,
    options: CacheOptions,
}

impl Clone for CacheManager {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            cleanup_running: Arc::clone(&self.cleanup_running),
            options: self.options.clone(),
        }
    }
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

struct CleanupGuard(Arc<AtomicBool>);

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn referenced_cover_paths(conn: &rusqlite::Connection) -> Option<HashSet<PathBuf>> {
    let mut stmt = conn
        .prepare("SELECT cover_path FROM tracks WHERE cover_path IS NOT NULL")
        .ok()?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0)).ok()?;
    rows.map(|row| row.ok().map(PathBuf::from)).collect()
}

fn prune_generated_cache_files(
    cache_root: &Path,
    cutoff: SystemTime,
    referenced_covers: Option<&HashSet<PathBuf>>,
) -> Result<()> {
    for (directory, extension) in [("covers", "jpg"), ("waveforms", "bin")] {
        let path = cache_root.join(directory);
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
        };

        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read an entry in {}", path.display()))?;
            let file_type = entry.file_type().with_context(|| {
                format!("failed to inspect cache file {}", entry.path().display())
            })?;
            if !file_type.is_file()
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some(extension)
                || (extension == "jpg"
                    && referenced_covers.is_none_or(|covers| covers.contains(&entry.path())))
            {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
                continue;
            };
            if modified < cutoff {
                std::fs::remove_file(entry.path())
                    .with_context(|| format!("failed to remove {}", entry.path().display()))?;
            }
        }
    }
    Ok(())
}

impl CacheManager {
    pub fn new(cfg: &config::AppConfig) -> anyhow::Result<Self> {
        let db_path = if cfg!(test) {
            ":memory:".to_string()
        } else {
            config::get_local_db_path()
        };
        Self::new_with_path_and_config(&db_path, cfg)
    }

    #[allow(dead_code)]
    pub fn new_with_path(db_path: &str) -> anyhow::Result<Self> {
        let options = CacheOptions::default();
        Self::new_with_options(db_path, options)
    }

    pub fn new_with_path_and_config(
        db_path: &str,
        cfg: &config::AppConfig,
    ) -> anyhow::Result<Self> {
        let options = CacheOptions {
            enabled: cfg.cache.enabled.unwrap_or(true),
            auto_cleanup: cfg.cache.auto_cleanup.unwrap_or(true),
            max_size_mb: cfg.cache.max_size_mb.unwrap_or(500),
            cleanup_interval_hours: cfg.cache.cleanup_interval_hours.unwrap_or(24).max(1),
            keep_days: cfg.cache.keep_days.unwrap_or(60),
        };
        Self::new_with_options(db_path, options)
    }

    fn new_with_options(db_path: &str, options: CacheOptions) -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open(db_path)
            .with_context(|| format!("failed to open cache db at {}", db_path))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
        )
        .context("failed to set pragmas")?;

        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
            cleanup_running: Arc::new(AtomicBool::new(false)),
            options,
        })
    }

    pub fn auto_cleanup_enabled(&self) -> bool {
        self.options.enabled && self.options.auto_cleanup
    }

    pub fn cleanup_interval_hours(&self) -> u32 {
        self.options.cleanup_interval_hours.max(1)
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
        if !self.options.enabled || self.cleanup_running.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        let keep_seconds = u64::from(self.options.keep_days).saturating_mul(86_400);
        let keep_seconds_sql = i64::try_from(keep_seconds).unwrap_or(i64::MAX);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let max_bytes = self.options.max_size_mb.saturating_mul(1024 * 1024);
        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(keep_seconds))
            .unwrap_or(UNIX_EPOCH);
        let cache_root = (!cfg!(test)).then(|| {
            dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("isi-music")
        });
        let conn = Arc::clone(&self.conn);
        let cleanup_running = Arc::clone(&self.cleanup_running);

        spawn_blocking(move || {
            let _guard = CleanupGuard(cleanup_running);
            let Ok(conn) = conn.lock() else { return };
            let _ = conn.execute(
                "DELETE FROM search_cache WHERE (?1 - saved_at) >= ?2",
                rusqlite::params![now as i64, keep_seconds_sql],
            );
            let _ = conn.execute(
                "DELETE FROM lyrics_cache WHERE (?1 - saved_at) >= ?2",
                rusqlite::params![now as i64, keep_seconds_sql],
            );
            let _ = conn.execute(
                "DELETE FROM library_cache WHERE (?1 - saved_at) >= ?2",
                rusqlite::params![now as i64, crate::spotify::LIBRARY_CACHE_TTL_SECS],
            );

            let db_size: i64 = conn
                .query_row(
                    "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if db_size as u64 > max_bytes {
                let _ = conn.execute(
                    "DELETE FROM search_cache WHERE rowid IN (SELECT rowid FROM search_cache ORDER BY saved_at ASC LIMIT (SELECT COUNT(*) / 4 FROM search_cache))",
                    [],
                );
                let _ = conn.execute(
                    "DELETE FROM lyrics_cache WHERE rowid IN (SELECT rowid FROM lyrics_cache ORDER BY saved_at ASC LIMIT (SELECT COUNT(*) / 4 FROM lyrics_cache))",
                    [],
                );
                let _ = conn.execute("VACUUM", []);
            }
            let referenced_covers = referenced_cover_paths(&conn);
            drop(conn);
            if let Some(cache_root) = cache_root
                && let Err(e) =
                    prune_generated_cache_files(&cache_root, cutoff, referenced_covers.as_ref())
            {
                warn!("Failed to prune generated cache files: {e:#}");
            }
        })
        .await
        .context("cache cleanup task failed")?;

        info!("Cache cleanup completed");
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/utils/cache.rs"]
mod tests;
