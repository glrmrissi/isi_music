pub mod handlers;
pub mod library;
pub mod metadata;
pub mod player;
pub mod ui;

use crate::utils::debug_overlay::{DebugOverlay, LogLevel};
use crate::utils::theme::ThemeWatcher;
use anyhow::Result;
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;

use crate::player::NativePlayer;
use crate::player::{AudioPlayer, LocalPlayer, PlayerNotification};
use crate::spotify::SpotifyClient;
use crate::ui::{AlbumArtData, Ui, UiState};
use crate::utils::discord::DiscordRpc;
use crate::utils::lastfm::LastfmClient;
#[cfg(feature = "mpris")]
use crate::utils::mpris::{MprisCmd, MprisHandle, MprisState};
use crate::utils::theme::Theme;
use rspotify::model::RepeatState;

pub struct App {
    pub seek_tx: mpsc::Sender<u32>,
    pub seek_rx: mpsc::Receiver<u32>,
    spotify: SpotifyClient,
    player: Option<Box<dyn AudioPlayer>>,
    parked_player: Option<Box<dyn AudioPlayer>>,
    local_active: bool,
    saved_volume: u8,
    local_db_path: String,
    lastfm: Option<Arc<LastfmClient>>,
    ui: Ui,
    state: UiState,
    last_tick: Instant,
    should_quit: bool,
    last_seek_time: Option<Instant>,
    seek_hold_count: u32,
    scrobble_sent: bool,
    track_start_unix: u64,
    current_track_uri: String,
    last_art_uri: String,
    album_art_pending: Option<tokio::sync::oneshot::Receiver<Vec<u8>>>,
    picker: Picker,
    #[cfg(feature = "mpris")]
    mpris: Option<MprisHandle>,
    discord: Option<DiscordRpc>,
    discord_last_title: String,
    discord_last_playing: bool,
    discord_pending_since: Option<Instant>,
    band_energies: Option<Arc<Mutex<Vec<f32>>>>,
    art_url: Option<String>,
    session_reconnecting: bool,
    radio_mode: bool,
    recent_track_uris: std::collections::VecDeque<String>,
    playing_tracks: Vec<crate::spotify::TrackSummary>,
    theme: Theme,
    theme_rx: ThemeWatcher,
    keybinds: crate::keybinds::Keybinds,
    keybinds_rx: crate::keybinds::KeybindsWatcher,
    consecutive_unavailable: u32,
    spotify_streaming_disabled: bool,
    local_scan_rx: Option<tokio::sync::oneshot::Receiver<Vec<crate::ui::LocalNode>>>,
    local_scan_total: usize,
    lyrics: crate::utils::lyrics::LyricsHandle,
    pub debug_overlay: Arc<DebugOverlay>,
    reconnect_attempts: u32,
    last_reconnect_attempt: Option<Instant>,
    last_playback_health_check: Instant,
    playing_started_at: Option<Instant>,
    progress_at_play_start: u64,
    initial_sync_done: bool,
    options_panel: Option<crate::ui::OptionsPanel>,
}

impl App {
    pub async fn new(
        picker: Picker,
        theme: Theme,
        theme_rx: ThemeWatcher,
        keybinds: crate::keybinds::Keybinds,
        keybinds_rx: crate::keybinds::KeybindsWatcher,
    ) -> Result<Self> {
        let (seek_tx, seek_rx) = mpsc::channel::<u32>();
        let cfg = crate::config::AppConfig::load().unwrap_or_default();
        let lastfm = match (
            &cfg.lastfm.api_key,
            &cfg.lastfm.api_secret,
            &cfg.lastfm.session_key,
        ) {
            (Some(k), Some(s), Some(sk)) => Some(Arc::new(LastfmClient::new(
                k.clone(),
                s.clone(),
                sk.clone(),
            ))),
            _ => None,
        };
        
        let debug_overlay = Arc::new(DebugOverlay::new());

        debug_overlay.log(LogLevel::Info, "isi-music starting up");

        let mut startup_warning: Option<String> = None;

        let mut spotify = match SpotifyClient::new().await {
            Ok(s) => s,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("SPOTIFY_FORBIDDEN") {
                    warn!("Spotify returned 403 — shared client_id may have hit 5-user Dev Mode limit");
                    debug_overlay.log(
                        LogLevel::Warn,
                        "Spotify 403 — create your own app: isi-music setup-spotify",
                    );
                    startup_warning = Some(
                        "⚠ Spotify 403: seu Client ID atingiu o limite do Development Mode. Crie seu próprio app: isi-music setup-spotify".to_string(),
                    );
                } else {
                    debug_overlay.log(
                        LogLevel::Warn,
                        format!("Spotify unavailable ({e:#}), starting in local-only mode"),
                    );
                }

                SpotifyClient::new_unauthenticated().await
            }
        };

        let volume = crate::config::load_volume();
        let db_path = crate::config::get_local_db_path();

        let lyrics = crate::utils::lyrics::LyricsHandle::new(
            db_path.clone().into(),
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
            debug_overlay.clone(),
        )
        .expect("Failed to open lyrics cache");

        let mut state = UiState::new();

        if let Some(msg) = startup_warning {
            state.status_msg = Some(msg);
        }

        if spotify.authenticated {
            match spotify.fetch_playlists().await {
                Ok(playlists) => {
                    state.playlists = playlists;
                    if !state.playlists.is_empty() {
                        state.playlist_list.select(Some(0));
                    }
                }
                Err(e) => {
                    warn!("Failed to load playlists: {e}");
                    state.status_msg = Some(format!("Failed to load playlists: {e}"));
                }
            }
        }

        let mut pb = spotify.fetch_playback().await.unwrap_or_default();
        pb.is_playing = false;
        let initial_art = pb.art_url.clone();
        state.art_url = initial_art.clone();
        state.playback = pb;

        #[cfg(feature = "mpris")]
        let mpris = match crate::utils::mpris::spawn().await {
            Ok(h) => {
                debug_overlay.log(LogLevel::Info, format!("MPRIS D-Bus server started"));
                Some(h)
            }
            Err(e) => {
                debug_overlay.log(LogLevel::Error, format!("MPRIS unavailable: {e}"));
                None
            }
        };
        
        let discord = if cfg.discord.enabled == Some(true) {
            let app_id = cfg
                .discord
                .app_id
                .as_deref()
                .unwrap_or(crate::utils::discord::DEFAULT_APP_ID);
            DiscordRpc::spawn(app_id)
        } else {
            None
        };

        let cache_manager = crate::utils::cache::CacheManager::new();
        let options_panel = crate::ui::OptionsPanel::new(cache_manager);

        Ok(Self {
            seek_tx,
            seek_rx,
            spotify,
            player: None,
            parked_player: None,
            local_active: false,
            saved_volume: volume,
            local_db_path: db_path,
            lastfm,
            ui: Ui::new(theme.clone(), debug_overlay.clone()),
            state,
            last_tick: Instant::now(),
            should_quit: false,
            last_seek_time: None,
            seek_hold_count: 0,
            scrobble_sent: false,
            track_start_unix: 0,
            current_track_uri: String::new(),
            last_art_uri: String::new(),
            album_art_pending: None,
            picker,
            #[cfg(feature = "mpris")]
            mpris,
            discord,
            discord_last_title: String::new(),
            discord_last_playing: false,
            discord_pending_since: None,
            band_energies: None,
            art_url: initial_art,
            session_reconnecting: false,
            radio_mode: false,
            recent_track_uris: std::collections::VecDeque::new(),
            playing_tracks: Vec::new(),
            theme,
            theme_rx,
            keybinds,
            keybinds_rx,
            consecutive_unavailable: 0,
            spotify_streaming_disabled: false,
            local_scan_rx: None,
            local_scan_total: 0,
            lyrics,
            debug_overlay,
            reconnect_attempts: 0,
            last_reconnect_attempt: None,
            last_playback_health_check: Instant::now(),
            playing_started_at: None,
            progress_at_play_start: 0,
            initial_sync_done: false,
            options_panel: Some(options_panel),
        })
    }

    #[cfg(test)]
    pub async fn new_for_test() -> Self {
        let (seek_tx, seek_rx) = mpsc::channel();
        let spotify = crate::spotify::SpotifyClient::new_unauthenticated().await;
        let debug_overlay = Arc::new(DebugOverlay::new());
        let lyrics = crate::utils::lyrics::LyricsHandle::new(
            std::env::temp_dir().join("isi-music-test-lyrics.db"),
            reqwest::Client::new(),
            debug_overlay.clone(),
        )
        .expect("Failed to create test lyrics handle");
        let cache_manager = crate::utils::cache::CacheManager::new();
        let mut state = crate::ui::UiState::new();

        if spotify.authenticated {
            if let Ok(playlists) = spotify.fetch_playlists().await {
                state.playlists = playlists;
                if !state.playlists.is_empty() {
                    state.playlist_list.select(Some(0));
                }
            }
        }

        Self {
            seek_tx,
            seek_rx,
            spotify,
            player: None,
            parked_player: None,
            local_active: false,
            saved_volume: 50,
            local_db_path: String::new(),
            lastfm: None,
            ui: crate::ui::Ui::new(Default::default(), debug_overlay.clone()),
            state,
            last_tick: Instant::now(),
            should_quit: false,
            last_seek_time: None,
            seek_hold_count: 0,
            scrobble_sent: false,
            track_start_unix: 0,
            current_track_uri: String::new(),
            last_art_uri: String::new(),
            album_art_pending: None,
            picker: ratatui_image::picker::Picker::halfblocks(),
            #[cfg(feature = "mpris")]
            mpris: None,
            discord: None,
            discord_last_title: String::new(),
            discord_last_playing: false,
            discord_pending_since: None,
            band_energies: None,
            art_url: None,
            session_reconnecting: false,
            radio_mode: false,
            recent_track_uris: std::collections::VecDeque::new(),
            playing_tracks: Vec::new(),
            theme: Default::default(),
            theme_rx: crate::utils::theme::ThemeWatcher::noop(),
            keybinds: crate::keybinds::Keybinds::defaults(),
            keybinds_rx: crate::keybinds::KeybindsWatcher::noop(),
            consecutive_unavailable: 0,
            spotify_streaming_disabled: false,
            local_scan_rx: None,
            local_scan_total: 0,
            lyrics,
            debug_overlay,
            reconnect_attempts: 0,
            last_reconnect_attempt: None,
            last_playback_health_check: Instant::now(),
            playing_started_at: None,
            progress_at_play_start: 0,
            initial_sync_done: false,
            options_panel: Some(crate::ui::OptionsPanel::new(cache_manager)),
        }
    }

    async fn ensure_spotify_player(&mut self) -> bool {
        if self.player.is_some() && !self.local_active {
            return true;
        }
        if self.parked_player.is_some() && self.local_active {
            std::mem::swap(&mut self.player, &mut self.parked_player);
            self.local_active = false;
            self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
            return true;
        }
        let Some(token) = self.spotify.get_access_token().await else {
            return false;
        };
        match NativePlayer::new(token, false).await {
            Ok(mut p) => {
                p.set_volume(self.saved_volume);
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
                self.debug_overlay
                    .log(LogLevel::Warn, format!("Failed to create Spotify player: {e:#}"));
                false
            }
        }
    }

    async fn ensure_local_player(&mut self) -> bool {
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
            Ok(p) => {
                self.band_energies = p.band_energies();
                self.player = Some(Box::new(p));
                self.local_active = true;
                true
            }
            Err(e) => {
                self.debug_overlay
                    .log(LogLevel::Error, format!("Failed to create local player: {e}"));
                false
            }
        }
    }

    pub async fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let tick_rate = Duration::from_millis(16);
        self.last_tick = Instant::now();

        let debug_overlay = Arc::new(DebugOverlay::new());

        loop {
            while let Ok(new_theme) = self.theme_rx.try_recv() {
                self.theme = new_theme.clone();
                self.ui = Ui::new(new_theme, self.debug_overlay.clone());
            }
            while let Ok(new_keybinds) = self.keybinds_rx.rx.try_recv() {
                self.keybinds = new_keybinds;
            }

            let now = Instant::now();
            let delta_ms = now.duration_since(self.last_tick).as_millis() as u64;
            self.last_tick = now;

            self.poll_local_scan();

            if let Some(player) = &self.player {
                if let Some(pb) = player.current_playback_state() {
                    let prev_title = self.state.playback.title.clone();
                    let progress = self.state.playback.progress_ms;
                    let radio_mode = self.state.playback.radio_mode;

                    if pb.is_local {
                        let saved_lyrics = self.state.playback.lyrics.take();
                        let saved_lyrics_loading = self.state.playback.lyrics_loading;
                        let saved_lyrics_scroll = self.state.playback.lyrics_scroll;

                        self.state.playback = pb;
                        self.state.playback.progress_ms = progress;
                        self.state.playback.radio_mode = radio_mode;
                        self.state.playback.lyrics = saved_lyrics;
                        self.state.playback.lyrics_loading = saved_lyrics_loading;
                        self.state.playback.lyrics_scroll = saved_lyrics_scroll;

                        if self.state.playback.title != prev_title {
                            self.state.album_art = None;
                            self.album_art_pending = None;
                            self.last_art_uri.clear();

                            if let Some(cover_str) = self.state.playback.cover_path.as_deref() {
                                let path = std::path::PathBuf::from(cover_str);
                                if path.exists() {
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    tokio::spawn(async move {
                                        if let Ok(bytes) = tokio::fs::read(&path).await {
                                            let _ = tx.send(bytes);
                                        }
                                    });
                                    self.album_art_pending = Some(rx);
                                }
                            }

                            self.lyrics.request(
                                &self.state.playback.title,
                                &self.state.playback.artist,
                                &self.current_track_uri,
                            );

                            self.state.playback.lyrics = None;
                            self.state.playback.lyrics_loading = true;
                        }
                    } else {
                        self.state.playback.volume = pb.volume;
                        self.state.playback.shuffle = pb.shuffle;
                        self.state.playback.repeat = pb.repeat;
                        if pb.is_playing {
                            self.state.playback.progress_ms = pb.progress_ms;
                        }
                        if self.playing_started_at.is_none() {
                            self.progress_at_play_start = pb.progress_ms;
                        }
                    }
                }
            }

            let mut needs_sync = false;
            let mut needs_reconnect = false;
            let mut needs_crossover = false;
            let mut needs_radio_refill = false;

            let parked_has_queue = self
                .parked_player
                .as_ref()
                .map(|p| !p.user_queue().is_empty())
                .unwrap_or(false);

            let mut latest_seek = None;

            while let Ok(pos) = self.seek_rx.try_recv() {
                latest_seek = Some(pos);
            }

            if let Some(target_pos) = latest_seek {
                self.state.viz_bands.fill(0.0);
                let target_pos_u64 = target_pos as u64;
                self.state.playback.progress_ms = target_pos_u64;
                self.progress_at_play_start = target_pos_u64;
                if self.state.playback.is_playing {
                    self.playing_started_at = Some(Instant::now());
                }

                if let Some(player) = &mut self.player {
                    if self.local_active {
                        player.seek_mut(target_pos);
                    } else {
                        player.seek(target_pos);
                    }
                }
            }

            match (self.lyrics.poll(), self.lyrics.is_loading()) {
                (Some(data), _) => {
                    self.state.playback.lyrics_loading = false;
                    self.state.playback.lyrics = if data.is_empty() { None } else { Some(data) };
                }
                (None, true) => {
                    self.state.playback.lyrics_loading = true;
                }
                (None, false) => {
                    self.state.playback.lyrics_loading = false;
                }
            }

            let mut needs_player_swap: bool = false;

            if let Some(player) = &mut self.player {
                while let Some(notif) = player.try_recv_event() {
                    match notif {
                        PlayerNotification::TrackEnded => {
                            self.consecutive_unavailable = 0;
                            if parked_has_queue {
                                needs_crossover = true;
                                self.state.playback.is_playing = false;
                            } else if player.next() {
                                needs_sync = true;
                            } else if self.radio_mode && !self.local_active {
                                needs_radio_refill = true;
                            } else {
                                needs_crossover = true;
                                self.state.playback.is_playing = false;
                            }
                        }
                        PlayerNotification::Playing => {
                            self.consecutive_unavailable = 0;
                            self.state.playback.is_playing = true;
                            if self.local_active && self.playing_started_at.is_none() {
                                self.playing_started_at = Some(Instant::now());
                                self.progress_at_play_start = self.state.playback.progress_ms;
                            }
                        }
                        PlayerNotification::Paused => self.state.playback.is_playing = false,
                        PlayerNotification::TrackUnavailable => {
                            self.consecutive_unavailable += 1;
                            self.state.status_msg =
                                Some("Track unavailable, skipping...".to_string());
                            if parked_has_queue {
                                needs_crossover = true;
                                self.state.playback.is_playing = false;
                            } else if player.next() {
                                needs_sync = true;
                            } else if self.radio_mode && !self.local_active {
                                needs_radio_refill = true;
                            } else {
                                needs_crossover = true;
                                self.state.playback.is_playing = false;
                            }
                        }
                        PlayerNotification::SessionLost => {
                            if !self.spotify_streaming_disabled {
                                self.state.status_msg =
                                    Some("Session lost, reconnecting...".to_string());
                                needs_reconnect = true;
                            }
                        }
                        PlayerNotification::FreeAccountDetected => {
                            if !self.spotify_streaming_disabled {
                                warn!("Free account detected - switching to local-only mode");
                                self.spotify_streaming_disabled = true;
                                self.consecutive_unavailable = 0;

                                debug_overlay.log(
                                    LogLevel::Warn,
                                    format!("Free account detected - switching to local-only mode"),
                                );
                                self.state.status_msg = Some(
                                    "⚠ Spotify Premium required. Switched to local-only mode."
                                        .to_string(),
                                );

                                needs_player_swap = true;
                            }
                        }
                    }
                }

                if needs_player_swap {
                    self.player = None;
                    self.band_energies = None;
                    if self.parked_player.is_some() {
                        std::mem::swap(&mut self.player, &mut self.parked_player);
                        self.local_active = true;
                        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
                        needs_sync = true;
                    } else {
                        needs_sync = false;
                    }
                }
            }

            {
                self.debug_overlay.update_metrics();
            }

            if needs_crossover {
                let parked_has_queue = self
                    .parked_player
                    .as_ref()
                    .map(|p| !p.user_queue().is_empty())
                    .unwrap_or(false);
                if parked_has_queue {
                    if let Some(ref mut p) = self.player {
                        if p.is_playing() {
                            p.pause();
                        }
                    }
                    std::mem::swap(&mut self.player, &mut self.parked_player);
                    self.local_active = !self.local_active;
                    self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
                    if let Some(player) = &mut self.player {
                        if player.next() {
                            needs_sync = true;
                        }
                    }
                }
            }

            if needs_radio_refill {
                self.radio_refill().await;
                if let Some(player) = &mut self.player {
                    if player.next() {
                        needs_sync = true;
                    }
                }
            }

            if needs_sync {
                self.sync_track_selection();
                self.sync_queue_display();
                if self.player.is_none() {
                    if let Ok(current_pb) = self.spotify.fetch_playback().await {
                        let saved_lyrics = self.state.playback.lyrics.take();
                        let saved_lyrics_loading = self.state.playback.lyrics_loading;
                        let saved_lyrics_scroll = self.state.playback.lyrics_scroll;
                        let pb_playing = current_pb.is_playing;
                        let pb_progress = current_pb.progress_ms;
                        self.art_url = current_pb.art_url.clone();
                        self.state.playback = current_pb;
                        self.state.playback.lyrics = saved_lyrics;
                        self.state.playback.lyrics_loading = saved_lyrics_loading;
                        self.state.playback.lyrics_scroll = saved_lyrics_scroll;
                        if self.playing_started_at.is_none() {
                            self.state.playback.is_playing = false;
                        }
                        if pb_playing {
                            if self.playing_started_at.is_some() {
                                self.playing_started_at = Some(Instant::now());
                                self.progress_at_play_start = pb_progress;
                            }
                        } else {
                            self.playing_started_at = None;
                            self.progress_at_play_start = pb_progress;
                        }
                    }
                }
            }

            if needs_reconnect && !self.session_reconnecting {
                self.session_reconnecting = true;
                self.reconnect_player().await;
            }

            if !self.spotify_streaming_disabled
                && self.last_playback_health_check.elapsed()
                    > if self.initial_sync_done {
                        Duration::from_secs(45)
                    } else {
                        Duration::from_secs(5)
                    }
            {
                self.last_playback_health_check = Instant::now();

                if let Some(_token) = self.spotify.get_access_token().await {}

                if self.player.is_none() {
                    if let Ok(current_pb) = self.spotify.fetch_playback().await {
                        let saved_lyrics = self.state.playback.lyrics.take();
                        let saved_lyrics_loading = self.state.playback.lyrics_loading;
                        let saved_lyrics_scroll = self.state.playback.lyrics_scroll;
                        let pb_playing = current_pb.is_playing;
                        let pb_progress = current_pb.progress_ms;
                        self.art_url = current_pb.art_url.clone();
                        self.state.playback = current_pb;
                        self.state.playback.lyrics = saved_lyrics;
                        self.state.playback.lyrics_loading = saved_lyrics_loading;
                        self.state.playback.lyrics_scroll = saved_lyrics_scroll;
                        if self.playing_started_at.is_none() {
                            self.state.playback.is_playing = false;
                        }
                        if pb_playing {
                            if self.playing_started_at.is_some() {
                                self.playing_started_at = Some(Instant::now());
                                self.progress_at_play_start = pb_progress;
                            }
                        } else {
                            self.playing_started_at = None;
                            self.progress_at_play_start = pb_progress;
                        }
                    }
                }

                self.initial_sync_done = true;
            }

            if self.session_reconnecting
                && self.reconnect_attempts > 0
                && self.reconnect_attempts < 5
            {
                self.reconnect_player().await;
            }

            if let Some(ref arc) = self.band_energies {
                if let Ok(bands) = arc.lock() {
                    self.state.viz_bands.clone_from(&*bands);
                }
            }

            #[cfg(feature = "mpris")]
            if let Some(mpris) = &mut self.mpris {
                let pb = &self.state.playback;
                let _art_url = self.state.art_url.clone();

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

                let cmds: Vec<MprisCmd> = {
                    let mut v = Vec::new();
                    while let Ok(c) = mpris.cmd_rx.try_recv() {
                        v.push(c);
                    }
                    v
                };

                for cmd in cmds {
                    match cmd {
                        MprisCmd::Play => {
                            self.ensure_spotify_player().await;
                            if let Some(p) = &mut self.player {
                                p.play();
                            }
                            self.state.playback.is_playing = true;
                        }
                        MprisCmd::Pause => {
                            if let Some(p) = &mut self.player {
                                p.pause();
                            }
                            self.state.playback.is_playing = false;
                        }
                        MprisCmd::Next => {
                            self.ensure_spotify_player().await;
                            if let Some(p) = &mut self.player {
                                p.next();
                            }
                            self.sync_track_selection();
                            self.sync_queue_display();
                        }
                        MprisCmd::Prev => {
                            self.ensure_spotify_player().await;
                            if let Some(p) = &mut self.player {
                                p.prev();
                            }
                            self.sync_track_selection();
                        }
                        MprisCmd::Seek(us) => {
                            let ms = (us / 1000) as u64;
                            self.state.playback.progress_ms = ms;
                            self.progress_at_play_start = ms;
                            if self.state.playback.is_playing {
                                self.playing_started_at = Some(Instant::now());
                            }
                            if let Some(p) = &mut self.player {
                                p.seek_mut(ms as u32);
                            }
                        }
                        MprisCmd::SetVolume(v) => {
                            self.saved_volume = (v * 100.0).round() as u8;
                            if let Some(p) = &mut self.player {
                                p.set_volume(self.saved_volume);
                                self.state.playback.volume = p.volume();
                            }
                        }
                    }
                }
            }

            if let Some(rx) = &mut self.album_art_pending {
                if let Ok(bytes) = rx.try_recv() {
                    self.album_art_pending = None;
                    match image::load_from_memory(&bytes) {
                        Ok(img) => {
                            let resized = img.thumbnail(256, 256);
                            let image_state = self.picker.new_resize_protocol(resized);
                            self.state.album_art = Some(AlbumArtData {
                                image_state: Some(image_state),
                            });
                        }
                        Err(e) => {
                            self.debug_overlay
                                .log(LogLevel::Error, format!("MPRIS unavailable: {e}"));
                        }
                    }
                }
            }

            if let Some(discord) = &self.discord {
                let pb = &self.state.playback;
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

            self.maybe_fetch_album_art().await;

            terminal.draw(|f| {
                self.ui.render(f, &mut self.state);
                if let Some(ref panel) = self.options_panel {
                    panel.render(f, &self.state);
                }
            })?;

            let timeout = tick_rate
                .checked_sub(now.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if crossterm::event::poll(timeout)? {
                if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
                    self.handle_key(key_event.code, key_event.modifiers).await?;
                }
            }

            if self.state.playback.is_playing {
                if self.player.is_some() && !self.local_active {
                    // NativePlayer: progress from current_playback_state
                } else {
                    if self.playing_started_at.is_none() {
                        self.playing_started_at = Some(Instant::now());
                        self.progress_at_play_start = self.state.playback.progress_ms;
                    }
                    let elapsed = self
                        .playing_started_at
                        .map(|t| t.elapsed().as_millis() as u64)
                        .unwrap_or(0);
                    self.state.playback.progress_ms = self.progress_at_play_start + elapsed;
                }
                if self.state.playback.progress_ms >= self.state.playback.duration_ms {
                    if self.player.is_none() {
                        self.state.playback.is_playing = false;
                        self.state.playback.progress_ms = self.state.playback.duration_ms;
                        self.playing_started_at = None;
                        self.progress_at_play_start = self.state.playback.duration_ms;
                    }
                }
            } else if self.playing_started_at.is_some() {
                let elapsed = self
                    .playing_started_at
                    .map(|t| t.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                self.progress_at_play_start = self.progress_at_play_start + elapsed;
                self.playing_started_at = None;
            }

            if self.state.playback.is_playing {
                self.state.spin_angle += delta_ms as f64 * 0.003;
                self.state.marquee_ms += delta_ms;
                if self.state.marquee_ms >= 120 {
                    self.state.marquee_offset += (self.state.marquee_ms / 120) as usize;
                    self.state.marquee_ms %= 120;
                }

                if !self.scrobble_sent {
                    let progress = self.state.playback.progress_ms;
                    let duration = self.state.playback.duration_ms;

                    if progress >= 30_000 && duration > 30_000 {
                        if let Some(lfm) = self.lastfm.clone() {
                            let artist = self.state.playback.artist.clone();
                            let track = self.state.playback.title.clone();
                            let ts = self.track_start_unix;
                            let dur = duration;
                            tokio::spawn(async move {
                                lfm.scrobble(&artist, &track, ts, dur).await;
                            });
                        }
                        self.scrobble_sent = true;
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }
}
