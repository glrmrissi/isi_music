use std::{
    fs::File,
    io::BufReader,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use rusqlite::Connection;
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
    pub duration_ms: u64,
    pub cover_path: Option<PathBuf>,
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
    load_guard: Option<Instant>,
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
                cover_path TEXT
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
            load_guard: None,
        };

        if let Err(e) = instance.reload_library_from_db() {
            error!("Failed to load songs from SQLite: {}", e);
        } else {
            info!("Songs loaded: {} tracks", instance.queue.len());
        }

        Ok(instance)
    }

    pub fn reload_library_from_db(&mut self) -> anyhow::Result<()> {
        let mut stmt = self.db_conn.prepare(
            "SELECT id, path, title, artist, album, duration_ms, cover_path
             FROM tracks
             ORDER BY artist, album, title",
        )?;

        let tracks = stmt
            .query_map([], |row| {
                let path_str: String = row.get(1)?;
                let cover_path_str: Option<String> = row.get(6)?;
                Ok(LocalTrack {
                    id: row.get(0)?,
                    path: PathBuf::from(&path_str),
                    uri: format!("file://{}", path_str),
                    name: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "Unknown".to_string()),
                    artist: row
                        .get::<_, Option<String>>(3)?
                        .unwrap_or_default(),
                    album: row
                        .get::<_, Option<String>>(4)?
                        .unwrap_or_default(),
                    duration_ms: row
                        .get::<_, Option<i64>>(5)?
                        .unwrap_or(0) as u64,
                    cover_path: cover_path_str.map(PathBuf::from),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        info!("Library reloaded: {} tracks from SQLite", tracks.len());
        self.queue = tracks;
        Ok(())
    }

    fn load_and_play(&mut self, idx: usize) {
        let Some(track) = self.queue.get(idx) else {
            warn!("LocalPlayer: index {idx} out of bounds");
            return;
        };
        let path = track.path.clone();

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
        self.load_guard = Some(Instant::now());
        let _ = self.event_tx.send(PlayerNotification::Playing);
    }

    fn seek_by_reload(&mut self, position_ms: u32) {
        let Some(idx) = self.current_idx else { return };
        let Some(track) = self.queue.get(idx) else { return };
        let path = track.path.clone();

        self.sink.stop();

        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                error!("LocalPlayer: seek_by_reload cannot open {:?}: {e}", path);
                return;
            }
        };

        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(d) => d,
            Err(e) => {
                error!("LocalPlayer: seek_by_reload cannot decode {:?}: {e}", path);
                return;
            }
        };

        let skip_duration = std::time::Duration::from_millis(position_ms as u64);
        let skipped = SkipDecoder::new(decoder, skip_duration);
        let analyzing = AnalyzingSource::new(skipped, Arc::clone(&self.band_energies));
        self.sink.append(analyzing);
        self.sink.play();
        self.is_playing = true;
        self.load_guard = Some(Instant::now());
    }

    fn poll_sink(&mut self) {
        if let Some(t) = self.load_guard {
            if t.elapsed().as_millis() < 500 {
                return;
            }
            self.load_guard = None;
        }

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
            let path = LocalTrack::uri_to_path(&track.uri);
            let lt = LocalTrack {
                id: -1,
                path,
                uri: track.uri.clone(),
                name: track.name.clone(),
                artist: track.artist.clone(),
                album: String::new(),
                duration_ms: track.duration_ms,
                cover_path: track.cover_path.clone(),
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

    pub fn prev_inner(&mut self) -> bool {
        if let Some(idx) = self.current_idx {
            let target = if idx > 0 { idx - 1 } else { 0 };
            self.load_and_play(target);
            return true;
        }
        false
    }

    fn apply_volume(&self) {
        self.sink.set_volume(self.volume as f32 / 100.0);
    }
}

impl LocalTrack {
    fn uri_to_path(uri: &str) -> PathBuf {
        if let Some(s) = uri.strip_prefix("file://") {
            PathBuf::from(s)
        } else {
            PathBuf::from(uri)
        }
    }
}

impl AudioPlayer for LocalPlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        let target_uri = match uris.get(start_index) {
            Some(u) => u,
            None => return,
        };
        if let Some(idx) = self.queue.iter().position(|t| &t.uri == target_uri) {
            self.load_and_play(idx);
        }
    }

    fn set_queue_tracks(&mut self, tracks: Vec<TrackSummary>, start_index: usize) {
        let target_uri = match tracks.get(start_index) {
            Some(t) => &t.uri,
            None => return,
        };
        if let Some(idx) = self.queue.iter().position(|t| &t.uri == target_uri) {
            self.load_and_play(idx);
        }
    }

    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64, cover_path: Option<PathBuf>) {
        self.user_queue.push(QueuedTrack { uri, name, artist, duration_ms, cover_path });
    }

    fn user_queue(&self) -> &[QueuedTrack] { &self.user_queue }

    fn remove_from_user_queue(&mut self, index: usize) {
        if index < self.user_queue.len() { self.user_queue.remove(index); }
    }

    fn take_playing_queued(&mut self) -> Option<QueuedTrack> { self.playing_queued.take() }

    fn play(&mut self) { self.sink.play(); self.is_playing = true; }

    fn pause(&mut self) { self.sink.pause(); self.is_playing = false; }

    fn toggle(&mut self) {
        if self.sink.is_paused() { self.play(); } else { self.pause(); }
    }

    fn next(&mut self) -> bool { self.next_inner() }

    fn prev(&mut self) -> bool { self.prev_inner() }

    fn play_at(&mut self, index: usize) { self.load_and_play(index); }

    fn seek(&self, _position_ms: u32) {}

    fn seek_mut(&mut self, position_ms: u32) { self.seek_by_reload(position_ms); }

    fn is_playing(&self) -> bool { self.is_playing }

    fn volume(&self) -> u8 { self.volume }

    fn shuffle(&self) -> bool { self.shuffle }

    fn repeat(&self) -> RepeatMode { self.repeat.clone() }

    fn current_index(&self) -> Option<usize> { self.current_idx }

    fn current_uri(&self) -> Option<String> {
        self.current_idx.and_then(|i| self.queue.get(i)).map(|t| t.uri.clone())
    }

    fn current_track_info(&self) -> Option<TrackInfo> {
        let t = self.current_track_meta()?;
        Some(TrackInfo {
            uri: t.uri.clone(),
            name: t.name.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
            path: Some(t.path.clone()),
            cover_path: t.cover_path.clone(),
        })
    }

    fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.apply_volume();
    }

    fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.apply_volume();
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.apply_volume();
    }

    fn toggle_shuffle(&mut self) { self.shuffle = !self.shuffle; }

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

#[allow(dead_code)]
struct SkipDecoder<D>
where
    D: Source,
{
    inner: D,
    remaining: u32,
}

impl<D> SkipDecoder<D>
where
    D: Source,
{
    #[allow(dead_code)]
    fn new(inner: D, duration: std::time::Duration) -> Self {
        let sample_rate = inner.sample_rate().max(1) as u32;
        let channels = inner.channels() as u32;
        let remaining = (duration.as_secs_f32() * sample_rate as f32 * channels as f32) as u32;
        Self { inner, remaining }
    }
}

impl<D> Iterator for SkipDecoder<D>
where
    D: Source,
{
    type Item = D::Item;

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining > 0 {
            self.remaining -= 1;
            let _ = self.inner.next();
        }
        self.inner.next()
    }
}

impl<D> rodio::Source for SkipDecoder<D>
where
    D: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}