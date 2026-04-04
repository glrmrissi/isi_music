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
}

// ── Backend-agnostic player trait ─────────────────────────────────────────────

/// Common interface for any audio backend (librespot, local files, etc.).
/// All methods are synchronous — backends manage their own async internals.
pub trait AudioPlayer: Send {
    // ── Queue ────────────────────────────────────────────────────────────────
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize);
    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64);
    fn user_queue(&self) -> &[QueuedTrack];
    fn remove_from_user_queue(&mut self, index: usize);
    /// Take the `QueuedTrack` that was just promoted from the user queue, if any.
    fn take_playing_queued(&mut self) -> Option<QueuedTrack>;

    // ── Playback ─────────────────────────────────────────────────────────────
    fn play(&mut self);
    fn pause(&mut self);
    fn toggle(&mut self);
    fn next(&mut self) -> bool;
    fn prev(&mut self) -> bool;
    fn play_at(&mut self, index: usize);
    fn seek(&self, position_ms: u32);

    // ── State ────────────────────────────────────────────────────────────────
    fn is_playing(&self) -> bool;
    fn volume(&self) -> u8;
    fn shuffle(&self) -> bool;
    fn repeat(&self) -> RepeatMode;
    fn current_index(&self) -> Option<usize>;

    // ── Volume / mode ────────────────────────────────────────────────────────
    fn volume_up(&mut self);
    fn volume_down(&mut self);
    fn set_volume(&mut self, volume: u8);
    fn toggle_shuffle(&mut self);
    fn cycle_repeat(&mut self);

    // ── Events ───────────────────────────────────────────────────────────────
    /// Non-blocking poll for the next player event.
    fn try_recv_event(&mut self) -> Option<PlayerNotification>;

    /// Returns false if the underlying session has been invalidated (e.g. connection lost).
    fn is_session_valid(&self) -> bool { true }

    /// Returns (queue_uris, current_index) for state restoration after reconnect.
    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) { (vec![], None) }

    /// Shared frequency-band energies updated in real time by the audio sink.
    /// Returns `None` if this player doesn't support audio analysis.
    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> { None }
}

pub struct QueuedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub duration_ms: u64,
}

pub struct NativePlayer {
    player: Arc<LibrespotPlayer>,
    session: Session,
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
    /// Real-time frequency band energies from the audio sink (N_BANDS values, 0..1)
    pub band_energies: Arc<Mutex<Vec<f32>>>,
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

        let bitrate = librespot_playback::config::Bitrate::Bitrate320;

        let bands = Arc::new(Mutex::new(vec![0.0f32; N_BANDS]));
        let bands_for_sink = Arc::clone(&bands);

        // Clone session before moving it into the player so we can check validity later
        let session_for_player = session.clone();
        let player = LibrespotPlayer::new(
            PlayerConfig { gapless: false, bitrate, ..PlayerConfig::default() },
            session_for_player,
            volume_getter,
            move || Box::new(AnalyzerSink::new(backend(None, audio_format), Arc::clone(&bands_for_sink))),
        );

        // Notification channel for the App
        let (notif_tx, notif_rx) = mpsc::unbounded_channel();

        // Monitor player events in background
        let mut event_channel = player.get_player_event_channel();
        let session_for_monitor = session.clone();
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
                        if session_for_monitor.is_invalid() {
                            error!("Session lost — track unavailable: {}", track_id);
                            let _ = notif_tx.send(PlayerNotification::SessionLost);
                        } else {
                            error!("Track unavailable (Premium required?): {}", track_id);
                            let _ = notif_tx.send(PlayerNotification::TrackUnavailable);
                        }
                    }
                    PlayerEvent::Loading { track_id, .. } => {
                        info!("Loading: {}", track_id);
                    }
                    _ => {}
                }
            }
            // Event channel closed — session likely died
            if session_for_monitor.is_invalid() {
                warn!("Player event channel closed with invalid session");
            }
        });

        let volume = config::load_volume();
        let mut instance = Self {
            player,
            session,
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

    pub fn is_session_valid(&self) -> bool {
        !self.session.is_invalid()
    }

    /// Returns the current queue URIs and the current index, useful for restoring
    /// state after a session reconnect.
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

impl AudioPlayer for NativePlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) { self.set_queue(uris, start_index); }
    fn add_to_queue(&mut self, uri: String, name: String, artist: String, duration_ms: u64) { self.add_to_queue(uri, name, artist, duration_ms); }
    fn user_queue(&self) -> &[QueuedTrack] { self.user_queue() }
    fn remove_from_user_queue(&mut self, index: usize) { self.user_queue.remove(index); }
    fn take_playing_queued(&mut self) -> Option<QueuedTrack> { self.playing_queued.take() }

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

    fn is_session_valid(&self) -> bool { self.is_session_valid() }
    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) { self.snapshot_queue() }
}
