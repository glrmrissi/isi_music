use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::info;

use super::LocalPlayer;
use super::track::LocalTrack;

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
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

fn load_tracks_from_db(conn: &mut Connection) -> Result<Vec<LocalTrack>> {
    let mut tracks = Vec::new();
    let mut stale_paths = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT path, title, artist, album, duration_ms, cover_path
             FROM tracks
             ORDER BY COALESCE(artist, '') ASC,
                      COALESCE(album, '') ASC,
                      COALESCE(title, '') ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let path_str: String = row.get(0)?;
            let cover_path_str: Option<String> = row.get(5)?;
            Ok(LocalTrack {
                path: PathBuf::from(&path_str),
                uri: format!("file://{}", path_str),
                name: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| "Unknown".to_string()),
                artist: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                album: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                duration_ms: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                cover_path: cover_path_str.map(PathBuf::from),
            })
        })?;
        for row in rows {
            let track = row?;
            match track.path.try_exists() {
                Ok(true) => tracks.push(track),
                Ok(false)
                    if track
                        .path
                        .parent()
                        .is_some_and(|parent| matches!(parent.try_exists(), Ok(true))) =>
                {
                    stale_paths.push(track.path);
                }
                Ok(false) | Err(_) => {}
            }
        }
    }

    if let Err(e) = delete_stale_paths(conn, &stale_paths) {
        tracing::warn!("Failed to prune stale local track metadata: {e}");
    }

    Ok(tracks)
}

fn delete_stale_paths(conn: &mut Connection, paths: &[PathBuf]) -> rusqlite::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare("DELETE FROM tracks WHERE path = ?1")?;
        for path in paths {
            stmt.execute([path.to_string_lossy().as_ref()])?;
        }
    }
    tx.commit()
}

impl LocalPlayer {
    pub fn reload_library_from_db(&mut self) -> Result<()> {
        let tracks = load_tracks_from_db(&mut self.db_conn)?;
        info!("Library reloaded: {} tracks from SQLite", tracks.len());
        self.queue = tracks;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resource_regression_reload_prunes_missing_files_but_keeps_unavailable_parents() {
        let dir = TempDir::new().unwrap();
        let existing_path = dir.path().join("present.mp3");
        std::fs::write(&existing_path, b"fixture").unwrap();
        let missing_path = dir.path().join("missing.mp3");
        let unavailable_path = dir.path().join("offline").join("track.mp3");
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        for path in [&existing_path, &missing_path, &unavailable_path] {
            conn.execute(
                "INSERT INTO tracks (path, title) VALUES (?1, ?2)",
                rusqlite::params![
                    path.to_string_lossy(),
                    path.file_name().unwrap().to_string_lossy()
                ],
            )
            .unwrap();
        }

        let tracks = load_tracks_from_db(&mut conn).unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))
            .unwrap();

        assert_eq!(tracks.len(), 1);
        assert_eq!(rows, 2);
        assert!(tracks.iter().any(|track| track.path == existing_path));
        assert!(!tracks.iter().any(|track| track.path == unavailable_path));
    }

    #[test]
    fn resource_regression_read_only_db_still_loads_existing_tracks() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("library.db");
        let existing_path = dir.path().join("present.mp3");
        std::fs::write(&existing_path, b"fixture").unwrap();
        let missing_path = dir.path().join("missing.mp3");
        {
            let conn = Connection::open(&db_path).unwrap();
            init_db(&conn).unwrap();
            for path in [&existing_path, &missing_path] {
                conn.execute(
                    "INSERT INTO tracks (path, title) VALUES (?1, ?2)",
                    rusqlite::params![
                        path.to_string_lossy(),
                        path.file_name().unwrap().to_string_lossy()
                    ],
                )
                .unwrap();
            }
        }
        let mut conn =
            Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();

        let tracks = load_tracks_from_db(&mut conn).unwrap();

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].path, existing_path);
    }
}
