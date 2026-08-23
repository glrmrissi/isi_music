use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::player::{AudioPlayer, LocalPlayer, NativePlayer, PlayerNotification};
use crate::spotify::SpotifyClient;
use crate::ui::UiState;
use crate::utils::debug_overlay::{DebugOverlay, LogLevel};

pub struct PlayerManager {
    pub player: Option<Box<dyn AudioPlayer>>,
    pub parked_player: Option<Box<dyn AudioPlayer>>,
    pub local_active: bool,
    pub saved_volume: u8,
    pub local_db_path: String,
    pub band_energies: Option<Arc<Mutex<Vec<f32>>>>,
    pub art_url: Option<String>,
    pub session_reconnecting: bool,
    pub radio_mode: bool,
    pub autoplay_enabled: bool,
    pub recent_track_uris: std::collections::VecDeque<String>,
    pub playing_tracks: Vec<crate::spotify::TrackSummary>,
    pub consecutive_unavailable: u32,
    pub spotify_streaming_disabled: bool,
    pub reconnect_attempts: u32,
    pub last_reconnect_attempt: Option<Instant>,
    pub last_playback_health_check: Instant,
    pub playing_started_at: Option<Instant>,
    pub progress_at_play_start: u64,
    pub initial_sync_done: bool,
}

impl PlayerManager {
    pub fn new(
        saved_volume: u8,
        local_db_path: String,
        autoplay_enabled: bool,
        art_url: Option<String>,
    ) -> Self {
        Self {
            player: None,
            parked_player: None,
            local_active: false,
            saved_volume,
            local_db_path,
            band_energies: None,
            art_url,
            session_reconnecting: false,
            radio_mode: false,
            autoplay_enabled,
            recent_track_uris: std::collections::VecDeque::new(),
            playing_tracks: Vec::new(),
            consecutive_unavailable: 0,
            spotify_streaming_disabled: false,
            reconnect_attempts: 0,
            last_reconnect_attempt: None,
            last_playback_health_check: Instant::now(),
            playing_started_at: None,
            progress_at_play_start: 0,
            initial_sync_done: false,
        }
    }

    pub async fn ensure_spotify_player(
        &mut self,
        spotify: &SpotifyClient,
        state: &UiState,
        debug_overlay: &DebugOverlay,
        audio: &crate::config::AudioConfig,
    ) -> bool {
        if self.player.is_some() && !self.local_active {
            return true;
        }
        if self.parked_player.is_some() && self.local_active {
            std::mem::swap(&mut self.player, &mut self.parked_player);
            self.local_active = false;
            self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
            return true;
        }
        let token = spotify.get_access_token().await;
        match NativePlayer::new(token, false, audio.librespot_bitrate(), audio.gapless).await {
            Ok(mut p) => {
                p.set_volume(self.saved_volume);
                p.set_visualizer_enabled(state.show_visualizer);
                self.band_energies = p.band_energies();
                self.player = Some(Box::new(p));
                self.local_active = false;
                true
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("free") || msg.contains("premium") {
                    self.spotify_streaming_disabled = true;
                }
                let status =
                    if msg.contains("setup-spotify") || msg.contains("streaming authentication") {
                        "Spotify streaming is not authenticated. Run `isi-music setup-spotify`."
                            .to_string()
                    } else {
                        format!("Failed to create Spotify player: {e:#}")
                    };
                debug_overlay.log(LogLevel::Warn, format!("{status}: {e:#}"));
                false
            }
        }
    }

    pub async fn ensure_local_player(
        &mut self,
        state: &UiState,
        debug_overlay: &DebugOverlay,
    ) -> bool {
        if self.player.is_some() && self.local_active {
            return true;
        }
        if self.parked_player.is_some() && !self.local_active {
            std::mem::swap(&mut self.player, &mut self.parked_player);
            self.local_active = true;
            self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
            return true;
        }
        match LocalPlayer::new(self.saved_volume, &self.local_db_path) {
            Ok(mut p) => {
                p.set_visualizer_enabled(state.show_visualizer);
                self.band_energies = p.band_energies();
                self.player = Some(Box::new(p));
                self.local_active = true;
                true
            }
            Err(e) => {
                debug_overlay.log(
                    LogLevel::Error,
                    format!("Failed to create local player: {e}"),
                );
                false
            }
        }
    }

    pub fn merge_playback_from_api(
        &mut self,
        state: &mut UiState,
        current_pb: crate::ui::PlaybackState,
    ) {
        let pb_playing = current_pb.is_playing;
        let pb_progress = current_pb.progress_ms;
        self.art_url = current_pb.art_url.clone();
        let local_progress = state.playback.progress_ms;
        let local_playing = state.playback.is_playing;
        state.playback.merge_from_api(current_pb);
        if local_playing && pb_playing {
            if local_progress.abs_diff(pb_progress) <= 2000 {
                state.playback.progress_ms = local_progress;
            } else {
                self.progress_at_play_start = pb_progress;
                self.playing_started_at = Some(Instant::now());
            }
        } else if pb_playing {
            self.progress_at_play_start = pb_progress;
            self.playing_started_at = Some(Instant::now());
        } else {
            self.playing_started_at = None;
            self.progress_at_play_start = pb_progress;
        }
    }

    pub fn interpolate_progress(&mut self, state: &mut UiState) {
        if state.playback.is_playing {
            if self.playing_started_at.is_none() {
                self.playing_started_at = Some(Instant::now());
                self.progress_at_play_start = state.playback.progress_ms;
            }
            let elapsed = self
                .playing_started_at
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            state.playback.progress_ms = self.progress_at_play_start + elapsed;
            if state.playback.progress_ms >= state.playback.duration_ms {
                if self.player.is_none() {
                    state.playback.is_playing = false;
                    state.playback.progress_ms = state.playback.duration_ms;
                    self.playing_started_at = None;
                    self.progress_at_play_start = state.playback.duration_ms;
                }
            }
        } else if self.playing_started_at.is_some() {
            let elapsed = self
                .playing_started_at
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            self.progress_at_play_start += elapsed;
            self.playing_started_at = None;
        }
    }

    pub fn handle_notifications(
        &mut self,
        state: &mut UiState,
        debug_overlay: &DebugOverlay,
    ) -> NotificationResult {
        let parked_has_queue = self
            .parked_player
            .as_ref()
            .map(|p| !p.user_queue().is_empty())
            .unwrap_or(false);

        let mut result = NotificationResult::default();

        if let Some(player) = &mut self.player {
            while let Some(notif) = player.try_recv_event() {
                match notif {
                    PlayerNotification::TrackEnded => {
                        self.consecutive_unavailable = 0;
                        if player.next() {
                            result.needs_sync = true;
                        } else if parked_has_queue {
                            result.needs_crossover = true;
                            state.playback.is_playing = false;
                        } else if (self.radio_mode || self.autoplay_enabled) && !self.local_active {
                            result.needs_radio_refill = true;
                        } else {
                            result.needs_crossover = true;
                            state.playback.is_playing = false;
                        }
                    }
                    PlayerNotification::Playing => {
                        self.consecutive_unavailable = 0;
                        state.playback.is_playing = true;
                        if self.local_active && self.playing_started_at.is_none() {
                            self.playing_started_at = Some(Instant::now());
                            self.progress_at_play_start = state.playback.progress_ms;
                        }
                    }
                    PlayerNotification::Paused => state.playback.is_playing = false,
                    PlayerNotification::TrackUnavailable => {
                        self.consecutive_unavailable += 1;
                        state.status_msg = Some("Track unavailable, skipping...".to_string());
                        if player.next() {
                            result.needs_sync = true;
                        } else if parked_has_queue {
                            result.needs_crossover = true;
                            state.playback.is_playing = false;
                        } else if (self.radio_mode || self.autoplay_enabled) && !self.local_active {
                            result.needs_radio_refill = true;
                        } else {
                            result.needs_crossover = true;
                            state.playback.is_playing = false;
                        }
                    }
                    PlayerNotification::SessionLost => {
                        if !self.spotify_streaming_disabled {
                            state.status_msg = Some("Session lost, reconnecting...".to_string());
                            result.needs_reconnect = true;
                        }
                    }
                    PlayerNotification::FreeAccountDetected => {
                        if !self.spotify_streaming_disabled {
                            tracing::warn!("Free account detected - switching to local-only mode");
                            self.spotify_streaming_disabled = true;
                            self.consecutive_unavailable = 0;

                            debug_overlay.log(
                                LogLevel::Warn,
                                format!("Free account detected - switching to local-only mode"),
                            );
                            state.status_msg = Some(
                                "Spotify Premium required. Switched to local-only mode."
                                    .to_string(),
                            );

                            result.needs_player_swap = true;
                        }
                    }
                    PlayerNotification::PreloadNextTrack => {
                        player.preload_next();
                    }
                }
            }

            if result.needs_player_swap {
                self.player = None;
                self.band_energies = None;
                if self.parked_player.is_some() {
                    std::mem::swap(&mut self.player, &mut self.parked_player);
                    self.local_active = true;
                    self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
                    result.needs_sync = true;
                } else {
                    result.needs_sync = false;
                }
            }
        }

        result
    }
}

#[derive(Default)]
pub struct NotificationResult {
    pub needs_sync: bool,
    pub needs_reconnect: bool,
    pub needs_crossover: bool,
    pub needs_radio_refill: bool,
    pub needs_player_swap: bool,
}
