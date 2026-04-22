pub mod local;
pub use local::LocalPlayer;

use anyhow::{Context, Result};
use librespot_core::{
    authentication::Credentials,
    config::SessionConfig,
    session::Session,
    spotify_uri::SpotifyUri,
};
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, Mixer, MixerConfig},
    player::{Player as LibrespotPlayer, PlayerEvent},
};
use crate::audio_sink::{AnalyzerSink, N_BANDS};
use crate::config;
use std::sync::{Arc, Mutex};
use rand::seq::SliceRandom;
#[cfg(target_os = "linux")]
use libc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Clone, Copy, PartialEq, Default)]
pub enum RepeatMode { #[default] Off, Track, Queue }

pub enum PlayerNotification {
    TrackEnded,
    TrackUnavailable,
    Playing,
    Paused,
    SessionLost,
    FreeAccountDetected,
}

pub trait AudioPlayer: Send {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize);
    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64);
    fn user_queue(&self) -> &[QueuedTrack];
    fn remove_from_user_queue(&mut self, index: usize);
    fn take_playing_queued(&mut self) -> Option<QueuedTrack>;

    fn play(&mut self);
    fn pause(&mut self);
    fn toggle(&mut self);
    fn next(&mut self) -> bool;
    fn prev(&mut self) -> bool;
    fn play_at(&mut self, index: usize);
    fn seek(&self, position_ms: u32);

    fn is_playing(&self) -> bool;
    fn volume(&self) -> u8;
    fn shuffle(&self) -> bool;
    fn repeat(&self) -> RepeatMode;
    fn current_index(&self) -> Option<usize>;

    fn volume_up(&mut self);
    fn volume_down(&mut self);
    fn set_volume(&mut self, volume: u8);
    fn toggle_shuffle(&mut self);
    fn cycle_repeat(&mut self);

    fn try_recv_event(&mut self) -> Option<PlayerNotification>;

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) { (vec![], None) }

    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> { None }

    fn current_uri(&self) -> Option<String>;
}

pub struct QueuedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub duration_ms: u64,
}

pub struct NativePlayer {
    player: Arc<LibrespotPlayer>,
    _session: Session,
    mixer: Arc<dyn Mixer>,
    queue: Vec<String>,
    pub user_queue: Vec<QueuedTrack>,
    pub playing_queued: Option<QueuedTrack>,
    current_index: Option<usize>,
    pub is_playing: bool,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub event_rx: mpsc::UnboundedReceiver<PlayerNotification>,
    pub band_energies: Arc<Mutex<Vec<f32>>>,
}

impl NativePlayer {
    pub async fn new(access_token: String, _low_resource: bool) -> Result<Self> {
        let session = Session::new(SessionConfig::default(), None);
        let credentials = Credentials::with_access_token(access_token);
        session
            .connect(credentials, false)
            .await
            .context("Failed to connect librespot session")?;

        info!("Librespot session established");

        let audio_format = AudioFormat::default();
        let backend = audio_backend::find(None)
            .context("No audio backend found")?;

        let mixer_fn = mixer::find(None).context("No mixer found")?;
        let soft_mixer = mixer_fn(MixerConfig::default()).context("Failed to create mixer")?;
        let volume_getter = soft_mixer.get_soft_volume();

        let bitrate = librespot_playback::config::Bitrate::Bitrate320;

        let bands = Arc::new(Mutex::new(vec![0.0f32; N_BANDS]));
        let bands_for_sink = Arc::clone(&bands);

        let session_for_player = session.clone();
        let player = LibrespotPlayer::new(
            PlayerConfig { gapless: false, bitrate, ..PlayerConfig::default() },
            session_for_player,
            volume_getter,
            move || Box::new(AnalyzerSink::new(backend(None, audio_format), Arc::clone(&bands_for_sink))),
        );

        let (notif_tx, notif_rx) = mpsc::unbounded_channel();

        let mut event_channel = player.get_player_event_channel();
        let session_for_monitor = session.clone();
        tokio::spawn(async move {
            let mut unavailable_count = 0u32;
            while let Some(event) = event_channel.recv().await {
                match event {
                    PlayerEvent::Playing { track_id, .. } => {
                        info!("Playing: {}", track_id);
                        unavailable_count = 0;
                        let _ = notif_tx.send(PlayerNotification::Playing);
                    }
                    PlayerEvent::Paused { track_id, .. } => {
                        info!("Paused: {}", track_id);
                        let _ = notif_tx.send(PlayerNotification::Paused);
                    }
                    PlayerEvent::EndOfTrack { track_id, .. } => {
                        info!("End of track: {}", track_id);
                        unavailable_count = 0;
                        let _ = notif_tx.send(PlayerNotification::TrackEnded);
                    }
                    PlayerEvent::Unavailable { track_id, .. } => {
                        error!("Track unavailable: {}", track_id);
                        unavailable_count += 1;
                        if unavailable_count >= 2 {
                            warn!("Multiple consecutive unavailable tracks — likely free account");
                            let _ = notif_tx.send(PlayerNotification::FreeAccountDetected);
                        } else if session_for_monitor.is_invalid() {
                            let _ = notif_tx.send(PlayerNotification::SessionLost);
                        } else {
                            let _ = notif_tx.send(PlayerNotification::TrackUnavailable);
                        }
                    }
                    PlayerEvent::Loading { track_id, .. } => {
                        info!("Loading: {}", track_id);
                    }
                    _ => {}
                }
            }
            if session_for_monitor.is_invalid() {
                warn!("Player event channel closed with invalid session");
            }
        });

        let volume = config::load_volume();
        let instance = Self {
            player,
            _session: session,
            mixer: soft_mixer,
            queue: Vec::new(),
            user_queue: Vec::new(),
            playing_queued: None,
            current_index: None,
            is_playing: false,
            volume,
            shuffle: false,
            repeat: RepeatMode::Off,
            event_rx: notif_rx,
            band_energies: bands,
        };
        instance.apply_volume();
        Ok(instance)
    }

    pub fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        (self.queue.clone(), self.current_index)
    }

    pub fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.queue = uris;
        self.play_at(start_index);
    }

    pub fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64) {
        self.user_queue.push(QueuedTrack { uri, name, artist, duration_ms });
    }

    pub fn user_queue(&self) -> &[QueuedTrack] {
        &self.user_queue
    }

    pub fn play_at(&mut self, index: usize) {
        let Some(uri) = self.queue.get(index) else {
            warn!("Index {index} out of queue bounds");
            return;
        };
        match SpotifyUri::from_uri(uri) {
            Ok(spotify_uri) => {
                info!("Loading URI: {uri}");
                self.player.stop();
                self.player.load(spotify_uri, true, 0);
                self.current_index = Some(index);
                self.is_playing = true;
                self.playing_queued = None;
                #[cfg(target_os = "linux")]
                unsafe { libc::malloc_trim(0); }
            }
            Err(e) => error!("Invalid URI '{uri}': {e}"),
        }
    }

    pub fn play(&mut self) {
        self.player.play();
        self.is_playing = true;
    }

    pub fn pause(&mut self) {
        if self.is_playing {
            self.player.pause();
            self.is_playing = false;
        }
    }

    pub fn toggle(&mut self) {
        if self.is_playing { self.pause() } else { self.play() }
    }

    pub fn next(&mut self) -> bool {
        self.playing_queued = None;
        if self.repeat == RepeatMode::Track {
            if let Some(idx) = self.current_index {
                self.play_at(idx);
                return true;
            }
        }
        if !self.user_queue.is_empty() {
            let track = self.user_queue.remove(0);
            match SpotifyUri::from_uri(&track.uri) {
                Ok(spotify_uri) => {
                    info!("Playing from user queue: {}", track.uri);
                    self.player.stop();
                    self.player.load(spotify_uri, true, 0);
                    self.is_playing = true;
                    #[cfg(target_os = "linux")]
                    unsafe { libc::malloc_trim(0); }
                    self.playing_queued = Some(track);
                    return true;
                }
                Err(e) => error!("Invalid URI in user queue: {e}"),
            }
        }
        if let Some(idx) = self.current_index {
            let len = self.queue.len();
            let next = if self.shuffle && len > 1 {
                let mut rng = rand::thread_rng();
                let candidates: Vec<usize> = (0..len).filter(|&i| i != idx).collect();
                *candidates.choose(&mut rng).unwrap_or(&((idx + 1) % len))
            } else {
                idx + 1
            };
            if next < len {
                self.play_at(next);
                return true;
            }
            if self.repeat == RepeatMode::Queue && len > 0 {
                self.play_at(0);
                return true;
            }
        }
        false
    }

    pub fn prev(&mut self) -> bool {
        if let Some(idx) = self.current_index {
            if idx > 0 {
                self.play_at(idx - 1);
                return true;
            }
        }
        false
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off   => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.apply_volume();
        config::save_volume(self.volume);
    }

    pub fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.apply_volume();
        config::save_volume(self.volume);
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    fn apply_volume(&self) {
        let v = (self.volume as u32 * 65535 / 100) as u16;
        self.mixer.set_volume(v);
    }

    pub fn current_uri(&self) -> Option<String> {
        self.current_index().and_then(|i| self.queue.get(i)).map(|u| u.clone())
    }
}

impl AudioPlayer for NativePlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) { self.set_queue(uris, start_index); }
    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64) { self.add_to_queue(uri, name, artist, duration_ms); }
    fn user_queue(&self) -> &[QueuedTrack] { self.user_queue() }
    fn remove_from_user_queue(&mut self, index: usize) { self.user_queue.remove(index); }
    fn take_playing_queued(&mut self) -> Option<QueuedTrack> { self.playing_queued.take() }
    fn current_uri(&self) -> Option<String> { self.current_uri() }

    fn play(&mut self) { self.play(); }
    fn pause(&mut self) { self.pause(); }
    fn toggle(&mut self) { self.toggle(); }
    fn next(&mut self) -> bool { self.next() }
    fn prev(&mut self) -> bool { self.prev() }
    fn play_at(&mut self, index: usize) { self.play_at(index); }
    fn seek(&self, position_ms: u32) { self.seek(position_ms); }

    fn is_playing(&self) -> bool { self.is_playing }
    fn volume(&self) -> u8 { self.volume }
    fn shuffle(&self) -> bool { self.shuffle }
    fn repeat(&self) -> RepeatMode { self.repeat }
    fn current_index(&self) -> Option<usize> { self.current_index() }

    fn volume_up(&mut self) { self.volume_up(); }
    fn volume_down(&mut self) { self.volume_down(); }
    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.apply_volume();
        config::save_volume(self.volume);
    }
    fn toggle_shuffle(&mut self) { self.toggle_shuffle(); }
    fn cycle_repeat(&mut self) { self.cycle_repeat(); }

    fn try_recv_event(&mut self) -> Option<PlayerNotification> {
        self.event_rx.try_recv().ok()
    }

    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        Some(Arc::clone(&self.band_energies))
    }

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) { self.snapshot_queue() }
}