use std::time::Duration;
use tokio::sync::oneshot;
use tracing::warn;

use crate::App;
use crate::ui::{ActiveContent, Focus, LocalNode};

impl App {
    pub async fn handle_library_item(&mut self, idx: usize) -> bool {
        if idx != 4 && !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected — only Local Files available".to_string());
            return false;
        }
        match idx {
             0 => {
                self.state.push_nav();
                self.state.status_msg = Some("Loading Liked Songs…".to_string());
                tokio::time::sleep(Duration::from_millis(100)).await;
                match self.spotify.fetch_liked_tracks(0).await {
                    Ok((tracks, total)) => {
                        self.state.tracks = tracks;
                        self.state.tracks_total = total;
                        self.state.tracks_offset = self.state.tracks.len() as u32;
                        self.state.active_playlist_uri = Some("liked_songs".to_string());
                        self.state.active_playlist_id = Some("liked_songs".to_string());
                        self.state
                            .track_list
                            .select(if self.state.tracks.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                        self.state.active_content = ActiveContent::Tracks;
                        self.state.search_results = None;
                        self.state.rebuild_sort_indices();
                        self.state.status_msg = None;
                        self.state.focus = Focus::Tracks;
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                            warn!("Got 401 - triggering reconnect");
                            self.state.status_msg =
                                Some("Authorization expired, reconnecting...".to_string());
                            return true;
                        } else {
                            self.state.status_msg = Some(format!("Error: {e}"));
                        }
                    }
                }
            }
            1 => {
                self.state.push_nav();
                self.state.status_msg = Some("Loading saved albums…".to_string());
                tokio::time::sleep(Duration::from_millis(100)).await;
                match self.spotify.fetch_saved_albums(0).await {
                    Ok((albums, total)) => {
                        self.state.albums = albums;
                        self.state.albums_total = total;
                        self.state.albums_offset = self.state.albums.len() as u32;
                        self.state
                            .album_list
                            .select(if self.state.albums.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                        self.state.active_content = ActiveContent::Albums;
                        self.state.search_results = None;
                        self.state.status_msg = None;
                        self.state.focus = Focus::Tracks;
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                            warn!("Got 401 - triggering reconnect");
                            self.state.status_msg =
                                Some("Authorization expired, reconnecting...".to_string());
                            return true;
                        } else {
                            self.state.status_msg = Some(format!("Error: {e}"));
                        }
                    }
                }
            }
            2 => {
                self.state.push_nav();
                self.state.status_msg = Some("Loading followed artists…".to_string());
                tokio::time::sleep(Duration::from_millis(100)).await;
                match self.spotify.fetch_followed_artists().await {
                    Ok(artists) => {
                        self.state.artists = artists;
                        self.state
                            .artist_list
                            .select(if self.state.artists.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                        self.state.active_content = ActiveContent::Artists;
                        self.state.search_results = None;
                        self.state.status_msg = None;
                        self.state.focus = Focus::Tracks;
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                            warn!("Got 401 - triggering reconnect");
                            self.state.status_msg =
                                Some("Authorization expired, reconnecting...".to_string());
                            return true;
                        } else {
                            self.state.status_msg = Some(format!("Error: {e}"));
                        }
                    }
                }
            }
            3 => {
                self.state.status_msg = Some("Podcasts — coming soon".to_string());
            }
            4 => {
                self.load_local_files().await;
            }
            _ => {}
        }
        false
    }

    pub async fn handle_playlist_item(&mut self, idx: usize) -> bool {
        let playlist = match self.state.playlists.get(idx) {
            Some(p) => p.clone(),
            None => return false,
        };
        self.state.push_nav();
        self.state.status_msg = Some(format!("Loading {}…", playlist.name));
        tokio::time::sleep(Duration::from_millis(100)).await;
        match self.spotify.fetch_playlist_tracks(&playlist.id, 0).await {
            Ok((tracks, total)) => {
                self.state.tracks = tracks;
                self.state.tracks_total = total;
                self.state.tracks_offset = self.state.tracks.len() as u32;
                self.state.active_playlist_uri = Some(playlist.uri.clone());
                self.state.active_playlist_id = Some(playlist.id.clone());
                self.state
                    .track_list
                    .select(if self.state.tracks.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.rebuild_sort_indices();
                self.state.status_msg = None;
                self.state.focus = Focus::Tracks;
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                    warn!("Got 401 - triggering reconnect");
                    self.state.status_msg =
                        Some("Authorization expired, reconnecting...".to_string());
                    return true;
                } else {
                    self.state.status_msg = Some(format!("Error: {e}"));
                }
            }
        }
        false
    }

    pub async fn load_local_files(&mut self) {
        let cfg = crate::config::AppConfig::load().unwrap_or_default();
        let raw_dir = match cfg.local.music_dir {
            Some(d) => d,
            None => {
                self.state.status_msg =
                    Some("Set [local] music_dir in ~/.config/isi-music/config.toml".to_string());
                return;
            }
        };

        let dir = if raw_dir.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(&raw_dir[2..])
            } else {
                std::path::PathBuf::from(&raw_dir)
            }
        } else {
            std::path::PathBuf::from(&raw_dir)
        };

        if !dir.exists() {
            self.state.status_msg = Some(format!("Directory not found: {}", dir.display()));
            return;
        }

        self.state.push_nav();
        self.state.status_msg = Some("Loading local files...".to_string());
        self.state.active_content = ActiveContent::LocalFiles;
        self.state.focus = Focus::Tracks;

        let (tx, rx) = oneshot::channel();
        self.local_scan_rx = Some(rx);

        tokio::task::spawn_blocking(move || {
            let extensions = ["mp3", "flac", "ogg", "wav", "aiff", "m4a", "opus"];
            let mut nodes: Vec<LocalNode> = Vec::new();

            let db_path = crate::config::get_local_db_path();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.execute_batch("PRAGMA journal_mode=WAL;");
                    Some(c)
                }
                Err(_) => None,
            };

            if let Some(ref c) = conn {
                let _ = c.execute(
                    "CREATE TABLE IF NOT EXISTS tracks (
                        id INTEGER PRIMARY KEY,
                        path TEXT NOT NULL UNIQUE,
                        title TEXT,
                        artist TEXT,
                        album TEXT,
                        duration_ms INTEGER,
                        cover_path TEXT
                    )",
                    [],
                );
            }

            fn scan_dir(
                dir: &std::path::Path,
                depth: usize,
                nodes: &mut Vec<LocalNode>,
                extensions: &[&str],
                conn: &Option<rusqlite::Connection>,
            ) {
                let mut subdirs: Vec<std::path::PathBuf> = Vec::new();
                let mut files: Vec<std::path::PathBuf> = Vec::new();

                if let Ok(entries) = std::fs::read_dir(dir) {
                    let mut entries_vec: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                    entries_vec.sort();
                    for path in entries_vec {
                        if path.is_dir() {
                            subdirs.push(path);
                        } else if path.is_file() {
                            let ext_ok = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|e| extensions.contains(&e.to_lowercase().as_str()))
                                .unwrap_or(false);
                            if ext_ok {
                                files.push(path);
                            }
                        }
                    }
                }

                for subdir in subdirs {
                    let name = subdir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let folder_idx = nodes.len();
                    nodes.push(LocalNode::Folder {
                        name,
                        depth,
                        expanded: true,
                        children_count: 0,
                    });
                    let before = nodes.len();
                    scan_dir(&subdir, depth + 1, nodes, extensions, conn);
                    let added = nodes.len() - before;
                    if let LocalNode::Folder { children_count, .. } = &mut nodes[folder_idx] {
                        *children_count = added;
                    }
                    if added == 0 {
                        nodes.pop();
                    }
                }

                for path in files {
                    let uri = format!("file://{}", path.display());
                    let path_str = path.to_str().unwrap_or_default();

                    let mut track_data: Option<crate::spotify::TrackSummary> = None;
                    if let Some(c) = conn {
                        let stmt = c
                            .prepare("SELECT title, artist, album, duration_ms, cover_path FROM tracks WHERE path = ?1")
                            .ok();
                        if let Some(mut s) = stmt {
                            track_data = s
                                .query_row([path_str], |row| {
                                    Ok(crate::spotify::TrackSummary {
                                        name: row.get(0)?,
                                        artist: row.get(1)?,
                                        album: row.get(2)?,
                                        duration_ms: row.get(3)?,
                                        uri: uri.clone(),
                                        cover_path: row.get(4).ok(),
                                    })
                                })
                                .ok();
                        }
                    }

                    let track = if let Some(t) = track_data {
                        t
                    } else {
                        let (name, artist, album, duration_ms, cover_art) =
                            crate::app::metadata::read_audio_metadata(&path);

                        let cover_path = if let Some(art_bytes) = cover_art {
                            let hash = format!("{:x}", md5::compute(&art_bytes));

                            let cache_dir = dirs::cache_dir()
                                .map(|d| d.join("isi-music/covers"))
                                .unwrap_or_else(|| {
                                    std::path::PathBuf::from("/tmp/isi-music/covers")
                                });

                            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                                warn!("Cannot create cover cache dir: {e}");
                                None
                            } else {
                                let cover_file = cache_dir.join(format!("{}.jpg", hash));
                                match std::fs::write(&cover_file, &art_bytes) {
                                    Ok(_) => cover_file.to_str().map(|s| s.to_string()),
                                    Err(e) => {
                                        warn!("Cannot write cover art: {e}");
                                        None
                                    }
                                }
                            }
                        } else {
                            None
                        };

                        if let Some(c) = conn {
                            let _ = c.execute(
                                "INSERT OR REPLACE INTO tracks (path, title, artist, album, duration_ms, cover_path)
                                VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                                rusqlite::params![
                                    path_str,
                                    name,
                                    artist,
                                    album,
                                    duration_ms as i64,
                                    cover_path
                                ],
                            );
                        }

                        crate::spotify::TrackSummary {
                            name,
                            artist,
                            album,
                            duration_ms,
                            uri,
                            cover_path,
                        }
                    };

                    nodes.push(LocalNode::Track { track, depth });
                }
            }

            scan_dir(&dir, 0, &mut nodes, &extensions, &conn);
            let _ = tx.send(nodes);
        });
    }

    pub fn poll_local_scan(&mut self) {
        let rx = match &mut self.local_scan_rx {
            Some(r) => r,
            None => return,
        };

        if let Ok(nodes) = rx.try_recv() {
            self.local_scan_rx = None;

            let track_count = nodes.iter().filter(|n| !n.is_folder()).count();
            let tree = crate::ui::LocalFileTree::new(nodes);
            let vis_len = tree.visible_len();

            self.state.tracks = tree.all_tracks_flat();
            self.state.tracks_total = track_count as u32;
            self.state.tracks_offset = track_count as u32;
            self.state.local_tree = tree;
            self.state
                .local_tree_list
                .select(if vis_len == 0 { None } else { Some(0) });
            self.state.active_playlist_uri = Some("local_files".to_string());
            self.state.active_playlist_id = Some("local_files".to_string());

            self.state.apply_quick_filter();

            self.local_scan_total = track_count;

            if track_count == 0 {
                self.state.status_msg = Some("No audio files found".to_string());
            } else {
                self.state.status_msg = Some(format!("{track_count} local tracks loaded"));
            }
        }
    }
}
