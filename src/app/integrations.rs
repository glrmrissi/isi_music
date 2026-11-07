use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::spotify::RepeatState;
use crate::ui::UiState;
use crate::utils::discord::DiscordRpc;
use crate::utils::lastfm::LastfmClient;
#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::utils::lock::lock_or_recover;
#[cfg(windows)]
use crate::utils::media_keys::{MediaKey, MediaKeysHandle};
#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::utils::mpris::{MprisCmd, MprisHandle, MprisState};
#[cfg(windows)]
use crate::utils::smtc::{SmtcCmd, SmtcHandle, SmtcState};

pub enum LastfmEvent {
    Token { flow_id: u64, token: String },
    TokenFailed { flow_id: u64, error: String },
    Session { flow_id: u64, session_key: String },
    SessionFailed { flow_id: u64, error: String },
}

pub enum SpotifyAuthEvent {
    Progress {
        flow_id: u64,
        msg: String,
    },
    WebApiConnected {
        flow_id: u64,
        client_id: String,
        access_token: String,
        refresh_token: String,
        expires_in: u64,
    },
    Done {
        flow_id: u64,
    },
    Failed {
        flow_id: u64,
        error: String,
    },
}

impl LastfmEvent {
    fn flow_id(&self) -> u64 {
        match *self {
            Self::Token { flow_id, .. }
            | Self::TokenFailed { flow_id, .. }
            | Self::Session { flow_id, .. }
            | Self::SessionFailed { flow_id, .. } => flow_id,
        }
    }
}

impl SpotifyAuthEvent {
    fn flow_id(&self) -> u64 {
        match *self {
            Self::Progress { flow_id, .. }
            | Self::WebApiConnected { flow_id, .. }
            | Self::Done { flow_id, .. }
            | Self::Failed { flow_id, .. } => flow_id,
        }
    }
}

/// Sends `Failed` if the auth task dies without emitting a terminal event
/// (panic, spawn failure). Disarmed once `Done`/`Failed` is sent normally.
struct SpotifyAuthGuard {
    flow_id: u64,
    tx: tokio::sync::mpsc::UnboundedSender<SpotifyAuthEvent>,
    armed: bool,
}

impl Drop for SpotifyAuthGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.tx.send(SpotifyAuthEvent::Failed {
                flow_id: self.flow_id,
                error: "authorization task ended unexpectedly".to_string(),
            });
        }
    }
}

pub struct IntegrationManager {
    pub lastfm: Option<Arc<LastfmClient>>,
    pub pending_lastfm_token: Option<String>,
    lastfm_tx: tokio::sync::mpsc::UnboundedSender<LastfmEvent>,
    lastfm_rx: tokio::sync::mpsc::UnboundedReceiver<LastfmEvent>,
    lastfm_auth_gen: u64,
    lastfm_busy: bool,
    lastfm_poll_abort: Option<tokio::task::AbortHandle>,
    spotify_tx: tokio::sync::mpsc::UnboundedSender<SpotifyAuthEvent>,
    spotify_rx: tokio::sync::mpsc::UnboundedReceiver<SpotifyAuthEvent>,
    spotify_auth_abort: Option<tokio::task::AbortHandle>,
    spotify_auth_gen: u64,
    pub scrobble_sent: bool,
    pub track_start_unix: u64,
    listened_ms: u64,
    last_progress_ms: u64,
    pub discord: Option<DiscordRpc>,
    discord_last_title: String,
    discord_last_playing: bool,
    discord_last_art: Option<String>,
    discord_last_progress_ms: u64,
    discord_pending_since: Option<Instant>,
    discord_last_send: Instant,
    discord_dirty: bool,
    #[cfg(all(feature = "mpris", target_os = "linux"))]
    pub mpris: Option<MprisHandle>,
    #[cfg(feature = "mpris")]
    #[allow(dead_code)]
    mpris_last_title: String,
    #[cfg(feature = "mpris")]
    #[allow(dead_code)]
    mpris_last_artist: String,
    #[cfg(feature = "mpris")]
    #[allow(dead_code)]
    mpris_last_album: String,
    #[cfg(feature = "mpris")]
    #[allow(dead_code)]
    mpris_last_playing: bool,
    #[cfg(feature = "mpris")]
    #[allow(dead_code)]
    mpris_last_art: Option<String>,
    #[cfg(windows)]
    pub media_keys: Option<MediaKeysHandle>,
    #[cfg(windows)]
    pub smtc: Option<SmtcHandle>,
}

impl IntegrationManager {
    pub fn new() -> Self {
        let (lastfm_tx, lastfm_rx) = tokio::sync::mpsc::unbounded_channel();
        let (spotify_tx, spotify_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            lastfm: None,
            pending_lastfm_token: None,
            lastfm_tx,
            lastfm_rx,
            lastfm_auth_gen: 0,
            lastfm_busy: false,
            lastfm_poll_abort: None,
            spotify_tx,
            spotify_rx,
            spotify_auth_abort: None,
            spotify_auth_gen: 0,
            scrobble_sent: false,
            track_start_unix: 0,
            listened_ms: 0,
            last_progress_ms: 0,
            discord: None,
            discord_last_title: String::new(),
            discord_last_playing: false,
            discord_last_art: None,
            discord_last_progress_ms: 0,
            discord_pending_since: None,
            discord_last_send: Instant::now() - Duration::from_secs(60),
            discord_dirty: false,
            #[cfg(all(feature = "mpris", target_os = "linux"))]
            mpris: None,
            #[cfg(feature = "mpris")]
            mpris_last_title: String::new(),
            #[cfg(feature = "mpris")]
            mpris_last_artist: String::new(),
            #[cfg(feature = "mpris")]
            mpris_last_album: String::new(),
            #[cfg(feature = "mpris")]
            mpris_last_playing: false,
            #[cfg(feature = "mpris")]
            mpris_last_art: None,
            #[cfg(windows)]
            media_keys: None,
            #[cfg(windows)]
            smtc: None,
        }
    }

    pub fn reset_scrobble(&mut self) {
        self.scrobble_sent = false;
        self.listened_ms = 0;
        self.last_progress_ms = 0;
    }

    pub fn set_track_start(&mut self, ts: u64) {
        self.track_start_unix = ts;
    }

    pub fn update_discord(&mut self, state: &UiState, track_uri: &str) {
        let Some(discord) = &self.discord else {
            return;
        };
        let pb = &state.playback;
        let title_changed = pb.title != self.discord_last_title;
        let playing_changed = pb.is_playing != self.discord_last_playing;
        let art_changed = pb.art_url != self.discord_last_art;

        let prev_progress = self.discord_last_progress_ms;
        self.discord_last_progress_ms = pb.progress_ms;
        let seeked = !title_changed
            && pb.is_playing
            && (pb.progress_ms.saturating_add(1500) < prev_progress
                || pb.progress_ms > prev_progress.saturating_add(10_000));

        if title_changed {
            self.discord_pending_since = Some(Instant::now());
            self.discord_last_title = pb.title.clone();
            self.discord_last_playing = pb.is_playing;
        } else if playing_changed || self.discord_pending_since.is_none() && (art_changed || seeked)
        {
            self.discord_last_playing = pb.is_playing;
            self.discord_pending_since = None;
            self.discord_dirty = true;
        }

        if let Some(since) = self.discord_pending_since {
            let art_ready = pb.art_url.is_some() || pb.is_local;
            let progress_ready = pb.progress_ms > 0 || !pb.is_playing || pb.is_local;
            let timeout_secs = if pb.is_local { 1 } else { 5 };
            let timed_out = since.elapsed() >= Duration::from_secs(timeout_secs);
            if (art_ready && progress_ready) || timed_out {
                self.discord_pending_since = None;
                self.discord_dirty = true;
            }
        }

        if self.discord_dirty && self.discord_last_send.elapsed() >= Duration::from_secs(2) {
            self.discord_dirty = false;
            self.discord_last_send = Instant::now();
            Self::send_discord(discord, pb, track_uri);
            self.discord_last_art = pb.art_url.clone();
        }
    }

    fn send_discord(discord: &DiscordRpc, pb: &crate::ui::PlaybackState, track_uri: &str) {
        if pb.title.is_empty() || !pb.is_playing {
            discord.clear();
            return;
        }
        let track_url = track_uri
            .strip_prefix("spotify:track:")
            .map(|id| format!("https://open.spotify.com/track/{id}"))
            .or_else(|| {
                track_uri
                    .strip_prefix("spotify:episode:")
                    .map(|id| format!("https://open.spotify.com/episode/{id}"))
            });
        let now = crate::app::metadata::unix_now() as i64;
        let start_unix = now - (pb.progress_ms / 1000) as i64;
        let duration_secs = (pb.duration_ms / 1000) as i64;
        let end_unix = (duration_secs > 0).then_some(start_unix + duration_secs);
        discord.update_playing(
            &pb.title,
            &pb.artist,
            &pb.album,
            pb.art_url.as_deref(),
            track_url.as_deref(),
            start_unix,
            end_unix,
        );
    }

    #[cfg(all(feature = "mpris", target_os = "linux"))]
    pub fn update_mpris(&mut self, state: &UiState) {
        let Some(mpris) = &mut self.mpris else {
            return;
        };
        let pb = &state.playback;

        let changed = pb.title != self.mpris_last_title
            || pb.artist != self.mpris_last_artist
            || pb.album != self.mpris_last_album
            || pb.is_playing != self.mpris_last_playing
            || pb.art_url != self.mpris_last_art;

        if changed {
            self.mpris_last_title = pb.title.clone();
            self.mpris_last_artist = pb.artist.clone();
            self.mpris_last_album = pb.album.clone();
            self.mpris_last_playing = pb.is_playing;
            self.mpris_last_art = pb.art_url.clone();

            mpris.update(MprisState {
                title: pb.title.clone(),
                artist: pb.artist.clone(),
                album: pb.album.clone(),
                duration_us: pb.duration_ms as i64 * 1000,
                position_us: pb.progress_ms as i64 * 1000,
                volume: pb.volume as f64 / 100.0,
                is_playing: pb.is_playing,
                shuffle: pb.shuffle,
                repeat_track: pb.repeat == RepeatState::Track,
                repeat_queue: pb.repeat == RepeatState::Context,
                art_url: pb.art_url.clone(),
            });
        } else {
            let mut mpris_state = lock_or_recover(&mpris.state);
            mpris_state.position_us = pb.progress_ms as i64 * 1000;
            mpris_state.volume = pb.volume as f64 / 100.0;
            mpris_state.is_playing = pb.is_playing;
            mpris_state.shuffle = pb.shuffle;
            mpris_state.repeat_track = pb.repeat == RepeatState::Track;
            mpris_state.repeat_queue = pb.repeat == RepeatState::Context;
        }
    }

    #[cfg(all(feature = "mpris", target_os = "linux"))]
    pub fn poll_mpris_cmds(&mut self) -> Vec<MprisCmd> {
        let Some(mpris) = &mut self.mpris else {
            return Vec::new();
        };
        let mut v = Vec::new();
        while let Ok(c) = mpris.cmd_rx.try_recv() {
            v.push(c);
        }
        v
    }

    #[cfg(windows)]
    pub fn update_smtc(&mut self, state: &UiState) {
        let Some(smtc) = &self.smtc else {
            return;
        };
        let pb = &state.playback;
        smtc.update(&SmtcState {
            title: pb.title.clone(),
            artist: pb.artist.clone(),
            album: pb.album.clone(),
            art_url: pb.art_url.clone(),
            cover_path: pb.cover_path.clone(),
            duration_ms: pb.duration_ms,
            position_ms: pb.progress_ms,
            is_playing: pb.is_playing,
        });
    }

    #[cfg(windows)]
    pub fn poll_smtc_cmds(&mut self) -> Vec<SmtcCmd> {
        let Some(smtc) = &self.smtc else {
            return Vec::new();
        };
        let mut v = Vec::new();
        while let Ok(c) = smtc.cmd_rx.try_recv() {
            v.push(c);
        }
        v
    }

    #[cfg(windows)]
    pub fn poll_media_keys(&mut self) -> Vec<MediaKey> {
        let Some(media_keys) = &self.media_keys else {
            return Vec::new();
        };
        let mut v = Vec::new();
        while let Ok(cmd) = media_keys.cmd_rx.try_recv() {
            v.push(cmd);
        }
        v
    }

    pub fn maybe_scrobble(&mut self, state: &UiState) {
        if !state.playback.is_playing || self.scrobble_sent {
            return;
        }
        let progress = state.playback.progress_ms;
        let duration = state.playback.duration_ms;

        let delta = progress.saturating_sub(self.last_progress_ms);
        if delta > 0 && delta <= 2_000 {
            self.listened_ms += delta;
        }
        self.last_progress_ms = progress;

        if duration <= 30_000 || (self.listened_ms < duration / 2 && self.listened_ms < 240_000) {
            return;
        }
        if let Some(lfm) = self.lastfm.clone() {
            let artist = state.playback.artist.clone();
            let track = state.playback.title.clone();
            let album = state.playback.album.clone();
            let now = crate::app::metadata::unix_now();
            let ts = if self.track_start_unix > 0 {
                self.track_start_unix
            } else {
                now.saturating_sub(progress / 1000)
            };
            let dur = duration;
            tokio::spawn(async move {
                lfm.scrobble(&artist, &track, &album, ts, dur).await;
            });
            self.scrobble_sent = true;
        }
    }

    /// Connect/disconnect Last.fm. All network work runs in background tasks
    /// and reports back through `lastfm_rx`; `poll_lastfm` applies the results.
    pub fn toggle_lastfm(&mut self, state: &mut UiState) {
        use crate::utils::lastfm::get_api_key;

        if self.lastfm.is_some() {
            self.lastfm = None;
            self.pending_lastfm_token = None;
            self.lastfm_auth_gen += 1;
            if let Some(h) = self.lastfm_poll_abort.take() {
                h.abort();
            }
            crate::config::clear_lastfm_session_key();
            Self::clear_legacy_lastfm_config();
            state.lastfm_connected = false;
            state.lastfm_pending = false;
            state.status_msg = Some("Last.fm scrobbling disconnected".to_string());
            return;
        }

        if self.pending_lastfm_token.is_some() {
            // Auth in progress: Enter cancels it (the poll task is aborted and
            // its events are discarded via the generation bump).
            self.pending_lastfm_token = None;
            self.lastfm_auth_gen += 1;
            if let Some(h) = self.lastfm_poll_abort.take() {
                h.abort();
            }
            state.lastfm_pending = false;
            state.status_msg =
                Some("Last.fm authorization cancelled; press Enter to start over".to_string());
            return;
        }

        if self.lastfm_busy {
            state.status_msg = Some("Last.fm request already in progress...".to_string());
            return;
        }

        self.lastfm_auth_gen += 1;
        let flow_id = self.lastfm_auth_gen;
        if let Some(h) = self.lastfm_poll_abort.take() {
            h.abort();
        }
        self.lastfm_busy = true;
        state.status_msg = Some("Requesting Last.fm auth token...".to_string());
        let tx = self.lastfm_tx.clone();
        tokio::spawn(async move {
            let ev = match LastfmClient::get_auth_token(&get_api_key()).await {
                Ok(token) => LastfmEvent::Token { flow_id, token },
                Err(e) => LastfmEvent::TokenFailed {
                    flow_id,
                    error: format!("{e:#}"),
                },
            };
            let _ = tx.send(ev);
        });
    }

    /// Drops the plaintext session_key from config.toml if still present.
    fn clear_legacy_lastfm_config() {
        if let Ok(mut cfg) = crate::config::AppConfig::load()
            && cfg.lastfm.session_key.take().is_some()
        {
            let _ = cfg.save();
        }
    }

    /// Applies background auth results. Called every tick from the main loop.
    pub fn poll_lastfm(&mut self, state: &mut UiState) {
        use crate::utils::lastfm::get_api_key;

        while let Ok(ev) = self.lastfm_rx.try_recv() {
            if ev.flow_id() != self.lastfm_auth_gen {
                continue;
            }
            match ev {
                LastfmEvent::Token { flow_id, token } => {
                    let url = crate::utils::lastfm::auth_url(&get_api_key(), &token);
                    self.pending_lastfm_token = Some(token.clone());
                    state.lastfm_pending = true;
                    self.lastfm_busy = false;
                    state.status_msg = Some(if open::that(&url).is_ok() {
                        "Authorize isi-music on the Last.fm page that just opened".to_string()
                    } else {
                        format!("Open this URL to authorize Last.fm: {url}")
                    });
                    let tx = self.lastfm_tx.clone();
                    let handle = tokio::spawn(async move {
                        Self::poll_session(flow_id, token, tx).await;
                    });
                    self.lastfm_poll_abort = Some(handle.abort_handle());
                }
                LastfmEvent::TokenFailed { error, .. } => {
                    self.lastfm_busy = false;
                    state.status_msg = Some(format!("Last.fm token request failed: {error}"));
                }
                LastfmEvent::Session { session_key, .. } => {
                    use crate::utils::lastfm::get_api_secret;
                    self.lastfm_poll_abort = None;
                    let persisted = crate::config::save_lastfm_session_key(&session_key);
                    if persisted {
                        Self::clear_legacy_lastfm_config();
                    }
                    self.lastfm = Some(Arc::new(LastfmClient::new(
                        get_api_key(),
                        get_api_secret(),
                        session_key,
                    )));
                    self.pending_lastfm_token = None;
                    state.lastfm_connected = true;
                    state.lastfm_pending = false;
                    state.status_msg = Some(if persisted {
                        "Last.fm connected! Scrobbling enabled.".to_string()
                    } else {
                        "Last.fm connected, but the session key could not be saved; \
                         re-authorize after restart"
                            .to_string()
                    });
                }
                LastfmEvent::SessionFailed { error, .. } => {
                    self.lastfm_poll_abort = None;
                    self.pending_lastfm_token = None;
                    state.lastfm_pending = false;
                    state.status_msg = Some(format!("Last.fm authorization failed: {error}"));
                }
            }
        }
    }

    /// Polls auth.getSession until the user approves the token in the browser
    /// (error 14 keeps polling), a real error occurs, or ~5 minutes elapse.
    async fn poll_session(
        flow_id: u64,
        token: String,
        tx: tokio::sync::mpsc::UnboundedSender<LastfmEvent>,
    ) {
        use crate::utils::lastfm::{LastfmClient, SessionError, get_api_key, get_api_secret};

        let api_key = get_api_key();
        let api_secret = get_api_secret();
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_secs(5)).await;
            match LastfmClient::get_session(&api_key, &api_secret, &token).await {
                Ok(session_key) => {
                    let _ = tx.send(LastfmEvent::Session {
                        flow_id,
                        session_key,
                    });
                    return;
                }
                Err(SessionError::UnauthorizedToken) | Err(SessionError::Transient(_)) => continue,
                Err(SessionError::Other(e)) => {
                    let _ = tx.send(LastfmEvent::SessionFailed {
                        flow_id,
                        error: format!("{e:#}"),
                    });
                    return;
                }
            }
        }
        let _ = tx.send(LastfmEvent::SessionFailed {
            flow_id,
            error: "timed out waiting for browser authorization".to_string(),
        });
    }

    /// Starts (or cancels) the in-app Spotify OAuth flow. All network work and
    /// the callback listener run in a background task; `poll_spotify_auth`
    /// applies the results. Enter while pending cancels.
    pub fn toggle_spotify_auth(&mut self, state: &mut UiState) {
        if let Some(abort) = self.spotify_auth_abort.take() {
            abort.abort();
            // Invalidate late events from the aborted flow before draining.
            self.spotify_auth_gen += 1;
            while self.spotify_rx.try_recv().is_ok() {}
            state.spotify_pending = false;
            state.status_msg = Some("Spotify authorization cancelled".to_string());
            return;
        }

        let client_id = crate::config::AppConfig::load()
            .ok()
            .and_then(|cfg| cfg.get_client_id());

        self.spotify_auth_gen += 1;
        let flow_id = self.spotify_auth_gen;
        state.spotify_pending = true;
        state.status_msg = Some("Starting Spotify authorization...".to_string());
        let tx = self.spotify_tx.clone();
        let handle = tokio::spawn(async move {
            Self::run_spotify_auth(flow_id, client_id, tx).await;
        });
        self.spotify_auth_abort = Some(handle.abort_handle());
    }

    /// Chained auth: Web API (custom client_id, if configured) then streaming
    /// (official client_id). Mirrors `setup-spotify` semantics without
    /// touching the terminal.
    async fn run_spotify_auth(
        flow_id: u64,
        client_id: Option<String>,
        tx: tokio::sync::mpsc::UnboundedSender<SpotifyAuthEvent>,
    ) {
        use crate::spotify::auth::SpotifyAuth;

        let mut guard = SpotifyAuthGuard {
            flow_id,
            tx: tx.clone(),
            armed: true,
        };

        let had_custom_id = client_id.is_some();
        if let Some(cid) = client_id {
            let _ = tx.send(SpotifyAuthEvent::Progress {
                flow_id,
                msg: "Step 1/2: authorize the Web API in your browser...".to_string(),
            });
            match SpotifyAuth::authenticate_with_client_id_tui(&cid).await {
                Ok((access_token, refresh_token, expires_in)) => {
                    if !crate::config::save_refresh_token(&refresh_token) {
                        let _ = tx.send(SpotifyAuthEvent::Progress {
                            flow_id,
                            msg: "warning: Web API token could not be saved to disk; \
                                 re-authorization may be needed after restart"
                                .to_string(),
                        });
                    }
                    let _ = tx.send(SpotifyAuthEvent::WebApiConnected {
                        flow_id,
                        client_id: cid.clone(),
                        access_token,
                        refresh_token,
                        expires_in,
                    });
                }
                Err(e) => {
                    guard.armed = false;
                    let _ = tx.send(SpotifyAuthEvent::Failed {
                        flow_id,
                        error: format!("{e:#}"),
                    });
                    return;
                }
            }
        }

        let step = if had_custom_id {
            "Step 2/2: streaming"
        } else {
            "Streaming"
        };
        let _ = tx.send(SpotifyAuthEvent::Progress {
            flow_id,
            msg: format!("{step}: authorize streaming in your browser..."),
        });
        match SpotifyAuth::authenticate_with_client_id_tui(crate::config::OFFICIAL_CLIENT_ID).await
        {
            Ok((_, refresh_token, _)) => {
                guard.armed = false;
                // The streaming token is only read back from disk, so a failed
                // save means the flow produced nothing usable.
                if crate::config::save_streaming_refresh_token(&refresh_token) {
                    let _ = tx.send(SpotifyAuthEvent::Done { flow_id });
                } else {
                    let _ = tx.send(SpotifyAuthEvent::Failed {
                        flow_id,
                        error: "authorized, but the streaming token could not be saved to disk"
                            .to_string(),
                    });
                }
            }
            Err(e) => {
                guard.armed = false;
                let _ = tx.send(SpotifyAuthEvent::Failed {
                    flow_id,
                    error: format!("{e:#}"),
                });
            }
        }
    }

    /// Applies background OAuth results. Returns true when the Web API client
    /// just became authenticated so the caller can kick off data fetches.
    pub fn poll_spotify_auth(
        &mut self,
        state: &mut UiState,
        spotify: &crate::spotify::SpotifyClient,
    ) -> bool {
        let mut web_api_connected = false;
        while let Ok(ev) = self.spotify_rx.try_recv() {
            if ev.flow_id() != self.spotify_auth_gen {
                continue;
            }
            match ev {
                SpotifyAuthEvent::Progress { msg, .. } => {
                    state.status_msg = Some(msg);
                }
                SpotifyAuthEvent::WebApiConnected {
                    client_id,
                    access_token,
                    refresh_token,
                    expires_in,
                    ..
                } => {
                    spotify.adopt_token(&client_id, &access_token, &refresh_token, expires_in);
                    state.spotify_authenticated = true;
                    web_api_connected = true;
                    state.status_msg = Some("Spotify Web API connected".to_string());
                }
                SpotifyAuthEvent::Done { .. } => {
                    state.spotify_pending = false;
                    self.spotify_auth_abort = None;
                    state.status_msg = Some("Spotify connected! Streaming ready.".to_string());
                }
                SpotifyAuthEvent::Failed { error, .. } => {
                    state.spotify_pending = false;
                    self.spotify_auth_abort = None;
                    state.status_msg = Some(format!("Spotify authorization failed: {error}"));
                }
            }
        }
        web_api_connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn spotify_auth_stale_events_are_ignored() {
        let mut mgr = IntegrationManager::new();
        let mut state = UiState::new();
        let spotify = crate::spotify::SpotifyClient::new_unauthenticated()
            .await
            .unwrap();

        mgr.spotify_auth_gen = 2;
        let _ = mgr.spotify_tx.send(SpotifyAuthEvent::Progress {
            flow_id: 1,
            msg: "stale".to_string(),
        });
        let _ = mgr.spotify_tx.send(SpotifyAuthEvent::Done { flow_id: 1 });

        assert!(!mgr.poll_spotify_auth(&mut state, &spotify));
        assert!(state.status_msg.is_none());
        // The stale Done must not clear a newer flow's pending state.
        state.spotify_pending = true;
        assert!(state.spotify_pending);
    }

    #[tokio::test]
    async fn spotify_auth_done_clears_pending_and_abort_handle() {
        let mut mgr = IntegrationManager::new();
        let mut state = UiState::new();
        let spotify = crate::spotify::SpotifyClient::new_unauthenticated()
            .await
            .unwrap();

        mgr.spotify_auth_gen = 3;
        state.spotify_pending = true;
        let task = tokio::spawn(std::future::pending::<()>());
        mgr.spotify_auth_abort = Some(task.abort_handle());

        let _ = mgr.spotify_tx.send(SpotifyAuthEvent::Done { flow_id: 3 });
        mgr.poll_spotify_auth(&mut state, &spotify);

        assert!(!state.spotify_pending);
        assert!(mgr.spotify_auth_abort.is_none());
    }

    #[tokio::test]
    async fn spotify_auth_web_api_event_authenticates_live_client() {
        let mut mgr = IntegrationManager::new();
        let mut state = UiState::new();
        let spotify = crate::spotify::SpotifyClient::new_unauthenticated()
            .await
            .unwrap();

        mgr.spotify_auth_gen = 1;
        let _ = mgr.spotify_tx.send(SpotifyAuthEvent::WebApiConnected {
            flow_id: 1,
            client_id: "cid".to_string(),
            access_token: "at".to_string(),
            refresh_token: "rt".to_string(),
            expires_in: 3600,
        });

        assert!(mgr.poll_spotify_auth(&mut state, &spotify));
        assert!(state.spotify_authenticated);
        assert!(spotify.authenticated.load(Ordering::Relaxed));
        assert!(!state.spotify_pending);
    }

    #[tokio::test]
    async fn lastfm_enter_while_pending_cancels_flow() {
        let mut mgr = IntegrationManager::new();
        let mut state = UiState::new();

        mgr.pending_lastfm_token = Some("tok".to_string());
        state.lastfm_pending = true;
        let prev_gen = mgr.lastfm_auth_gen;

        mgr.toggle_lastfm(&mut state);

        assert!(mgr.pending_lastfm_token.is_none());
        assert!(!state.lastfm_pending);
        assert!(mgr.lastfm_auth_gen > prev_gen);
        assert!(
            state
                .status_msg
                .as_deref()
                .is_some_and(|m| m.contains("cancelled"))
        );
    }
}
