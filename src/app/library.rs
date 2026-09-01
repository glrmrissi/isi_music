use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::App;
use crate::app::FetchResult;
use crate::spotify::{PlaylistSummary, TrackSummary};
use crate::ui::{ActiveContent, Focus, LocalNode};

const LOCAL_FOLDER_URI_PREFIX: &str = "local:folder:";
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "wav", "aiff", "opus"];

pub fn scan_local_files(dir: &Path) -> Vec<LocalNode> {
    let extensions = AUDIO_EXTENSIONS;
    let db_path = crate::config::get_local_db_path();
    let conn = rusqlite::Connection::open(&db_path).ok();
    if let Some(ref c) = conn {
        let _ = c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
        );
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

    let mut nodes: Vec<LocalNode> = Vec::new();
    if let Some(ref c) = conn {
        let _ = c.execute_batch("BEGIN TRANSACTION;");
    }
    scan_dir(dir, 0, &mut nodes, extensions, &conn);
    if let Some(ref c) = conn {
        let _ = c.execute_batch("COMMIT;");
    }
    nodes
}

fn scan_dir(
    dir: &Path,
    depth: usize,
    nodes: &mut Vec<LocalNode>,
    extensions: &[&str],
    conn: &Option<rusqlite::Connection>,
) {
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();

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
        nodes.push(LocalNode::Track {
            track: build_track_from_path(&path, &uri, path_str, conn),
            depth,
        });
    }
}

impl App {
    pub async fn handle_library_item(&mut self, idx: usize) -> bool {
        if !self.spotify_enabled {
            // Spotify is disabled, only "Local Files" exists in the library
            self.load_local_files().await;
            return false;
        }
        if idx != 4 && !self.spotify.authenticated {
            self.state.status_msg = Some(
                "Spotify not connected - only Local Files available. Run: isi-music setup-spotify"
                    .to_string(),
            );
            return false;
        }
        match idx {
            0 => {
                self.state.push_nav();
                let first_load = !self.spotify.library_cache.has_liked_tracks_cache();
                self.state.status_msg = Some(if first_load {
                    "Loading Liked Songs (first load may take a while)…".to_string()
                } else {
                    "Loading Liked Songs…".to_string()
                });
                self.state.loading = true;
                let spotify = Arc::clone(&self.spotify);
                let (tx, rx) = oneshot::channel();
                self.fetcher.pending_fetch = Some(rx);
                tokio::spawn(async move {
                    let result = spotify.sync_liked_tracks().await.map_err(|e| e.to_string());
                    let _ = tx.send(FetchResult::LikedTracks(result));
                });
            }
            1 => {
                self.state.push_nav();
                self.state.status_msg = Some("Loading saved albums…".to_string());
                self.state.loading = true;
                let spotify = Arc::clone(&self.spotify);
                let (tx, rx) = oneshot::channel();
                self.fetcher.pending_fetch = Some(rx);
                tokio::spawn(async move {
                    let result = spotify
                        .fetch_saved_albums(0)
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(FetchResult::Albums(result));
                });
            }
            2 => {
                self.state.push_nav();
                self.state.status_msg = Some("Loading followed artists…".to_string());
                self.state.loading = true;
                let spotify = Arc::clone(&self.spotify);
                let (tx, rx) = oneshot::channel();
                self.fetcher.pending_fetch = Some(rx);
                tokio::spawn(async move {
                    let result = spotify
                        .fetch_followed_artists()
                        .await
                        .map_err(|e| e.to_string());
                    let _ = tx.send(FetchResult::Artists(result));
                });
            }
            3 => {
                self.state.status_msg = Some("Podcasts: coming soon".to_string());
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

        if playlist.uri.starts_with(LOCAL_FOLDER_URI_PREFIX) {
            self.load_local_folder(&playlist).await;
            return false;
        }

        self.state.push_nav();
        self.state.status_msg = Some(format!("Loading {}…", playlist.name));
        self.state.loading = true;
        self.state.active_playlist_uri = Some(playlist.uri.clone());
        self.state.active_playlist_id = Some(playlist.id.clone());
        let spotify = Arc::clone(&self.spotify);
        let playlist_id = playlist.id;
        let (tx, rx) = oneshot::channel();
        self.fetcher.pending_fetch = Some(rx);
        tokio::spawn(async move {
            let result = spotify
                .fetch_playlist_tracks(&playlist_id, 0)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(FetchResult::PlaylistTracks(result));
        });
        false
    }

    pub async fn load_local_folder(&mut self, playlist: &PlaylistSummary) {
        self.state.push_nav();
        self.state.status_msg = Some(format!("Loading {}…", playlist.name));
        self.state.loading = true;
        self.state.active_playlist_uri = Some(playlist.uri.clone());
        self.state.active_playlist_id = Some(playlist.id.clone());

        let folder_path = playlist.id.clone();
        let (tx, rx) = oneshot::channel();
        self.fetcher.pending_fetch = Some(rx);

        tokio::task::spawn_blocking(move || {
            let result =
                scan_folder_tracks_flat(Path::new(&folder_path)).map_err(|e| e.to_string());
            let _ = tx.send(FetchResult::LocalFolderTracks(result));
        });
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
        self.fetcher.local_scan_rx = Some(rx);

        tokio::task::spawn_blocking(move || {
            let nodes = scan_local_files(&dir);
            let _ = tx.send(nodes);
        });
    }

    pub fn poll_local_scan(&mut self) {
        let rx = match &mut self.fetcher.local_scan_rx {
            Some(r) => r,
            None => return,
        };

        if let Ok(nodes) = rx.try_recv() {
            self.fetcher.local_scan_rx = None;

            let track_count = nodes.iter().filter(|n| !n.is_folder()).count();
            let tree = crate::ui::LocalFileTree::new(nodes);
            let vis_len = tree.visible_len();

            self.state.tracks = tree.all_tracks_flat();
            self.state.tracks_total = track_count as u32;
            self.state.tracks_offset = track_count as u32;
            self.state.tracks_api_offset = track_count as u32;
            self.state.local_tree = tree;
            self.state
                .local_tree_list
                .select(if vis_len == 0 { None } else { Some(0) });
            self.state.active_playlist_uri = Some("local_files".to_string());
            self.state.active_playlist_id = Some("local_files".to_string());

            self.state.apply_quick_filter();

            self.fetcher.local_scan_total = track_count;

            if track_count == 0 {
                self.state.status_msg = Some("No audio files found".to_string());
            } else {
                self.state.status_msg = Some(format!("{track_count} local tracks loaded"));
            }
            self.needs_redraw = true;
        }
    }
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn count_audio_files_recursive(dir: &Path) -> u32 {
    let mut count = 0u32;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_audio_files_recursive(&path);
            } else if is_audio_file(&path) {
                count += 1;
            }
        }
    }
    count
}

pub fn resolve_music_dir(cfg: &crate::config::AppConfig) -> Option<PathBuf> {
    let raw_dir = cfg.local.music_dir.as_ref()?;
    let dir = if raw_dir.starts_with('~') {
        dirs::home_dir()
            .map(|home| home.join(&raw_dir[2..]))
            .unwrap_or_else(|| PathBuf::from(&raw_dir))
    } else {
        PathBuf::from(&raw_dir)
    };
    if dir.exists() { Some(dir) } else { None }
}

pub fn scan_local_folder_playlists(music_dir: &Path) -> Vec<PlaylistSummary> {
    let mut playlists = Vec::new();
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut root_files = 0u32;

    if let Ok(entries) = std::fs::read_dir(music_dir) {
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                subdirs.push(path);
            } else if is_audio_file(&path) {
                root_files += 1;
            }
        }
    }

    if root_files > 0 {
        playlists.push(PlaylistSummary {
            id: music_dir.to_string_lossy().to_string(),
            name: music_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Music")
                .to_string(),
            uri: format!("{LOCAL_FOLDER_URI_PREFIX}{}", music_dir.display()),
            total_tracks: root_files,
            art_url: None,
        });
    }

    for subdir in subdirs {
        let count = count_audio_files_recursive(&subdir);
        if count == 0 {
            continue;
        }
        playlists.push(PlaylistSummary {
            id: subdir.to_string_lossy().to_string(),
            name: subdir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string(),
            uri: format!("{LOCAL_FOLDER_URI_PREFIX}{}", subdir.display()),
            total_tracks: count,
            art_url: None,
        });
    }
    playlists
}

pub fn scan_folder_tracks_flat(folder: &Path) -> anyhow::Result<Vec<TrackSummary>> {
    let db_path = crate::config::get_local_db_path();
    let conn = rusqlite::Connection::open(&db_path).ok().inspect(|c| {
        let _ = c.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        );
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
    });

    let mut tracks = Vec::new();
    collect_tracks_recursive(folder, &mut tracks, &conn);
    Ok(tracks)
}

fn collect_tracks_recursive(
    dir: &Path,
    tracks: &mut Vec<TrackSummary>,
    conn: &Option<rusqlite::Connection>,
) {
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                subdirs.push(path);
            } else if is_audio_file(&path) {
                files.push(path);
            }
        }
    }

    for subdir in subdirs {
        collect_tracks_recursive(&subdir, tracks, conn);
    }
    for path in files {
        let uri = format!("file://{}", path.display());
        let path_str = path.to_str().unwrap_or_default();
        tracks.push(build_track_from_path(&path, &uri, path_str, conn));
    }
}

fn build_track_from_path(
    path: &Path,
    uri: &str,
    path_str: &str,
    conn: &Option<rusqlite::Connection>,
) -> TrackSummary {
    if let Some(c) = conn {
        let cached = c
            .query_row(
                "SELECT title, artist, album, duration_ms, cover_path FROM tracks WHERE path = ?1",
                [path_str],
                |row| {
                    Ok(TrackSummary {
                        name: crate::app::metadata::sanitize_control_chars(
                            &row.get::<_, String>(0)?,
                        )
                        .into_owned(),
                        artist: crate::app::metadata::sanitize_control_chars(
                            &row.get::<_, String>(1)?,
                        )
                        .into_owned(),
                        album: crate::app::metadata::sanitize_control_chars(
                            &row.get::<_, String>(2)?,
                        )
                        .into_owned(),
                        duration_ms: row.get(3)?,
                        uri: uri.to_string(),
                        cover_path: row.get(4).ok(),
                        added_at: None,
                    })
                },
            )
            .ok();
        if let Some(t) = cached {
            return t;
        }
    }

    let (name, artist, album, duration_ms, cover_art) =
        crate::app::metadata::read_audio_metadata(path);

    let cover_path = cover_art.and_then(|art_bytes| {
        let hash = format!("{:x}", md5::compute(&art_bytes));
        let cache_dir = dirs::cache_dir()
            .map(|d| d.join("isi-music/covers"))
            .unwrap_or_else(|| std::env::temp_dir().join("isi-music/covers"));
        std::fs::create_dir_all(&cache_dir).ok().and_then(|_| {
            let cover_file = cache_dir.join(format!("{}.jpg", hash));
            std::fs::write(&cover_file, &art_bytes)
                .ok()
                .and_then(|_| cover_file.to_str().map(|s| s.to_string()))
        })
    });

    if let Some(c) = conn {
        let _ = c.execute(
            "INSERT OR REPLACE INTO tracks (path, title, artist, album, duration_ms, cover_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                path_str,
                crate::app::metadata::sanitize_control_chars(&name),
                crate::app::metadata::sanitize_control_chars(&artist),
                crate::app::metadata::sanitize_control_chars(&album),
                duration_ms as i64,
                cover_path,
            ],
        );
    }

    TrackSummary {
        name,
        artist,
        album,
        duration_ms,
        uri: uri.to_string(),
        cover_path,
        added_at: None,
    }
}
