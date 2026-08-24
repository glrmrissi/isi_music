// TODO: modularize this file (~970 lines) into smaller modules
pub mod fetcher;
pub mod handlers;
pub mod integrations;
pub mod library;
pub mod metadata;
pub mod player;
pub mod player_mgr;
pub mod theme_mgr;
pub mod ui;

use crate::utils::debug_overlay::{DebugOverlay, LogLevel};
use crate::utils::lock::lock_or_recover;
use crate::utils::theme::ThemeWatcher;
use anyhow::Result;
use ratatui::Terminal;
#[cfg(feature = "album-art")]
use ratatui_image::picker::Picker;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;

use crate::spotify::SpotifyClient;
#[cfg(feature = "album-art")]
use crate::ui::AlbumArtData;
use crate::ui::{Ui, UiState};
use crate::utils::discord::DiscordRpc;
use crate::utils::lastfm::LastfmClient;
#[cfg(windows)]
use crate::utils::media_keys::MediaKey;
#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::utils::mpris::MprisCmd;
#[cfg(windows)]
use crate::utils::smtc::SmtcCmd;
use crate::utils::theme::Theme;

#[cfg(target_os = "linux")]
use libc;

pub use fetcher::FetchResult;

pub struct App {
    pub seek_tx: mpsc::Sender<u32>,
    pub seek_rx: mpsc::Receiver<u32>,
    spotify: Arc<SpotifyClient>,
    pub player_mgr: player_mgr::PlayerManager,
    pub integrations: integrations::IntegrationManager,
    ui: Ui,
    state: UiState,
    last_tick: Instant,
    should_quit: bool,
    last_seek_time: Option<Instant>,
    seek_hold_count: u32,
    current_track_uri: String,
    #[cfg(feature = "album-art")]
    picker: Picker,
    pub theme_mgr: theme_mgr::ThemeManager,
    keybinds: crate::keybinds::Keybinds,
    keybinds_rx: crate::keybinds::KeybindsWatcher,
    pub fetcher: fetcher::FetchCoordinator,
    pub debug_overlay: Arc<DebugOverlay>,
    settings_panel: Option<crate::ui::SettingsPanel>,
    #[cfg(target_os = "linux")]
    trim_counter: u64,
    audio: crate::config::AudioConfig,
    needs_redraw: bool,
    last_click_time: Option<Instant>,
    last_click_pos: (u16, u16),
}

impl App {
    pub async fn new(
        #[cfg(feature = "album-art")] picker: Picker,
        theme: Theme,
        theme_rx: ThemeWatcher,
        keybinds: crate::keybinds::Keybinds,
        keybinds_rx: crate::keybinds::KeybindsWatcher,
    ) -> Result<Self> {
        let (seek_tx, seek_rx) = mpsc::channel::<u32>();
        let settings = Arc::new(Mutex::new(
            crate::settings::Settings::load().unwrap_or_default(),
        ));
        let cfg = lock_or_recover(&settings).config.clone();
        let autoplay_enabled = cfg.autoplay_enabled();
        let lastfm = match &cfg.lastfm.session_key {
            Some(sk) => {
                use crate::utils::lastfm::{get_api_key, get_api_secret};
                Some(Arc::new(LastfmClient::new(
                    get_api_key(),
                    get_api_secret(),
                    sk.clone(),
                )))
            }
            _ => None,
        };

        let debug_overlay = Arc::new(DebugOverlay::new());

        debug_overlay.log(LogLevel::Info, "isi-music starting up");

        let mut startup_warning: Option<String> = None;

        let spotify = match SpotifyClient::new().await {
            Ok(s) => s,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("SPOTIFY_FORBIDDEN") {
                    warn!(
                        "Spotify returned 403 — shared client_id may have hit 5-user Dev Mode limit"
                    );
                    debug_overlay.log(
                        LogLevel::Warn,
                        "Spotify 403 — create your own app: isi-music setup-spotify",
                    );
                    startup_warning = Some(
                        "Spotify 403: your Client ID hit the Development Mode limit. Create your own app: isi-music setup-spotify".to_string(),
                    );
                } else {
                    debug_overlay.log(
                        LogLevel::Warn,
                        format!("Spotify unavailable ({e:#}), starting in local-only mode"),
                    );
                }

                SpotifyClient::new_unauthenticated().await?
            }
        };

        if spotify.authenticated || crate::config::load_streaming_refresh_token().is_some() {
            crate::player::ensure_streaming_auth()
                .await
                .map_err(|e| anyhow::anyhow!("Streaming authentication failed: {e}"))?;
        }

        let volume = crate::config::load_volume();
        let mut saved_volume = volume;
        let db_path = crate::config::get_local_db_path();

        let mut state = UiState::new();
        state.show_album_art = cfg.show_cover_images();
        state.show_visualizer = cfg.show_visualizer();
        state.show_breadcrumb = cfg.show_breadcrumb();
        state.compact_mode = cfg.compact_mode_default();
        state.reactive_theme_enabled = theme.reactive_theme;
        state.first_run = std::env::var("ISI_MUSIC_FIRST_RUN").is_ok();
        state.spotify_authenticated = spotify.authenticated;

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
        state.restore_session(&cfg.session);
        if let Some(vol) = cfg.session.volume {
            state.playback.volume = vol.min(100);
            saved_volume = state.playback.volume;
        }

        #[cfg(all(feature = "mpris", target_os = "linux"))]
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

        #[cfg(windows)]
        let media_keys = if cfg.media_keys_enabled() {
            match crate::utils::media_keys::spawn() {
                Ok(h) => {
                    debug_overlay.log(LogLevel::Info, format!("Global media hotkeys registered"));
                    Some(h)
                }
                Err(e) => {
                    debug_overlay.log(LogLevel::Warn, format!("Media hotkeys unavailable: {e}"));
                    None
                }
            }
        } else {
            None
        };

        #[cfg(windows)]
        {
            if cfg.smtc_enabled() {
                crate::utils::smtc::cleanup_cover_cache();
            }
        }
        #[cfg(windows)]
        let smtc = if cfg.smtc_enabled() {
            match crate::utils::smtc::spawn() {
                Ok(h) => {
                    debug_overlay.log(LogLevel::Info, format!("SMTC integration enabled"));
                    Some(h)
                }
                Err(e) => {
                    debug_overlay.log(LogLevel::Warn, format!("SMTC unavailable: {e}"));
                    None
                }
            }
        } else {
            debug_overlay.log(LogLevel::Info, format!("SMTC disabled by config"));
            None
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

        let cache_manager = crate::utils::cache::CacheManager::new()?;
        {
            let cm = cache_manager.clone();
            tokio::spawn(async move {
                let _ = cm.cleanup_expired().await;
            });
        }
        let settings_panel = crate::ui::SettingsPanel::new(cache_manager, Arc::clone(&settings));

        state.lastfm_connected = lastfm.is_some();

        Ok(Self {
            seek_tx,
            seek_rx,
            spotify: Arc::new(spotify),
            player_mgr: player_mgr::PlayerManager::new(
                saved_volume,
                db_path,
                autoplay_enabled,
                initial_art,
            ),
            integrations: {
                let mut mgr = integrations::IntegrationManager::new();
                mgr.lastfm = lastfm;
                mgr.discord = discord;
                #[cfg(all(feature = "mpris", target_os = "linux"))]
                {
                    mgr.mpris = mpris;
                }
                #[cfg(windows)]
                {
                    mgr.media_keys = media_keys;
                    mgr.smtc = smtc;
                }
                mgr
            },
            ui: Ui::new(theme.clone(), debug_overlay.clone()),
            state,
            last_tick: Instant::now(),
            should_quit: false,
            last_seek_time: None,
            seek_hold_count: 0,
            current_track_uri: String::new(),
            #[cfg(feature = "album-art")]
            picker,
            theme_mgr: theme_mgr::ThemeManager::new(theme, theme_rx),
            keybinds,
            keybinds_rx,
            fetcher: fetcher::FetchCoordinator::new(),
            debug_overlay,
            settings_panel: Some(settings_panel),
            #[cfg(target_os = "linux")]
            trim_counter: 0,
            audio: cfg.audio.clone(),
            needs_redraw: true,
            last_click_time: None,
            last_click_pos: (0, 0),
        })
    }

    #[cfg(test)]
    pub async fn new_for_test() -> Self {
        let (seek_tx, seek_rx) = mpsc::channel();
        let spotify = crate::spotify::SpotifyClient::new_unauthenticated()
            .await
            .expect("test client init");
        let debug_overlay = Arc::new(DebugOverlay::new());
        let cache_manager = crate::utils::cache::CacheManager::new().expect("test cache init");
        let settings = Arc::new(Mutex::new(crate::settings::Settings::default()));
        let mut state = crate::ui::UiState::new();
        {
            let cfg = lock_or_recover(&*settings).config.clone();
            state.show_album_art = cfg.show_cover_images();
            state.show_visualizer = cfg.show_visualizer();
            state.show_breadcrumb = cfg.show_breadcrumb();
            state.compact_mode = cfg.compact_mode_default();
        }

        if spotify.authenticated {
            if let Ok(playlists) = spotify.fetch_playlists().await {
                state.playlists = playlists;
                if !state.playlists.is_empty() {
                    state.playlist_list.select(Some(0));
                }
            }
        }

        let autoplay_enabled = lock_or_recover(&*settings).config.autoplay_enabled();
        Self {
            seek_tx,
            seek_rx,
            spotify: Arc::new(spotify),
            player_mgr: player_mgr::PlayerManager::new(50, String::new(), autoplay_enabled, None),
            integrations: integrations::IntegrationManager::new(),
            ui: crate::ui::Ui::new(Default::default(), debug_overlay.clone()),
            state,
            last_tick: Instant::now(),
            should_quit: false,
            last_seek_time: None,
            seek_hold_count: 0,
            current_track_uri: String::new(),
            #[cfg(feature = "album-art")]
            picker: ratatui_image::picker::Picker::halfblocks(),
            theme_mgr: theme_mgr::ThemeManager::new(
                Default::default(),
                crate::utils::theme::ThemeWatcher::noop(),
            ),
            keybinds: crate::keybinds::Keybinds::defaults(),
            keybinds_rx: crate::keybinds::KeybindsWatcher::noop(),
            fetcher: fetcher::FetchCoordinator::new(),
            debug_overlay,
            #[cfg(target_os = "linux")]
            trim_counter: 0,
            settings_panel: Some(crate::ui::SettingsPanel::new(
                cache_manager,
                Arc::clone(&settings),
            )),
            audio: crate::config::AudioConfig::default(),
            needs_redraw: true,
            last_click_time: None,
            last_click_pos: (0, 0),
        }
    }

    async fn ensure_spotify_player(&mut self) -> bool {
        let ok = self
            .player_mgr
            .ensure_spotify_player(&self.spotify, &self.state, &self.debug_overlay, &self.audio)
            .await;
        if !ok {
            self.state.status_msg = Some(
                "Spotify streaming is not authenticated. Run `isi-music setup-spotify`."
                    .to_string(),
            );
        }
        ok
    }

    async fn ensure_local_player(&mut self) -> bool {
        self.player_mgr
            .ensure_local_player(&self.state, &self.debug_overlay)
            .await
    }

    fn save_session(&self) -> Result<()> {
        let mut cfg = crate::config::AppConfig::load().unwrap_or_default();
        cfg.session.focus = Some(match self.state.focus {
            crate::ui::Focus::Library => "library".to_string(),
            crate::ui::Focus::Playlists => "playlists".to_string(),
            crate::ui::Focus::Tracks => "tracks".to_string(),
            crate::ui::Focus::Queue => "queue".to_string(),
            crate::ui::Focus::Search => "search".to_string(),
        });
        cfg.session.active_content = Some(match self.state.active_content {
            crate::ui::ActiveContent::None => "none".to_string(),
            crate::ui::ActiveContent::Tracks => "tracks".to_string(),
            crate::ui::ActiveContent::Albums => "albums".to_string(),
            crate::ui::ActiveContent::Artists => "artists".to_string(),
            crate::ui::ActiveContent::Shows => "shows".to_string(),
            crate::ui::ActiveContent::LocalFiles => "local_files".to_string(),
        });
        cfg.session.compact_mode = Some(self.state.compact_mode);
        cfg.session.library_selected = self.state.library_list.selected();
        cfg.session.volume = Some(self.player_mgr.saved_volume);
        cfg.save()
    }

    pub async fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()>
    where
        B::Error: Send + Sync + 'static,
    {
        let tick_rate = Duration::from_millis(33);
        self.last_tick = Instant::now();

        loop {
            match self.theme_mgr.poll_theme_changes() {
                theme_mgr::ThemeChange::Apply { theme } => {
                    self.ui = Ui::new(theme, self.debug_overlay.clone());
                    self.state.reactive_theme_enabled = self.theme_mgr.theme.reactive_theme;
                }
                theme_mgr::ThemeChange::None => {}
            }
            while let Ok(new_keybinds) = self.keybinds_rx.rx.try_recv() {
                self.keybinds = new_keybinds;
            }

            let now = Instant::now();
            let delta_ms = now.duration_since(self.last_tick).as_millis() as u64;
            self.last_tick = now;

            #[cfg(all(feature = "palette", feature = "album-art"))]
            if let Some(blended) = self.theme_mgr.lerp_reactive(now) {
                self.ui = Ui::new(blended, self.debug_overlay.clone());
                self.needs_redraw = true;
            }

            self.poll_local_scan();
            {
                let (redraw, reconnect) = self
                    .fetcher
                    .poll_pending_fetch(&mut self.state, &self.spotify);
                if redraw {
                    self.needs_redraw = true;
                }
                if reconnect {
                    self.player_mgr.session_reconnecting = true;
                }
            }

            if let Some(player) = &self.player_mgr.player {
                if let Some(pb) = player.current_playback_state() {
                    let prev_title = self.state.playback.title.clone();
                    let progress = self.state.playback.progress_ms;

                    if pb.is_local {
                        self.state.playback.merge_from_api(pb);
                        self.state.playback.progress_ms = progress;

                        if self.state.playback.title != prev_title {
                            #[cfg(feature = "album-art")]
                            let _ = self.state.album_art.take();
                            self.fetcher.album_art_pending = None;
                            self.fetcher.last_art_uri.clear();

                            if let Some(cover_str) = self.state.playback.cover_path.as_deref() {
                                let path = std::path::PathBuf::from(cover_str);
                                if path.exists() {
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    tokio::spawn(async move {
                                        if let Ok(bytes) = tokio::fs::read(&path).await {
                                            let _ = tx.send(bytes);
                                        }
                                    });
                                    self.fetcher.album_art_pending = Some(rx);
                                }
                            }

                            self.fetcher.ensure_lyrics(&self.debug_overlay);
                            if let Some(lyrics) = &self.fetcher.lyrics {
                                lyrics.request(
                                    &self.state.playback.title,
                                    &self.state.playback.artist,
                                    &self.current_track_uri,
                                );
                            }

                            self.state.playback.lyrics = None;
                            self.state.playback.lyrics_loading = true;
                        }
                    } else {
                        self.state.playback.volume = pb.volume;
                        self.state.playback.shuffle = pb.shuffle;
                        self.state.playback.repeat = pb.repeat;
                        if pb.is_playing {
                            // Only adopt the player's progress if we have no
                            // local clock running yet, or if it has drifted
                            // significantly (>2s). This prevents the progress
                            // bar from jumping back and forth between the
                            // locally-interpolated value and the player's
                            // (potentially stale) reported position.
                            let local_progress = self
                                .player_mgr
                                .playing_started_at
                                .map(|t| {
                                    self.player_mgr.progress_at_play_start
                                        + t.elapsed().as_millis() as u64
                                })
                                .unwrap_or(u64::MAX);
                            if self.player_mgr.playing_started_at.is_none()
                                || local_progress.abs_diff(pb.progress_ms) > 2000
                            {
                                self.player_mgr.progress_at_play_start = pb.progress_ms;
                                self.player_mgr.playing_started_at = Some(Instant::now());
                            }
                        } else {
                            self.state.playback.progress_ms = pb.progress_ms;
                            self.player_mgr.playing_started_at = None;
                            self.player_mgr.progress_at_play_start = pb.progress_ms;
                        }
                    }
                }
            }

            let mut latest_seek = None;

            while let Ok(pos) = self.seek_rx.try_recv() {
                latest_seek = Some(pos);
            }

            if let Some(target_pos) = latest_seek {
                self.state.viz_bands.fill(0.0);
                let target_pos_u64 = target_pos as u64;
                self.state.playback.progress_ms = target_pos_u64;
                self.player_mgr.progress_at_play_start = target_pos_u64;
                if self.state.playback.is_playing {
                    self.player_mgr.playing_started_at = Some(Instant::now());
                }

                if let Some(player) = &mut self.player_mgr.player {
                    if self.player_mgr.local_active {
                        player.seek_mut(target_pos);
                    } else {
                        player.seek(target_pos);
                    }
                }
            }

            if self.fetcher.poll_lyrics(&mut self.state) {
                self.needs_redraw = true;
            }

            let notif_result = self
                .player_mgr
                .handle_notifications(&mut self.state, &self.debug_overlay);
            let mut needs_sync = notif_result.needs_sync;
            let needs_reconnect = notif_result.needs_reconnect;
            let needs_crossover = notif_result.needs_crossover;
            let needs_radio_refill = notif_result.needs_radio_refill;

            {
                self.debug_overlay.update_metrics();
            }

            if needs_crossover {
                let parked_has_queue = self
                    .player_mgr
                    .parked_player
                    .as_ref()
                    .map(|p| !p.user_queue().is_empty())
                    .unwrap_or(false);
                if parked_has_queue {
                    if let Some(ref mut p) = self.player_mgr.player {
                        if p.is_playing() {
                            p.pause();
                        }
                    }
                    std::mem::swap(
                        &mut self.player_mgr.player,
                        &mut self.player_mgr.parked_player,
                    );
                    self.player_mgr.local_active = !self.player_mgr.local_active;
                    self.player_mgr.band_energies = self
                        .player_mgr
                        .player
                        .as_ref()
                        .and_then(|p| p.band_energies());
                    if let Some(player) = &mut self.player_mgr.player {
                        if player.next() {
                            needs_sync = true;
                        }
                    }
                }
            }

            if needs_radio_refill {
                self.radio_refill().await;
                if let Some(player) = &mut self.player_mgr.player {
                    if player.next() {
                        needs_sync = true;
                    }
                }
            }

            if needs_sync {
                self.sync_track_selection();
                self.sync_queue_display();
                if self.player_mgr.player.is_none() {
                    if let Ok(Ok(current_pb)) =
                        tokio::time::timeout(Duration::from_secs(5), self.spotify.fetch_playback())
                            .await
                    {
                        self.player_mgr
                            .merge_playback_from_api(&mut self.state, current_pb);
                    }
                }
            }

            if needs_reconnect && !self.player_mgr.session_reconnecting {
                self.player_mgr.session_reconnecting = true;
                self.reconnect_player().await;
            }

            if !self.player_mgr.spotify_streaming_disabled
                && self.player_mgr.last_playback_health_check.elapsed()
                    > if self.player_mgr.initial_sync_done {
                        Duration::from_secs(45)
                    } else {
                        Duration::from_secs(5)
                    }
            {
                self.player_mgr.last_playback_health_check = Instant::now();

                if let Ok(Some(_token)) =
                    tokio::time::timeout(Duration::from_secs(5), self.spotify.get_access_token())
                        .await
                {}

                if self.player_mgr.player.is_none() {
                    if let Ok(Ok(current_pb)) =
                        tokio::time::timeout(Duration::from_secs(5), self.spotify.fetch_playback())
                            .await
                    {
                        self.player_mgr
                            .merge_playback_from_api(&mut self.state, current_pb);
                    }
                }

                self.player_mgr.initial_sync_done = true;
            }

            if self.player_mgr.session_reconnecting
                && self.player_mgr.reconnect_attempts > 0
                && self.player_mgr.reconnect_attempts < 5
            {
                self.reconnect_player().await;
            }

            if let Some(ref arc) = self.player_mgr.band_energies {
                if let Ok(bands) = arc.lock() {
                    self.state.viz_bands.clone_from(&*bands);
                    if self.state.show_visualizer {
                        self.needs_redraw = true;
                    }
                }
            }

            #[cfg(all(feature = "mpris", target_os = "linux"))]
            {
                self.integrations.update_mpris(&self.state);
                let cmds = self.integrations.poll_mpris_cmds();
                for cmd in cmds {
                    match cmd {
                        MprisCmd::Play => {
                            self.ensure_spotify_player().await;
                            if let Some(p) = &mut self.player_mgr.player {
                                p.play();
                            }
                            self.state.playback.is_playing = true;
                        }
                        MprisCmd::Pause => {
                            if let Some(p) = &mut self.player_mgr.player {
                                p.pause();
                            }
                            self.state.playback.is_playing = false;
                        }
                        MprisCmd::Next => {
                            self.ensure_spotify_player().await;
                            if let Some(p) = &mut self.player_mgr.player {
                                p.next();
                            }
                            self.sync_track_selection();
                            self.sync_queue_display();
                        }
                        MprisCmd::Prev => {
                            self.ensure_spotify_player().await;
                            if let Some(p) = &mut self.player_mgr.player {
                                p.prev();
                            }
                            self.sync_track_selection();
                        }
                        MprisCmd::Seek(us) => {
                            let ms = (us / 1000) as u64;
                            self.state.playback.progress_ms = ms;
                            self.player_mgr.progress_at_play_start = ms;
                            if self.state.playback.is_playing {
                                self.player_mgr.playing_started_at = Some(Instant::now());
                            }
                            if let Some(p) = &mut self.player_mgr.player {
                                p.seek_mut(ms as u32);
                            }
                        }
                        MprisCmd::SetVolume(v) => {
                            self.player_mgr.saved_volume = (v * 100.0).round() as u8;
                            if let Some(p) = &mut self.player_mgr.player {
                                p.set_volume(self.player_mgr.saved_volume);
                                self.state.playback.volume = p.volume();
                            }
                        }
                    }
                }
            }

            #[cfg(windows)]
            {
                self.integrations.update_smtc(&self.state);
                let smtc_cmds = self.integrations.poll_smtc_cmds();
                for cmd in smtc_cmds {
                    match cmd {
                        SmtcCmd::Play => {
                            if let Some(p) = &mut self.player_mgr.player {
                                p.play();
                                self.state.playback.is_playing = true;
                            } else {
                                self.ensure_spotify_player().await;
                                if self.player_mgr.player.is_none() {
                                    self.ensure_local_player().await;
                                }
                                if let Some(p) = &mut self.player_mgr.player {
                                    p.play();
                                    self.state.playback.is_playing = true;
                                }
                            }
                        }
                        SmtcCmd::Pause => {
                            if let Some(p) = &mut self.player_mgr.player {
                                p.pause();
                                self.state.playback.is_playing = false;
                            }
                        }
                        SmtcCmd::Next => {
                            if self.player_mgr.player.is_none() {
                                self.ensure_spotify_player().await;
                            }
                            if let Some(p) = &mut self.player_mgr.player {
                                if p.next() {
                                    self.sync_track_selection();
                                    self.sync_queue_display();
                                }
                            } else if self.spotify.authenticated {
                                let _ = self.spotify.next_track().await;
                            }
                        }
                        SmtcCmd::Previous => {
                            if self.player_mgr.player.is_none() {
                                self.ensure_spotify_player().await;
                            }
                            if let Some(p) = &mut self.player_mgr.player {
                                if p.prev() {
                                    self.sync_track_selection();
                                    self.sync_queue_display();
                                }
                            } else if self.spotify.authenticated {
                                let _ = self.spotify.prev_track().await;
                            }
                        }
                        SmtcCmd::Seek(ms) => {
                            self.state.playback.progress_ms = ms;
                            self.player_mgr.progress_at_play_start = ms;
                            if self.state.playback.is_playing {
                                self.player_mgr.playing_started_at = Some(Instant::now());
                            }
                            if let Some(p) = &mut self.player_mgr.player {
                                p.seek_mut(ms as u32);
                            }
                        }
                    }
                }
            }

            #[cfg(windows)]
            {
                let media_key_cmds = self.integrations.poll_media_keys();
                for cmd in media_key_cmds {
                    match cmd {
                        MediaKey::PlayPause => {
                            if self.state.playback.is_playing {
                                if let Some(p) = &mut self.player_mgr.player {
                                    p.pause();
                                }
                                self.state.playback.is_playing = false;
                            } else if let Some(p) = &mut self.player_mgr.player {
                                p.play();
                                self.state.playback.is_playing = true;
                            } else {
                                self.ensure_spotify_player().await;
                                if self.player_mgr.player.is_none() {
                                    self.ensure_local_player().await;
                                }
                                if let Some(p) = &mut self.player_mgr.player {
                                    if !p.is_playing() {
                                        p.play();
                                    }
                                    self.state.playback.is_playing = true;
                                } else if self.spotify.authenticated {
                                    let _ = self.spotify.toggle_playback().await;
                                }
                            }
                        }
                        MediaKey::Next => {
                            if self.player_mgr.player.is_none() {
                                self.ensure_spotify_player().await;
                            }
                            if let Some(p) = &mut self.player_mgr.player {
                                if p.next() {
                                    self.sync_track_selection();
                                    self.sync_queue_display();
                                }
                            } else if self.spotify.authenticated {
                                let _ = self.spotify.next_track().await;
                            }
                        }
                        MediaKey::Previous => {
                            if self.player_mgr.player.is_none() {
                                self.ensure_spotify_player().await;
                            }
                            if let Some(p) = &mut self.player_mgr.player {
                                if p.prev() {
                                    self.sync_track_selection();
                                    self.sync_queue_display();
                                }
                            } else if self.spotify.authenticated {
                                let _ = self.spotify.prev_track().await;
                            }
                        }
                    }
                }
            }

            #[cfg(feature = "album-art")]
            if let Some(rx) = &mut self.fetcher.album_art_pending {
                if let Ok(bytes) = rx.try_recv() {
                    self.fetcher.album_art_pending = None;

                    // Feed the SMTC thumbnail: Spotify tracks played via
                    // librespot never populate `pb.art_url`/`pb.cover_path`,
                    // so we materialise the downloaded cover bytes into a temp
                    // file and expose it via `cover_path` for the SMTC worker.
                    #[cfg(windows)]
                    if let Some(path) = crate::utils::smtc::cache_cover_bytes(&bytes)
                        && let Some(s) = path.to_str()
                    {
                        self.state.playback.cover_path = Some(s.to_string());
                    }

                    let reactive_enabled = self.theme_mgr.reactive_theme_enabled();
                    let decoded = tokio::task::spawn_blocking(move || {
                        let img = image::load_from_memory(&bytes)?;
                        let img = if img.width() <= 256 && img.height() <= 256 {
                            img
                        } else {
                            img.thumbnail(256, 256)
                        };
                        Ok::<_, anyhow::Error>(img)
                    })
                    .await;
                    match decoded {
                        Ok(Ok(img)) => {
                            #[cfg(all(feature = "palette", feature = "album-art"))]
                            if reactive_enabled {
                                let swatches = crate::utils::palette::extract_palette(&img, 5);
                                tracing::debug!(
                                    "reactive: swatches={} theme.reactive_theme={}",
                                    swatches.len(),
                                    reactive_enabled
                                );
                                self.theme_mgr.store_swatches(swatches.clone());
                                self.theme_mgr.start_reactive(&swatches, &self.ui);
                                tracing::debug!("reactive: start_reactive_theme called");
                            }

                            let image_state = self.picker.new_resize_protocol(img);
                            self.state.album_art = Some(AlbumArtData {
                                image_state: Some(image_state),
                            });
                            self.needs_redraw = true;
                        }
                        Ok(Err(e)) => {
                            self.debug_overlay
                                .log(LogLevel::Error, format!("Failed to decode album art: {e}"));
                        }
                        Err(e) => {
                            self.debug_overlay
                                .log(LogLevel::Error, format!("Album art task failed: {e}"));
                        }
                    }
                }
            }

            self.integrations.update_discord(&self.state);

            #[cfg(feature = "album-art")]
            self.maybe_fetch_album_art().await;

            let visual_active = self.state.loading
                || self.state.search_active
                || self.state.quick_search_active
                || self.state.command_mode
                || self.state.add_to_playlist_mode
                || self.state.delete_playlist_confirm
                || self.settings_panel.as_ref().map_or(false, |p| p.visible)
                || self.fetcher.pending_fetch.is_some()
                || self.fetcher.pending_pagination.is_some()
                || self.fetcher.local_scan_rx.is_some()
                || self.fetcher.album_art_pending.is_some()
                || self
                    .fetcher
                    .lyrics
                    .as_ref()
                    .map_or(false, |l| l.is_loading())
                || self.state.status_msg.is_some();

            let active = self.state.playback.is_playing || visual_active;

            if self.needs_redraw || visual_active {
                terminal.draw(|f| {
                    self.ui.render(f, &mut self.state);
                    if let Some(ref panel) = self.settings_panel {
                        panel.render(
                            f,
                            &self.state,
                            &self.theme_mgr.theme,
                            self.player_mgr.autoplay_enabled,
                        );
                    }
                })?;
                self.needs_redraw = false;
            }

            let timeout = if active {
                tick_rate
            } else {
                Duration::from_millis(100)
            }
            .checked_sub(now.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

            let mut event_received = false;
            if crossterm::event::poll(timeout)? {
                match crossterm::event::read()? {
                    crossterm::event::Event::Key(key_event) => {
                        // Windows consoles also emit Release events — handle only
                        // Press/Repeat or every physical keypress is processed twice
                        if matches!(
                            key_event.kind,
                            crossterm::event::KeyEventKind::Press
                                | crossterm::event::KeyEventKind::Repeat
                        ) {
                            self.handle_key(key_event.code, key_event.modifiers).await?;
                            event_received = true;
                        }
                    }
                    crossterm::event::Event::Mouse(mouse_event) => {
                        self.handle_mouse(mouse_event).await?;
                        event_received = true;
                    }
                    _ => {}
                }
                if event_received {
                    self.needs_redraw = true;
                }
            }

            self.player_mgr.interpolate_progress(&mut self.state);

            if self.state.playback.is_playing {
                self.state.spin_angle += delta_ms as f64 * 0.003;
                self.state.marquee_ms += delta_ms;
                if self.state.marquee_ms >= 120 {
                    self.state.marquee_offset += (self.state.marquee_ms / 120) as usize;
                    self.state.marquee_ms %= 120;
                }

                self.integrations.maybe_scrobble(&self.state);
                self.needs_redraw = true;
            }

            #[cfg(target_os = "linux")]
            {
                self.trim_counter += 1;
                if self.trim_counter % 300 == 0 {
                    unsafe {
                        libc::malloc_trim(0);
                    }
                }
            }

            if self.should_quit {
                let _ = self.save_session();
                break;
            }
        }

        Ok(())
    }

    pub async fn toggle_lastfm_scrobbling(&mut self) {
        self.integrations.toggle_lastfm(&mut self.state).await;
    }
}
