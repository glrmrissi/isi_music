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
use crate::config;
use std::sync::Arc;
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
}

pub struct QueuedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub duration_ms: u64,
}

pub struct NativePlayer {
    player: Arc<LibrespotPlayer>,
    mixer: Arc<dyn Mixer>,
    queue: Vec<String>,
    pub user_queue: Vec<QueuedTrack>,
    /// Set after next() plays from user_queue; caller should read and clear
    pub playing_queued: Option<QueuedTrack>,
    current_index: Option<usize>,
    pub is_playing: bool,
    pub volume: u8, // 0–100
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub event_rx: mpsc::UnboundedReceiver<PlayerNotification>,
}

impl NativePlayer {
    /// `low_resource`: use 96kbps bitrate + smaller buffer (for daemon/background mode)
    pub async fn new(access_token: String, low_resource: bool) -> Result<Self> {
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

        let bitrate = if low_resource {
            librespot_playback::config::Bitrate::Bitrate96
        } else {
            librespot_playback::config::Bitrate::Bitrate320
        };
        let player = LibrespotPlayer::new(
            PlayerConfig { gapless: false, bitrate, ..PlayerConfig::default() },
            session,
            volume_getter,
            move || backend(None, audio_format),
        );

        // Notification channel for the App
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();

        // Monitor player events in background
        let mut event_channel = player.get_player_event_channel();
        tokio::spawn(async move {
            while let Some(event) = event_channel.recv().await {
                match event {
                    PlayerEvent::Playing { track_id, .. } => {
                        info!("Playing: {}", track_id);
                        let _ = notif_tx.send(PlayerNotification::Playing);
                    }
                    PlayerEvent::Paused { track_id, .. } => {
                        info!("Paused: {}", track_id);
                        let _ = notif_tx.send(PlayerNotification::Paused);
                    }
                    PlayerEvent::EndOfTrack { track_id, .. } => {
                        info!("End of track: {}", track_id);
                        let _ = notif_tx.send(PlayerNotification::TrackEnded);
                    }
                    PlayerEvent::Unavailable { track_id, .. } => {
                        error!("Track unavailable (Premium required?): {}", track_id);
                        let _ = notif_tx.send(PlayerNotification::TrackUnavailable);
                    }
                    PlayerEvent::Loading { track_id, .. } => {
                        info!("Loading: {}", track_id);
                    }
                    _ => {}
                }
            }
        });

        let volume = config::load_volume();
        let mut instance = Self {
            player,
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
        };
        instance.apply_volume();
        Ok(instance)
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
                // Tell glibc to return freed pages (decoder/buffer from previous track) to the OS
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
        self.player.pause();
        self.is_playing = false;
    }

    pub fn toggle(&mut self) {
        if self.is_playing { self.pause() } else { self.play() }
    }

    pub fn next(&mut self) -> bool {
        self.playing_queued = None;
        // Track repeat: re-play the same track
        if self.repeat == RepeatMode::Track {
            if let Some(idx) = self.current_index {
                self.play_at(idx);
                return true;
            }
        }
        // User queue has priority
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
                let mut candidates: Vec<usize> = (0..len).filter(|&i| i != idx).collect();
                *candidates.choose(&mut rng).unwrap_or(&((idx + 1) % len))
            } else {
                idx + 1
            };
            if next < len {
                self.play_at(next);
                return true;
            }
            // End of queue — wrap around if repeat Queue
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
}
