use crate::utils::debug_overlay::{DebugOverlay, LogLevel};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{info, warn};

use super::types::LyricsData;

const LYRICS_CACHE_EXPIRY_DAYS: i64 = 60;

pub struct LyricsCache {
    conn: Connection,
}

impl LyricsCache {
    pub fn open(db_path: &PathBuf, enabled: bool) -> Result<Self> {
        let conn = if enabled {
            Connection::open(db_path)?
        } else {
            Connection::open_in_memory()?
        };
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
