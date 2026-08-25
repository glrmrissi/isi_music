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

pub struct IntegrationManager {
    pub lastfm: Option<Arc<LastfmClient>>,
    pub pending_lastfm_token: Option<String>,
    pub scrobble_sent: bool,
    pub track_start_unix: u64,
    pub discord: Option<DiscordRpc>,
    discord_last_title: String,
    discord_last_playing: bool,
    discord_pending_since: Option<Instant>,
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
        Self {
            lastfm: None,
            pending_lastfm_token: None,
            scrobble_sent: false,
            track_start_unix: 0,
            discord: None,
            discord_last_title: String::new(),
            discord_last_playing: false,
            discord_pending_since: None,
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
    }

    pub fn set_track_start(&mut self, ts: u64) {
        self.track_start_unix = ts;
    }

    pub fn update_discord(&mut self, state: &UiState) {
        let Some(discord) = &self.discord else {
            return;
        };
        let pb = &state.playback;
        let title_changed = pb.title != self.discord_last_title;
        let playing_changed = pb.is_playing != self.discord_last_playing;

        if title_changed {
            self.discord_pending_since = Some(Instant::now());
            self.discord_last_title = pb.title.clone();
            self.discord_last_playing = pb.is_playing;
        } else if playing_changed {
            self.discord_last_playing = pb.is_playing;
            self.discord_pending_since = None;
            if pb.title.is_empty() {
                discord.clear();
            } else if pb.is_playing {
                discord.update_playing(&pb.title, &pb.artist, pb.art_url.as_deref());
            } else {
                discord.update_paused(&pb.title, &pb.artist);
            }
        }

        if let Some(since) = self.discord_pending_since {
            let art_ready = pb.art_url.is_some() || pb.is_local;
            let timeout_secs = if pb.is_local { 1 } else { 5 };
            let timed_out = since.elapsed() >= Duration::from_secs(timeout_secs);
            if art_ready || timed_out {
                self.discord_pending_since = None;
                if pb.title.is_empty() {
                    discord.clear();
                } else if pb.is_playing {
                    discord.update_playing(&pb.title, &pb.artist, pb.art_url.as_deref());
                } else {
                    discord.update_paused(&pb.title, &pb.artist);
                }
            }
        }
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

        if duration < 30_000 || (progress < duration / 2 && progress < 240_000) {
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
        }
        self.scrobble_sent = true;
    }

    pub async fn toggle_lastfm(&mut self, state: &mut UiState) {
        use crate::utils::lastfm::{LastfmClient, get_api_key, get_api_secret};

        let mut cfg = crate::config::AppConfig::load().unwrap_or_default();

        if cfg.lastfm.session_key.is_some() {
            cfg.lastfm.session_key = None;
            let _ = cfg.save();
            self.lastfm = None;
            self.pending_lastfm_token = None;
            state.lastfm_connected = false;
            state.lastfm_pending = false;
            state.status_msg = Some("Last.fm scrobbling disconnected".to_string());
            return;
        }

        if let Some(token) = self.pending_lastfm_token.clone() {
            state.status_msg = Some("Exchanging session key with Last.fm...".to_string());
            match LastfmClient::get_session(&get_api_key(), &get_api_secret(), &token).await {
                Ok(session_key) => {
                    cfg.lastfm.session_key = Some(session_key.clone());
                    let _ = cfg.save();
                    self.lastfm = Some(Arc::new(LastfmClient::new(
                        get_api_key(),
                        get_api_secret(),
                        session_key,
                    )));
                    self.pending_lastfm_token = None;
                    state.lastfm_connected = true;
                    state.lastfm_pending = false;
                    state.status_msg = Some("Last.fm connected! Scrobbling enabled.".to_string());
                }
                Err(e) => {
                    state.status_msg = Some(format!(
                        "Last.fm auth not complete. Please authorize in browser, then press Enter again. ({e})"
                    ));
                }
            }
            return;
        }

        state.status_msg = Some("Requesting Last.fm auth token...".to_string());
        match LastfmClient::get_auth_token(&get_api_key()).await {
            Ok(token) => {
                let auth_url = format!(
                    "https://www.last.fm/api/auth/?api_key={}&token={}",
                    get_api_key(),
                    token
                );
                let _ = open::that(&auth_url);
                self.pending_lastfm_token = Some(token);
                state.lastfm_pending = true;
                state.status_msg = Some(
                    "Opened Last.fm in browser. Authorize, then press Enter on Last.fm in Options to finish.".to_string(),
                );
            }
            Err(e) => {
                state.status_msg = Some(format!("Failed to get Last.fm token: {e}"));
            }
        }
    }
}
