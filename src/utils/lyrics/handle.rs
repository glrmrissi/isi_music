use crate::utils::debug_overlay::{DebugOverlay, LogLevel};
use crate::utils::lock::lock_or_recover;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use super::cache::LyricsCache;
use super::files::{load_lrc_file, local_lrc_path, lyrics_cache_path, save_lrc_file};
use super::providers::fetch_lyrics;
use super::types::LyricsData;

#[derive(Default)]
struct HandleInner {
    last_uri: String,
    pending: Option<oneshot::Receiver<Option<LyricsData>>>,
    task: Option<tokio::task::AbortHandle>,
}

impl HandleInner {
    fn cancel_pending(&mut self) {
        self.pending = None;
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub struct LyricsHandle {
    inner: Arc<Mutex<HandleInner>>,
    cache: Arc<Mutex<LyricsCache>>,
    http: Arc<reqwest::Client>,
    debug_overlay: Arc<DebugOverlay>,
    musixmatch_api_key: Option<String>,
}

impl LyricsHandle {
    pub fn new(
        db_path: PathBuf,
        http: reqwest::Client,
        debug_overlay: Arc<DebugOverlay>,
    ) -> Result<Self> {
        Self::with_shared_client(db_path, Arc::new(http), debug_overlay)
    }

    pub fn with_shared_client(
        db_path: PathBuf,
        http: Arc<reqwest::Client>,
        debug_overlay: Arc<DebugOverlay>,
    ) -> Result<Self> {
        let cfg = crate::config::AppConfig::load().ok();
        let cache = LyricsCache::open(
            &db_path,
            cfg.as_ref().and_then(|c| c.cache.enabled).unwrap_or(true),
        )?;
        let musixmatch_api_key = cfg.and_then(|cfg| cfg.get_musixmatch_api_key());

        if musixmatch_api_key.is_none() {
            debug_overlay.log(
                LogLevel::Warn,
                "lyrics: no musixmatch API key found, using fallback (may have low rate limits)"
                    .to_string(),
            );
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(HandleInner::default())),
            cache: Arc::new(Mutex::new(cache)),
            http,
            debug_overlay,
            musixmatch_api_key,
        })
    }

    pub fn request(&self, title: &str, artist: &str, uri: &str) {
        let mut inner = lock_or_recover(&self.inner);
        if inner.last_uri == uri {
            return;
        }

        inner.last_uri = uri.to_string();
        inner.cancel_pending();

        if let Ok(cache) = self.cache.lock()
            && let Some(cached) = cache.get(uri)
        {
            self.debug_overlay
                .log(LogLevel::Info, format!("lyrics: found in cache -> {}", uri));
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Some(cached));
            inner.pending = Some(rx);
            return;
        }

        // Fallback 1: local .lrc next to the audio file.
        if let Some(path) = local_lrc_path(uri)
            && let Some(data) = load_lrc_file(&path)
        {
            self.debug_overlay.log(
                LogLevel::Info,
                format!("lyrics: found local .lrc -> {}", path.display()),
            );
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Some(data));
            inner.pending = Some(rx);
            return;
        }

        // Fallback 2: cached .lrc file in cache dir.
        if let Some(path) = lyrics_cache_path(uri)
            && let Some(data) = load_lrc_file(&path)
        {
            self.debug_overlay.log(
                LogLevel::Info,
                format!("lyrics: found cached .lrc -> {}", path.display()),
            );
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Some(data));
            inner.pending = Some(rx);
            return;
        }

        let (tx, rx) = oneshot::channel();
        inner.pending = Some(rx);

        let http = self.http.clone();
        let cache = Arc::clone(&self.cache);
        let title = title.to_string();
        let artist = artist.to_string();
        let uri = uri.to_string();
        let debug_overlay = self.debug_overlay.clone();
        let musixmatch_api_key = self.musixmatch_api_key.clone();

        let task = tokio::spawn(async move {
            let result =
                fetch_lyrics(&http, &title, &artist, &debug_overlay, musixmatch_api_key).await;
            if let Some(ref data) = result {
                if let Ok(c) = cache.lock() {
                    c.save(&uri, data, &debug_overlay);
                }
                // Mirror to cache dir so users can keep an offline .lrc collection.
                if let Some(path) = lyrics_cache_path(&uri) {
                    save_lrc_file(&path, data);
                }
            } else {
                debug_overlay.log(
                    LogLevel::Warn,
                    format!("lyrics: could not fetch lyrics for {} - {}", artist, title),
                );
            }
            let _ = tx.send(result);
        });
        inner.task = Some(task.abort_handle());
    }

    pub fn poll(&self) -> Option<LyricsData> {
        let mut inner = lock_or_recover(&self.inner);
        let rx = inner.pending.as_mut()?;

        match rx.try_recv() {
            Ok(result) => {
                inner.pending = None;
                inner.task = None;
                result
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(_) => {
                inner.pending = None;
                inner.task = None;
                None
            }
        }
    }

    pub fn is_loading(&self) -> bool {
        lock_or_recover(&self.inner).pending.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn resource_regression_cancel_pending_aborts_the_previous_lyrics_task() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let mut inner = HandleInner::default();
        let (result_tx, result_rx) = oneshot::channel();
        let (started_tx, started_rx) = oneshot::channel();
        let completed = Arc::new(AtomicBool::new(false));
        let completed_by_task = Arc::clone(&completed);
        inner.pending = Some(result_rx);
        let task = tokio::spawn(async move {
            let _ = started_tx.send(());
            tokio::time::sleep(Duration::from_secs(30)).await;
            completed_by_task.store(true, Ordering::SeqCst);
            let _ = result_tx.send(Some(LyricsData::default()));
        });
        inner.task = Some(task.abort_handle());
        started_rx.await.expect("task started");

        inner.cancel_pending();

        assert!(inner.pending.is_none());
        assert!(inner.task.is_none());
        assert!(task.await.is_err());
        assert!(!completed.load(Ordering::SeqCst));
    }
}
