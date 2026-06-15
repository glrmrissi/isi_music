use std::time::Instant;
use tracing::{info, warn};

use crate::App;
use crate::player::{AudioPlayer, NativePlayer};
use crate::ui::{Focus, SearchPanel};

impl App {
    pub fn on_track_started(&mut self) {
        self.scrobble_sent = false;

        self.track_start_unix = crate::app::metadata::unix_now();

        self.state.playback.progress_ms = 0;
        self.playing_started_at = None;
        self.progress_at_play_start = 0;
        self.state.playback.radio_mode = self.radio_mode;

        if self.current_track_uri.starts_with("spotify:track:") {
            self.recent_track_uris
                .push_back(self.current_track_uri.clone());
            if self.recent_track_uris.len() > 5 {
                self.recent_track_uris.pop_front();
            }
        }

        self.state.album_art = None;
        self.album_art_pending = None;
        self.last_art_uri.clear();

        if let Some(lfm) = self.lastfm.clone() {
            let artist = self.state.playback.artist.clone();
            let track = self.state.playback.title.clone();
            let album = self.state.playback.album.clone();
            let duration = self.state.playback.duration_ms;

            if !artist.trim().is_empty() && !track.trim().is_empty() && duration > 30_000 {
                tokio::spawn(async move {
                    lfm.update_now_playing(&artist, &track, &album, duration)
                        .await;
                });
            }
        }

        self.state.playback.lyrics = None;
        self.state.playback.lyrics_loading = false;
        self.state.playback.lyrics_scroll = 0;

        let title = self.state.playback.title.clone();
        let artist = self.state.playback.artist.clone();
        let uri = self.current_track_uri.clone();

        if !title.is_empty() && !artist.is_empty() {
            self.ensure_lyrics();
            self.state.playback.lyrics_loading = true;
            if let Some(ref lyrics) = self.lyrics {
                lyrics.request(&title, &artist, &uri);
            }
        }
    }

    pub fn sync_track_selection(&mut self) {
        let queued = self.player.as_mut().and_then(|p| p.take_playing_queued());

        if let Some(qt) = queued {
            self.state.playback.title = qt.name;
            self.state.playback.artist = qt.artist;
            self.state.playback.album = String::new();
            self.state.playback.duration_ms = qt.duration_ms;
            self.state.playback.progress_ms = 0;
            self.state.playback.is_playing = true;

            self.state.playback.art_url = None;

            self.current_track_uri = qt.uri;
            self.on_track_started();
            return;
        }

        if let Some(player) = &self.player {
            if let Some(idx) = player.current_index() {
                if let Some(track) = self.playing_tracks.get(idx) {
                    self.state.playback.title = track.name.clone();
                    self.state.playback.artist = track.artist.clone();
                    self.state.playback.album = track.album.clone();

                    self.state.playback.art_url = track.cover_path.clone();
                    self.debug_overlay.log(
                        crate::utils::debug_overlay::LogLevel::Info,
                        format!("Loading cover from: {:?}", self.state.playback.art_url),
                    );

                    self.state.playback.duration_ms = track.duration_ms;
                    self.state.playback.progress_ms = 0;

                    self.current_track_uri = track.uri.clone();

                    self.on_track_started();
                }

                if self.playing_tracks.len() == self.state.tracks.len()
                    && self.playing_tracks.get(idx).map(|t| &t.uri)
                        == self.state.tracks.get(idx).map(|t| &t.uri)
                {
                    self.state.track_list.select(Some(idx));
                }
            }
        }
    }

    pub fn activate_local_player(&mut self) {
        if self.local_active {
            return;
        }
        if let Some(ref mut p) = self.player {
            if p.is_playing() {
                p.pause();
            }
        }
        self.player = None;
        self.band_energies = None;
        if self.parked_player.is_some() {
            std::mem::swap(&mut self.player, &mut self.parked_player);
            self.local_active = true;
            self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
        } else {
            self.local_active = true;
        }
    }

    pub fn activate_spotify_player(&mut self) {
        if !self.local_active {
            return;
        }
        if let Some(ref mut p) = self.player {
            if p.is_playing() {
                p.pause();
            }
        }
        self.player = None;
        self.band_energies = None;
        if self.parked_player.is_some() {
            std::mem::swap(&mut self.player, &mut self.parked_player);
            self.local_active = false;
            self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
        } else {
            self.local_active = false;
        }
    }

    pub fn sync_queue_display(&mut self) {
        let mut items: Vec<(String, String)> = Vec::new();

        if let Some(p) = &self.player {
            items.extend(
                p.user_queue()
                    .iter()
                    .map(|t| (t.name.clone(), t.artist.clone())),
            );
        }
        if let Some(p) = &self.parked_player {
            let prefix = if self.local_active { " " } else { "* " };
            items.extend(
                p.user_queue()
                    .iter()
                    .map(|t| (format!("{}{}", prefix, t.name), t.artist.clone())),
            );
        }

        self.state.queue_items = items;
    }

    pub async fn radio_refill(&mut self) {
        let seeds: Vec<String> = self.recent_track_uris.iter().cloned().collect();
        if seeds.is_empty() {
            self.state.status_msg =
                Some("Radio: no seed tracks yet — play a Spotify track first".to_string());
            return;
        }

        match self.spotify.fetch_recommendations(&seeds, 20).await {
            Ok(tracks) if !tracks.is_empty() => {
                let count = tracks.len();
                if let Some(player) = &mut self.player {
                    for t in tracks {
                        player.add_to_queue(t.uri, t.name, t.artist, t.duration_ms, None);
                    }
                    self.state.status_msg = Some(format!("Radio: queued {count} tracks"));
                    self.sync_queue_display();
                }
            }
            Ok(_) | Err(_) => {
                self.radio_mode = false;
                self.state.playback.radio_mode = false;
                self.state.status_msg = Some("Radio: could not find tracks, radio off".to_string());
            }
        }
    }

    pub async fn get_similar_tracks(&mut self) {
        let seed_uri = match self.state.focus {
            Focus::Tracks => self
                .state
                .track_list
                .selected()
                .and_then(|i| self.state.tracks.get(i))
                .map(|t| t.uri.clone()),
            Focus::Search => self
                .state
                .search_results
                .as_ref()
                .and_then(|sr| match sr.panel {
                    SearchPanel::Tracks => sr.selected_track_uri().map(|s| s.to_string()),
                    SearchPanel::Artists => sr
                        .selected_artist()
                        .map(|a| format!("spotify:artist:{}", a.id)),
                    _ => None,
                }),
            _ => None,
        };

        let Some(uri) = seed_uri else {
            self.state.status_msg = Some("Select a track or artist first".to_string());
            return;
        };

        if uri.starts_with("file://") {
            self.state.status_msg =
                Some("Recommendations require a Spotify track or artist".to_string());
            return;
        }

        self.state.status_msg = Some("Fetching similar tracks…".to_string());

        match self.spotify.fetch_recommendations(&[uri], 30).await {
            Ok(tracks) => {
                let count = tracks.len();
                self.state.tracks = tracks;
                self.state.tracks_total = count as u32;
                self.state.tracks_offset = count as u32;
                self.state.active_playlist_uri = Some("radio:recommendations".to_string());
                self.state.active_playlist_id = Some("radio:recommendations".to_string());
                self.state
                    .track_list
                    .select(if count == 0 { None } else { Some(0) });
                self.state.push_nav();
                self.state.active_content = crate::ui::ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.focus = Focus::Tracks;
                self.state.status_msg = if count == 0 {
                    Some("No recommendations found".to_string())
                } else {
                    Some(format!("{count} similar tracks"))
                };
            }
            Err(e) => {
                self.state.status_msg = Some(format!("Recommendations failed: {e}"));
            }
        }
    }

    pub async fn reconnect_player(&mut self) {
        const MAX_RECONNECT_ATTEMPTS: u32 = 5;

        if self.session_reconnecting {
            if let Some(last_attempt) = self.last_reconnect_attempt {
                let attempts_so_far = self.reconnect_attempts.min(5);
                let delay_secs = 2u64.pow(attempts_so_far);
                let max_delay = std::time::Duration::from_secs(60);
                let delay = std::time::Duration::from_secs(delay_secs).min(max_delay);

                if last_attempt.elapsed() < delay {
                    return;
                }
            }

            if self.reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                warn!(
                    "Max reconnect attempts ({}) reached",
                    MAX_RECONNECT_ATTEMPTS
                );
                self.debug_overlay.log(
                    crate::utils::debug_overlay::LogLevel::Warn,
                    format!(
                        "Max reconnect attempts ({}) reached",
                        MAX_RECONNECT_ATTEMPTS
                    ),
                );
                self.state.status_msg =
                    Some("Connection lost - max retry attempts reached.".to_string());
                self.session_reconnecting = false;
                return;
            }
        }

        self.reconnect_attempts += 1;
        self.last_reconnect_attempt = Some(Instant::now());
        self.session_reconnecting = true;

        warn!(
            "Attempting librespot reconnection ({}/{})",
            self.reconnect_attempts, MAX_RECONNECT_ATTEMPTS
        );

        self.debug_overlay.log(
            crate::utils::debug_overlay::LogLevel::Warn,
            format!(
                "Attempting librespot reconnection ({}/{})",
                self.reconnect_attempts, MAX_RECONNECT_ATTEMPTS
            ),
        );
        self.state.status_msg = Some(format!(
            "Reconnecting ({}/{})...",
            self.reconnect_attempts, MAX_RECONNECT_ATTEMPTS
        ));

        if self.spotify_streaming_disabled {
            self.session_reconnecting = false;
            return;
        }

        let (saved_queue, saved_index) = self
            .player
            .as_ref()
            .map(|p| p.snapshot_queue())
            .unwrap_or_default();
        let saved_volume = self.player.as_ref().map(|p| p.volume()).unwrap_or(50);

        self.player = None;
        self.band_energies = None;

        let Some(token) = self.spotify.get_access_token().await else {
            warn!("Could not get access token for reconnect");
            self.debug_overlay.log(
                crate::utils::debug_overlay::LogLevel::Warn,
                format!("Could not get access token for reconnect"),
            );
            self.state.status_msg = Some("Reconnect failed: no token".to_string());
            return;
        };

        match NativePlayer::new(token, false).await {
            Ok(mut p) => {
                p.set_volume(saved_volume);
                if !saved_queue.is_empty() {
                    let start = saved_index.unwrap_or(0);
                    p.set_queue(saved_queue, start);
                }
                self.band_energies = p.band_energies();
                self.player = Some(Box::new(p));
                self.state.status_msg = Some("Reconnected!".to_string());
                info!("Librespot session reconnected successfully");
                self.debug_overlay.log(
                    crate::utils::debug_overlay::LogLevel::Info,
                    format!("Librespot session reconnected successfully"),
                );
                self.reconnect_attempts = 0;
                self.last_reconnect_attempt = None;
                self.session_reconnecting = false;
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("free") || msg.contains("premium") {
                    warn!("Spotify free account — disabling streaming permanently");
                    self.debug_overlay.log(
                        crate::utils::debug_overlay::LogLevel::Warn,
                        format!("Spotify free account — disabling streaming permanently"),
                    );
                    self.spotify_streaming_disabled = true;
                    self.state.status_msg =
                        Some("Spotify Premium required. Switched to local-only mode.".to_string());
                    if self.parked_player.is_some() {
                        std::mem::swap(&mut self.player, &mut self.parked_player);
                        self.local_active = true;
                        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
                    }
                    self.session_reconnecting = false;
                } else if msg.contains("401") || msg.contains("unauthorized") {
                    warn!(
                        "Reconnect failed with 401 - will retry ({}/{})",
                        self.reconnect_attempts, MAX_RECONNECT_ATTEMPTS
                    );
                    self.state.status_msg = Some(format!(
                        "Authorization expired, retrying ({}/{})",
                        self.reconnect_attempts, MAX_RECONNECT_ATTEMPTS
                    ));
                } else {
                    warn!("Reconnect failed: {e:#}");
                    self.debug_overlay.log(
                        crate::utils::debug_overlay::LogLevel::Warn,
                        format!("Reconnect failed: {e:#}"),
                    );
                    self.state.status_msg = Some(format!("Reconnect failed: {e}"));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/player.rs"]
mod tests;
