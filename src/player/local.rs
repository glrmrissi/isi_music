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
        
        conn.execute("CREATE INDEX IF NOT EXISTS idx_path ON tracks (path)", [])?;

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
                    let _ = sync_tx.send(Err(anyhow::anyhow!("Audio output unavailable: {e}")));
                }
            }
        });

        let sink = sync_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Audio thread panicked during startup"))??;

        sink.set_volume(volume as f32 / 100.0);

        let (tx, rx) = mpsc::unbounded_channel();

        Ok(Self {
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
        })
    }

    pub fn reload_library_from_db(&mut self) -> anyhow::Result<()> {
        let mut stmt = self.db_conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, cover_art
            FROM tracks"
        )?;
        let track_iter = stmt.query_map([], |row| {
            let path_str: String = row.get(1)?;
            Ok(LocalTrack {
                id: row.get(0)?,
                path: PathBuf::from(&path_str),
                uri: format!("file://{}", path_str),
                name: row.get(2).unwrap_or_else(|_| "Unknown".to_string()),
                artist: row.get(3).unwrap_or_else(|_| "Unknown Artist".to_string()),
                album: row.get(4).unwrap_or_else(|_| "Unknown Album".to_string()),
                duration_ms: row.get::<_, i64>(5)? as u64,
                cover_art: row.get(6)?
            })
        })?;

        self.queue = track_iter.collect::<Result<Vec<_>, _>>()?;
        info!("Library reloaded: {} tracks from SQLite", self.queue.len());
        Ok(())
    }
    
    pub fn index_file(&self, path: &PathBuf) -> anyhow::Result<()> {
        let (name, artist, album, duration_ms, cover_art) = crate::app::read_audio_metadata(path);
        let path_str = path.to_str().unwrap_or_default();
        
        self.db_conn.execute(
            "INSERT INTO tracks (
                path,
                title,
                artist,
                album,
                duration_ms,
                cover_art
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)

            ON CONFLICT(path) DO UPDATE SET
                title = excluded.title,
                artist = excluded.artist,
                album = excluded.album,
                duration_ms = excluded.duration_ms,
                cover_art = excluded.cover_art",
            params![
                path_str,
                name,
                artist,
                album,
                duration_ms,
                cover_art
            ],
        )?;
        Ok(())
    }

    pub fn uri_to_path(uri: &str) -> PathBuf {
        if let Some(stripped) = uri.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(uri)
        }
    }

    fn load_and_play(&mut self, idx: usize) {
        let Some(track) = self.queue.get(idx) else {
            warn!("LocalPlayer: index {idx} out of bounds");
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

    pub fn play_at(&mut self, index: usize) {
        self.load_and_play(index);
    }

    pub fn current_track_meta(&self) -> Option<&LocalTrack> {
        self.current_idx.and_then(|i| self.queue.get(i))
    }

    pub fn next(&mut self) -> bool {
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
                let mut candidates: Vec<usize> = (0..len).filter(|&i| i != idx).collect();
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

    pub fn prev(&mut self) -> bool {
        if let Some(idx) = self.current_idx {
            if idx > 0 {
                self.load_and_play(idx - 1);
                return true;
            }
        }
        false
    }

    pub fn toggle(&mut self) {
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
}

impl AudioPlayer for LocalPlayer {
    fn set_queue(&mut self, _uris: Vec<String>, start_index: usize) {
        if self.queue.is_empty() {
            let _ = self.reload_library_from_db();
        }

        if self.current_idx.is_none() && !self.queue.is_empty() {
           if let Some(target_uri) = _uris.get(start_index) {
                if let Some(index) = self.queue.iter().position(|t| &t.uri == target_uri) {
                    self.play_at(index);
                }
            }
        }
    }

    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64) {
        self.user_queue.push(QueuedTrack { uri, name, artist, duration_ms });
    }

    fn user_queue(&self) -> &[QueuedTrack] {
        &self.user_queue
    }

    fn remove_from_user_queue(&mut self, index: usize) {
        self.user_queue.remove(index);
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
        self.toggle();
    }

    fn next(&mut self) -> bool {
        self.next()
    }

    fn prev(&mut self) -> bool {
        self.prev()
    }

    fn play_at(&mut self, index: usize) {
        self.play_at(index);
    }

    fn seek(&self, position_ms: u32) {
        let pos = std::time::Duration::from_millis(position_ms as u64);
        if let Err(e) = self.sink.try_seek(pos) {
            warn!("LocalPlayer: seek failed: {e}");
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
        self.repeat
    }

    fn current_index(&self) -> Option<usize> {
        self.current_idx
    }

    fn current_uri(&self) -> Option<String> {
        self.current_idx
            .and_then(|idx| self.queue.get(idx))
            .map(|t| t.uri.clone())
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

    fn current_track_info(&self) -> Option<TrackInfo> {
        let t = self.current_track_meta()?;
        Some(TrackInfo {
            name: t.name.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
            uri: t.uri.clone(),
        })
    }
}