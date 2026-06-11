use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::player::{AudioPlayer, QueuedTrack, RepeatMode};

pub struct MockPlayer {
    pub is_playing: bool,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub next_called: Arc<AtomicBool>,
    pub prev_called: Arc<AtomicBool>,
    pub queue: Vec<String>,
    pub user_queue: Vec<QueuedTrack>,
    pub playing_queued: Option<QueuedTrack>,
    pub current_index: Option<usize>,
}

impl MockPlayer {
    pub fn new(next_called: Arc<AtomicBool>, prev_called: Arc<AtomicBool>) -> Self {
        Self {
            is_playing: false,
            volume: 50,
            shuffle: false,
            repeat: RepeatMode::Off,
            next_called,
            prev_called,
            queue: Vec::new(),
            user_queue: Vec::new(),
            playing_queued: None,
            current_index: None,
        }
    }

    pub fn with_queue(queue: Vec<QueuedTrack>) -> Self {
        Self {
            is_playing: false,
            volume: 50,
            shuffle: false,
            repeat: RepeatMode::Off,
            next_called: Arc::default(),
            prev_called: Arc::default(),
            queue: Vec::new(),
            user_queue: queue,
            playing_queued: None,
            current_index: None,
        }
    }
}

impl AudioPlayer for MockPlayer {
    fn play(&mut self) {
        self.is_playing = true;
    }
    fn pause(&mut self) {
        self.is_playing = false;
    }
    fn toggle(&mut self) {
        self.is_playing = !self.is_playing;
    }
    fn is_playing(&self) -> bool {
        self.is_playing
    }
    fn next(&mut self) -> bool {
        self.next_called.store(true, Ordering::Relaxed);
        true
    }
    fn prev(&mut self) -> bool {
        self.prev_called.store(true, Ordering::Relaxed);
        true
    }
    fn play_at(&mut self, _index: usize) {}
    fn seek(&self, _position_ms: u32) {}
    fn volume(&self) -> u8 {
        self.volume
    }
    fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(10).min(100);
    }
    fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(10);
    }
    fn set_volume(&mut self, volume: u8) {
        self.volume = volume;
    }
    fn shuffle(&self) -> bool {
        self.shuffle
    }
    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
    }
    fn repeat(&self) -> RepeatMode {
        self.repeat
    }
    fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Off,
        };
    }
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.queue = uris;
        self.current_index = Some(start_index);
    }
    fn add_to_queue(
        &mut self,
        uri: String,
        name: String,
        artist: String,
        duration_ms: u64,
        cover_path: Option<PathBuf>,
    ) {
        self.user_queue.push(QueuedTrack {
            uri,
            name,
            artist,
            duration_ms,
            cover_path,
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
    fn current_index(&self) -> Option<usize> {
        self.current_index
    }
    fn try_recv_event(&mut self) -> Option<crate::player::PlayerNotification> {
        None
    }
    fn current_uri(&self) -> Option<String> {
        None
    }
    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        None
    }
}
