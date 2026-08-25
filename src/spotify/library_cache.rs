use anyhow::Context;
use rusqlite::params;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::search_cache::{CachedAlbum, CachedArtist, CachedTrack};
use super::types::{AlbumSummary, ArtistSummary, TrackSummary};

#[derive(Clone)]
pub struct LibraryCache {
    pub(super) conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
}

impl LibraryCache {
    pub async fn new() -> anyhow::Result<Self> {
        let conn = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            let conn = rusqlite::Connection::open_in_memory()?;
            #[cfg(not(test))]
            let conn = {
                let db_path = crate::config::get_local_db_path();
                let conn = rusqlite::Connection::open(&db_path)?;
                conn.execute_batch(
                    "PRAGMA journal_mode=WAL;
                    PRAGMA synchronous=NORMAL;
                    PRAGMA busy_timeout=5000;
                    PRAGMA wal_autocheckpoint=1000;",
                )?;
                conn
            };
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS library_cache (
                    key      TEXT PRIMARY KEY,
                    data     TEXT NOT NULL,
                    total    INTEGER NOT NULL,
                    saved_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS liked_tracks_cache (
                    added_at    TEXT NOT NULL,
                    uri         TEXT NOT NULL,
                    name        TEXT NOT NULL DEFAULT '',
                    artist      TEXT NOT NULL DEFAULT '',
                    album       TEXT NOT NULL DEFAULT '',
                    duration_ms INTEGER NOT NULL DEFAULT 0,
                    cover_path  TEXT,
                    PRIMARY KEY (added_at, uri)
                );",
            )?;
            Ok::<_, rusqlite::Error>(conn)
        })
        .await
        .context("failed to spawn blocking task")??;

        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    fn unix_now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    pub fn get_tracks(&self, key: &str) -> Option<(Vec<TrackSummary>, u32)> {
        let conn = self.conn.lock().ok()?;
        let (data, total): (String, u32) = conn
            .query_row(
                "SELECT data, total FROM library_cache WHERE key = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?;
        let rows: Vec<CachedTrack> = serde_json::from_str(&data).ok()?;
        let tracks = rows
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
            .collect();
        Some((tracks, total))
    }

    pub fn save_tracks(&self, key: &str, tracks: &[TrackSummary], total: u32) {
        if tracks.is_empty() && total > 0 {
            return;
        }
        let rows: Vec<CachedTrack> = tracks
            .iter()
            .map(|t| CachedTrack {
                name: t.name.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
                duration_ms: t.duration_ms,
                uri: t.uri.clone(),
                cover_path: t.cover_path.clone(),
            })
            .collect();
        let Ok(data) = serde_json::to_string(&rows) else {
            return;
        };
        let Ok(conn) = self.conn.lock() else {
            return;
        };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO library_cache (key, data, total, saved_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![key, data, total, Self::unix_now()],
        );
    }

    pub fn get_albums(&self) -> Option<(Vec<AlbumSummary>, u32)> {
        let conn = self.conn.lock().ok()?;
        let (data, total): (String, u32) = conn
            .query_row(
                "SELECT data, total FROM library_cache WHERE key = 'albums'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?;
        let rows: Vec<CachedAlbum> = serde_json::from_str(&data).ok()?;
        let albums = rows
            .into_iter()
            .map(|a| AlbumSummary {
                id: a.id,
                name: a.name,
                artist: a.artist,
                uri: a.uri,
                total_tracks: a.total_tracks,
            })
            .collect();
        Some((albums, total))
    }

    pub fn save_albums(&self, albums: &[AlbumSummary], total: u32) {
        let rows: Vec<CachedAlbum> = albums
            .iter()
            .map(|a| CachedAlbum {
                id: a.id.clone(),
                name: a.name.clone(),
                artist: a.artist.clone(),
                uri: a.uri.clone(),
                total_tracks: a.total_tracks,
            })
            .collect();
        let Ok(data) = serde_json::to_string(&rows) else {
            return;
        };
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO library_cache (key, data, total, saved_at)
             VALUES ('albums', ?1, ?2, ?3)",
            params![data, total, Self::unix_now()],
        );
    }

    pub fn get_artists(&self) -> Option<Vec<ArtistSummary>> {
        let conn = self.conn.lock().ok()?;
        let (data,): (String,) = conn
            .query_row(
                "SELECT data FROM library_cache WHERE key = 'artists'",
                [],
                |r| Ok((r.get(0)?,)),
            )
            .ok()?;
        let rows: Vec<CachedArtist> = serde_json::from_str(&data).ok()?;
        Some(
            rows.into_iter()
                .map(|a| ArtistSummary {
                    id: a.id,
                    name: a.name,
                    uri: a.uri,
                    genres: a.genres,
                })
                .collect(),
        )
    }

    pub fn delete_key_pattern(&self, pattern: &str) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "DELETE FROM library_cache WHERE key LIKE ?1",
            params![pattern],
        );
    }

    pub fn insert_liked_track(&self, added_at: &str, track: &TrackSummary) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO liked_tracks_cache (added_at, uri, name, artist, album, duration_ms, cover_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                added_at,
                track.uri,
                track.name,
                track.artist,
                track.album,
                track.duration_ms as i64,
                track.cover_path,
            ],
        );
    }

    pub fn delete_liked_track(&self, track_uri: &str) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "DELETE FROM liked_tracks_cache WHERE uri = ?1",
            params![track_uri],
        );
    }

    pub fn has_liked_tracks_cache(&self) -> bool {
        let Ok(conn) = self.conn.lock() else {
            return false;
        };
        conn.query_row("SELECT COUNT(*) FROM liked_tracks_cache", [], |r| r.get(0))
            .unwrap_or(0)
            > 0
    }

    pub fn get_liked_tracks_page(
        &self,
        after: Option<&str>,
        limit: u32,
    ) -> Option<(Vec<TrackSummary>, u32, Option<String>)> {
        let Ok(conn) = self.conn.lock() else {
            return None;
        };
        let total: u32 = conn
            .query_row("SELECT COUNT(*) FROM liked_tracks_cache", [], |r| r.get(0))
            .unwrap_or(0);
        if total == 0 {
            return None;
        }
        if let Some(cursor) = after {
            let (added_at, uri) = cursor.split_once('\t').unwrap_or((cursor, ""));
            let mut s = conn
                .prepare(
                    "SELECT added_at, uri, name, artist, album, duration_ms, cover_path
                     FROM liked_tracks_cache
                     WHERE (added_at, uri) < (?1, ?2)
                     ORDER BY added_at DESC, uri DESC
                     LIMIT ?3",
                )
                .ok()?;
            let rows = s
                .query_map(params![added_at, uri, limit], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        TrackSummary {
                            name: r.get(2)?,
                            artist: r.get(3)?,
                            album: r.get(4)?,
                            duration_ms: r.get::<_, i64>(5)? as u64,
                            uri: r.get(1)?,
                            cover_path: r.get(6)?,
                            added_at: r.get(0)?,
                        },
                    ))
                })
                .ok()?;
            let mut tracks: Vec<(String, String, TrackSummary)> = Vec::new();
            for row in rows.flatten() {
                tracks.push(row);
            }
            if tracks.is_empty() {
                return None;
            }
            let next_cursor = tracks.last().map(|last| format!("{}\t{}", last.0, last.1));
            let summaries: Vec<TrackSummary> = tracks.into_iter().map(|(_, _, t)| t).collect();
            Some((summaries, total, next_cursor))
        } else {
            let mut s = conn
                .prepare(
                    "SELECT added_at, uri, name, artist, album, duration_ms, cover_path
                     FROM liked_tracks_cache
                     ORDER BY added_at DESC, uri DESC
                     LIMIT ?1",
                )
                .ok()?;
            let rows = s
                .query_map(params![limit], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        TrackSummary {
                            name: r.get(2)?,
                            artist: r.get(3)?,
                            album: r.get(4)?,
                            duration_ms: r.get::<_, i64>(5)? as u64,
                            uri: r.get(1)?,
                            cover_path: r.get(6)?,
                            added_at: r.get(0)?,
                        },
                    ))
                })
                .ok()?;
            let mut tracks: Vec<(String, String, TrackSummary)> = Vec::new();
            for row in rows.flatten() {
                tracks.push(row);
            }
            if tracks.is_empty() {
                return None;
            }
            let next_cursor = tracks.last().map(|last| format!("{}\t{}", last.0, last.1));
            let summaries: Vec<TrackSummary> = tracks.into_iter().map(|(_, _, t)| t).collect();
            Some((summaries, total, next_cursor))
        }
    }

    pub fn reset_liked_tracks_cache(&self, tracks: &[TrackSummary], added_ats: &[String]) {
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute("DELETE FROM liked_tracks_cache", []);
        let mut stmt = match conn.prepare(
            "INSERT INTO liked_tracks_cache (added_at, uri, name, artist, album, duration_ms, cover_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        ) {
            Ok(s) => s,
            Err(_) => return,
        };
        for (track, added_at) in tracks.iter().zip(added_ats.iter()) {
            let _ = stmt.execute(params![
                added_at,
                track.uri,
                track.name,
                track.artist,
                track.album,
                track.duration_ms as i64,
                track.cover_path,
            ]);
        }
    }

    pub fn append_liked_tracks_batch(&self, tracks: &[TrackSummary], added_ats: &[String]) {
        let Ok(conn) = self.conn.lock() else { return };
        let mut stmt = match conn.prepare(
            "INSERT OR IGNORE INTO liked_tracks_cache (added_at, uri, name, artist, album, duration_ms, cover_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        ) {
            Ok(s) => s,
            Err(_) => return,
        };
        for (track, added_at) in tracks.iter().zip(added_ats.iter()) {
            let _ = stmt.execute(params![
                added_at,
                track.uri,
                track.name,
                track.artist,
                track.album,
                track.duration_ms as i64,
                track.cover_path,
            ]);
        }
    }

    pub fn clear_all_library_cache(&self) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute("DELETE FROM library_cache", []);
            let _ = conn.execute("DELETE FROM liked_tracks_cache", []);
        }
    }

    pub fn save_artists(&self, artists: &[ArtistSummary]) {
        let rows: Vec<CachedArtist> = artists
            .iter()
            .map(|a| CachedArtist {
                id: a.id.clone(),
                name: a.name.clone(),
                uri: a.uri.clone(),
                genres: a.genres.clone(),
            })
            .collect();
        let Ok(data) = serde_json::to_string(&rows) else {
            return;
        };
        let Ok(conn) = self.conn.lock() else { return };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO library_cache (key, data, total, saved_at)
             VALUES ('artists', ?1, 0, ?2)",
            params![data, Self::unix_now()],
        );
    }
}
