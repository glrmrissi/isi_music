use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

/// Read title/artist/album/duration from an audio file using symphonia.
/// Falls back to filename for missing tags.
fn read_audio_metadata(path: &Path) -> (String, String, String, u64) {
    use symphonia::core::{
        formats::FormatOptions,
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTagKey},
        probe::Hint,
    };

    let fallback_name = path.file_stem()
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
        &hint, mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(_) => return (fallback_name, String::new(), String::new(), 0),
    };

    let mut format = probed.format;

    let duration_ms = format.default_track()
        .and_then(|t| {
            let tb = t.codec_params.time_base?;
            let n_frames = t.codec_params.n_frames?;
            let secs = tb.calc_time(n_frames).seconds;
            Some(secs * 1000)
        })
        .unwrap_or(0);

    // Pull tags from the first available metadata revision
    let mut title = fallback_name.clone();
    let mut artist = String::new();
    let mut album = String::new();

    let meta_ref = format.metadata();
    if let Some(rev) = meta_ref.current() {
        for tag in rev.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => title = tag.value.to_string(),
                Some(StandardTagKey::Artist) | Some(StandardTagKey::AlbumArtist) => {
                    if artist.is_empty() { artist = tag.value.to_string(); }
                }
                Some(StandardTagKey::Album) => album = tag.value.to_string(),
                _ => {}
            }
        }
    }

    (title, artist, album, duration_ms)
}

use crate::discord::DiscordRpc;
use crate::lastfm::LastfmClient;
use crate::player::{AudioPlayer, LocalPlayer, NativePlayer, PlayerNotification, RepeatMode};
#[cfg(feature = "mpris")]
use crate::mpris::{MprisCmd, MprisHandle, MprisState};
use rspotify::model::RepeatState;
use crate::spotify::SpotifyClient;
use crate::ui::{ActiveContent, AlbumArtData, Focus, SearchPanel, SearchResults, Ui, UiState};

pub struct App {
    spotify: SpotifyClient,
    /// The currently active player (Spotify or local).
    player: Option<Box<dyn AudioPlayer>>,
    /// The Spotify player kept aside while local playback is active, and vice-versa.
    parked_player: Option<Box<dyn AudioPlayer>>,
    /// True when `player` is the LocalPlayer, false when it's the NativePlayer.
    local_active: bool,
    lastfm: Option<Arc<LastfmClient>>,
    ui: Ui,
    state: UiState,
    last_tick: Instant,
    should_quit: bool,
    // Seek hold detection
    last_seek_time: Option<Instant>,
    seek_hold_count: u32,
    // Scrobbling state
    scrobble_sent: bool,
    track_start_unix: u64,
    // Album art
    current_track_uri: String,
    last_art_uri: String,
    album_art_pending: Option<tokio::sync::oneshot::Receiver<(Option<String>, Option<Vec<u8>>)>>,
    picker: Picker,
    // MPRIS D-Bus integration
    #[cfg(feature = "mpris")]
    mpris: Option<MprisHandle>,
    // Discord Rich Presence
    discord: Option<DiscordRpc>,
    discord_last_title: String,
    discord_last_playing: bool,
    discord_pending_since: Option<Instant>,
    // Real-time audio band energies from AnalyzerSink
    band_energies: Option<Arc<Mutex<Vec<f32>>>>,
    art_url: Option<String>,
    // Session reconnection state
    session_reconnecting: bool,
    // Radio Mode: auto-fetch recommendations when the queue runs dry
    radio_mode: bool,
    /// Ring buffer of the last 5 Spotify track URIs played — used as seeds for recommendations.
    recent_track_uris: std::collections::VecDeque<String>,
}

impl App {
    pub async fn new(picker: Picker) -> Result<Self> {
        let cfg = crate::config::AppConfig::load().unwrap_or_default();
        let lastfm = match (&cfg.lastfm.api_key, &cfg.lastfm.api_secret, &cfg.lastfm.session_key) {
            (Some(k), Some(s), Some(sk)) => Some(Arc::new(LastfmClient::new(k.clone(), s.clone(), sk.clone()))),
            _ => None,
        };

        let mut spotify = SpotifyClient::new().await?;

        let volume = crate::config::load_volume();

        // Try to start the Spotify (native) player if authenticated
        let spotify_player: Option<Box<dyn AudioPlayer>> = if spotify.authenticated {
            match spotify.get_access_token().await {
                Some(token) => match NativePlayer::new(token, false).await {
                    Ok(p) => {
                        tracing::info!("Native player started");
                        Some(Box::new(p) as Box<dyn AudioPlayer>)
                    }
                    Err(e) => { warn!("Native player unavailable: {e:#}"); None }
                },
                None => { warn!("Token not available for native player"); None }
            }
        } else {
            None
        };

        // Always try to start the local player (used for local files regardless of Spotify)
        let local_player: Option<Box<dyn AudioPlayer>> = match LocalPlayer::new(volume) {
            Ok(p) => {
                tracing::info!("Local player started");
                Some(Box::new(p) as Box<dyn AudioPlayer>)
            }
            Err(e) => { warn!("Local player unavailable: {e:#}"); None }
        };

        // Active player: Spotify when available, otherwise local
        let (player, parked_player, local_active) = match (spotify_player, local_player) {
            (Some(sp), local) => (Some(sp), local, false),
            (None, Some(lp)) => (Some(lp), None, true),
            (None, None) => (None, None, false),
        };

        let band_energies = player.as_ref().and_then(|p| p.band_energies());

        let mut state = UiState::new();

        match spotify.fetch_playlists().await {
            Ok(playlists) => {
                state.playlists = playlists;
                if !state.playlists.is_empty() {
                    state.playlist_list.select(Some(0));
                }
            }
            Err(e) => warn!("Failed to load playlists: {e}"),
        }

        let initial_playback = spotify.fetch_playback().await.unwrap_or_default();
        let initial_art = initial_playback.art_url.clone(); 
        state.playback = initial_playback;

        #[cfg(feature = "mpris")]
        let mpris = match crate::mpris::spawn().await {
            Ok(h) => { tracing::info!("MPRIS D-Bus server started"); Some(h) }
            Err(e) => { tracing::warn!("MPRIS unavailable: {e}"); None }
        };

        let discord = if cfg.discord.enabled == Some(true) {
            let app_id = cfg.discord.app_id.as_deref().unwrap_or(crate::discord::DEFAULT_APP_ID);
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
            ui: Ui::new(),
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
        })
    }

   pub async fn run<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let tick_rate = Duration::from_millis(16);
        self.last_tick = Instant::now();

        loop {
            let now = Instant::now();
            let delta_ms = now.duration_since(self.last_tick).as_millis() as u64;
            self.last_tick = now;

            let mut needs_sync = false;
            let mut needs_reconnect = false;
            let mut needs_crossover = false;
            let mut needs_radio_refill = false;

            let parked_has_queue = self.parked_player
                .as_ref()
                .map(|p| !p.user_queue().is_empty())
                .unwrap_or(false);

            if let Some(player) = &mut self.player {
                while let Some(notif) = player.try_recv_event() {
                    match notif {
                        PlayerNotification::TrackEnded => {
                            if parked_has_queue {
                                // Cross-player queue has priority over the active playlist
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
                        PlayerNotification::Playing => self.state.playback.is_playing = true,
                        PlayerNotification::Paused => self.state.playback.is_playing = false,
                        PlayerNotification::TrackUnavailable => {
                            self.state.status_msg = Some("Track unavailable, skipping...".to_string());
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
                            self.state.status_msg = Some("Session lost, reconnecting...".to_string());
                            needs_reconnect = true;
                        }
                    }
                }
                self.state.playback.is_playing = player.is_playing();
                self.state.playback.volume = player.volume();
                self.state.playback.shuffle = player.shuffle();
                self.state.playback.repeat = match player.repeat() {
                    RepeatMode::Off => RepeatState::Off,
                    RepeatMode::Queue => RepeatState::Context,
                    RepeatMode::Track => RepeatState::Track,
                };
            }

            // If the active player's queue ended, check if the parked player has queued tracks
            if needs_crossover {
                let parked_has_queue = self.parked_player
                    .as_ref()
                    .map(|p| !p.user_queue().is_empty())
                    .unwrap_or(false);
                if parked_has_queue {
                    // Swap players and play next from the newly-active player's user queue
                    std::mem::swap(&mut self.player, &mut self.parked_player);
                    self.local_active = !self.local_active;
                    self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
                    if let Some(player) = &mut self.player {
                        if player.next() { needs_sync = true; }
                    }
                }
            }

            if needs_radio_refill {
                self.radio_refill().await;
                if let Some(player) = &mut self.player {
                    if player.next() { needs_sync = true; }
                }
            }

            if needs_sync {
                // sync_track_selection must run first — it reads take_playing_queued()
                // which is consumed once, and sets local metadata from the native player.
                self.sync_track_selection();
                self.sync_queue_display();
                // Only ask Spotify's API when no native player is in control.
                // With librespot running, the API lags behind and would overwrite
                // the correct local state we just set above.
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
                let art_url = self.state.art_url.clone(); 

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
                    while let Ok(c) = mpris.cmd_rx.try_recv() { v.push(c); }
                    v
                };

                for cmd in cmds {
                    match cmd {
                        MprisCmd::Play => { if let Some(p) = &mut self.player { p.play(); } }
                        MprisCmd::Pause => { if let Some(p) = &mut self.player { p.pause(); } }
                        MprisCmd::Next => {
                            if let Some(p) = &mut self.player { p.next(); }
                            self.sync_track_selection();
                            self.sync_queue_display();
                        }
                        MprisCmd::Prev => {
                            if let Some(p) = &mut self.player { p.prev(); }
                            self.sync_track_selection();
                        }
                        MprisCmd::Seek(us) => {
                            let ms = (us / 1000) as u64;
                            self.state.playback.progress_ms = ms;
                            if let Some(p) = &self.player { p.seek(ms as u32); }
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
                        let image_state = image::load_from_memory(&bytes).ok()
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
                    // Track changed — wait for art_url before sending
                    self.discord_pending_since = Some(Instant::now());
                    self.discord_last_title = pb.title.clone();
                    self.discord_last_playing = pb.is_playing;
                } else if playing_changed {
                    // Pause/resume — send immediately, no need to wait for art
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

                // Flush pending track update once art arrives or after 5s timeout
                if let Some(since) = self.discord_pending_since {
                    let art_ready = pb.art_url.is_some();
                    let timed_out = since.elapsed() >= Duration::from_secs(5);
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
                    let threshold = (duration / 2).min(4 * 60 * 1000);
                    if progress >= 30_000 && progress >= threshold {
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

            if self.should_quit { break; }
        }

        Ok(())
    }
    
    async fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        self.state.status_msg = None;

        // Search input mode
        if self.state.search_active {
            match code {
                KeyCode::Esc => self.state.cancel_search(),
                KeyCode::Enter => {
                    let query = self.state.search_query.trim().to_string();
                    if query.is_empty() {
                        self.state.cancel_search();
                    } else {
                        self.state.status_msg = Some(format!("Searching \"{query}\"..."));
                        match self.spotify.search_all(&query).await {
                            Ok(results) => {
                                let total = results.tracks.len()
                                    + results.artists.len()
                                    + results.albums.len()
                                    + results.playlists.len();
                                self.state.search_results = Some(SearchResults::new(query.clone(), results));
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
                KeyCode::Up                         => self.state.nav_up(),
                KeyCode::Down                       => self.state.nav_down(),
                KeyCode::Backspace                  => self.state.search_pop(),
                KeyCode::Tab                        => self.state.switch_focus(),
                KeyCode::Char(c)                    => self.state.search_push(c),
                _ => {}
            }
            return Ok(());
        }

        // Seek: ← → with hold acceleration
        match code {
            KeyCode::Left | KeyCode::Right => {
                let is_held = self.last_seek_time
                    .map(|t| t.elapsed() < Duration::from_millis(300))
                    .unwrap_or(false);
                if is_held { self.seek_hold_count += 1; } else { self.seek_hold_count = 0; }
                self.last_seek_time = Some(Instant::now());
                let step_ms = if self.seek_hold_count > 4 { 10_000u64 } else { 5_000u64 };

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

            (KeyCode::Up, KeyModifiers::CONTROL)   => self.state.nav_first(),
            (KeyCode::Down, KeyModifiers::CONTROL) => {
                self.state.nav_last();
                self.maybe_load_more().await;
            }

            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => self.state.nav_up(),
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.state.nav_down();
                self.maybe_load_more().await;
            }

            // Tab: cycle focus forward; Shift+Tab: cycle focus backward
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

            // Backspace → go back to previous search results if available
            (KeyCode::Backspace, _) => {
                if let Some(prev) = self.state.previous_search.take() {
                    self.state.search_results = Some(prev);
                    self.state.active_content = ActiveContent::None;
                    self.state.focus = Focus::Search;
                }
            }

            // Esc: exit fullscreen or exit search
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
                } else {
                    let _ = self.spotify.toggle_playback().await;
                }
            }
            (KeyCode::Char('a'), _) => {
                let track = self.state.track_list.selected()
                    .and_then(|i| self.state.tracks.get(i))
                    .map(|t| (t.uri.clone(), t.name.clone(), t.artist.clone(), t.duration_ms));
                if let Some((uri, name, artist, duration_ms)) = track {
                    // Route to the player that owns this URI type
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
                    let active_len = self.player.as_ref()
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
                    let new_sel = if self.state.queue_items.is_empty() { None }
                        else { Some(idx.min(self.state.queue_items.len() - 1)) };
                    self.state.queue_list.select(new_sel);
                }
            }

            (KeyCode::Char('n'), _) => {
                if let Some(player) = &mut self.player {
                    player.next();
                    self.sync_track_selection();
                    self.sync_queue_display();
                } else {
                    let _ = self.spotify.next_track().await;
                }
            }
            (KeyCode::Char('p'), _) => {
                if let Some(player) = &mut self.player {
                    player.prev();
                    self.sync_track_selection();
                } else {
                    let _ = self.spotify.prev_track().await;
                }
            }
            (KeyCode::Char('s'), _) => {
                if let Some(player) = &mut self.player {
                    player.toggle_shuffle();
                    self.state.playback.shuffle = player.shuffle();
                }
            }
            // Alt+r → Get similar tracks for the selected track or artist
            (KeyCode::Char('r'), KeyModifiers::ALT) => {
                self.get_similar_tracks().await;
            }
            // Shift+r (uppercase R) → Toggle Radio Mode
            (KeyCode::Char('R'), _) => {
                self.radio_mode = !self.radio_mode;
                self.state.playback.radio_mode = self.radio_mode;
                let msg = if self.radio_mode { "󰐇 Radio Mode on (Test)" } else { "Radio Mode off" };
                self.state.status_msg = Some(msg.to_string());
            }
            (KeyCode::Char('r'), _) => {
                if let Some(player) = &mut self.player {
                    player.cycle_repeat();
                    self.state.playback.repeat = match player.repeat() {
                        RepeatMode::Off   => RepeatState::Off,
                        RepeatMode::Queue => RepeatState::Context,
                        RepeatMode::Track => RepeatState::Track,
                    };
                }
            }
            (KeyCode::Char('z'), _) => {
                if !self.state.playback.title.is_empty() {
                    self.state.fullscreen_player = !self.state.fullscreen_player;
                    if self.state.fullscreen_player {
                        self.state.active_content = ActiveContent::None;
                    }
                }
            }
            (KeyCode::Char('c'), _) if modifiers != KeyModifiers::CONTROL
                && !self.state.fullscreen_player
                && !(self.state.search_results.is_none()
                     && self.state.active_content == ActiveContent::None
                     && !self.state.playback.title.is_empty()) => {
                self.state.show_album_art = !self.state.show_album_art;
                if self.state.show_album_art {
                    // Reset so maybe_fetch_album_art triggers a new fetch
                    self.last_art_uri.clear();
                } else {
                    self.state.album_art = None;
                    self.album_art_pending = None;
                }
            }
            (KeyCode::Char('l'), _) => {
                match self.spotify.save_current_track().await {
                    Ok(_)  => self.state.status_msg = Some("♥ Liked!".to_string()),
                    Err(e) => self.state.status_msg = Some(format!("Error liking track: {e}")),
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
            // Library: Liked Songs, Albums (placeholder), Artists (placeholder), Podcasts (placeholder)
            Focus::Library => {
                let idx = match self.state.library_list.selected() { Some(i) => i, None => return };
                match idx {
                    0 => { // Liked Songs
                        self.state.status_msg = Some("Loading Liked Songs…".to_string());
                        match self.spotify.fetch_liked_tracks(0).await {
                            Ok((tracks, total)) => {
                                self.state.tracks = tracks;
                                self.state.tracks_total = total;
                                self.state.tracks_offset = self.state.tracks.len() as u32;
                                self.state.active_playlist_uri = Some("liked_songs".to_string());
                                self.state.active_playlist_id = Some("liked_songs".to_string());
                                self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
                                self.state.active_content = ActiveContent::Tracks;
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    1 => { // Albums
                        self.state.status_msg = Some("Loading saved albums…".to_string());
                        match self.spotify.fetch_saved_albums(0).await {
                            Ok((albums, total)) => {
                                self.state.albums = albums;
                                self.state.albums_total = total;
                                self.state.albums_offset = self.state.albums.len() as u32;
                                self.state.album_list.select(if self.state.albums.is_empty() { None } else { Some(0) });
                                self.state.active_content = ActiveContent::Albums;
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    2 => { // Artists
                        self.state.status_msg = Some("Loading followed artists…".to_string());
                        match self.spotify.fetch_followed_artists().await {
                            Ok(artists) => {
                                self.state.artists = artists;
                                self.state.artist_list.select(if self.state.artists.is_empty() { None } else { Some(0) });
                                self.state.active_content = ActiveContent::Artists;
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    3 => { // Podcasts — coming soon
                        self.state.status_msg = Some("Podcasts — coming soon".to_string());
                    }
                    4 => { // Local Files
                        self.load_local_files().await;
                    }
                    _ => {}
                }
            }

            Focus::Playlists => {
                if let Some(playlist) = self.state.selected_playlist() {
                    let id  = playlist.id.clone();
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
                            self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
                            self.state.active_content = ActiveContent::Tracks;
                            self.state.search_results = None;
                            self.state.status_msg = None;
                            self.state.focus = Focus::Tracks;
                        }
                        Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                    }
                }
            }

            Focus::Tracks => {
                match &self.state.active_content {
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
                                        self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
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
                                        self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
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
                                        self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
                                        self.state.active_content = ActiveContent::Tracks;
                                        self.state.status_msg = None;
                                    }
                                    Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                                }
                            }
                        }
                    }
                    ActiveContent::LocalFiles | ActiveContent::Tracks | ActiveContent::None => {
                        if let Some(idx) = self.state.selected_track_index() {
                            // Switch to the appropriate player based on content type
                            if self.state.active_content == ActiveContent::LocalFiles {
                                self.activate_local_player();
                            } else {
                                self.activate_spotify_player();
                            }
                            // Podcast episodes cannot be played via librespot
                            if self.state.tracks.get(idx)
                                .map(|t| t.uri.starts_with("spotify:episode:"))
                                .unwrap_or(false)
                            {
                                self.state.status_msg = Some("Podcast playback not supported".to_string());
                            } else
                            if let Some(player) = &mut self.player {
                                let uris: Vec<String> = self.state.tracks.iter()
                                    .filter(|t| !t.uri.starts_with("spotify:episode:"))
                                    .map(|t| t.uri.clone())
                                    .collect();
                                let adjusted_idx = self.state.tracks[..idx].iter()
                                    .filter(|t| !t.uri.starts_with("spotify:episode:"))
                                    .count();
                                player.set_queue(uris, adjusted_idx);
                                if let Some(track) = self.state.tracks.get(idx) {
                                    self.state.playback.title = track.name.clone();
                                    self.state.playback.artist = track.artist.clone();
                                    self.state.playback.album = track.album.clone();
                                    self.state.playback.duration_ms = track.duration_ms;
                                    self.state.playback.progress_ms = 0;
                                    self.state.playback.is_playing = true;
                                    self.current_track_uri = track.uri.clone();
                                    self.on_track_started();
                                }
                            } else {
                                let track_uri = self.state.tracks[idx].uri.clone();
                                let is_playlist = self.state.active_playlist_uri
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
                }
            }

            Focus::Search => {
                let panel = self.state.search_results.as_ref().map(|sr| sr.panel);
                match panel {
                    Some(SearchPanel::Tracks) => {
                        let uri = self.state.search_results.as_ref()
                            .and_then(|sr| sr.selected_track_uri())
                            .map(|s| s.to_string());
                        if let Some(track_uri) = uri {
                            self.activate_spotify_player();
                            if let Some(player) = &mut self.player {
                                // load just this track into queue
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
                                            self.on_track_started();
                                        }
                                    }
                                }
                            } else {
                                let _ = self.spotify.play_track_uri(&track_uri).await;
                            }
                        }
                    }
                    Some(SearchPanel::Albums) => {
                        let album = self.state.search_results.as_ref()
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
                                    self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
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
                        let playlist = self.state.search_results.as_ref()
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
                                    self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
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
                        let artist = self.state.search_results.as_ref()
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
                                    self.state.track_list.select(if self.state.tracks.is_empty() { None } else { Some(0) });
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

        self.state.playback.is_local = self.local_active;
        self.state.playback.radio_mode = self.radio_mode;

        // Keep a rolling window of the last 5 Spotify tracks for radio seeds
        if self.current_track_uri.starts_with("spotify:track:") {
            self.recent_track_uris.push_back(self.current_track_uri.clone());
            if self.recent_track_uris.len() > 5 {
                self.recent_track_uris.pop_front();
            }
        }

        // Reset album art so maybe_fetch_album_art will kick off a new fetch
        self.state.album_art = None;
        self.state.playback.art_url = None;
        self.album_art_pending = None;
        self.last_art_uri.clear();

        if let Some(lfm) = self.lastfm.clone() {
            let artist = self.state.playback.artist.clone();
            let track = self.state.playback.title.clone();
            let duration = self.state.playback.duration_ms;
            tokio::spawn(async move {
                lfm.update_now_playing(&artist, &track, duration).await;
            });
        }
    }

    /// Load next page of tracks for the active context.
    async fn maybe_load_more(&mut self) {
        // Handle search results pagination
        if self.state.focus == Focus::Search {
            let should_load = self.state.search_results.as_ref().map(|sr| {
                if sr.loading { return None; }
                let (selected, len, total, stype) = match sr.panel {
                    SearchPanel::Tracks    => (sr.track_list.selected().unwrap_or(0),    sr.tracks.len(),    sr.tracks_total,    "track"),
                    SearchPanel::Artists   => (sr.artist_list.selected().unwrap_or(0),   sr.artists.len(),   sr.artists_total,   "artist"),
                    SearchPanel::Albums    => (sr.album_list.selected().unwrap_or(0),    sr.albums.len(),    sr.albums_total,    "album"),
                    SearchPanel::Playlists => (sr.playlist_list.selected().unwrap_or(0), sr.playlists.len(), sr.playlists_total, "playlist"),
                };
                if len == 0 || selected < len.saturating_sub(3) || len >= total as usize {
                    return None;
                }
                Some((sr.query.clone(), len as u32, stype))
            }).flatten();

            if let Some((query, offset, stype)) = should_load {
                self.state.search_results.as_mut().unwrap().loading = true;
                match self.spotify.search_more(&query, stype, offset).await {
                    Ok(more) => {
                        let sr = self.state.search_results.as_mut().unwrap();
                        match stype {
                            "track"    => { sr.tracks_total = more.tracks_total;       sr.tracks.extend(more.tracks); }
                            "artist"   => { sr.artists_total = more.artists_total;     sr.artists.extend(more.artists); }
                            "album"    => { sr.albums_total = more.albums_total;       sr.albums.extend(more.albums); }
                            "playlist" => { sr.playlists_total = more.playlists_total; sr.playlists.extend(more.playlists); }
                            _ => {}
                        }
                        sr.loading = false;
                    }
                    Err(e) => {
                        if let Some(sr) = self.state.search_results.as_mut() { sr.loading = false; }
                        self.state.status_msg = Some(format!("Load more error: {e}"));
                    }
                }
            }
            return;
        }

        // Handle album list pagination
        if self.state.active_content == ActiveContent::Albums {
            let selected = self.state.album_list.selected().unwrap_or(0);
            let len = self.state.albums.len();
            if len > 0 && selected >= len.saturating_sub(3) && len < self.state.albums_total as usize {
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

        // Handle show list pagination
        if self.state.active_content == ActiveContent::Shows {
            let selected = self.state.show_list.selected().unwrap_or(0);
            let len = self.state.shows.len();
            if len > 0 && selected >= len.saturating_sub(3) && len < self.state.shows_total as usize {
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

        if self.state.tracks_loading { return; }
        let selected = self.state.track_list.selected().unwrap_or(0);
        let len = self.state.tracks.len();
        if len == 0 || selected < len.saturating_sub(3) { return; }
        if (self.state.tracks_offset as usize) >= len
            && len < self.state.tracks_total as usize
        {
            self.state.tracks_loading = true;
            let offset = self.state.tracks_offset;
            let id = self.state.active_playlist_id.clone();

            let result = match id.as_deref() {
                Some("liked_songs") => {
                    self.spotify.fetch_liked_tracks(offset).await
                        .map(|(t, total)| (t, total))
                }
                Some(id) if id.starts_with("album:") => {
                    let album_id = &id["album:".len()..];
                    self.spotify.fetch_album_tracks(album_id, offset).await
                }
                Some(id) if id.starts_with("artist:") => {
                    let name = self.state.active_artist_name.clone().unwrap_or_default();
                    self.spotify.fetch_artist_tracks(&name, offset).await
                }
                Some(id) => {
                    self.spotify.fetch_playlist_tracks(id, offset).await
                }
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
        let need_art_for_now_playing = self.state.search_results.is_none()
            && self.state.active_content == ActiveContent::None
            && !self.state.playback.title.is_empty();

        // Always fetch art so Discord RPC and MPRIS have the URL regardless of TUI state
        if (!self.state.show_album_art && !need_art_for_now_playing)
            && self.discord.is_none()
        {
            return;
        }

        if self.current_track_uri.is_empty()
            || self.current_track_uri == self.last_art_uri
            || self.album_art_pending.is_some()
        {
            return;
        }
        let uri = self.current_track_uri.clone();
        let Some(token) = self.spotify.get_access_token().await else { return };
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
                .send().await
            else {
                let _ = tx.send((None, None));
                return;
            };
            let Ok(json) = resp.json::<serde_json::Value>().await else {
                let _ = tx.send((None, None));
                return;
            };
            // Use the largest image (first) for better quality in terminal
            let Some(url) = json["album"]["images"].as_array()
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

    fn sync_track_selection(&mut self) {
        // If we just played a user_queue track, update playback from that
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
                self.state.track_list.select(Some(idx));
                if let Some(track) = self.state.tracks.get(idx) {
                    self.state.playback.title = track.name.clone();
                    self.state.playback.artist = track.artist.clone();
                    self.state.playback.album = track.album.clone();
                    self.state.playback.duration_ms = track.duration_ms;
                    self.state.playback.progress_ms = 0;
                    self.current_track_uri = track.uri.clone();
                    self.on_track_started();
                }
            }
        }
    }

    /// Swap the active player to the LocalPlayer.
    /// Pauses Spotify playback and moves it to `parked_player`.
    fn activate_local_player(&mut self) {
        if self.local_active { return; }
        // Pause whatever is currently playing in the Spotify player
        if let Some(ref mut p) = self.player {
            p.pause();
        }
        std::mem::swap(&mut self.player, &mut self.parked_player);
        self.local_active = true;
        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
    }

    /// Swap the active player back to the Spotify/NativePlayer.
    /// Pauses local playback and moves it to `parked_player`.
    fn activate_spotify_player(&mut self) {
        if !self.local_active { return; }
        if let Some(ref mut p) = self.player {
            p.pause();
        }
        std::mem::swap(&mut self.player, &mut self.parked_player);
        self.local_active = false;
        self.band_energies = self.player.as_ref().and_then(|p| p.band_energies());
    }

    async fn load_local_files(&mut self) {
        let cfg = crate::config::AppConfig::load().unwrap_or_default();
        let raw_dir = match cfg.local.music_dir {
            Some(d) => d,
            None => {
                self.state.status_msg = Some(
                    "Set [local] music_dir in ~/.config/isi-music/config.toml".to_string()
                );
                return;
            }
        };

        // Expand leading ~
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

        self.state.status_msg = Some("Scanning local files…".to_string());

        let extensions = ["mp3", "flac", "ogg", "wav", "aiff", "m4a", "opus"];
        let mut tracks = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut paths: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| extensions.contains(&e.to_lowercase().as_str()))
                        .unwrap_or(false)
                })
                .collect();
            paths.sort();

            for path in paths {
                let (name, artist, album, duration_ms) = read_audio_metadata(&path);
                let uri = format!("file://{}", path.display());
                tracks.push(crate::spotify::TrackSummary { name, artist, album, duration_ms, uri });
            }
        }

        let count = tracks.len();
        self.state.tracks = tracks;
        self.state.tracks_total = count as u32;
        self.state.tracks_offset = count as u32;
        self.state.active_playlist_uri = Some("local_files".to_string());
        self.state.active_playlist_id = Some("local_files".to_string());
        self.state.track_list.select(if count == 0 { None } else { Some(0) });
        self.state.active_content = crate::ui::ActiveContent::LocalFiles;
        self.state.search_results = None;
        self.state.focus = crate::ui::Focus::Tracks;

        if count == 0 {
            self.state.status_msg = Some(format!("No audio files found in {}", dir.display()));
        } else {
            self.state.status_msg = Some(format!("{count} local tracks loaded"));
        }
    }

    /// Fetch recommendations from Spotify and add them to the active player's user queue.
    async fn radio_refill(&mut self) {
        let seeds: Vec<String> = self.recent_track_uris.iter().cloned().collect();
        if seeds.is_empty() {
            self.state.status_msg = Some("Radio: no seed tracks yet — play a Spotify track first".to_string());
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
                // Could not find recommendations — turn off radio so the player doesn't get stuck
                self.radio_mode = false;
                self.state.playback.radio_mode = false;
                self.state.status_msg = Some("Radio: could not find tracks, radio off".to_string());
            }
        }
    }

    /// Fetch similar tracks for the currently selected track or artist and display them.
    async fn get_similar_tracks(&mut self) {
        let seed_uri = match self.state.focus {
            Focus::Tracks => self
                .state
                .track_list
                .selected()
                .and_then(|i| self.state.tracks.get(i))
                .map(|t| t.uri.clone()),
            Focus::Search => self.state.search_results.as_ref().and_then(|sr| match sr.panel {
                SearchPanel::Tracks => sr.selected_track_uri().map(|s| s.to_string()),
                SearchPanel::Artists => sr.selected_artist().map(|a| format!("spotify:artist:{}", a.id)),
                _ => None,
            }),
            _ => None,
        };

        let Some(uri) = seed_uri else {
            self.state.status_msg = Some("Select a track or artist first".to_string());
            return;
        };

        if uri.starts_with("file://") {
            self.state.status_msg = Some("Recommendations require a Spotify track or artist".to_string());
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
                self.state.track_list.select(if count == 0 { None } else { Some(0) });
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

        // Active player's queue comes first
        if let Some(p) = &self.player {
            items.extend(p.user_queue().iter().map(|t| (t.name.clone(), t.artist.clone())));
        }
        // Parked player's queue appended after (shown with a marker)
        if let Some(p) = &self.parked_player {
            let prefix = if self.local_active { " " } else { "󰈣 " };
            items.extend(p.user_queue().iter().map(|t| {
                (format!("{}{}", prefix, t.name), t.artist.clone())
            }));
        }

        self.state.queue_items = items;
    }

    /// Recreates the librespot session after it has been invalidated.
    /// Saves the current queue and index, creates a new player, then restores playback.
    async fn reconnect_player(&mut self) {
        warn!("Session lost — attempting to reconnect librespot...");

        // Snapshot what was queued before the session died
        let (saved_queue, saved_index) = self
            .player
            .as_ref()
            .map(|p| p.snapshot_queue())
            .unwrap_or_default();
        let saved_volume = self.player.as_ref().map(|p| p.volume()).unwrap_or(50);

        // Drop the old (dead) player
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
                // Restore the queue at the track that was playing, without auto-playing
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
                warn!("Reconnect failed: {e:#}");
                self.state.status_msg = Some(format!("Reconnect failed: {e}"));
            }
        }

        self.session_reconnecting = false;
    }
}
