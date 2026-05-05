use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use symphonia::core::meta::Limit;
use tokio::sync::oneshot;
use tracing::warn;

pub fn read_audio_metadata(path: &Path) -> (String, String, String, u64) {
    use symphonia::core::{
        formats::FormatOptions,
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTagKey},
        probe::Hint,
    };

    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (fallback_name, String::new(), String::new(), 0),
    };

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = match symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(_) => return (fallback_name, String::new(), String::new(), 0),
    };

    let mut format = probed.format;

    let duration_ms = format
        .default_track()
        .and_then(|t| {
            let tb = t.codec_params.time_base?;
            let n_frames = t.codec_params.n_frames?;
            let secs = tb.calc_time(n_frames).seconds;
            Some(secs * 1000)
        })
        .unwrap_or(0);

    let mut title = fallback_name.clone();
    let mut artist = String::new();
    let mut album = String::new();

    let meta_ref = format.metadata();
    if let Some(rev) = meta_ref.current() {
        for tag in rev.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => title = tag.value.to_string(),
                Some(StandardTagKey::Artist) => {
                    if artist.is_empty() {
                        artist = tag.value.to_string();
                    }
                }
                Some(StandardTagKey::AlbumArtist) => {
                    if artist.is_empty() {
                        artist = tag.value.to_string();
                    }
                }
                Some(StandardTagKey::Album) => album = tag.value.to_string(),
                _ => {}
            }
        }
    }

    (title, artist, album, duration_ms)
}

pub fn extract_embedded_art(path: &std::path::Path) -> Option<Vec<u8>> {
    use symphonia::core::{
        formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
    };

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions {
                limit_metadata_bytes: Limit::Maximum(std::usize::MAX),
                limit_visual_bytes: Limit::Maximum(std::usize::MAX),
            },
        )
        .ok()?;

    if let Some(rev) = probed.format.metadata().current() {
        if let Some(v) = rev.visuals().first() {
            return Some(v.data.to_vec());
        }
    }

    if let Some(meta) = probed.metadata.get() {
        if let Some(rev) = meta.current() {
            if let Some(v) = rev.visuals().first() {
                return Some(v.data.to_vec());
            }
        }
    }

    None
}

use crate::discord::DiscordRpc;
use crate::lastfm::LastfmClient;
#[cfg(feature = "mpris")]
use crate::mpris::{MprisCmd, MprisHandle, MprisState};
use crate::player::NativePlayer;
use crate::player::{AudioPlayer, LocalPlayer, PlayerNotification, RepeatMode};
use crate::spotify::SpotifyClient;
use crate::theme::Theme;
use crate::ui::{ActiveContent, AlbumArtData, Focus, SearchPanel, SearchResults, Ui, UiState};
use rspotify::model::RepeatState;

pub struct App {
    spotify: SpotifyClient,
    player: Option<Box<dyn AudioPlayer>>,
    parked_player: Option<Box<dyn AudioPlayer>>,
    local_active: bool,
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
    album_art_pending: Option<tokio::sync::oneshot::Receiver<(Option<String>, Option<Vec<u8>>)>>,
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
    theme_rx: std::sync::mpsc::Receiver<Theme>,
    consecutive_unavailable: u32,
    spotify_streaming_disabled: bool,
    local_scan_rx: Option<tokio::sync::oneshot::Receiver<Vec<crate::ui::LocalNode>>>,
    local_scan_total: usize,
}

impl App {
    pub async fn new(
        picker: Picker,
        theme: Theme,
        theme_rx: std::sync::mpsc::Receiver<Theme>,
    ) -> Result<Self> {
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

        let mut spotify = match SpotifyClient::new().await {
            Ok(s) => s,
            Err(e) => {
                warn!("Spotify unavailable ({e:#}), starting in local-only mode");
                SpotifyClient::new_unauthenticated()
            }
        };

        let volume = crate::config::load_volume();

        let mut startup_warning: Option<String> = None;

        let spotify_player: Option<Box<dyn AudioPlayer>> = if spotify.authenticated {
            match spotify.get_access_token().await {
                Some(token) => match NativePlayer::new(token, false).await {
                    Ok(p) => {
                        tracing::info!("Native player started");
                        Some(Box::new(p) as Box<dyn AudioPlayer>)
                    }
                    Err(e) => {
                        let msg = e.to_string().to_lowercase();
                        if msg.contains("free") || msg.contains("premium") {
                            warn!("Spotify free account detected — streaming disabled");
                            startup_warning = Some(
                                "⚠ Spotify Premium required for streaming. Starting in local-only mode.".to_string(),
                            );
                        } else {
                            warn!("Native player unavailable: {e:#}");
                        }
                        None
                    }
                },
                None => {
                    warn!("Token not available for native player");
                    None
                }
            }
        } else {
            None
        };

        let db_path = crate::config::get_local_db_path();

        let local_player: Option<Box<dyn AudioPlayer>> = match LocalPlayer::new(volume, &db_path) {
            Ok(p) => Some(Box::new(p) as Box<dyn AudioPlayer>),
            Err(e) => {
                warn!("Local player: {e:#}");
                None
            }
        };

        let (player, parked_player, local_active) = match (spotify_player, local_player) {
            (Some(sp), local) => (Some(sp), local, false),
            (None, Some(lp)) => (Some(lp), None, true),
            (None, None) => (None, None, false),
        };

        let band_energies = player.as_ref().and_then(|p| p.band_energies());

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
                Err(e) => warn!("Failed to load playlists: {e}"),
            }
        }

        let initial_playback = spotify.fetch_playback().await.unwrap_or_default();
        let initial_art = initial_playback.art_url.clone();
        state.playback = initial_playback;

        #[cfg(feature = "mpris")]
        let mpris = match crate::mpris::spawn().await {
            Ok(h) => {
                tracing::info!("MPRIS D-Bus server started");
                Some(h)
            }
            Err(e) => {
                tracing::warn!("MPRIS unavailable: {e}");
                None
            }
        };

        let discord = if cfg.discord.enabled == Some(true) {
            let app_id = cfg
                .discord
                .app_id
                .as_deref()
                .unwrap_or(crate::discord::DEFAULT_APP_ID);
            DiscordRpc::spawn(app_id)
        } else {
            None
        };

        Ok(Self {
            spotify,
            player,
            parked_player,
            local_active,
            lastfm,
            ui: Ui::new(theme.clone()),
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
            band_energies,
            art_url: initial_art,
            session_reconnecting: false,
            radio_mode: false,
            recent_track_uris: std::collections::VecDeque::new(),
            playing_tracks: Vec::new(),
            theme,
            theme_rx,
            consecutive_unavailable: 0,
            spotify_streaming_disabled: false,
            local_scan_rx: None,
            local_scan_total: 0,
        })
    }

    pub async fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let tick_rate = Duration::from_millis(16);
        self.last_tick = Instant::now();

        loop {
            if let Ok(new_theme) = self.theme_rx.try_recv() {
                self.theme = new_theme.clone();
                self.ui = Ui::new(new_theme);
            }

            let now = Instant::now();
            let delta_ms = now.duration_since(self.last_tick).as_millis() as u64;
            self.last_tick = now;

            self.poll_local_scan();

            let mut needs_sync = false;
            let mut needs_reconnect = false;
            let mut needs_crossover = false;
            let mut needs_radio_refill = false;

            let parked_has_queue = self
                .parked_player
                .as_ref()
                .map(|p| !p.user_queue().is_empty())
                .unwrap_or(false);

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
                            self.spotify_streaming_disabled = true;
                            self.consecutive_unavailable = 0;
                            self.state.status_msg = Some(
                                "⚠ Spotify Premium required for streaming. Switched to local-only mode.".to_string(),
                            );
                            self.player = None;
                            self.band_energies = None;
                            if self.parked_player.is_some() {
                                std::mem::swap(&mut self.player, &mut self.parked_player);
                                self.local_active = true;
                                self.band_energies =
                                    self.player.as_ref().and_then(|p| p.band_energies());
                            }
                            needs_sync = false;
                            break;
                        }
                    }
                }
            }

            if let Some(player) = &mut self.player {
                self.state.playback.is_playing = player.is_playing();
                self.state.playback.volume = player.volume();
                self.state.playback.shuffle = player.shuffle();
                self.state.playback.repeat = match player.repeat() {
                    RepeatMode::Off => RepeatState::Off,
                    RepeatMode::Queue => RepeatState::Context,
                    RepeatMode::Track => RepeatState::Track,
                };
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
                        self.state.playback = current_pb.clone();
                        self.art_url = current_pb.art_url;
                    }
                }
            }

            if needs_reconnect && !self.session_reconnecting {
                self.session_reconnecting = true;
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
                            if let Some(p) = &mut self.player {
                                p.play();
                            }
                        }
                        MprisCmd::Pause => {
                            if let Some(p) = &mut self.player {
                                p.pause();
                            }
                        }
                        MprisCmd::Next => {
                            if let Some(p) = &mut self.player {
                                p.next();
                            }
                            self.sync_track_selection();
                            self.sync_queue_display();
                        }
                        MprisCmd::Prev => {
                            if let Some(p) = &mut self.player {
                                p.prev();
                            }
                            self.sync_track_selection();
                        }
                        MprisCmd::Seek(us) => {
                            let ms = (us / 1000) as u64;
                            self.state.playback.progress_ms = ms;
                            if let Some(p) = &self.player {
                                p.seek(ms as u32);
                            }
                        }
                        MprisCmd::SetVolume(v) => {
                            if let Some(p) = &mut self.player {
                                p.set_volume((v * 100.0).round() as u8);
                                self.state.playback.volume = p.volume();
                            }
                        }
                    }
                }
            }

            if let Some(rx) = &mut self.album_art_pending {
                if let Ok((art_url, bytes)) = rx.try_recv() {
                    self.album_art_pending = None;
                    if let Some(url) = art_url {
                        self.state.playback.art_url = Some(url);
                    }
                    if let Some(bytes) = bytes {
                        let image_state = image::load_from_memory(&bytes)
                            .ok()
                            .map(|img| self.picker.new_resize_protocol(img));
                        self.state.album_art = Some(AlbumArtData { image_state });
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
                    let art_ready = pb.art_url.is_some();
                    // For local files art_url is never set; use a shorter timeout
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

            terminal.draw(|f| self.ui.render(f, &mut self.state))?;

            let timeout = tick_rate
                .checked_sub(now.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if crossterm::event::poll(timeout)? {
                if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
                    self.handle_key(key_event.code, key_event.modifiers).await?;
                }
            }

            if self.state.playback.is_playing {
                let new_progress = self.state.playback.progress_ms + delta_ms;
                if new_progress < self.state.playback.duration_ms {
                    self.state.playback.progress_ms = new_progress;
                } else if self.player.is_none() {
                    self.state.playback.is_playing = false;
                    self.state.playback.progress_ms = self.state.playback.duration_ms;
                }

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

    async fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        self.state.status_msg = None;

        if self.state.search_active {
            match code {
                KeyCode::Esc => self.state.cancel_search(),
                KeyCode::Enter => {
                    let query = self.state.search_query.trim().to_string();
                    if query.is_empty() {
                        self.state.cancel_search();
                    } else if !self.spotify.authenticated {
                        self.state.status_msg = Some("Search requires Spotify".to_string());
                        self.state.search_active = false;
                    } else {
                        self.state.status_msg = Some(format!("Searching \"{query}\"..."));
                        match self.spotify.search_all(&query).await {
                            Ok(results) => {
                                let total = results.tracks.len()
                                    + results.artists.len()
                                    + results.albums.len()
                                    + results.playlists.len();
                                self.state.search_results =
                                    Some(SearchResults::new(query.clone(), results));
                                self.state.tracks.clear();
                                self.state.active_playlist_uri = None;
                                self.state.search_active = false;
                                self.state.focus = Focus::Search;
                                self.state.status_msg = if total == 0 {
                                    Some(format!("No results for \"{query}\""))
                                } else {
                                    Some(format!("{total} results for \"{query}\""))
                                };
                            }
                            Err(e) => {
                                self.state.status_msg = Some(format!("Search error: {e:#}"));
                                self.state.search_active = false;
                                tracing::error!("Search failed for \"{query}\": {e:#}");
                            }
                        }
                    }
                }
                KeyCode::Up => self.state.nav_up(),
                KeyCode::Down => self.state.nav_down(),
                KeyCode::Backspace => self.state.search_pop(),
                KeyCode::Tab => self.state.switch_focus(),
                KeyCode::Char(c) => self.state.search_push(c),
                _ => {}
            }
            return Ok(());
        }

        match code {
            KeyCode::Left | KeyCode::Right => {
                let is_held = self
                    .last_seek_time
                    .map(|t| t.elapsed() < Duration::from_millis(300))
                    .unwrap_or(false);
                if is_held {
                    self.seek_hold_count += 1;
                } else {
                    self.seek_hold_count = 0;
                }
                self.last_seek_time = Some(Instant::now());
                let step_ms = if self.seek_hold_count > 4 {
                    10_000u64
                } else {
                    5_000u64
                };

                let new_pos = match code {
                    KeyCode::Right => (self.state.playback.progress_ms + step_ms)
                        .min(self.state.playback.duration_ms),
                    _ => self.state.playback.progress_ms.saturating_sub(step_ms),
                };
                self.state.playback.progress_ms = new_pos;
                if let Some(player) = &self.player {
                    player.seek(new_pos as u32);
                }
                return Ok(());
            }
            _ => {}
        }

        match (code, modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            (KeyCode::Up, KeyModifiers::CONTROL) => self.state.nav_first(),
            (KeyCode::Down, KeyModifiers::CONTROL) => {
                self.state.nav_last();
                self.maybe_load_more().await;
            }

            (KeyCode::Char('v'), _) => {
                self.state.show_visualizer = !self.state.show_visualizer;
            }

            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => self.state.nav_up(),
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.state.nav_down();
                self.maybe_load_more().await;
            }

            (KeyCode::Tab, _) => {
                if self.state.focus == Focus::Search {
                    self.state.switch_search_panel();
                } else {
                    self.state.switch_focus();
                }
            }
            (KeyCode::BackTab, _) => {
                self.state.switch_focus_prev();
            }

            (KeyCode::Enter, _) => self.handle_enter().await,

            (KeyCode::Char('/'), _) => self.state.start_search(),

            (KeyCode::Backspace, _) => {
                if let Some(prev) = self.state.previous_search.take() {
                    self.state.search_results = Some(prev);
                    self.state.active_content = ActiveContent::None;
                    self.state.focus = Focus::Search;
                }
            }

            (KeyCode::Esc, _) => {
                if self.state.fullscreen_player {
                    self.state.fullscreen_player = false;
                } else if self.state.search_results.is_some() {
                    self.state.search_results = None;
                    self.state.previous_search = None;
                    self.state.active_content = ActiveContent::None;
                    self.state.focus = Focus::Library;
                }
            }

            (KeyCode::Char(' '), _) => {
                if let Some(player) = &mut self.player {
                    player.toggle();
                } else if self.spotify.authenticated {
                    let _ = self.spotify.toggle_playback().await;
                }
            }
            (KeyCode::Char('a'), _) => {
                let track = if self.state.active_content == ActiveContent::LocalFiles {
                    self.state
                        .local_tree_list
                        .selected()
                        .and_then(|vi| self.state.local_tree.get_visible(vi))
                        .and_then(|n| n.track().cloned())
                        .map(|t| (t.uri, t.name, t.artist, t.duration_ms))
                } else {
                    self.state
                        .track_list
                        .selected()
                        .and_then(|i| self.state.tracks.get(i))
                        .map(|t| {
                            (
                                t.uri.clone(),
                                t.name.clone(),
                                t.artist.clone(),
                                t.duration_ms,
                            )
                        })
                };
                if let Some((uri, name, artist, duration_ms)) = track {
                    let is_local = uri.starts_with("file://");
                    let target = if is_local == self.local_active {
                        self.player.as_mut()
                    } else {
                        self.parked_player.as_mut()
                    };
                    if let Some(player) = target {
                        player.add_to_queue(uri, name.clone(), artist, duration_ms);
                        self.state.status_msg = Some(format!("+ {name} added to queue"));
                        self.sync_queue_display();
                    }
                }
            }

            (KeyCode::Delete, _) if self.state.focus == Focus::Queue => {
                if let Some(idx) = self.state.queue_list.selected() {
                    let active_len = self
                        .player
                        .as_ref()
                        .map(|p| p.user_queue().len())
                        .unwrap_or(0);

                    if idx < active_len {
                        if let Some(player) = &mut self.player {
                            player.remove_from_user_queue(idx);
                        }
                    } else {
                        let parked_idx = idx - active_len;
                        if let Some(player) = &mut self.parked_player {
                            if parked_idx < player.user_queue().len() {
                                player.remove_from_user_queue(parked_idx);
                            }
                        }
                    }

                    self.sync_queue_display();
                    let new_sel = if self.state.queue_items.is_empty() {
                        None
                    } else {
                        Some(idx.min(self.state.queue_items.len() - 1))
                    };
                    self.state.queue_list.select(new_sel);
                }
            }

            (KeyCode::Char('n'), _) => {
                if let Some(player) = &mut self.player {
                    player.next();
                    self.sync_track_selection();
                    self.sync_queue_display();
                } else if self.spotify.authenticated {
                    let _ = self.spotify.next_track().await;
                }
            }
            (KeyCode::Char('p'), _) => {
                if let Some(player) = &mut self.player {
                    player.prev();
                    self.sync_track_selection();
                } else if self.spotify.authenticated {
                    let _ = self.spotify.prev_track().await;
                }
            }
            (KeyCode::Char('s'), _) => {
                if let Some(player) = &mut self.player {
                    player.toggle_shuffle();
                    self.state.playback.shuffle = player.shuffle();
                }
            }
            (KeyCode::Char('r'), KeyModifiers::ALT) => {
                self.get_similar_tracks().await;
            }
            (KeyCode::Char('R'), _) => {
                self.radio_mode = !self.radio_mode;
                self.state.playback.radio_mode = self.radio_mode;
                let msg = if self.radio_mode {
                    "󰐇 Radio Mode on"
                } else {
                    "Radio Mode off"
                };
                self.state.status_msg = Some(msg.to_string());
            }
            (KeyCode::Char('r'), _) => {
                if let Some(player) = &mut self.player {
                    player.cycle_repeat();
                    self.state.playback.repeat = match player.repeat() {
                        RepeatMode::Off => RepeatState::Off,
                        RepeatMode::Queue => RepeatState::Context,
                        RepeatMode::Track => RepeatState::Track,
                    };
                }
            }
            (KeyCode::Char('z'), _) => {
                if !self.state.playback.title.is_empty() {
                    self.state.fullscreen_player = !self.state.fullscreen_player;
                }
            }
            (KeyCode::Char('c'), _)
                if modifiers != KeyModifiers::CONTROL
                    && !self.state.fullscreen_player
                    && !(self.state.search_results.is_none()
                        && self.state.active_content == ActiveContent::None
                        && !self.state.playback.title.is_empty()) =>
            {
                self.state.show_album_art = !self.state.show_album_art;
                if self.state.show_album_art {
                    self.last_art_uri.clear();
                } else {
                    self.state.album_art = None;
                    self.album_art_pending = None;
                }
            }
            (KeyCode::Char('l'), _) => {
                if !self.spotify.authenticated {
                    self.state.status_msg = Some("Spotify not connected".to_string());
                } else {
                    match self.spotify.save_current_track().await {
                        Ok(_) => self.state.status_msg = Some("♥ Liked!".to_string()),
                        Err(e) => self.state.status_msg = Some(format!("Error liking track: {e}")),
                    }
                }
            }
            (KeyCode::Char('+'), _) | (KeyCode::Char('='), _) => {
                if let Some(player) = &mut self.player {
                    player.volume_up();
                    self.state.playback.volume = player.volume();
                }
            }
            (KeyCode::Char('-'), _) => {
                if let Some(player) = &mut self.player {
                    player.volume_down();
                    self.state.playback.volume = player.volume();
                }
            }

            _ => {}
        }
        Ok(())
    }

    async fn handle_enter(&mut self) {
        match self.state.focus {
            Focus::Library => {
                let idx = match self.state.library_list.selected() {
                    Some(i) => i,
                    None => return,
                };
                if idx != 4 && !self.spotify.authenticated {
                    self.state.status_msg =
                        Some("Spotify not connected — only Local Files available".to_string());
                    return;
                }
                match idx {
                    0 => {
                        self.state.status_msg = Some("Loading Liked Songs…".to_string());
                        match self.spotify.fetch_liked_tracks(0).await {
                            Ok((tracks, total)) => {
                                self.state.tracks = tracks;
                                self.state.tracks_total = total;
                                self.state.tracks_offset = self.state.tracks.len() as u32;
                                self.state.active_playlist_uri = Some("liked_songs".to_string());
                                self.state.active_playlist_id = Some("liked_songs".to_string());
                                self.state
                                    .track_list
                                    .select(if self.state.tracks.is_empty() {
                                        None
                                    } else {
                                        Some(0)
                                    });
                                self.state.active_content = ActiveContent::Tracks;
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    1 => {
                        self.state.status_msg = Some("Loading saved albums…".to_string());
                        match self.spotify.fetch_saved_albums(0).await {
                            Ok((albums, total)) => {
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
                                self.state.active_content = ActiveContent::Albums;
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    2 => {
                        self.state.status_msg = Some("Loading followed artists…".to_string());
                        match self.spotify.fetch_followed_artists().await {
                            Ok(artists) => {
                                self.state.artists = artists;
                                self.state
                                    .artist_list
                                    .select(if self.state.artists.is_empty() {
                                        None
                                    } else {
                                        Some(0)
                                    });
                                self.state.active_content = ActiveContent::Artists;
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    3 => {
                        self.state.status_msg = Some("Podcasts — coming soon".to_string());
                    }
                    4 => {
                        self.load_local_files().await;
                    }
                    _ => {}
                }
            }

            Focus::Playlists => {
                if let Some(playlist) = self.state.selected_playlist() {
                    let id = playlist.id.clone();
                    let uri = playlist.uri.clone();
                    let name = playlist.name.clone();
                    self.state.status_msg = Some(format!("Loading {name}…"));
                    match self.spotify.fetch_playlist_tracks(&id, 0).await {
                        Ok((tracks, total)) => {
                            self.state.tracks = tracks;
                            self.state.tracks_total = total;
                            self.state.tracks_offset = self.state.tracks.len() as u32;
                            self.state.active_playlist_uri = Some(uri.clone());
                            self.state.active_playlist_id = Some(id.clone());
                            self.state
                                .track_list
                                .select(if self.state.tracks.is_empty() {
                                    None
                                } else {
                                    Some(0)
                                });
                            self.state.active_content = ActiveContent::Tracks;
                            self.state.search_results = None;
                            self.state.status_msg = None;
                            self.state.focus = Focus::Tracks;
                        }
                        Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                    }
                }
            }

            Focus::Tracks => match &self.state.active_content {
                ActiveContent::Albums => {
                    if let Some(idx) = self.state.selected_album_index() {
                        if let Some(album) = self.state.albums.get(idx) {
                            let id = album.id.clone();
                            let name = album.name.clone();
                            self.state.status_msg = Some(format!("Loading {name}…"));
                            match self.spotify.fetch_album_tracks(&id, 0).await {
                                Ok((tracks, total)) => {
                                    self.state.tracks = tracks;
                                    self.state.tracks_total = total;
                                    self.state.tracks_offset = self.state.tracks.len() as u32;
                                    self.state.active_playlist_uri = Some(format!("album:{id}"));
                                    self.state.active_playlist_id = Some(format!("album:{id}"));
                                    self.state
                                        .track_list
                                        .select(if self.state.tracks.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.status_msg = None;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                }
                ActiveContent::Artists => {
                    if let Some(idx) = self.state.selected_artist_index() {
                        if let Some(artist) = self.state.artists.get(idx) {
                            let id = artist.uri.trim_start_matches("spotify:artist:").to_string();
                            let name = artist.name.clone();
                            self.state.status_msg = Some(format!("Loading top tracks for {name}…"));
                            match self.spotify.fetch_artist_tracks(&name, 0).await {
                                Ok((tracks, total)) => {
                                    self.state.tracks = tracks;
                                    self.state.tracks_total = total;
                                    self.state.tracks_offset = self.state.tracks.len() as u32;
                                    self.state.active_artist_name = Some(name.clone());
                                    self.state.active_playlist_uri = Some(format!("artist:{id}"));
                                    self.state.active_playlist_id = Some(format!("artist:{id}"));
                                    self.state
                                        .track_list
                                        .select(if self.state.tracks.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.status_msg = None;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                }
                ActiveContent::Shows => {
                    if let Some(idx) = self.state.selected_show_index() {
                        if let Some(show) = self.state.shows.get(idx) {
                            let id = show.id.clone();
                            let name = show.name.clone();
                            self.state.status_msg = Some(format!("Loading {name}…"));
                            match self.spotify.fetch_show_episodes(&id, 0).await {
                                Ok((tracks, total)) => {
                                    self.state.tracks = tracks;
                                    self.state.tracks_total = total;
                                    self.state.tracks_offset = self.state.tracks.len() as u32;
                                    self.state.active_playlist_uri = Some(format!("show:{id}"));
                                    self.state.active_playlist_id = Some(format!("show:{id}"));
                                    self.state
                                        .track_list
                                        .select(if self.state.tracks.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.status_msg = None;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                }
                ActiveContent::LocalFiles => {
                    let vi = match self.state.local_tree_list.selected() {
                        Some(i) => i,
                        None => return,
                    };
                    let node = match self.state.local_tree.get_visible(vi) {
                        Some(n) => n.clone(),
                        None => return,
                    };
                    match node {
                        crate::ui::LocalNode::Folder { .. } => {
                            self.state.local_tree.toggle_folder(vi);
                            let new_len = self.state.local_tree.visible_len();
                            let cur = self.state.local_tree_list.selected().unwrap_or(0);
                            self.state
                                .local_tree_list
                                .select(Some(cur.min(new_len.saturating_sub(1))));
                        }
                        crate::ui::LocalNode::Track { track, .. } => {
                            self.activate_local_player();
                            let all_tracks = self.state.local_tree.all_tracks_flat();
                            let start_idx = all_tracks
                                .iter()
                                .position(|t| t.uri == track.uri)
                                .unwrap_or(0);
                            if let Some(player) = &mut self.player {
                                player.set_queue_tracks(all_tracks.clone(), start_idx);
                                self.playing_tracks = all_tracks;
                                self.state.playback.title = track.name.clone();
                                self.state.playback.artist = track.artist.clone();
                                self.state.playback.album = track.album.clone();
                                self.state.playback.duration_ms = track.duration_ms;
                                self.state.playback.progress_ms = 0;
                                self.state.playback.is_playing = true;
                                self.state.playback.is_local = true;
                                self.current_track_uri = track.uri.clone();
                                self.on_track_started();
                            }
                        }
                    }
                }
                ActiveContent::Tracks | ActiveContent::None => {
                    if let Some(idx) = self.state.selected_track_index() {
                        if self.spotify_streaming_disabled {
                            self.state.status_msg =
                                Some("⚠ Spotify Premium required for streaming".to_string());
                            return;
                        }
                        self.activate_spotify_player();
                        if self
                            .state
                            .tracks
                            .get(idx)
                            .map(|t| t.uri.starts_with("spotify:episode:"))
                            .unwrap_or(false)
                        {
                            self.state.status_msg =
                                Some("Podcast playback not supported".to_string());
                        } else if let Some(player) = &mut self.player {
                            let uris: Vec<String> = self
                                .state
                                .tracks
                                .iter()
                                .filter(|t| !t.uri.starts_with("spotify:episode:"))
                                .map(|t| t.uri.clone())
                                .collect();
                            let adjusted_idx = self.state.tracks[..idx]
                                .iter()
                                .filter(|t| !t.uri.starts_with("spotify:episode:"))
                                .count();
                            player.set_queue(uris, adjusted_idx);
                            self.playing_tracks = self.state.tracks.clone();
                            if let Some(track) = self.state.tracks.get(idx) {
                                self.state.playback.title = track.name.clone();
                                self.state.playback.artist = track.artist.clone();
                                self.state.playback.album = track.album.clone();
                                self.state.playback.duration_ms = track.duration_ms;
                                self.state.playback.progress_ms = 0;
                                self.state.playback.is_playing = true;
                                self.state.playback.is_local = false;
                                self.current_track_uri = track.uri.clone();
                                self.on_track_started();
                            }
                        } else if self.spotify.authenticated {
                            let track_uri = self.state.tracks[idx].uri.clone();
                            let is_playlist = self
                                .state
                                .active_playlist_uri
                                .as_deref()
                                .map(|u| u != "liked_songs" && !u.starts_with("search:"))
                                .unwrap_or(false);
                            let result = if is_playlist {
                                let uri = self.state.active_playlist_uri.clone().unwrap();
                                self.spotify.play_in_context(&uri, &track_uri).await
                            } else {
                                self.spotify.play_track_uri(&track_uri).await
                            };
                            if let Err(e) = result {
                                self.state.status_msg = Some(format!("Error: {e}"));
                            }
                        }
                    }
                }
            },

            Focus::Search => {
                let panel = self.state.search_results.as_ref().map(|sr| sr.panel);
                match panel {
                    Some(SearchPanel::Tracks) => {
                        let uri = self
                            .state
                            .search_results
                            .as_ref()
                            .and_then(|sr| sr.selected_track_uri())
                            .map(|s| s.to_string());
                        if let Some(track_uri) = uri {
                            if self.spotify_streaming_disabled {
                                self.state.status_msg =
                                    Some("⚠ Spotify Premium required for streaming".to_string());
                                return;
                            }
                            self.activate_spotify_player();
                            if let Some(player) = &mut self.player {
                                self.current_track_uri = track_uri.clone();
                                player.set_queue(vec![track_uri], 0);
                                if let Some(sr) = &self.state.search_results {
                                    if let Some(idx) = sr.track_list.selected() {
                                        if let Some(t) = sr.tracks.get(idx) {
                                            self.state.playback.title = t.name.clone();
                                            self.state.playback.artist = t.artist.clone();
                                            self.state.playback.album = t.album.clone();
                                            self.state.playback.duration_ms = t.duration_ms;
                                            self.state.playback.progress_ms = 0;
                                            self.state.playback.is_playing = true;
                                            self.state.playback.is_local = false;
                                            self.playing_tracks =
                                                vec![crate::spotify::TrackSummary {
                                                    uri: t.uri.clone(),
                                                    name: t.name.clone(),
                                                    artist: t.artist.clone(),
                                                    album: t.album.clone(),
                                                    duration_ms: t.duration_ms,
                                                }];
                                            self.on_track_started();
                                        }
                                    }
                                }
                            } else if self.spotify.authenticated {
                                let _ = self.spotify.play_track_uri(&track_uri).await;
                            }
                        }
                    }
                    Some(SearchPanel::Albums) => {
                        let album = self
                            .state
                            .search_results
                            .as_ref()
                            .and_then(|sr| sr.selected_album())
                            .map(|a| (a.id.clone(), a.name.clone(), a.uri.clone()));
                        if let Some((id, name, uri)) = album {
                            self.state.status_msg = Some(format!("Loading {name}…"));
                            match self.spotify.fetch_album_tracks(&id, 0).await {
                                Ok((tracks, total)) => {
                                    self.state.tracks = tracks;
                                    self.state.tracks_total = total;
                                    self.state.tracks_offset = self.state.tracks.len() as u32;
                                    self.state.active_playlist_uri = Some(format!("album:{uri}"));
                                    self.state.active_playlist_id = Some(format!("album:{id}"));
                                    self.state
                                        .track_list
                                        .select(if self.state.tracks.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.previous_search = self.state.search_results.take();
                                    self.state.status_msg = None;
                                    self.state.focus = Focus::Tracks;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                    Some(SearchPanel::Playlists) => {
                        let playlist = self
                            .state
                            .search_results
                            .as_ref()
                            .and_then(|sr| sr.selected_playlist())
                            .map(|p| (p.id.clone(), p.name.clone(), p.uri.clone()));
                        if let Some((id, name, uri)) = playlist {
                            self.state.status_msg = Some(format!("Loading {name}…"));
                            match self.spotify.fetch_playlist_tracks(&id, 0).await {
                                Ok((tracks, total)) => {
                                    self.state.tracks = tracks;
                                    self.state.tracks_total = total;
                                    self.state.tracks_offset = self.state.tracks.len() as u32;
                                    self.state.active_playlist_uri = Some(uri);
                                    self.state.active_playlist_id = Some(id);
                                    self.state
                                        .track_list
                                        .select(if self.state.tracks.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.previous_search = self.state.search_results.take();
                                    self.state.status_msg = None;
                                    self.state.focus = Focus::Tracks;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                    Some(SearchPanel::Artists) => {
                        let artist = self
                            .state
                            .search_results
                            .as_ref()
                            .and_then(|sr| sr.selected_artist())
                            .map(|a| (a.id.clone(), a.name.clone()));
                        if let Some((id, name)) = artist {
                            self.state.status_msg = Some(format!("Loading top tracks for {name}…"));
                            match self.spotify.fetch_artist_tracks(&name, 0).await {
                                Ok((tracks, total)) => {
                                    self.state.tracks = tracks;
                                    self.state.tracks_total = total;
                                    self.state.tracks_offset = self.state.tracks.len() as u32;
                                    self.state.active_artist_name = Some(name.clone());
                                    self.state.active_playlist_uri = Some(format!("artist:{id}"));
                                    self.state.active_playlist_id = Some(format!("artist:{id}"));
                                    self.state
                                        .track_list
                                        .select(if self.state.tracks.is_empty() {
                                            None
                                        } else {
                                            Some(0)
                                        });
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.previous_search = self.state.search_results.take();
                                    self.state.status_msg = None;
                                    self.state.focus = Focus::Tracks;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                    None => {}
                }
            }
            Focus::Queue => {}
        }
    }

    fn on_track_started(&mut self) {
        self.scrobble_sent = false;

        self.track_start_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.state.playback.progress_ms = 0;
        self.state.playback.radio_mode = self.radio_mode;

        if self.current_track_uri.starts_with("spotify:track:") {
            self.recent_track_uris
                .push_back(self.current_track_uri.clone());
            if self.recent_track_uris.len() > 5 {
                self.recent_track_uris.pop_front();
            }
        }

        self.state.album_art = None;
        self.state.playback.art_url = None;
        self.album_art_pending = None;
        self.last_art_uri.clear();

        if let Some(lfm) = self.lastfm.clone() {
            let artist = self.state.playback.artist.clone();
            let track = self.state.playback.title.clone();
            let duration = self.state.playback.duration_ms;

            if !artist.trim().is_empty() && !track.trim().is_empty() && duration > 30_000 {
                tokio::spawn(async move {
                    lfm.update_now_playing(&artist, &track, duration).await;
                });
            }
        }
    }

    async fn maybe_load_more(&mut self) {
        if self.state.focus == Focus::Search {
            let should_load = self
                .state
                .search_results
                .as_ref()
                .map(|sr| {
                    if sr.loading {
                        return None;
                    }
                    let (selected, len, total, stype) = match sr.panel {
                        SearchPanel::Tracks => (
                            sr.track_list.selected().unwrap_or(0),
                            sr.tracks.len(),
                            sr.tracks_total,
                            "track",
                        ),
                        SearchPanel::Artists => (
                            sr.artist_list.selected().unwrap_or(0),
                            sr.artists.len(),
                            sr.artists_total,
                            "artist",
                        ),
                        SearchPanel::Albums => (
                            sr.album_list.selected().unwrap_or(0),
                            sr.albums.len(),
                            sr.albums_total,
                            "album",
                        ),
                        SearchPanel::Playlists => (
                            sr.playlist_list.selected().unwrap_or(0),
                            sr.playlists.len(),
                            sr.playlists_total,
                            "playlist",
                        ),
                    };
                    if len == 0 || selected < len.saturating_sub(3) || len >= total as usize {
                        return None;
                    }
                    Some((sr.query.clone(), len as u32, stype))
                })
                .flatten();

            if let Some((query, offset, stype)) = should_load {
                self.state.search_results.as_mut().unwrap().loading = true;
                match self.spotify.search_more(&query, stype, offset).await {
                    Ok(more) => {
                        let sr = self.state.search_results.as_mut().unwrap();
                        match stype {
                            "track" => {
                                sr.tracks_total = more.tracks_total;
                                sr.tracks.extend(more.tracks);
                            }
                            "artist" => {
                                sr.artists_total = more.artists_total;
                                sr.artists.extend(more.artists);
                            }
                            "album" => {
                                sr.albums_total = more.albums_total;
                                sr.albums.extend(more.albums);
                            }
                            "playlist" => {
                                sr.playlists_total = more.playlists_total;
                                sr.playlists.extend(more.playlists);
                            }
                            _ => {}
                        }
                        sr.loading = false;
                    }
                    Err(e) => {
                        if let Some(sr) = self.state.search_results.as_mut() {
                            sr.loading = false;
                        }
                        self.state.status_msg = Some(format!("Load more error: {e}"));
                    }
                }
            }
            return;
        }

        if self.state.active_content == ActiveContent::Albums {
            let selected = self.state.album_list.selected().unwrap_or(0);
            let len = self.state.albums.len();
            if len > 0
                && selected >= len.saturating_sub(3)
                && len < self.state.albums_total as usize
            {
                let offset = self.state.albums_offset;
                match self.spotify.fetch_saved_albums(offset).await {
                    Ok((mut new_albums, total)) => {
                        self.state.albums_total = total;
                        self.state.albums_offset += new_albums.len() as u32;
                        self.state.albums.append(&mut new_albums);
                    }
                    Err(e) => self.state.status_msg = Some(format!("Load more error: {e}")),
                }
            }
            return;
        }

        if self.state.active_content == ActiveContent::Shows {
            let selected = self.state.show_list.selected().unwrap_or(0);
            let len = self.state.shows.len();
            if len > 0 && selected >= len.saturating_sub(3) && len < self.state.shows_total as usize
            {
                let offset = self.state.shows_offset;
                match self.spotify.fetch_saved_shows(offset).await {
                    Ok((mut new_shows, total)) => {
                        self.state.shows_total = total;
                        self.state.shows_offset += new_shows.len() as u32;
                        self.state.shows.append(&mut new_shows);
                    }
                    Err(e) => self.state.status_msg = Some(format!("Load more error: {e}")),
                }
            }
            return;
        }

        if self.state.tracks_loading {
            return;
        }
        let selected = self.state.track_list.selected().unwrap_or(0);
        let len = self.state.tracks.len();
        if len == 0 || selected < len.saturating_sub(3) {
            return;
        }
        if (self.state.tracks_offset as usize) >= len && len < self.state.tracks_total as usize {
            self.state.tracks_loading = true;
            let offset = self.state.tracks_offset;
            let id = self.state.active_playlist_id.clone();

            let result = match id.as_deref() {
                Some("liked_songs") => self.spotify.fetch_liked_tracks(offset).await,
                Some(id) if id.starts_with("album:") => {
                    let album_id = &id["album:".len()..];
                    self.spotify.fetch_album_tracks(album_id, offset).await
                }
                Some(id) if id.starts_with("artist:") => {
                    let name = self.state.active_artist_name.clone().unwrap_or_default();
                    self.spotify.fetch_artist_tracks(&name, offset).await
                }
                Some(id) => self.spotify.fetch_playlist_tracks(id, offset).await,
                None => return,
            };

            match result {
                Ok((mut new_tracks, total)) => {
                    self.state.tracks_total = total;
                    self.state.tracks_offset += new_tracks.len() as u32;
                    self.state.tracks.append(&mut new_tracks);
                }
                Err(e) => {
                    self.state.status_msg = Some(format!("Load more error: {e}"));
                }
            }
            self.state.tracks_loading = false;
        }
    }

    async fn maybe_fetch_album_art(&mut self) {
        let need_art_for_now_playing = !self.state.playback.title.is_empty();

        if (!self.state.show_album_art && !need_art_for_now_playing) && self.discord.is_none() {
            return;
        }

        if self.current_track_uri.is_empty()
            || self.current_track_uri == self.last_art_uri
            || self.album_art_pending.is_some()
        {
            return;
        }

        if self.current_track_uri.starts_with("file://") {
            self.fetch_local_album_art();
            return;
        }

        if !self.spotify.authenticated {
            return;
        }

        let uri = self.current_track_uri.clone();
        let Some(token) = self.spotify.get_access_token().await else {
            return;
        };
        let http = self.spotify.http_client();
        self.last_art_uri = uri.clone();

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.album_art_pending = Some(rx);

        tokio::spawn(async move {
            let Some(track_id) = uri.strip_prefix("spotify:track:").map(|s| s.to_string()) else {
                let _ = tx.send((None, None));
                return;
            };
            let Ok(resp) = http
                .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
                .bearer_auth(&token)
                .send()
                .await
            else {
                let _ = tx.send((None, None));
                return;
            };
            let Ok(json) = resp.json::<serde_json::Value>().await else {
                let _ = tx.send((None, None));
                return;
            };
            let Some(url) = json["album"]["images"]
                .as_array()
                .and_then(|imgs| imgs.first())
                .and_then(|img| img["url"].as_str())
                .map(|s| s.to_string())
            else {
                let _ = tx.send((None, None));
                return;
            };
            let bytes = match http.get(&url).send().await {
                Ok(resp) => resp.bytes().await.ok().map(|b| b.to_vec()),
                Err(_) => None,
            };
            let _ = tx.send((Some(url), bytes));
        });
    }

    fn fetch_local_album_art(&mut self) {
        if self.current_track_uri == self.last_art_uri || self.album_art_pending.is_some() {
            return;
        }
        self.last_art_uri = self.current_track_uri.clone();

        let path = self
            .current_track_uri
            .strip_prefix("file://")
            .map(std::path::PathBuf::from);

        let Some(path) = path else { return };

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.album_art_pending = Some(rx);

        tokio::spawn(async move {
            let bytes = extract_embedded_art(&path);
            let _ = tx.send((None, bytes));
        });
    }

    fn sync_track_selection(&mut self) {
        let queued = self.player.as_mut().and_then(|p| p.take_playing_queued());
        if let Some(qt) = queued {
            self.state.playback.title = qt.name;
            self.state.playback.artist = qt.artist;
            self.state.playback.album = String::new();
            self.state.playback.duration_ms = qt.duration_ms;
            self.state.playback.progress_ms = 0;
            self.state.playback.is_playing = true;
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

    fn activate_local_player(&mut self) {
        if self.local_active {
            return;
        }
        if let Some(ref mut p) = self.player {
            if p.is_playing() {
                p.pause();
            }
        }
        std::mem::swap(&mut self.player, &mut self.parked_player);
        self.local_active = true;
        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
    }

    fn activate_spotify_player(&mut self) {
        if !self.local_active {
            return;
        }
        if let Some(ref mut p) = self.player {
            if p.is_playing() {
                p.pause();
            }
        }
        std::mem::swap(&mut self.player, &mut self.parked_player);
        self.local_active = false;
        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
    }


    // This function... Jesus. So complicated to make this shit work. 
    // And
    // TODO: Fix the art cover, because it don't show yourself into player, just hollow and name of artist too, stay "unknown artist"(fallback)
    // SO, I NEED FIX THIS _-(°0-0°)-_
    async fn load_local_files(&mut self) {
        let cfg = crate::config::AppConfig::load().unwrap_or_default();
        let raw_dir = match cfg.local.music_dir {
            Some(d) => d,
            None => {
                self.state.status_msg =
                    Some("Set [local] music_dir in ~/.config/isi-music/config.toml".to_string());
                return;
            }
        };

        let dir = if raw_dir.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(&raw_dir[2..])
            } else {
                std::path::PathBuf::from(&raw_dir)
            }
        } else {
            std::path::PathBuf::from(&raw_dir)
        };

        if !dir.exists() {
            self.state.status_msg = Some(format!("Directory not found: {}", dir.display()));
            return;
        }

        self.state.status_msg = Some("Loading local files...".to_string());
        self.state.active_content = ActiveContent::LocalFiles;
        self.state.focus = Focus::Tracks;

        let (tx, rx) = oneshot::channel();
        self.local_scan_rx = Some(rx);

        tokio::task::spawn_blocking(move || {
            let extensions = ["mp3", "flac", "ogg", "wav", "aiff", "m4a", "opus"];
            let mut nodes: Vec<crate::ui::LocalNode> = Vec::new();

            let db_path = crate::config::get_local_db_path();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.execute_batch("PRAGMA journal_mode=WAL;");
                    Some(c)
                }
                Err(_) => None,
            };

            if let Some(ref c) = conn {
                let _ = c.execute(
                    "CREATE TABLE IF NOT EXISTS tracks (
                        id INTEGER PRIMARY KEY,
                        path TEXT NOT NULL UNIQUE,
                        title TEXT,
                        artist TEXT,
                        album TEXT,
                        duration_ms INTEGER,
                        cover_path TEXT
                    )",
                    [],
                );
            }

            fn scan_dir(
                dir: &std::path::Path,
                depth: usize,
                nodes: &mut Vec<crate::ui::LocalNode>,
                extensions: &[&str],
                conn: &Option<rusqlite::Connection>,
            ) {
                let mut subdirs: Vec<std::path::PathBuf> = Vec::new();
                let mut files: Vec<std::path::PathBuf> = Vec::new();

                if let Ok(entries) = std::fs::read_dir(dir) {
                    let mut entries_vec: Vec<_> = entries.flatten().map(|e| e.path()).collect();
                    entries_vec.sort();
                    for path in entries_vec {
                        if path.is_dir() {
                            subdirs.push(path);
                        } else if path.is_file() {
                            let ext_ok = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|e| extensions.contains(&e.to_lowercase().as_str()))
                                .unwrap_or(false);
                            if ext_ok {
                                files.push(path);
                            }
                        }
                    }
                }

                for subdir in subdirs {
                    let name = subdir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let folder_idx = nodes.len();
                    nodes.push(crate::ui::LocalNode::Folder {
                        name,
                        depth,
                        expanded: true,
                        children_start: folder_idx + 1,
                        children_count: 0,
                    });
                    let before = nodes.len();
                    scan_dir(&subdir, depth + 1, nodes, extensions, conn);
                    let added = nodes.len() - before;
                    if let crate::ui::LocalNode::Folder { children_count, .. } =
                        &mut nodes[folder_idx]
                    {
                        *children_count = added;
                    }
                    if added == 0 {
                        nodes.pop();
                    }
                }

                for path in files {
                    let uri = format!("file://{}", path.display());
                    let path_str = path.to_str().unwrap_or_default();

                    let mut track_data: Option<crate::spotify::TrackSummary> = None;
                    if let Some(c) = conn {
                        let stmt = c
                            .prepare("SELECT title, artist, album, duration_ms FROM tracks WHERE path = ?1")
                            .ok();
                        if let Some(mut s) = stmt {
                            track_data = s
                                .query_row([path_str], |row| {
                                    Ok(crate::spotify::TrackSummary {
                                        name: row.get(0)?,
                                        artist: row.get(1)?,
                                        album: row.get(2)?,
                                        duration_ms: row.get(3)?,
                                        uri: uri.clone(),
                                    })
                                })
                                .ok();
                        }
                    }

                    let track = if let Some(t) = track_data {
                        t
                    } else {
                        let (name, artist, album, duration_ms) = read_audio_metadata(&path);

                        let cover_art: Option<Vec<u8>> = None;

                        let cover_path = if let Some(art_bytes) = cover_art {
                            let hash = format!("{:x}", md5::compute(&art_bytes));

                            let cache_dir = dirs::cache_dir()
                                .map(|d| d.join("isi-music/covers"))
                                .unwrap_or_else(|| {
                                    std::path::PathBuf::from("/tmp/isi-music/covers")
                                });

                            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                                warn!("Cannot create cover cache dir: {e}");
                                None
                            } else {
                                let cover_file = cache_dir.join(format!("{}.jpg", hash));
                                match std::fs::write(&cover_file, &art_bytes) {
                                    Ok(_) => cover_file.to_str().map(|s| s.to_string()),
                                    Err(e) => {
                                        warn!("Cannot write cover art: {e}");
                                        None
                                    }
                                }
                            }
                        } else {
                            None
                        };

                        if let Some(c) = conn {
                            let _ = c.execute(
                                "INSERT OR REPLACE INTO tracks (path, title, artist, album, duration_ms, cover_path)
                                VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                                rusqlite::params![
                                    path_str,
                                    name,
                                    artist,
                                    album,
                                    duration_ms as i64,
                                    cover_path
                                ],
                            );
                        }

                        crate::spotify::TrackSummary {
                            name,
                            artist,
                            album,
                            duration_ms,
                            uri,
                        }
                    };

                    nodes.push(crate::ui::LocalNode::Track { track, depth });
                }
            }

            scan_dir(&dir, 0, &mut nodes, &extensions, &conn);
            let _ = tx.send(nodes);
        });
    }

    fn poll_local_scan(&mut self) {
        let rx = match &mut self.local_scan_rx {
            Some(r) => r,
            None => return,
        };

        if let Ok(nodes) = rx.try_recv() {
            self.local_scan_rx = None;

            let track_count = nodes.iter().filter(|n| !n.is_folder()).count();
            let tree = crate::ui::LocalFileTree::new(nodes);
            let vis_len = tree.visible_len();

            self.state.tracks = tree.all_tracks_flat();
            self.state.tracks_total = track_count as u32;
            self.state.tracks_offset = track_count as u32;
            self.state.local_tree = tree;
            self.state
                .local_tree_list
                .select(if vis_len == 0 { None } else { Some(0) });
            self.state.active_playlist_uri = Some("local_files".to_string());
            self.state.active_playlist_id = Some("local_files".to_string());

            self.local_scan_total = track_count;

            if track_count == 0 {
                self.state.status_msg = Some("No audio files found".to_string());
            } else {
                self.state.status_msg = Some(format!("{track_count} local tracks loaded"));
            }
        }
    }

    async fn radio_refill(&mut self) {
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
                        player.add_to_queue(t.uri, t.name, t.artist, t.duration_ms);
                    }
                    self.state.status_msg = Some(format!("󰐇 Radio: queued {count} tracks"));
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

    async fn get_similar_tracks(&mut self) {
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
                self.state.active_content = ActiveContent::Tracks;
                self.state.search_results = None;
                self.state.focus = Focus::Tracks;
                self.state.status_msg = if count == 0 {
                    Some("No recommendations found".to_string())
                } else {
                    Some(format!("󰐇 {count} similar tracks"))
                };
            }
            Err(e) => {
                self.state.status_msg = Some(format!("Recommendations failed: {e}"));
            }
        }
    }

    fn sync_queue_display(&mut self) {
        let mut items: Vec<(String, String)> = Vec::new();

        if let Some(p) = &self.player {
            items.extend(
                p.user_queue()
                    .iter()
                    .map(|t| (t.name.clone(), t.artist.clone())),
            );
        }
        if let Some(p) = &self.parked_player {
            let prefix = if self.local_active { " " } else { "󰈣 " };
            items.extend(
                p.user_queue()
                    .iter()
                    .map(|t| (format!("{}{}", prefix, t.name), t.artist.clone())),
            );
        }

        self.state.queue_items = items;
    }

    async fn reconnect_player(&mut self) {
        warn!("Session lost — attempting to reconnect librespot...");

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
            self.state.status_msg = Some("Reconnect failed: no token".to_string());
            self.session_reconnecting = false;
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
                warn!("Librespot session reconnected successfully");
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("free") || msg.contains("premium") {
                    warn!("Spotify free account — disabling streaming permanently");
                    self.spotify_streaming_disabled = true;
                    self.state.status_msg = Some(
                        "⚠ Spotify Premium required. Switched to local-only mode.".to_string(),
                    );
                    if self.parked_player.is_some() {
                        std::mem::swap(&mut self.player, &mut self.parked_player);
                        self.local_active = true;
                        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
                    }
                } else {
                    warn!("Reconnect failed: {e:#}");
                    self.state.status_msg = Some(format!("Reconnect failed: {e}"));
                }
            }
        }

        self.session_reconnecting = false;
    }
}
