use std::{
    fs::File,
    io::BufReader,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use rodio::{Decoder, OutputStreamBuilder, Sink};
use rusqlite::{params, Connection};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::{AudioPlayer, PlayerNotification, QueuedTrack, RepeatMode, TrackInfo};
use crate::audio_sink::{AnalyzingSource, N_BANDS};
use crate::spotify::TrackSummary;

#[derive(Clone, Debug)]
pub struct LocalTrack {
    pub id: i64,
    pub path: PathBuf,
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub cover_art: Option<Vec<u8>>,
    pub duration_ms: u64,
}

pub struct LocalPlayer {
    sink: Sink,
    db_conn: Connection,
    queue: Vec<LocalTrack>,
    user_queue: Vec<QueuedTrack>,
    playing_queued: Option<QueuedTrack>,
    current_idx: Option<usize>,
    pub is_playing: bool,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    event_tx: mpsc::UnboundedSender<PlayerNotification>,
    event_rx: mpsc::UnboundedReceiver<PlayerNotification>,
    pub band_energies: Arc<Mutex<Vec<f32>>>,
}

impl LocalPlayer {
    pub fn new(volume: u8, db_path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tracks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                title TEXT,
                artist TEXT,
                album TEXT,
                duration_ms INTEGER,
                cover_art BLOB
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_path ON tracks (path)",
            [],
        )?;

        let (sync_tx, sync_rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            match OutputStreamBuilder::open_default_stream() {
                Ok(stream) => {
                    let sink = Sink::connect_new(&stream.mixer());
                    if sync_tx.send(Ok(sink)).is_ok() {
                        let _keep_alive = stream;
                        loop {
                            std::thread::park();
                        }
                    }
                }
                Err(e) => {
                    let _ = sync_tx
                        .send(Err(anyhow::anyhow!("Audio output unavailable: {e}")));
                }
            }
        });

        let sink = sync_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Audio thread panicked during startup"))??;

        sink.set_volume(volume as f32 / 100.0);

        let (tx, rx) = mpsc::unbounded_channel();

        // Ok(Self {
        //     sink,
        //     db_conn: conn,
        //     queue: Vec::new(),
        //     user_queue: Vec::new(),
        //     playing_queued: None,
        //     current_idx: None,
        //     is_playing: false,
        //     volume,
        //     shuffle: false,
        //     repeat: RepeatMode::Off,
        //     event_tx: tx,
        //     event_rx: rx,
        //     band_energies: Arc::new(Mutex::new(vec![0.0f32; N_BANDS])),
        // })

        let mut instance = Self {
            sink,
            db_conn: conn,
            queue: Vec::new(),
            user_queue: Vec::new(),
            playing_queued: None,
            current_idx: None,
            is_playing: false,
            volume,
            shuffle: false,
            repeat: RepeatMode::Off,
            event_tx: tx,
            event_rx: rx,
            band_energies: Arc::new(Mutex::new(vec![0.0f32; N_BANDS])),
        };

        if let Err(e) = instance.reload_library_from_db() {
            error!("Failed to load songs from SQLite: {}", e);
        } else {
            warn!("Songs loaded successfully: {} found", instance.queue.len());
        }

        Ok(instance)
    }

    pub fn reload_library_from_db(&mut self) -> anyhow::Result<()> {
        let mut stmt = self.db_conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, cover_art FROM tracks ORDER BY artist, album, title",
        )?;

        let tracks = stmt.query_map([], |row| {
            let path_str: String = row.get(1)?;
            Ok(LocalTrack {
                id: row.get(0)?,
                path: PathBuf::from(&path_str),
                uri: format!("file://{}", path_str),
                name: row.get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "Unknown".to_string()),
                artist: row.get::<_, Option<String>>(3)?
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: row.get::<_, Option<String>>(4)?
                    .unwrap_or_else(|| "".to_string()),
                duration_ms: row.get::<_, Option<i64>>(5)?
                    .unwrap_or(0) as u64,
                cover_art: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

        info!("Library reloaded: {} tracks from SQLite", tracks.len());
        self.queue = tracks;
        Ok(())
    }

    pub fn index_file(&self, path: &PathBuf) -> anyhow::Result<()> {
        let (name, artist, album, duration_ms) =
            crate::app::read_audio_metadata(path);
        let cover_art = crate::app::extract_embedded_art(path);
        let path_str = path.to_str().unwrap_or_default();

        self.db_conn.execute(
            "INSERT INTO tracks (path, title, artist, album, duration_ms, cover_art)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
                title      = excluded.title,
                artist     = excluded.artist,
                album      = excluded.album,
                duration_ms = excluded.duration_ms,
                cover_art  = excluded.cover_art",
            params![path_str, name, artist, album, duration_ms as i64, cover_art],
        )?;
        Ok(())
    }

    pub fn db_track_count(&self) -> i64 {
        self.db_conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn uri_to_path(uri: &str) -> PathBuf {
        if let Some(s) = uri.strip_prefix("file://") {
            PathBuf::from(s)
        } else {
            PathBuf::from(uri)
        }
    }

    fn load_and_play(&mut self, idx: usize) {
        let Some(track) = self.queue.get(idx) else {
            warn!("LocalPlayer: index {idx} out of bounds (queue len={})", self.queue.len());
            return;
        };
        let path = track.path.clone();
        info!("LocalPlayer: loading {:?}", path);

        self.sink.stop();

        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                error!("LocalPlayer: cannot open {:?}: {e}", path);
                let _ = self.event_tx.send(PlayerNotification::TrackUnavailable);
                return;
            }
        };

        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(d) => d,
            Err(e) => {
                error!("LocalPlayer: cannot decode {:?}: {e}", path);
                let _ = self.event_tx.send(PlayerNotification::TrackUnavailable);
                return;
            }
        };

        let analyzing = AnalyzingSource::new(decoder, Arc::clone(&self.band_energies));
        self.sink.append(analyzing);
        self.sink.play();
        self.current_idx = Some(idx);
        self.is_playing = true;
        let _ = self.event_tx.send(PlayerNotification::Playing);
    }

    fn poll_sink(&mut self) {
        if self.is_playing && self.sink.empty() {
            self.is_playing = false;
            let _ = self.event_tx.send(PlayerNotification::TrackEnded);
        }
    }

    pub fn current_track_meta(&self) -> Option<&LocalTrack> {
        self.current_idx.and_then(|i| self.queue.get(i))
    }

    pub fn next_inner(&mut self) -> bool {
        self.playing_queued = None;

        if self.repeat == RepeatMode::Track {
            if let Some(idx) = self.current_idx {
                self.load_and_play(idx);
                return true;
            }
        }

        if !self.user_queue.is_empty() {
            let track = self.user_queue.remove(0);
            let path = Self::uri_to_path(&track.uri);
            let lt = LocalTrack {
                id: -1,
                path,
                uri: track.uri.clone(),
                name: track.name.clone(),
                artist: track.artist.clone(),
                album: String::new(),
                duration_ms: track.duration_ms,
                cover_art: None,
            };
            let idx = self.queue.len();
            self.queue.push(lt);
            self.playing_queued = Some(track);
            self.load_and_play(idx);
            return true;
        }

        if let Some(idx) = self.current_idx {
            let len = self.queue.len();
            let next = if self.shuffle && len > 1 {
                use rand::seq::SliceRandom;
                let mut candidates: Vec<usize> =
                    (0..len).filter(|&i| i != idx).collect();
                candidates.shuffle(&mut rand::thread_rng());
                candidates[0]
            } else {
                idx + 1
            };

            if next < len {
                self.load_and_play(next);
                return true;
            }
            if self.repeat == RepeatMode::Queue && len > 0 {
                self.load_and_play(0);
                return true;
            }
        }
        false
    }

    pub fn prev_inner(&mut self) -> bool {
        if let Some(idx) = self.current_idx {
            if idx > 0 {
                self.load_and_play(idx - 1);
                return true;
            }
        }
        false
    }

    pub fn toggle_inner(&mut self) {
        if self.sink.is_paused() {
            self.sink.play();
            self.is_playing = true;
        } else {
            self.sink.pause();
            self.is_playing = false;
        }
    }

    fn apply_volume(&self) {
        self.sink.set_volume(self.volume as f32 / 100.0);
    }

    pub fn queue_as_track_summaries(&self) -> Vec<crate::spotify::TrackSummary> {
        self.queue
            .iter()
            .map(|t| crate::spotify::TrackSummary {
                uri: t.uri.clone(),
                name: t.name.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
                duration_ms: t.duration_ms,
            })
            .collect()
    }
}

impl AudioPlayer for LocalPlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        if self.queue.is_empty() {
            warn!("LocalPlayer: set_queue called but internal queue is empty — did you call reload_library_from_db?");
            return;
        }

        let target_uri = match uris.get(start_index) {
            Some(u) => u,
            None => {
                warn!("LocalPlayer: start_index {start_index} out of range for uris len={}", uris.len());
                return;
            }
        };

        match self.queue.iter().position(|t| &t.uri == target_uri) {
            Some(idx) => self.load_and_play(idx),
            None => {
                warn!("LocalPlayer: URI not found in queue: {target_uri}");
            }
        }
    }

    fn get_tracks_paginated(&self, limit: usize, offset: usize) -> Vec<crate::player::TrackInfo> {
        let mut stmt = match self.db_conn.prepare(
            "SELECT title, artist, album, duration_ms FROM tracks LIMIT ? OFFSET ?"
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Error on queue: {e}");
                return Vec::new();
            }
        };

        let rows = stmt.query_map([limit, offset], |row| {
            Ok(crate::player::TrackInfo {
                name: row.get(0).unwrap_or_default(),
                artist: row.get(1).unwrap_or_default(),
                album: row.get(2).unwrap_or_default(),
                duration_ms: row.get::<_, i64>(3).unwrap_or(0) as u64,
                uri: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        });

        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn set_queue_tracks(&mut self, tracks: Vec<TrackSummary>, start_index: usize) {
        info!("Debug: Tentando dar play. Queue interna len: {}, Tracks recebidas len: {}", 
            self.queue.len(), tracks.len());

        if self.queue.is_empty() {
            warn!("LocalPlayer: set_queue_tracks chamado mas a fila interna está vazia");
            return;
        }

        let target_uri = match tracks.get(start_index) {
            Some(t) => &t.uri,
            None => return,
        };

        match self.queue.iter().position(|t| &t.uri == target_uri) {
            Some(idx) => self.load_and_play(idx),
            None => {
                warn!("LocalPlayer: URI not found in internal queue: {target_uri}");
            }
        }
    }

    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64) {
        self.user_queue.push(QueuedTrack {
            uri,
            name,
            artist,
            duration_ms,
        });
    }

    fn user_queue(&self) -> &[QueuedTrack] {
        &self.user_queue
    }

    fn remove_from_user_queue(&mut self, index: usize) {
        if index < self.user_queue.len() {
            self.user_queue.remove(index);
        }
    }

    fn take_playing_queued(&mut self) -> Option<QueuedTrack> {
        self.playing_queued.take()
    }

    fn play(&mut self) {
        self.sink.play();
        self.is_playing = true;
    }

    fn pause(&mut self) {
        self.sink.pause();
        self.is_playing = false;
    }

    fn toggle(&mut self) {
        self.toggle_inner();
    }

    fn next(&mut self) -> bool {
        self.next_inner()
    }

    fn prev(&mut self) -> bool {
        self.prev_inner()
    }

    fn play_at(&mut self, index: usize) {
        if let Some(track) = self.queue.get(index) {
            let path = &track.path; 
            
            match std::fs::File::open(path) {
                Ok(file) => {
                    let source = rodio::Decoder::new(std::io::BufReader::new(file)).unwrap();
                    self.sink.append(source);
                    self.sink.play();
                }
                Err(e) => error!("Failed to load track: {}", e), 
            }
        }
    }

    fn seek(&self, position_ms: u32) {
        let pos = std::time::Duration::from_millis(position_ms as u64);
        if let Err(e) = self.sink.try_seek(pos) {
            warn!("LocalPlayer: seek to {}ms failed: {e}", position_ms);
        }
    }

    fn is_playing(&self) -> bool {
        self.is_playing
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn shuffle(&self) -> bool {
        self.shuffle
    }

    fn repeat(&self) -> RepeatMode {
        self.repeat.clone()
    }

    fn current_index(&self) -> Option<usize> {
        self.current_idx
    }

    fn current_uri(&self) -> Option<String> {
        self.current_idx
            .and_then(|i| self.queue.get(i))
            .map(|t| t.uri.clone())
    }

    fn current_track_info(&self) -> Option<TrackInfo> {
        let t = self.current_track_meta()?;
        Some(TrackInfo {
            uri: t.uri.clone(),
            name: t.name.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
        })
    }

    fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.apply_volume();
        crate::config::save_volume(self.volume);
    }

    fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.apply_volume();
        crate::config::save_volume(self.volume);
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.apply_volume();
        crate::config::save_volume(self.volume);
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
    }

    fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };
    }

    fn try_recv_event(&mut self) -> Option<PlayerNotification> {
        self.poll_sink();
        self.event_rx.try_recv().ok()
    }

    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        Some(Arc::clone(&self.band_energies))
    }

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        let uris = self.queue.iter().map(|t| t.uri.clone()).collect();
        (uris, self.current_idx)
    }
}