mod control;
mod mercury;
mod queue;
mod trait_impl;

use crate::audio::audio_sink::{AnalyzerSink, N_BANDS};
use crate::audio::spotify_sink::{AudioOutputLoss, SpotifyAudioSink};
use crate::config;
use crate::config::OFFICIAL_CLIENT_ID;
use anyhow::{Context, Result};
use librespot_core::{
    authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
};
use librespot_playback::{
    audio_backend::Sink,
    config::PlayerConfig,
    mixer::{self, Mixer, MixerConfig},
    player::{Player as LibrespotPlayer, PlayerEvent},
};
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64},
};
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::token::obtain_streaming_token;
use super::types::{PlayerNotification, QueuedTrack, RepeatMode};

pub struct NativePlayer {
    pub(super) player: Arc<LibrespotPlayer>,
    pub(super) _session: Session,
    pub(super) mixer: Arc<dyn Mixer>,
    pub(super) queue: Vec<String>,
    pub user_queue: Vec<QueuedTrack>,
    pub playing_queued: Option<QueuedTrack>,
    pub(super) current_index: Option<usize>,
    pub is_playing: bool,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub event_rx: mpsc::UnboundedReceiver<PlayerNotification>,
    pub band_energies: Arc<Mutex<Vec<f32>>>,
    pub(super) server_position: Arc<Mutex<(u64, Instant)>>,
    pub(super) analyzer_enabled: Arc<AtomicBool>,
    pub(super) mono_enabled: Arc<AtomicBool>,
    pub(super) eq_gains: Arc<AtomicU64>,
    pub(super) audio_output_loss: Arc<AudioOutputLoss>,
    pub(super) audio_output_lost: bool,
    pub(super) play_history: VecDeque<usize>,
}

impl NativePlayer {
    pub async fn new(
        access_token: Option<String>,
        _low_resource: bool,
        bitrate: librespot_playback::config::Bitrate,
        gapless: bool,
    ) -> Result<Self> {
        let cache_dir = dirs::cache_dir().or_else(dirs::config_dir).map(|mut p| {
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

        let audio_output_loss = Arc::new(AudioOutputLoss::default());
        let mono_enabled = Arc::new(AtomicBool::new(false));
        let eq_gains = Arc::new(AtomicU64::new(0));
        let loss_for_initial_sink = Arc::clone(&audio_output_loss);
        let mono_for_initial_sink = Arc::clone(&mono_enabled);
        let eq_for_initial_sink = Arc::clone(&eq_gains);
        let initial_sink = tokio::task::spawn_blocking(move || {
            SpotifyAudioSink::open_or_unavailable(
                loss_for_initial_sink,
                mono_for_initial_sink,
                eq_for_initial_sink,
            )
        })
        .await
        .context("failed to spawn audio output initialization")?;

        let mixer_fn = mixer::find(None).context("No mixer found")?;
        let soft_mixer = mixer_fn(MixerConfig::default()).context("Failed to create mixer")?;
        let volume_getter = soft_mixer.get_soft_volume();

        let bands = Arc::new(Mutex::new(vec![0.0f32; N_BANDS]));
        let bands_for_sink = Arc::clone(&bands);
        let analyzer_enabled = Arc::new(AtomicBool::new(false));
        let analyzer_enabled_for_sink = Arc::clone(&analyzer_enabled);

        let session_for_player = session.clone();
        let server_position: Arc<Mutex<(u64, Instant)>> = Arc::new(Mutex::new((0, Instant::now())));
        let loss_for_sink_factory = Arc::clone(&audio_output_loss);
        let mono_for_sink_factory = Arc::clone(&mono_enabled);
        let eq_for_sink_factory = Arc::clone(&eq_gains);
        let sink_factory: Box<dyn Fn() -> Box<dyn Sink> + Send> = Box::new(move || {
            Box::new(SpotifyAudioSink::open_or_unavailable(
                Arc::clone(&loss_for_sink_factory),
                Arc::clone(&mono_for_sink_factory),
                Arc::clone(&eq_for_sink_factory),
            ))
        });

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
                Box::new(AnalyzerSink::with_factory(
                    Box::new(initial_sink),
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
            mono_enabled,
            eq_gains,
            audio_output_loss,
            audio_output_lost: false,
            play_history: VecDeque::new(),
        };
        instance.apply_volume();
        Ok(instance)
    }
}

impl Drop for NativePlayer {
    fn drop(&mut self) {
        self.player.stop();
    }
}
