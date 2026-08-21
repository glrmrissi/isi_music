// TODO: modularize this file (~970 lines) into smaller modules
pub mod handlers;
pub mod library;
pub mod metadata;
pub mod player;
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

use crate::player::NativePlayer;
use crate::player::{AudioPlayer, LocalPlayer, PlayerNotification};
#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::spotify::RepeatState;
use crate::spotify::SpotifyClient;
#[cfg(feature = "album-art")]
use crate::ui::AlbumArtData;
use crate::ui::{Ui, UiState};
use crate::utils::discord::DiscordRpc;
use crate::utils::lastfm::LastfmClient;
#[cfg(windows)]
use crate::utils::media_keys::{MediaKey, MediaKeysHandle};
#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::utils::mpris::{MprisCmd, MprisHandle, MprisState};
#[cfg(windows)]
use crate::utils::smtc::{SmtcCmd, SmtcHandle, SmtcState};
use crate::utils::theme::Theme;

#[cfg(target_os = "linux")]
use libc;

pub enum FetchResult {
    LikedTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    Albums(Result<(Vec<crate::spotify::AlbumSummary>, u32), String>),
    Artists(Result<Vec<crate::spotify::ArtistSummary>, String>),
    PlaylistTracks(Result<(Vec<crate::spotify::TrackSummary>, u32, u32), String>),
    AlbumTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    ArtistTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    MoreTracks(
        Result<
            (
                Vec<crate::spotify::TrackSummary>,
                u32,
                Option<String>,
                Option<u32>,
            ),
            String,
        >,
    ),
}

pub struct App {
    pub seek_tx: mpsc::Sender<u32>,
    pub seek_rx: mpsc::Receiver<u32>,
    spotify: Arc<SpotifyClient>,
    player: Option<Box<dyn AudioPlayer>>,
    parked_player: Option<Box<dyn AudioPlayer>>,
    local_active: bool,
    saved_volume: u8,
    local_db_path: String,
    lastfm: Option<Arc<LastfmClient>>,
    pending_lastfm_token: Option<String>,
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
    #[cfg(feature = "album-art")]
    picker: Picker,
    #[cfg(all(feature = "mpris", target_os = "linux"))]
    mpris: Option<MprisHandle>,
    #[cfg(windows)]
    media_keys: Option<MediaKeysHandle>,
    #[cfg(windows)]
    smtc: Option<SmtcHandle>,
    discord: Option<DiscordRpc>,
    discord_last_title: String,
    discord_last_playing: bool,
    discord_pending_since: Option<Instant>,
    #[cfg(feature = "mpris")]
    mpris_last_title: String,
    #[cfg(feature = "mpris")]
    mpris_last_artist: String,
    #[cfg(feature = "mpris")]
    mpris_last_album: String,
    #[cfg(feature = "mpris")]
    mpris_last_playing: bool,
    #[cfg(feature = "mpris")]
    mpris_last_art: Option<String>,
    band_energies: Option<Arc<Mutex<Vec<f32>>>>,
    art_url: Option<String>,
    session_reconnecting: bool,
    radio_mode: bool,
    autoplay_enabled: bool,
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
    lyrics: Option<crate::utils::lyrics::LyricsHandle>,
    pub debug_overlay: Arc<DebugOverlay>,
    reconnect_attempts: u32,
    last_reconnect_attempt: Option<Instant>,
    last_playback_health_check: Instant,
    playing_started_at: Option<Instant>,
    progress_at_play_start: u64,
    initial_sync_done: bool,
    settings_panel: Option<crate::ui::SettingsPanel>,
    #[cfg(target_os = "linux")]
    trim_counter: u64,
    pending_fetch: Option<tokio::sync::oneshot::Receiver<FetchResult>>,
    pending_pagination: Option<tokio::sync::oneshot::Receiver<FetchResult>>,
    pending_nav_down: bool,
    audio: crate::config::AudioConfig,
    needs_redraw: bool,
    last_click_time: Option<Instant>,
    last_click_pos: (u16, u16),
    #[cfg(all(feature = "palette", feature = "album-art"))]
    reactive_target: Option<Theme>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    reactive_from: Option<Theme>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    reactive_start: Option<Instant>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    reactive_swatches: Option<Vec<crate::utils::palette::Rgb>>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    reactive_toggle_pending: bool,
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
        let media_keys = match crate::utils::media_keys::spawn() {
            Ok(h) => {
                debug_overlay.log(LogLevel::Info, format!("Global media hotkeys registered"));
                Some(h)
            }
            Err(e) => {
                debug_overlay.log(LogLevel::Warn, format!("Media hotkeys unavailable: {e}"));
                None
            }
        };

        #[cfg(windows)]
        let smtc = match crate::utils::smtc::spawn() {
            Ok(h) => {
                debug_overlay.log(LogLevel::Info, format!("SMTC integration enabled"));
                Some(h)
            }
            Err(e) => {
                debug_overlay.log(LogLevel::Warn, format!("SMTC unavailable: {e}"));
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

        let cache_manager = crate::utils::cache::CacheManager::new()?;
        let settings_panel = crate::ui::SettingsPanel::new(cache_manager, Arc::clone(&settings));

        state.lastfm_connected = lastfm.is_some();

        Ok(Self {
            seek_tx,
            seek_rx,
            spotify: Arc::new(spotify),
            player: None,
            parked_player: None,
            local_active: false,
            saved_volume,
            local_db_path: db_path,
            lastfm,
            pending_lastfm_token: None,
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
            #[cfg(feature = "album-art")]
            picker,
            #[cfg(all(feature = "mpris", target_os = "linux"))]
            mpris,
            #[cfg(windows)]
            media_keys,
            #[cfg(windows)]
            smtc,
            discord,
            discord_last_title: String::new(),
            discord_last_playing: false,
            discord_pending_since: None,
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
            band_energies: None,
            art_url: initial_art,
            session_reconnecting: false,
            radio_mode: false,
            autoplay_enabled,
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
            lyrics: None,
            debug_overlay,
            reconnect_attempts: 0,
            last_reconnect_attempt: None,
            last_playback_health_check: Instant::now(),
            playing_started_at: None,
            progress_at_play_start: 0,
            initial_sync_done: false,
            settings_panel: Some(settings_panel),
            #[cfg(target_os = "linux")]
            trim_counter: 0,
            pending_fetch: None,
            pending_pagination: None,
            pending_nav_down: false,
            audio: cfg.audio.clone(),
            needs_redraw: true,
            last_click_time: None,
            last_click_pos: (0, 0),
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_target: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_from: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_start: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_swatches: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_toggle_pending: false,
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
            player: None,
            parked_player: None,
            local_active: false,
            saved_volume: 50,
            local_db_path: String::new(),
            lastfm: None,
            pending_lastfm_token: None,
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
            #[cfg(feature = "album-art")]
            picker: ratatui_image::picker::Picker::halfblocks(),
            #[cfg(all(feature = "mpris", target_os = "linux"))]
            mpris: None,
            #[cfg(windows)]
            media_keys: None,
            #[cfg(windows)]
            smtc: None,
            discord: None,
            discord_last_title: String::new(),
            discord_last_playing: false,
            discord_pending_since: None,
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
            band_energies: None,
            art_url: None,
            session_reconnecting: false,
            radio_mode: false,
            autoplay_enabled,
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
            lyrics: None,
            debug_overlay,
            reconnect_attempts: 0,
            last_reconnect_attempt: None,
            last_playback_health_check: Instant::now(),
            playing_started_at: None,
            progress_at_play_start: 0,
            initial_sync_done: false,
            #[cfg(target_os = "linux")]
            trim_counter: 0,
            settings_panel: Some(crate::ui::SettingsPanel::new(
                cache_manager,
                Arc::clone(&settings),
            )),
            pending_fetch: None,
            pending_pagination: None,
            pending_nav_down: false,
            audio: crate::config::AudioConfig::default(),
            needs_redraw: true,
            last_click_time: None,
            last_click_pos: (0, 0),
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_target: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_from: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_start: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_swatches: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_toggle_pending: false,
        }
    }

    fn ensure_lyrics(&mut self) {
        if self.lyrics.is_none() {
            self.lyrics = crate::utils::lyrics::LyricsHandle::new(
                crate::config::get_local_db_path().into(),
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(8))
                    .build()
                    .unwrap_or_default(),
                self.debug_overlay.clone(),
            )
            .ok();
        }
    }

    fn poll_pending_fetch(&mut self) {
        if let Some(rx) = &mut self.pending_fetch {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending_fetch = None;
                    self.state.loading = false;
                    self.handle_fetch_result(result);
                    self.needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.pending_fetch = None;
                    self.state.loading = false;
                    self.state.status_msg = Some("Fetch task failed".to_string());
                    self.needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &mut self.pending_pagination {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending_pagination = None;
                    self.handle_fetch_result(result);
                    self.needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.pending_pagination = None;
                    self.pending_nav_down = false;
                    self.needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
    }

    fn handle_fetch_result(&mut self, result: FetchResult) {
        match result {
            FetchResult::LikedTracks(Ok((tracks, total))) => {
                self.state.tracks = tracks;
                self.state.tracks_total = total;
                self.state.tracks_offset = self.state.tracks.len() as u32;
                self.state.tracks_api_offset = self.state.tracks.len() as u32;
                self.state.active_playlist_uri = Some("liked_songs".to_string());
                self.state.active_playlist_id = Some("liked_songs".to_string());
                self.state
                    .track_list
                    .select(if self.state.tracks.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = crate::ui::ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.rebuild_sort_indices();
                self.state.status_msg = None;
                self.state.focus = crate::ui::Focus::Tracks;
                self.state.tracks_cursor = self
                    .spotify
                    .library_cache
                    .get_liked_tracks_page(None, 50)
                    .and_then(|(_, _, next)| next);
            }
            FetchResult::LikedTracks(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    self.state.status_msg =
                        Some("Authorization expired, reconnecting...".to_string());
                    self.session_reconnecting = true;
                } else {
                    self.state.status_msg = Some(format!("Error: {e}"));
                }
            }
            FetchResult::Albums(Ok((albums, total))) => {
                self.state.albums = albums;
                self.state.albums_total = total;
                self.state.albums_offset = self.state.albums.len() as u32;
                self.state
                    .album_list
                    .select(if self.state.albums.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = crate::ui::ActiveContent::Albums;
                self.state.search_results = None;
                self.state.status_msg = None;
                self.state.focus = crate::ui::Focus::Tracks;
            }
            FetchResult::Albums(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    self.state.status_msg =
                        Some("Authorization expired, reconnecting...".to_string());
                    self.session_reconnecting = true;
                } else {
                    self.state.status_msg = Some(format!("Error: {e}"));
                }
            }
            FetchResult::Artists(Ok(artists)) => {
                self.state.artists = artists;
                self.state
                    .artist_list
                    .select(if self.state.artists.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = crate::ui::ActiveContent::Artists;
                self.state.search_results = None;
                self.state.status_msg = None;
                self.state.focus = crate::ui::Focus::Tracks;
            }
            FetchResult::Artists(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    self.state.status_msg =
                        Some("Authorization expired, reconnecting...".to_string());
                    self.session_reconnecting = true;
                } else {
                    self.state.status_msg = Some(format!("Error: {e}"));
                }
            }
            FetchResult::PlaylistTracks(Ok((tracks, total, page_items))) => {
                self.state.tracks = tracks;
                self.state.tracks_total = total;
                self.state.tracks_offset = self.state.tracks.len() as u32;
                self.state.tracks_api_offset = page_items;
                self.state
                    .track_list
                    .select(if self.state.tracks.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = crate::ui::ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.rebuild_sort_indices();
                self.state.status_msg = None;
                self.state.focus = crate::ui::Focus::Tracks;
            }
            FetchResult::PlaylistTracks(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    self.state.status_msg =
                        Some("Authorization expired, reconnecting...".to_string());
                    self.session_reconnecting = true;
                } else {
                    self.state.status_msg = Some(format!("Error: {e}"));
                }
            }
            FetchResult::AlbumTracks(Ok((tracks, total))) => {
                self.state.tracks = tracks;
                self.state.tracks_total = total;
                self.state.tracks_offset = self.state.tracks.len() as u32;
                self.state.tracks_api_offset = self.state.tracks.len() as u32;
                self.state
                    .track_list
                    .select(if self.state.tracks.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = crate::ui::ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.rebuild_sort_indices();
                self.state.status_msg = None;
                self.state.focus = crate::ui::Focus::Tracks;
            }
            FetchResult::AlbumTracks(Err(e)) => {
                self.state.status_msg = Some(format!("Error: {e}"));
            }
            FetchResult::ArtistTracks(Ok((tracks, total))) => {
                self.state.tracks = tracks;
                self.state.tracks_total = total;
                self.state.tracks_offset = self.state.tracks.len() as u32;
                self.state.tracks_api_offset = self.state.tracks.len() as u32;
                self.state
                    .track_list
                    .select(if self.state.tracks.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.state.active_content = crate::ui::ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.rebuild_sort_indices();
                self.state.status_msg = None;
                self.state.focus = crate::ui::Focus::Tracks;
            }
            FetchResult::ArtistTracks(Err(e)) => {
                self.state.status_msg = Some(format!("Error: {e}"));
            }
            FetchResult::MoreTracks(Ok((mut new_tracks, total, cursor, page_items))) => {
                let advance_selection = self.pending_nav_down;
                self.pending_nav_down = false;
                let selected_display = self.state.track_list.selected();
                let selected_raw = selected_display
                    .and_then(|display_idx| self.state.sorted_track_indices.get(display_idx))
                    .copied();
                let old_track_len = self.state.tracks.len();
                self.state.tracks_loading = false;
                self.state.status_msg = None;
                if self.state.active_playlist_id.as_deref() == Some("liked_songs") {
                    if total > self.state.tracks_total {
                        self.state.tracks_total = total;
                    }
                    self.state.tracks_cursor = cursor;
                } else {
                    self.state.tracks_total = total;
                }
                self.state.tracks_offset += new_tracks.len() as u32;
                // Advance the API offset by the number of items the API returned
                // (before episode filtering) to avoid re-fetching items on the next page
                if let Some(pi) = page_items {
                    self.state.tracks_api_offset += pi;
                } else {
                    self.state.tracks_api_offset += new_tracks.len() as u32;
                }
                self.state.tracks.append(&mut new_tracks);
                self.state.rebuild_sort_indices();
                if advance_selection {
                    let next_display = selected_raw
                        .and_then(|raw_idx| {
                            self.state
                                .sorted_track_indices
                                .iter()
                                .position(|&idx| idx == raw_idx)
                        })
                        .map(|display_idx| display_idx + 1)
                        .or_else(|| selected_display.map(|display_idx| display_idx + 1));
                    if let Some(next_display) = next_display {
                        if next_display < self.state.sorted_track_indices.len()
                            && self.state.tracks.len() > old_track_len
                        {
                            self.state.track_list.select(Some(next_display));
                        }
                    }
                }
            }
            FetchResult::MoreTracks(Err(e)) => {
                self.pending_nav_down = false;
                self.state.tracks_loading = false;
                self.state.status_msg = Some(format!("Load more error: {e}"));
            }
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
        let token = self.spotify.get_access_token().await;
        match NativePlayer::new(
            token,
            false,
            self.audio.librespot_bitrate(),
            self.audio.gapless,
        )
        .await
        {
            Ok(mut p) => {
                p.set_volume(self.saved_volume);
                p.set_visualizer_enabled(self.state.show_visualizer);
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
                self.state.status_msg = Some(status.clone());
                self.debug_overlay
                    .log(LogLevel::Warn, format!("{status}: {e:#}"));
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
            Ok(mut p) => {
                p.set_visualizer_enabled(self.state.show_visualizer);
                self.band_energies = p.band_energies();
                self.player = Some(Box::new(p));
                self.local_active = true;
                true
            }
            Err(e) => {
                self.debug_overlay.log(
                    LogLevel::Error,
                    format!("Failed to create local player: {e}"),
                );
                false
            }
        }
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
        cfg.session.volume = Some(self.saved_volume);
        cfg.save()
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    fn toggle_reactive_theme(&mut self, enabled: bool) -> anyhow::Result<()> {
        use crate::utils::theme::Theme;
        let path = Theme::get_path().unwrap_or_else(|| std::path::PathBuf::from("theme.toml"));
        let content = std::fs::read_to_string(&path)?;
        let mut theme: Theme = toml::from_str(&content).unwrap_or_default();
        theme.reactive_theme = enabled;
        let new_content = toml::to_string_pretty(&theme)?;
        std::fs::write(&path, new_content)?;
        Ok(())
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    fn start_reactive_theme(&mut self, swatches: &[crate::utils::palette::Rgb]) {
        if swatches.is_empty() {
            return;
        }
        let new_theme = crate::utils::palette::derive_theme(swatches, &self.theme);
        self.reactive_from = Some(self.ui.theme_snapshot());
        self.reactive_target = Some(new_theme);
        self.reactive_start = Some(Instant::now());
    }

    pub async fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()>
    where
        B::Error: Send + Sync + 'static,
    {
        let tick_rate = Duration::from_millis(16);
        self.last_tick = Instant::now();

        let debug_overlay = Arc::new(DebugOverlay::new());

        loop {
            while let Ok(new_theme) = self.theme_rx.try_recv() {
                let preserve_reactive_transition = {
                    #[cfg(all(feature = "palette", feature = "album-art"))]
                    {
                        let preserve =
                            self.reactive_toggle_pending || self.reactive_start.is_some();
                        self.reactive_toggle_pending = false;
                        preserve
                    }
                    #[cfg(not(all(feature = "palette", feature = "album-art")))]
                    {
                        false
                    }
                };
                #[cfg(all(feature = "palette", feature = "album-art"))]
                tracing::debug!(
                    "reactive: theme_watcher fired, preserve={}, reactive_start={}",
                    preserve_reactive_transition,
                    self.reactive_start.is_some()
                );
                self.theme = new_theme.clone();
                if !preserve_reactive_transition {
                    #[cfg(all(feature = "palette", feature = "album-art"))]
                    let skip_apply = self.theme.reactive_theme
                        && self.reactive_start.is_none()
                        && self.reactive_target.is_none();
                    #[cfg(not(all(feature = "palette", feature = "album-art")))]
                    let skip_apply = false;

                    if !skip_apply {
                        self.ui = Ui::new(new_theme, self.debug_overlay.clone());
                        #[cfg(all(feature = "palette", feature = "album-art"))]
                        {
                            self.reactive_start = None;
                            self.reactive_from = None;
                            self.reactive_target = None;
                        }
                    }
                }
                self.state.reactive_theme_enabled = self.theme.reactive_theme;
            }
            while let Ok(new_keybinds) = self.keybinds_rx.rx.try_recv() {
                self.keybinds = new_keybinds;
            }

            let now = Instant::now();
            let delta_ms = now.duration_since(self.last_tick).as_millis() as u64;
            self.last_tick = now;

            #[cfg(all(feature = "palette", feature = "album-art"))]
            if let (Some(start), Some(from), Some(target)) = (
                self.reactive_start,
                self.reactive_from.as_ref(),
                self.reactive_target.as_ref(),
            ) {
                let elapsed = now.duration_since(start).as_millis() as f32;
                let dur = self.theme.reactive_cross_fade_ms.max(1) as f32;
                let t = (elapsed / dur).min(1.0);
                let blended = Theme::lerp(from, target, t);
                self.ui = Ui::new(blended, self.debug_overlay.clone());
                self.needs_redraw = true;
                if t >= 1.0 {
                    self.theme = target.clone();
                    self.reactive_start = None;
                    self.reactive_from = None;
                    self.reactive_target = None;
                }
            }

            self.poll_local_scan();
            self.poll_pending_fetch();

            if let Some(player) = &self.player {
                if let Some(pb) = player.current_playback_state() {
                    let prev_title = self.state.playback.title.clone();
                    let progress = self.state.playback.progress_ms;

                    if pb.is_local {
                        self.state.playback.merge_from_api(pb);
                        self.state.playback.progress_ms = progress;

                        if self.state.playback.title != prev_title {
                            #[cfg(feature = "album-art")]
                            let _ = self.state.album_art.take();
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

                            self.ensure_lyrics();
                            if let Some(ref lyrics) = self.lyrics {
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
                                .playing_started_at
                                .map(|t| {
                                    self.progress_at_play_start + t.elapsed().as_millis() as u64
                                })
                                .unwrap_or(u64::MAX);
                            if self.playing_started_at.is_none()
                                || local_progress.abs_diff(pb.progress_ms) > 2000
                            {
                                self.progress_at_play_start = pb.progress_ms;
                                self.playing_started_at = Some(Instant::now());
                            }
                        } else {
                            self.state.playback.progress_ms = pb.progress_ms;
                            self.playing_started_at = None;
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

            if let Some(ref lyrics) = self.lyrics {
                match (lyrics.poll(), lyrics.is_loading()) {
                    (Some(data), _) => {
                        self.state.playback.lyrics_loading = false;
                        self.state.playback.lyrics =
                            if data.is_empty() { None } else { Some(data) };
                        self.needs_redraw = true;
                    }
                    (None, true) => {
                        self.state.playback.lyrics_loading = true;
                    }
                    (None, false) => {
                        self.state.playback.lyrics_loading = false;
                    }
                }
            }

            let mut needs_player_swap: bool = false;

            if let Some(player) = &mut self.player {
                while let Some(notif) = player.try_recv_event() {
                    match notif {
                        PlayerNotification::TrackEnded => {
                            self.consecutive_unavailable = 0;
                            if player.next() {
                                needs_sync = true;
                            } else if parked_has_queue {
                                needs_crossover = true;
                                self.state.playback.is_playing = false;
                            } else if (self.radio_mode || self.autoplay_enabled)
                                && !self.local_active
                            {
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
                            if player.next() {
                                needs_sync = true;
                            } else if parked_has_queue {
                                needs_crossover = true;
                                self.state.playback.is_playing = false;
                            } else if (self.radio_mode || self.autoplay_enabled)
                                && !self.local_active
                            {
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
                                    "Spotify Premium required. Switched to local-only mode."
                                        .to_string(),
                                );

                                needs_player_swap = true;
                            }
                        }
                        PlayerNotification::PreloadNextTrack => {
                            player.preload_next();
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
                    if let Ok(Ok(current_pb)) =
                        tokio::time::timeout(Duration::from_secs(5), self.spotify.fetch_playback())
                            .await
                    {
                        let pb_playing = current_pb.is_playing;
                        let pb_progress = current_pb.progress_ms;
                        self.art_url = current_pb.art_url.clone();
                        // Preserve locally-interpolated progress to avoid
                        // the bar jumping backwards when the API reports a
                        // slightly stale position.
                        let local_progress = self.state.playback.progress_ms;
                        let local_playing = self.state.playback.is_playing;
                        self.state.playback.merge_from_api(current_pb);
                        if local_playing && pb_playing {
                            // Keep our local progress if it's close to API's
                            if local_progress.abs_diff(pb_progress) <= 2000 {
                                self.state.playback.progress_ms = local_progress;
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

                if let Ok(Some(_token)) =
                    tokio::time::timeout(Duration::from_secs(5), self.spotify.get_access_token())
                        .await
                {}

                if self.player.is_none() {
                    if let Ok(Ok(current_pb)) =
                        tokio::time::timeout(Duration::from_secs(5), self.spotify.fetch_playback())
                            .await
                    {
                        let pb_playing = current_pb.is_playing;
                        let pb_progress = current_pb.progress_ms;
                        self.art_url = current_pb.art_url.clone();
                        let local_progress = self.state.playback.progress_ms;
                        let local_playing = self.state.playback.is_playing;
                        self.state.playback.merge_from_api(current_pb);
                        if local_playing && pb_playing {
                            if local_progress.abs_diff(pb_progress) <= 2000 {
                                self.state.playback.progress_ms = local_progress;
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

            #[cfg(all(feature = "mpris", target_os = "linux"))]
            if let Some(mpris) = &mut self.mpris {
                let pb = &self.state.playback;

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
                    // Update position without cloning strings
                    let mut state = lock_or_recover(&mpris.state);
                    state.position_us = pb.progress_ms as i64 * 1000;
                    state.volume = pb.volume as f64 / 100.0;
                    state.is_playing = pb.is_playing;
                    state.shuffle = pb.shuffle;
                    state.repeat_track = pb.repeat == RepeatState::Track;
                    state.repeat_queue = pb.repeat == RepeatState::Context;
                }

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

            #[cfg(windows)]
            if let Some(smtc) = &self.smtc {
                let pb = &self.state.playback;
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
            let smtc_cmds: Vec<SmtcCmd> = {
                if let Some(smtc) = &self.smtc {
                    let mut v = Vec::new();
                    while let Ok(c) = smtc.cmd_rx.try_recv() {
                        v.push(c);
                    }
                    v
                } else {
                    Vec::new()
                }
            };

            #[cfg(windows)]
            for cmd in smtc_cmds {
                match cmd {
                    SmtcCmd::Play => {
                        if let Some(p) = &mut self.player {
                            p.play();
                            self.state.playback.is_playing = true;
                        } else {
                            self.ensure_spotify_player().await;
                            if self.player.is_none() {
                                self.ensure_local_player().await;
                            }
                            if let Some(p) = &mut self.player {
                                p.play();
                                self.state.playback.is_playing = true;
                            }
                        }
                    }
                    SmtcCmd::Pause => {
                        if let Some(p) = &mut self.player {
                            p.pause();
                            self.state.playback.is_playing = false;
                        }
                    }
                    SmtcCmd::Next => {
                        if self.player.is_none() {
                            self.ensure_spotify_player().await;
                        }
                        if let Some(p) = &mut self.player {
                            if p.next() {
                                self.sync_track_selection();
                                self.sync_queue_display();
                            }
                        } else if self.spotify.authenticated {
                            let _ = self.spotify.next_track().await;
                        }
                    }
                    SmtcCmd::Previous => {
                        if self.player.is_none() {
                            self.ensure_spotify_player().await;
                        }
                        if let Some(p) = &mut self.player {
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
                        self.progress_at_play_start = ms;
                        if self.state.playback.is_playing {
                            self.playing_started_at = Some(Instant::now());
                        }
                        if let Some(p) = &mut self.player {
                            p.seek_mut(ms as u32);
                        }
                    }
                }
            }

            #[cfg(windows)]
            let media_key_cmds: Vec<MediaKey> = {
                if let Some(media_keys) = &self.media_keys {
                    let mut v = Vec::new();
                    while let Ok(cmd) = media_keys.cmd_rx.try_recv() {
                        v.push(cmd);
                    }
                    v
                } else {
                    Vec::new()
                }
            };

            #[cfg(windows)]
            for cmd in media_key_cmds {
                match cmd {
                    MediaKey::PlayPause => {
                        if self.state.playback.is_playing {
                            if let Some(p) = &mut self.player {
                                p.pause();
                            }
                            self.state.playback.is_playing = false;
                        } else if let Some(p) = &mut self.player {
                            p.play();
                            self.state.playback.is_playing = true;
                        } else {
                            self.ensure_spotify_player().await;
                            if self.player.is_none() {
                                self.ensure_local_player().await;
                            }
                            if let Some(p) = &mut self.player {
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
                        if self.player.is_none() {
                            self.ensure_spotify_player().await;
                        }
                        if let Some(p) = &mut self.player {
                            if p.next() {
                                self.sync_track_selection();
                                self.sync_queue_display();
                            }
                        } else if self.spotify.authenticated {
                            let _ = self.spotify.next_track().await;
                        }
                    }
                    MediaKey::Previous => {
                        if self.player.is_none() {
                            self.ensure_spotify_player().await;
                        }
                        if let Some(p) = &mut self.player {
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

            #[cfg(feature = "album-art")]
            if let Some(rx) = &mut self.album_art_pending {
                if let Ok(bytes) = rx.try_recv() {
                    self.album_art_pending = None;

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

                    let decoded = tokio::task::spawn_blocking(move || {
                        let img = image::load_from_memory(&bytes)?;
                        Ok::<_, anyhow::Error>(img.thumbnail(256, 256))
                    })
                    .await;
                    match decoded {
                        Ok(Ok(img)) => {
                            #[cfg(all(feature = "palette", feature = "album-art"))]
                            {
                                let swatches = crate::utils::palette::extract_palette(&img, 5);
                                tracing::debug!(
                                    "reactive: swatches={} theme.reactive_theme={}",
                                    swatches.len(),
                                    self.theme.reactive_theme
                                );
                                self.reactive_swatches = Some(swatches.clone());
                                if self.theme.reactive_theme {
                                    self.start_reactive_theme(&swatches);
                                    tracing::debug!("reactive: start_reactive_theme called");
                                }
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

            #[cfg(feature = "album-art")]
            self.maybe_fetch_album_art().await;

            let active = self.state.playback.is_playing
                || self.state.loading
                || self.state.search_active
                || self.state.quick_search_active
                || self.state.command_mode
                || self.state.add_to_playlist_mode
                || self.state.delete_playlist_confirm
                || self.settings_panel.as_ref().map_or(false, |p| p.visible)
                || self.pending_fetch.is_some()
                || self.pending_pagination.is_some()
                || self.local_scan_rx.is_some()
                || self.album_art_pending.is_some()
                || self.lyrics.as_ref().map_or(false, |l| l.is_loading())
                || self.state.status_msg.is_some();

            if self.needs_redraw || active {
                terminal.draw(|f| {
                    self.ui.render(f, &mut self.state);
                    if let Some(ref panel) = self.settings_panel {
                        panel.render(f, &self.state, &self.theme, self.autoplay_enabled);
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

            if self.state.playback.is_playing {
                // Interpolate progress locally using progress_at_play_start
                // + elapsed time. For Spotify, progress_at_play_start is
                // synced from current_playback_state() every tick (line ~954),
                // so this produces a smooth progress bar without relying on
                // the player's (potentially stale) reported position.
                if self.playing_started_at.is_none() {
                    self.playing_started_at = Some(Instant::now());
                    self.progress_at_play_start = self.state.playback.progress_ms;
                }
                let elapsed = self
                    .playing_started_at
                    .map(|t| t.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                self.state.playback.progress_ms = self.progress_at_play_start + elapsed;
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

                    if duration >= 30_000 && (progress >= duration / 2 || progress >= 240_000) {
                        if let Some(lfm) = self.lastfm.clone() {
                            let artist = self.state.playback.artist.clone();
                            let track = self.state.playback.title.clone();
                            let album = self.state.playback.album.clone();
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
                }
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
        use crate::utils::lastfm::{LastfmClient, get_api_key, get_api_secret};

        let mut cfg = crate::config::AppConfig::load().unwrap_or_default();

        if cfg.lastfm.session_key.is_some() {
            cfg.lastfm.session_key = None;
            let _ = cfg.save();
            self.lastfm = None;
            self.pending_lastfm_token = None;
            self.state.lastfm_connected = false;
            self.state.lastfm_pending = false;
            self.state.status_msg = Some("Last.fm scrobbling disconnected".to_string());
            return;
        }

        if let Some(token) = self.pending_lastfm_token.clone() {
            self.state.status_msg = Some("Exchanging session key with Last.fm...".to_string());
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
                    self.state.lastfm_connected = true;
                    self.state.lastfm_pending = false;
                    self.state.status_msg =
                        Some("Last.fm connected! Scrobbling enabled.".to_string());
                }
                Err(e) => {
                    self.state.status_msg = Some(format!(
                        "Last.fm auth not complete. Please authorize in browser, then press Enter again. ({e})"
                    ));
                }
            }
            return;
        }

        self.state.status_msg = Some("Requesting Last.fm auth token...".to_string());
        match LastfmClient::get_auth_token(&get_api_key()).await {
            Ok(token) => {
                let auth_url = format!(
                    "https://www.last.fm/api/auth/?api_key={}&token={}",
                    get_api_key(),
                    token
                );
                let _ = open::that(&auth_url);
                self.pending_lastfm_token = Some(token);
                self.state.lastfm_pending = true;
                self.state.status_msg = Some(
                    "Opened Last.fm in browser. Authorize, then press Enter on Last.fm in Options to finish.".to_string(),
                );
            }
            Err(e) => {
                self.state.status_msg = Some(format!("Failed to get Last.fm token: {e}"));
            }
        }
    }
}
