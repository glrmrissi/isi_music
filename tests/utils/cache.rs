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
    let cm = CacheManager::new_with_path(tmp.path().to_str().unwrap());

    let mut lib = cm.library_cache.lock().unwrap();
    lib.liked.push(LikedTrack {
        uri: "spotify:track:test".into(),
        name: "Test Track".into(),
        artist: "Test Artist".into(),
        album: "Test Album".into(),
        duration_ms: 200000,
        cover_path: None,
        saved_at: 0,
    });
    drop(lib);

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
