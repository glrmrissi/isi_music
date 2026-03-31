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
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

pub enum PlayerNotification {
    TrackEnded,
    TrackUnavailable,
    Playing,
    Paused,
}

pub struct NativePlayer {
    player: Arc<LibrespotPlayer>,
    mixer: Arc<dyn Mixer>,
    queue: Vec<String>,
    current_index: Option<usize>,
    pub is_playing: bool,
    pub volume: u8, // 0–100
    pub event_rx: mpsc::UnboundedReceiver<PlayerNotification>,
}

impl NativePlayer {
    pub async fn new(access_token: String) -> Result<Self> {
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

        let player = LibrespotPlayer::new(
            PlayerConfig::default(),
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

        Ok(Self {
            player,
            mixer: soft_mixer,
            queue: Vec::new(),
            current_index: None,
            is_playing: false,
            volume: 100,
            event_rx: notif_rx,
        })
    }

    pub fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.queue = uris;
        self.play_at(start_index);
    }

    pub fn play_at(&mut self, index: usize) {
        let Some(uri) = self.queue.get(index) else {
            warn!("Index {index} out of queue bounds");
            return;
        };
        match SpotifyUri::from_uri(uri) {
            Ok(spotify_uri) => {
                info!("Loading URI: {uri}");
                self.player.load(spotify_uri, true, 0);
                self.current_index = Some(index);
                self.is_playing = true;
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
        if let Some(idx) = self.current_index {
            let next = idx + 1;
            if next < self.queue.len() {
                self.play_at(next);
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

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.apply_volume();
    }

    pub fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.apply_volume();
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    fn apply_volume(&self) {
        let v = (self.volume as u32 * 65535 / 100) as u16;
        self.mixer.set_volume(v);
    }
}
