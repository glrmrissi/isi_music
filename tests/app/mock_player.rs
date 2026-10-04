use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::player::{AudioPlayer, PlayerNotification, QueuedTrack, RepeatMode};

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
    pub notifications: std::collections::VecDeque<PlayerNotification>,
    pub current_index: Option<usize>,
    pub mono_enabled: Arc<AtomicBool>,
}

impl MockPlayer {
    #[allow(dead_code)]
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
            notifications: std::collections::VecDeque::new(),
            current_index: None,
            mono_enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)]
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
            notifications: std::collections::VecDeque::new(),
            current_index: None,
            mono_enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)]
    pub fn push_notification(&mut self, notification: PlayerNotification) {
        self.notifications.push_back(notification);
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
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
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
        album: String,
        duration_ms: u64,
        cover_path: Option<PathBuf>,
    ) {
        self.user_queue.push(QueuedTrack {
            uri,
            name,
            artist,
            album,
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
    fn play_from_user_queue(&mut self, index: usize) -> bool {
        if index < self.user_queue.len() {
            let track = self.user_queue.remove(index);
            self.playing_queued = Some(track);
            true
        } else {
            false
        }
    }
    fn current_index(&self) -> Option<usize> {
        self.current_index
    }
    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        (self.queue.clone(), self.current_index)
    }
    fn snapshot_user_queue(&self) -> Vec<QueuedTrack> {
        self.user_queue.clone()
    }
    fn current_track_summary(&self) -> Option<crate::spotify::TrackSummary> {
        self.queue
            .get(self.current_index?)
            .map(|uri| crate::spotify::TrackSummary {
                name: String::new(),
                artist: String::new(),
                album: String::new(),
                duration_ms: 0,
                uri: uri.clone(),
                cover_path: None,
                added_at: None,
            })
    }
    fn try_recv_event(&mut self) -> Option<crate::player::PlayerNotification> {
        self.notifications.pop_front()
    }
    fn set_mono_enabled(&mut self, enabled: bool) {
        self.mono_enabled.store(enabled, Ordering::Relaxed);
    }
    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        None
    }
}
