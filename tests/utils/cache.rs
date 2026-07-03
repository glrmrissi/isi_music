use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn new_creates_empty_cache() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
    let stats = cm.get_stats().await;
    assert_eq!(stats.search_cache_entries, 0);
    assert_eq!(stats.library_cache_entries, 0);
    assert_eq!(stats.lyrics_cache_entries, 0);
}

#[tokio::test]
async fn clear_search_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
    assert!(cm.clear_search().await.is_ok());
}

#[tokio::test]
async fn clear_library_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
    assert!(cm.clear_library().await.is_ok());
}

#[tokio::test]
async fn clear_lyrics_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
    assert!(cm.clear_lyrics().await.is_ok());
}

#[tokio::test]
async fn clear_all_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
    assert!(cm.clear_all().await.is_ok());
}

#[tokio::test]
async fn stats_after_clear_is_zero() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
    let _ = cm.clear_all().await;
    let stats = cm.get_stats().await;
    assert_eq!(stats.search_cache_entries, 0);
    assert_eq!(stats.library_cache_entries, 0);
    assert_eq!(stats.lyrics_cache_entries, 0);
}

#[tokio::test]
async fn cleanup_expired_on_empty_is_ok() {
    let tmp = NamedTempFile::new().unwrap();
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());
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

    let cm = CacheManager::new_with_path(&path);
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
