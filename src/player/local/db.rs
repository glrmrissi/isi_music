use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::info;

use super::LocalPlayer;
use super::track::LocalTrack;

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=1000;",
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS tracks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL UNIQUE,
            title TEXT,
            artist TEXT,
            album TEXT,
            duration_ms INTEGER,
            cover_path TEXT
        )",
        [],
    )?;
    conn.execute("CREATE INDEX IF NOT EXISTS idx_path ON tracks (path)", [])?;
    Ok(())
}

impl LocalPlayer {
    pub fn reload_library_from_db(&mut self) -> Result<()> {
        let mut stmt = self.db_conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, cover_path
             FROM tracks
             ORDER BY COALESCE(artist, '') ASC,
                      COALESCE(album, '') ASC,
                      COALESCE(title, '') ASC",
        )?;

        let tracks = stmt
            .query_map([], |row| {
                let path_str: String = row.get(1)?;
                let cover_path_str: Option<String> = row.get(6)?;
                Ok(LocalTrack {
                    path: PathBuf::from(&path_str),
                    uri: format!("file://{}", path_str),
                    name: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "Unknown".to_string()),
                    artist: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    album: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    duration_ms: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                    cover_path: cover_path_str.map(PathBuf::from),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        info!("Library reloaded: {} tracks from SQLite", tracks.len());
        self.queue = tracks;
        Ok(())
    }
}
