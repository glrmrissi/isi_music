use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn new_creates_empty_cache() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    let stats = cm.get_stats().await;
    assert_eq!(stats.search_cache_entries, 0);
    assert_eq!(stats.library_cache_entries, 0);
    assert_eq!(stats.lyrics_cache_entries, 0);
}

#[tokio::test]
async fn clear_search_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    assert!(cm.clear_search().await.is_ok());
}

#[tokio::test]
async fn clear_library_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    assert!(cm.clear_library().await.is_ok());
}

#[tokio::test]
async fn clear_lyrics_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    assert!(cm.clear_lyrics().await.is_ok());
}

#[tokio::test]
async fn clear_all_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    assert!(cm.clear_all().await.is_ok());
}

#[tokio::test]
async fn stats_after_clear_is_zero() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    let _ = cm.clear_all().await;
    let stats = cm.get_stats().await;
    assert_eq!(stats.search_cache_entries, 0);
    assert_eq!(stats.library_cache_entries, 0);
    assert_eq!(stats.lyrics_cache_entries, 0);
}

#[tokio::test]
async fn cleanup_expired_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap()).unwrap();
    assert!(cm.cleanup_expired().await.is_ok());
}

#[tokio::test]
async fn stats_after_library_update() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA wal_autocheckpoint=1000;
             CREATE TABLE IF NOT EXISTS library_cache (
                 key      TEXT PRIMARY KEY,
                 data     TEXT NOT NULL,
                 total    INTEGER NOT NULL,
                 saved_at INTEGER NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO library_cache (key, data, total, saved_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["test_key", "[]", 1, 0],
        )
        .unwrap();
    }

    let cm = CacheManager::new_with_path(&path).unwrap();
    let stats = cm.get_stats().await;
    assert_eq!(stats.library_cache_entries, 1);
}

#[test]
fn cache_options_defaults() {
    let opts = CacheOptions::default();
    assert!(opts.enabled);
    assert!(opts.auto_cleanup);
    assert_eq!(opts.max_size_mb, 500);
    assert_eq!(opts.cleanup_interval_hours, 24);
    assert_eq!(opts.keep_days, 60);
}

#[test]
fn resource_regression_cache_options_clamp_zero_cleanup_interval_and_respect_disabled_auto_cleanup()
{
    let mut cfg = crate::config::AppConfig::default();
    cfg.cache.cleanup_interval_hours = Some(0);
    cfg.cache.auto_cleanup = Some(false);
    let cm = CacheManager::new_with_path_and_config(":memory:", &cfg).unwrap();

    assert_eq!(cm.cleanup_interval_hours(), 1);
    assert!(!cm.auto_cleanup_enabled());
}

#[test]
fn resource_regression_prune_generated_cache_files_preserves_lyrics_and_logs() {
    let temp = TempDir::new().unwrap();
    let covers = temp.path().join("covers");
    let waveforms = temp.path().join("waveforms");
    let lyrics = temp.path().join("lyrics");
    std::fs::create_dir_all(&covers).unwrap();
    std::fs::create_dir_all(&waveforms).unwrap();
    std::fs::create_dir_all(&lyrics).unwrap();
    let cover = covers.join("old.jpg");
    let referenced_cover = covers.join("referenced.jpg");
    let waveform = waveforms.join("old.bin");
    let lyric = lyrics.join("offline.lrc");
    let log = temp.path().join("isi-music.log");
    std::fs::write(&cover, b"cover").unwrap();
    std::fs::write(&referenced_cover, b"referenced cover").unwrap();
    std::fs::write(&waveform, b"waveform").unwrap();
    std::fs::write(&lyric, b"offline lyrics").unwrap();
    std::fs::write(&log, b"log").unwrap();

    let referenced_covers = std::collections::HashSet::from([referenced_cover.clone()]);
    prune_generated_cache_files(
        temp.path(),
        std::time::SystemTime::now() + Duration::from_secs(1),
        Some(&referenced_covers),
    )
    .unwrap();

    assert!(!cover.exists());
    assert!(referenced_cover.exists());
    assert!(!waveform.exists());
    assert!(lyric.exists());
    assert!(log.exists());
}

#[tokio::test]
async fn resource_regression_cleanup_expires_spotify_library_pages_after_their_ttl() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE library_cache (
                key TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                total INTEGER NOT NULL,
                saved_at INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO library_cache (key, data, total, saved_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "expired",
                "[]",
                0,
                now - crate::spotify::LIBRARY_CACHE_TTL_SECS - 1
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO library_cache (key, data, total, saved_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["fresh", "[]", 0, now],
        )
        .unwrap();
    }
    let cm = CacheManager::new_with_path(path.to_str().unwrap()).unwrap();

    cm.cleanup_expired().await.unwrap();

    let conn = rusqlite::Connection::open(path).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM library_cache", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
    let key: String = conn
        .query_row("SELECT key FROM library_cache", [], |row| row.get(0))
        .unwrap();
    assert_eq!(key, "fresh");
}
