// TODO: modularize this file (~560 lines) into smaller modules
pub mod local;
pub use local::LocalPlayer;

use crate::audio::audio_sink::{AnalyzerSink, N_BANDS};
use crate::config;
use crate::spotify::TrackSummary;
use crate::ui::PlaybackState;
use anyhow::{Context, Result};
use librespot_core::{
    authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
    spotify_uri::SpotifyUri,
};
use librespot_playback::{
    audio_backend::{self, Sink},
    config::{AudioFormat, PlayerConfig},
    mixer::{self, Mixer, MixerConfig},
    player::{Player as LibrespotPlayer, PlayerEvent},
};

use rand::seq::SliceRandom;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::config::OFFICIAL_CLIENT_ID;

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum RepeatMode {
    #[default]
    Off,
    Track,
    Queue,
}

pub enum PlayerNotification {
    TrackEnded,
    TrackUnavailable,
    Playing,
    Paused,
    SessionLost,
    FreeAccountDetected,
    PreloadNextTrack,
}

pub trait AudioPlayer: Send {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize);
    fn add_to_queue(
        &mut self,
        uri: String,
        name: String,
        artist: String,
        album: String,
        duration_ms: u64,
        cover_path: Option<PathBuf>,
    );
    fn user_queue(&self) -> &[QueuedTrack];
    fn remove_from_user_queue(&mut self, index: usize);
    fn take_playing_queued(&mut self) -> Option<QueuedTrack>;
    fn play_from_user_queue(&mut self, index: usize) -> bool;

    fn set_queue_tracks(&mut self, tracks: &[TrackSummary], start_index: usize) {
        let uris = tracks.iter().map(|t| t.uri.clone()).collect();
        self.set_queue(uris, start_index);
    }

    fn play(&mut self);
    fn pause(&mut self);
    fn toggle(&mut self);
    fn next(&mut self) -> bool;
    fn prev(&mut self) -> bool;
    fn play_at(&mut self, index: usize);
    fn seek(&self, position_ms: u32);
    fn seek_mut(&mut self, position_ms: u32) {
        self.seek(position_ms);
    }

    fn is_playing(&self) -> bool;
    fn volume(&self) -> u8;
    fn shuffle(&self) -> bool;
    fn repeat(&self) -> RepeatMode;
    fn current_index(&self) -> Option<usize>;

    /// Remove already-played tracks from the front of the queue.
    /// Returns true if the queue was actually trimmed.
    fn trim_played(&mut self, _keep_behind: usize) -> bool {
        false
    }

    fn volume_up(&mut self);
    fn volume_down(&mut self);
    fn set_volume(&mut self, volume: u8);
    fn toggle_shuffle(&mut self);
    fn cycle_repeat(&mut self);

    fn try_recv_event(&mut self) -> Option<PlayerNotification>;
    fn preload_next(&mut self) {}

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        (vec![], None)
    }
    fn set_visualizer_enabled(&mut self, _enabled: bool) {}
    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        None
    }
    fn current_playback_state(&self) -> Option<PlaybackState> {
        None
    }
}

#[derive(Clone)]
pub struct QueuedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub cover_path: Option<PathBuf>,
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
    server_position: Arc<Mutex<(u64, Instant)>>,
    analyzer_enabled: Arc<AtomicBool>,
    play_history: Vec<usize>,
}

pub async fn ensure_streaming_auth() -> Result<()> {
    if let Some(rt) = config::load_streaming_refresh_token() {
        match refresh_streaming_token(&rt).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                warn!("Streaming token refresh failed ({e}); re-authenticating");
                config::clear_streaming_refresh_token();
            }
        }
    }

    info!("Launching browser for streaming OAuth...");
    let (_, refresh_token, _) =
        crate::spotify::auth::SpotifyAuth::authenticate_with_client_id(OFFICIAL_CLIENT_ID).await?;
    config::save_streaming_refresh_token(&refresh_token);
    Ok(())
}

async fn obtain_streaming_token() -> Result<String> {
    let Some(rt) = config::load_streaming_refresh_token() else {
        anyhow::bail!("Streaming authentication is not initialized; run setup-spotify first");
    };

    match refresh_streaming_token(&rt).await {
        Ok(token) => {
            debug!("Refreshed streaming token from stored streaming refresh token");
            Ok(token)
        }
        Err(e) => {
            config::clear_streaming_refresh_token();
            Err(e.context("Streaming authentication expired; run setup-spotify again"))
        }
    }
}

async fn refresh_streaming_token(refresh_token: &str) -> Result<String> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let resp = http
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OFFICIAL_CLIENT_ID),
        ])
        .send()
        .await?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        let body = serde_json::to_string(&json).unwrap_or_default();
        anyhow::bail!("streaming token refresh {status}: {body}");
    }

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no access_token in streaming refresh response"))?
        .to_string();

    if let Some(new_rt) = json["refresh_token"].as_str() {
        config::save_streaming_refresh_token(new_rt);
    }

    Ok(access_token)
}

impl NativePlayer {
    pub async fn new(
        access_token: Option<String>,
        _low_resource: bool,
        bitrate: librespot_playback::config::Bitrate,
        gapless: bool,
    ) -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .or_else(|| dirs::config_dir())
            .map(|mut p| {
                p.push("isi-music");
                p.push("audio-cache");
                p
            });

        let cache = cache_dir.and_then(|dir| {
            match Cache::new::<std::path::PathBuf>(
                None,
                None,
                Some(dir.clone()),
                Some(1024 * 1024 * 1024),
            ) {
                Ok(c) => {
                    info!("Spotify audio cache enabled at {:?}", dir);
                    Some(c)
                }
                Err(e) => {
                    warn!("Failed to create audio cache at {:?}: {e}", dir);
                    None
                }
            }
        });

        let cfg = config::AppConfig::load()?;
        let access_token = if cfg.get_client_id().is_some() {
            obtain_streaming_token().await?
        } else {
            match access_token {
                Some(token) => token,
                None => obtain_streaming_token().await?,
            }
        };

        let session_config = SessionConfig {
            client_id: OFFICIAL_CLIENT_ID.to_string(),
            ..SessionConfig::default()
        };
        let session = Session::new(session_config, cache);
        let credentials = Credentials::with_access_token(access_token);
        session
            .connect(credentials, false)
            .await
            .context("Failed to connect librespot session")?;

        info!("Librespot session established");

        let audio_format = AudioFormat::default();
        let backend = audio_backend::find(None).context("No audio backend found")?;

        let mixer_fn = mixer::find(None).context("No mixer found")?;
        let soft_mixer = mixer_fn(MixerConfig::default()).context("Failed to create mixer")?;
        let volume_getter = soft_mixer.get_soft_volume();

        let bands = Arc::new(Mutex::new(vec![0.0f32; N_BANDS]));
        let bands_for_sink = Arc::clone(&bands);
        let analyzer_enabled = Arc::new(AtomicBool::new(false));
        let analyzer_enabled_for_sink = Arc::clone(&analyzer_enabled);

        let session_for_player = session.clone();
        let server_position: Arc<Mutex<(u64, Instant)>> = Arc::new(Mutex::new((0, Instant::now())));

        // factory to recreate inner sink (drops queued audio)
        let sink_factory: Box<dyn Fn() -> Box<dyn Sink> + Send> =
            Box::new(move || backend(None, audio_format));

        let player = LibrespotPlayer::new(
            PlayerConfig {
                gapless,
                bitrate,
                normalisation: false,
                normalisation_pregain_db: 0.0,
                position_update_interval: Some(std::time::Duration::from_millis(250)),
                ..PlayerConfig::default()
            },
            session_for_player,
            volume_getter,
            move || {
                let inner = backend(None, audio_format);
                Box::new(AnalyzerSink::with_factory(
                    inner,
                    Arc::clone(&bands_for_sink),
                    Arc::clone(&analyzer_enabled_for_sink),
                    sink_factory,
                ))
            },
        );

        let (notif_tx, notif_rx) = mpsc::unbounded_channel();

        let mut event_channel = player.get_player_event_channel();
        let session_for_monitor = session.clone();
        let sp = Arc::clone(&server_position);
        tokio::spawn(async move {
            let mut unavailable_count = 0u32;
            while let Some(event) = event_channel.recv().await {
                match event {
                    PlayerEvent::Playing {
                        track_id,
                        position_ms,
                        ..
                    } => {
                        info!("Playing: {} at {}ms", track_id, position_ms);
                        unavailable_count = 0;
                        if let Ok(mut pos) = sp.lock() {
                            *pos = (position_ms as u64, Instant::now());
                        }
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
                    PlayerEvent::TimeToPreloadNextTrack { .. } => {
                        debug!("Time to preload next track");
                        let _ = notif_tx.send(PlayerNotification::PreloadNextTrack);
                    }
                    PlayerEvent::Preloading { track_id, .. } => {
                        debug!("Preloading: {}", track_id);
                    }
                    PlayerEvent::PositionChanged { position_ms, .. } => {
                        if let Ok(mut pos) = sp.lock() {
                            *pos = (position_ms as u64, Instant::now());
                        }
                    }
                    PlayerEvent::Seeked { position_ms, .. }
                    | PlayerEvent::PositionCorrection { position_ms, .. } => {
                        if let Ok(mut pos) = sp.lock() {
                            *pos = (position_ms as u64, Instant::now());
                        }
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
            server_position,
            analyzer_enabled,
            play_history: Vec::new(),
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

    pub fn add_to_queue(
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
                if let Some(prev) = self.current_index
                    && prev != index
                {
                    self.play_history.push(prev);
                }
                self.current_index = Some(index);
                self.is_playing = true;
                self.playing_queued = None;
                self.preload_next();
            }
            Err(e) => error!("Invalid URI '{uri}': {e}"),
        }
    }

    fn load_index(&mut self, index: usize) {
        let Some(uri) = self.queue.get(index) else {
            warn!("Index {index} out of queue bounds");
            return;
        };
        match SpotifyUri::from_uri(uri) {
            Ok(spotify_uri) => {
                self.player.stop();
                self.player.load(spotify_uri, true, 0);
                self.current_index = Some(index);
                self.is_playing = true;
                self.playing_queued = None;
                self.preload_next();
            }
            Err(e) => error!("Invalid URI '{uri}': {e}"),
        }
    }

    pub fn preload_next(&mut self) {
        if self.repeat == RepeatMode::Track || self.shuffle {
            return;
        }

        if self.playing_queued.is_some() {
            if let Some(track) = self.user_queue.first()
                && let Ok(uri) = SpotifyUri::from_uri(&track.uri)
            {
                debug!("Preloading next user-queue track");
                self.player.preload(uri);
            }
            return;
        }

        let next_idx = self.next_index();

        if let Some(idx) = next_idx {
            if let Some(uri) = self.queue.get(idx) {
                if let Ok(spotify_uri) = SpotifyUri::from_uri(uri) {
                    debug!("Preloading next track at index {idx}: {uri}");
                    self.player.preload(spotify_uri);
                }
            }
        }
    }

    fn next_index(&self) -> Option<usize> {
        let current = self.current_index?;
        let len = self.queue.len();
        if len == 0 {
            return None;
        }
        if self.repeat == RepeatMode::Queue {
            return Some((current + 1) % len);
        }
        if current + 1 < len {
            Some(current + 1)
        } else {
            None
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
        if self.is_playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn next(&mut self) -> bool {
        self.playing_queued = None;
        if self.repeat == RepeatMode::Track
            && let Some(idx) = self.current_index
        {
            self.play_at(idx);
            return true;
        }
        if !self.user_queue.is_empty() {
            let track = self.user_queue.remove(0);
            match SpotifyUri::from_uri(&track.uri) {
                Ok(spotify_uri) => {
                    info!("Playing from user queue: {}", track.uri);
                    self.player.stop();
                    self.player.load(spotify_uri, true, 0);
                    self.is_playing = true;
                    self.playing_queued = Some(track);
                    self.preload_next();
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
        if self.shuffle {
            if let Some(prev_idx) = self.play_history.pop() {
                self.load_index(prev_idx);
                return true;
            }
            return false;
        }
        if let Some(idx) = self.current_index
            && idx > 0
        {
            self.play_at(idx - 1);
            return true;
        }
        false
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.play_history.clear();
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn trim_played(&mut self, keep_behind: usize) -> bool {
        let Some(idx) = self.current_index else {
            return false;
        };
        if idx <= keep_behind {
            return false;
        }
        let remove = idx - keep_behind;
        self.queue.drain(0..remove);
        if let Some(i) = &mut self.current_index {
            *i -= remove;
        }
        true
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
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.set_queue(uris, start_index);
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
        self.add_to_queue(uri, name, artist, album, duration_ms, cover_path);
    }
    fn user_queue(&self) -> &[QueuedTrack] {
        self.user_queue()
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
        if index >= self.user_queue.len() {
            return false;
        }
        let track = self.user_queue.remove(index);
        match SpotifyUri::from_uri(&track.uri) {
            Ok(spotify_uri) => {
                info!("Playing from user queue: {}", track.uri);
                self.player.stop();
                self.player.load(spotify_uri, true, 0);
                self.is_playing = true;
                self.playing_queued = Some(track);
                self.preload_next();
                true
            }
            Err(e) => {
                error!("Invalid URI in user queue: {e}");
                false
            }
        }
    }

    fn play(&mut self) {
        self.play();
    }
    fn pause(&mut self) {
        self.pause();
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
        self.seek(position_ms);
    }
    fn seek_mut(&mut self, position_ms: u32) {
        self.seek(position_ms);
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
        self.current_index()
    }

    fn trim_played(&mut self, keep_behind: usize) -> bool {
        self.trim_played(keep_behind)
    }

    fn volume_up(&mut self) {
        self.volume_up();
    }
    fn volume_down(&mut self) {
        self.volume_down();
    }
    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.apply_volume();
        config::save_volume(self.volume);
    }
    fn toggle_shuffle(&mut self) {
        self.toggle_shuffle();
    }
    fn cycle_repeat(&mut self) {
        self.cycle_repeat();
    }

    fn try_recv_event(&mut self) -> Option<PlayerNotification> {
        let notif = self.event_rx.try_recv().ok()?;
        match &notif {
            PlayerNotification::Playing => self.is_playing = true,
            PlayerNotification::Paused => self.is_playing = false,
            _ => {}
        }
        Some(notif)
    }

    fn preload_next(&mut self) {
        self.preload_next();
    }

    fn set_visualizer_enabled(&mut self, enabled: bool) {
        self.analyzer_enabled.store(enabled, Ordering::Relaxed);
    }

    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        if self.analyzer_enabled.load(Ordering::Relaxed) {
            Some(Arc::clone(&self.band_energies))
        } else {
            None
        }
    }

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        self.snapshot_queue()
    }

    fn current_playback_state(&self) -> Option<PlaybackState> {
        let guard = self.server_position.lock().ok()?;
        let (base, recorded_at) = *guard;
        let elapsed = if self.is_playing {
            recorded_at.elapsed().as_millis() as u64
        } else {
            0
        };
        Some(PlaybackState {
            is_playing: self.is_playing,
            volume: self.volume,
            shuffle: self.shuffle,
            repeat: match self.repeat {
                RepeatMode::Off => crate::spotify::RepeatState::Off,
                RepeatMode::Queue => crate::spotify::RepeatState::Context,
                RepeatMode::Track => crate::spotify::RepeatState::Track,
            },
            is_local: false,
            progress_ms: base.saturating_add(elapsed),
            ..PlaybackState::default()
        })
    }
}
