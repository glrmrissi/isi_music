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
    mixer::NoOpVolume,
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
    queue: Vec<String>,
    current_index: Option<usize>,
    pub is_playing: bool,
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

        let player = LibrespotPlayer::new(
            PlayerConfig::default(),
            session,
            Box::new(NoOpVolume),
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
            queue: Vec::new(),
            current_index: None,
            is_playing: false,
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
}
