use std::{
    fs::File,
    io::BufReader,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use rodio::{Decoder, OutputStreamBuilder, Sink};
use crate::audio_sink::AnalyzingSource;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::{AudioPlayer, PlayerNotification, QueuedTrack, RepeatMode};
use crate::audio_sink::N_BANDS;

struct LocalTrack {
    path: PathBuf,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    artist: String,
    #[allow(dead_code)]
    album: String,
    #[allow(dead_code)]
    duration_ms: u64,
}

pub struct LocalPlayer {
    sink: Sink,
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
    pub fn new(volume: u8) -> anyhow::Result<Self> {
        let stream = OutputStreamBuilder::open_default_stream()
            .map_err(|e| anyhow::anyhow!("Audio output unavailable: {e}"))?;

        let sink = Sink::connect_new(&stream.mixer());

        std::thread::spawn(move || {
            let _keep_alive = stream;
            std::thread::park(); 
        });

        sink.set_volume(volume as f32 / 100.0);

        let (tx, rx) = mpsc::unbounded_channel();

        Ok(Self {
            sink,
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

    fn uri_to_path(uri: &str) -> PathBuf {
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

        let analyzing = AnalyzingSource::new(
            decoder,
            Arc::clone(&self.band_energies),
        );
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
                path,
                name: track.name.clone(),
                artist: track.artist.clone(),
                album: String::new(),
                duration_ms: track.duration_ms,
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

     fn current_uri(&self) -> Option<String> {
        self.current_idx.and_then(|idx| {
            self.queue.get(idx).map(|track| track.path.to_string_lossy().to_string())
        })
    }
}

impl AudioPlayer for LocalPlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.queue = uris.into_iter().map(|uri| {
            let path = Self::uri_to_path(&uri);
            let name = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
            LocalTrack { path, name, artist: String::new(), album: String::new(), duration_ms: 0, }
        }).collect();
        self.play_at(start_index);
    }

    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64) {
        self.user_queue.push(QueuedTrack { uri, name, artist, duration_ms });
    }

    fn user_queue(&self) -> &[QueuedTrack] { &self.user_queue }
    fn remove_from_user_queue(&mut self, index: usize) { self.user_queue.remove(index); }
    fn take_playing_queued(&mut self) -> Option<QueuedTrack> { self.playing_queued.take() }

    fn play(&mut self) { self.sink.play(); self.is_playing = true; }
    fn pause(&mut self) { self.sink.pause(); self.is_playing = false; }
    fn toggle(&mut self) { self.toggle(); }
    fn next(&mut self) -> bool { self.next() }
    fn prev(&mut self) -> bool { self.prev() }
    fn play_at(&mut self, index: usize) { self.play_at(index); }
    fn seek(&self, position_ms: u32) {
        let pos = std::time::Duration::from_millis(position_ms as u64);
        let _ = self.sink.try_seek(pos);
    }

    fn is_playing(&self) -> bool { self.is_playing }
    fn volume(&self) -> u8 { self.volume }
    fn shuffle(&self) -> bool { self.shuffle }
    fn repeat(&self) -> RepeatMode { self.repeat }
    
    fn current_index(&self) -> Option<usize> { self.current_idx }
    fn current_uri(&self) -> Option<String> { self.current_uri() }

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
    fn toggle_shuffle(&mut self) { self.shuffle = !self.shuffle; }
    fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off   => RepeatMode::Queue,
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
}