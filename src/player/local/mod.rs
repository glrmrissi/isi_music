pub mod db;
pub mod decoder;
pub mod playback;
pub mod queue;
pub mod track;
pub mod trait_impl;

pub use track::LocalTrack;

use anyhow::Result;
use rodio::{OutputStreamBuilder, Sink};
use rusqlite::Connection;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{PlayerNotification, QueuedTrack, RepeatMode};
use crate::audio::audio_sink::SharedAnalyzerState;

pub struct LocalPlayer {
    sink: Arc<Sink>,
    db_conn: Connection,
    queue: Vec<LocalTrack>,
    user_queue: Vec<QueuedTrack>,
    temp_queue: Vec<LocalTrack>,
    temp_playing: Option<usize>,
    playing_context: Option<Vec<String>>,
    playing_context_pos: Option<usize>,
    playing_queued: Option<QueuedTrack>,
    current_idx: Option<usize>,
    pub is_playing: bool,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    event_tx: mpsc::UnboundedSender<PlayerNotification>,
    event_rx: mpsc::UnboundedReceiver<PlayerNotification>,
    pub analyzer: Arc<SharedAnalyzerState>,
    load_guard: Option<Instant>,
    pub is_seeking: Arc<AtomicBool>,
    seek_deadline: Arc<Mutex<Option<Instant>>>,
    waveform: Arc<Mutex<Option<Vec<u8>>>>,
    duration_measured: Arc<Mutex<Option<u64>>>,
    track_token: Arc<AtomicU64>,
    play_history: Vec<usize>,
    audio_shutdown: Arc<AtomicBool>,
    audio_thread: Option<std::thread::JoinHandle<()>>,
}

impl LocalPlayer {
    pub fn new(volume: u8, db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        db::init_db(&conn)?;

        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        let audio_shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&audio_shutdown);

        let audio_thread =
            std::thread::spawn(move || match OutputStreamBuilder::open_default_stream() {
                Ok(stream) => {
                    let sink = Sink::connect_new(stream.mixer());
                    if sync_tx.send(Ok(sink)).is_ok() {
                        let _keep_alive = stream;
                        while !shutdown_clone.load(Ordering::Relaxed) {
                            std::thread::park_timeout(std::time::Duration::from_millis(500));
                        }
                    }
                }
                Err(e) => {
                    let _ = sync_tx.send(Err(anyhow::anyhow!("Audio output unavailable: {e}")));
                }
            });

        let sink = sync_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Audio thread panicked during startup"))??;

        sink.set_volume(volume as f32 / 100.0);
        let (tx, rx) = mpsc::unbounded_channel();

        let mut instance = Self {
            sink: Arc::new(sink),
            db_conn: conn,
            queue: Vec::new(),
            user_queue: Vec::new(),
            temp_queue: Vec::new(),
            temp_playing: None,
            playing_context: None,
            playing_context_pos: None,
            playing_queued: None,
            current_idx: None,
            is_playing: false,
            volume,
            shuffle: false,
            repeat: RepeatMode::Off,
            event_tx: tx,
            event_rx: rx,
            analyzer: Arc::new(SharedAnalyzerState::new()),
            load_guard: None,
            is_seeking: Arc::new(AtomicBool::new(false)),
            seek_deadline: Arc::new(Mutex::new(None)),
            waveform: Arc::new(Mutex::new(None)),
            duration_measured: Arc::new(Mutex::new(None)),
            track_token: Arc::new(AtomicU64::new(0)),
            play_history: Vec::new(),
            audio_shutdown,
            audio_thread: Some(audio_thread),
        };

        if let Err(e) = instance.reload_library_from_db() {
            warn!("Failed to load songs from SQLite: {}", e);
        } else {
            info!("Songs loaded: {} tracks", instance.queue.len());
        }

        Ok(instance)
    }

    pub fn current_track_meta(&self) -> Option<&LocalTrack> {
        if let Some(temp_idx) = self.temp_playing {
            self.temp_queue.get(temp_idx)
        } else {
            self.current_idx.and_then(|i| self.queue.get(i))
        }
    }

    pub(super) fn apply_volume(&self) {
        self.sink.set_volume(self.volume as f32 / 100.0);
    }
}

impl Drop for LocalPlayer {
    fn drop(&mut self) {
        self.audio_shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.audio_thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}
