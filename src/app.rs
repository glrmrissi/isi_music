use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::{Duration, Instant};
use tracing::warn;

use crate::player::{NativePlayer, PlayerNotification};
use crate::spotify::SpotifyClient;
use crate::ui::{Ui, UiState};

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct App {
    spotify: SpotifyClient,
    player: Option<NativePlayer>,
    ui: Ui,
    state: UiState,
    last_poll: Instant,
    last_tick: Instant,
    should_quit: bool,
}

impl App {
    pub async fn new() -> Result<Self> {
        let mut spotify = SpotifyClient::new().await?;

        // Initialize native player with the current OAuth token
        let player = match spotify.get_access_token().await {
            Some(token) => match NativePlayer::new(token).await {
                Ok(p) => {
                    tracing::info!("Native player started");
                    Some(p)
                }
                Err(e) => {
                    warn!("Native player unavailable: {e:#}");
                    None
                }
            },
            None => {
                warn!("Token not available for native player");
                None
            }
        };

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

        // Fetch initial playback state from Spotify
        state.playback = spotify.fetch_playback().await.unwrap_or_default();

        Ok(Self {
            spotify,
            player,
            ui: Ui::new(),
            state,
            last_poll: Instant::now(),
            last_tick: Instant::now(),
            should_quit: false,
        })
    }

   pub async fn run<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
    self.last_tick = Instant::now();

    loop {
        let now = Instant::now();
        let delta_time = now.duration_since(self.last_tick).as_millis() as u64;
        self.last_tick = now;

        let mut needs_sync = false; 

        if let Some(player) = &mut self.player {
            while let Ok(notif) = player.event_rx.try_recv() {
                match notif {
                    PlayerNotification::TrackEnded => {
                        if player.next() {
                            needs_sync = true; 
                        } else {
                            self.state.playback.is_playing = false;
                        }
                    }
                    PlayerNotification::Playing => self.state.playback.is_playing = true,
                    PlayerNotification::Paused => self.state.playback.is_playing = false,
                    PlayerNotification::TrackUnavailable => {
                        self.state.status_msg = Some("Track unavailable (Premium required)".to_string());
                        self.state.playback.is_playing = false;
                    }
                }
            }
            self.state.playback.is_playing = player.is_playing;
            self.state.playback.volume = player.volume;
        } 

        if needs_sync {
            self.sync_track_selection();
        }

        terminal.draw(|f| {
            self.ui.render(f, &mut self.state);
        })?;

        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
                self.handle_key(key_event.code, key_event.modifiers).await?;
            }
        }

        if self.state.playback.is_playing {
            if self.state.playback.progress_ms + delta_time < self.state.playback.duration_ms {
                self.state.playback.progress_ms += delta_time;
            } else if self.player.is_none() {
                self.state.playback.is_playing = false;
                self.state.playback.progress_ms = self.state.playback.duration_ms;
            }

            // Animate disc and marquee
            self.state.spin_angle += delta_time as f64 * 0.003;
            self.state.marquee_ms += delta_time;
            if self.state.marquee_ms >= 120 {
                self.state.marquee_offset += (self.state.marquee_ms / 120) as usize;
                self.state.marquee_ms %= 120;
            }
        }

        if self.should_quit {
            break;
        }
    }

    Ok(())
}

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        loop {
            // Process native player events (track ended, errors)
            if let Some(player) = &mut self.player {
                let mut notifications = Vec::new();
                while let Ok(n) = player.event_rx.try_recv() {
                    notifications.push(n);
                }
                for notif in notifications {
                    match notif {
                        PlayerNotification::TrackEnded => {
                            if let Some(player) = &mut self.player {
                                if !player.next() {
                                    player.is_playing = false;
                                }
                            }
                            self.sync_track_selection();
                        }
                        PlayerNotification::TrackUnavailable => {
                            self.state.status_msg =
                                Some("Track unavailable (Spotify Premium required)".to_string());
                            if let Some(p) = &mut self.player { p.is_playing = false; }
                        }
                        PlayerNotification::Playing => {
                            if let Some(p) = &mut self.player { p.is_playing = true; }
                        }
                        PlayerNotification::Paused => {
                            if let Some(p) = &mut self.player { p.is_playing = false; }
                        }
                    }
                }
            }
            if let Some(player) = &self.player {
                self.state.playback.is_playing = player.is_playing;
                self.state.playback.volume = player.volume;
            }

            // Periodic polling — only fetches Spotify metadata when no native player
            if self.last_poll.elapsed() >= POLL_INTERVAL {
                if self.player.is_none() {
                    self.state.playback = self.spotify.fetch_playback().await.unwrap_or_default();
                }
                self.last_poll = Instant::now();
            }

            terminal.draw(|frame| self.ui.render(frame, &mut self.state))?;

            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers).await?;
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

        // Search mode: most keys feed into the query
        if self.state.search_active {
            match code {
                KeyCode::Esc => self.state.cancel_search(),
                KeyCode::Enter => {
                    let query = self.state.search_query.trim().to_string();
                    if query.is_empty() {
                        self.state.cancel_search();
                    } else {
                        self.state.status_msg = Some(format!("Searching \"{query}\"..."));
                        match self.spotify.search_tracks(&query).await {
                            Ok(tracks) => {
                                let count = tracks.len();
                                self.state.tracks = tracks;
                                self.state.track_list.select(if count == 0 { None } else { Some(0) });
                                // Use a sentinel so render_tracks knows a search ran (not an empty playlist)
                                self.state.active_playlist_uri = Some(format!("search:{query}"));
                                self.state.status_msg = if count == 0 {
                                    Some(format!("No results for \"{query}\""))
                                } else {
                                    Some(format!("{count} results for \"{query}\""))
                                };
                                self.state.search_active = false;
                                self.state.focus = crate::ui::Focus::Tracks;
                            }
                            Err(e) => {
                                self.state.status_msg = Some(format!("Search error: {e:#}"));
                                self.state.search_active = false;
                                tracing::error!("Search failed for \"{query}\": {e:#}");
                            }
                        }
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => self.state.nav_up(),
                KeyCode::Down | KeyCode::Char('j') => self.state.nav_down(),
                KeyCode::Backspace => self.state.search_pop(),
                KeyCode::Tab => self.state.switch_focus(),
                KeyCode::Char(c) => self.state.search_push(c),
                _ => {}
            }
            return Ok(());
        }

        match (code, modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => self.state.nav_up(),
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => self.state.nav_down(),
            (KeyCode::Tab, _) => self.state.switch_focus(),

            (KeyCode::Enter, _) => self.handle_enter().await,

            (KeyCode::Char('/'), _) => self.state.start_search(),

            (KeyCode::Char(' '), _) => {
                if let Some(player) = &mut self.player {
                    player.toggle();
                } else {
                    let _ = self.spotify.toggle_playback().await;
                }
            }
            (KeyCode::Char('n'), _) => {
                if let Some(player) = &mut self.player {
                    player.next();
                    self.sync_track_selection();
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
                let _ = self.spotify.toggle_shuffle().await;
            }
            (KeyCode::Char('r'), _) => {
                let _ = self.spotify.cycle_repeat().await;
            }
            (KeyCode::Char('l'), _) => {
                match self.spotify.save_current_track().await {
                    Ok(_) => self.state.status_msg = Some("♥ Liked!".to_string()),
                    Err(e) => self.state.status_msg = Some(format!("Error liking track: {e}")),
                }
            }
            // Volume control
            (KeyCode::Char('+'), _) | (KeyCode::Char('='), _) => {
                if let Some(player) = &mut self.player {
                    player.volume_up();
                    self.state.playback.volume = player.volume;
                }
            }
            (KeyCode::Char('-'), _) => {
                if let Some(player) = &mut self.player {
                    player.volume_down();
                    self.state.playback.volume = player.volume;
                }
            }

            _ => {}
        }
        Ok(())
    }

    async fn handle_enter(&mut self) {
        use crate::ui::Focus;
        match self.state.focus {
            Focus::Playlists => {
                if let Some(playlist) = self.state.selected_playlist() {
                    let id = playlist.id.clone();
                    let name = playlist.name.clone();
                    let uri = playlist.uri.clone();
                    self.state.status_msg = Some(format!("Loading {name}..."));
                    let result = if id == "liked_songs" {
                        self.spotify
                            .fetch_liked_tracks()
                            .await
                            .map(|(tracks, total)| {
                                // Update the liked songs entry with the real count
                                if let Some(entry) = self.state.playlists.iter_mut().find(|p| p.id == "liked_songs") {
                                    entry.total_tracks = total;
                                }
                                tracks
                            })
                    } else {
                        self.spotify.fetch_playlist_tracks(&id).await
                    };
                    match result {
                        Ok(tracks) => {
                            self.state.tracks = tracks;
                            if !self.state.tracks.is_empty() {
                                self.state.track_list.select(Some(0));
                            }
                            self.state.active_playlist_uri = Some(uri);
                            self.state.status_msg = None;
                            self.state.switch_focus();
                        }
                        Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                    }
                }
            }
            Focus::Tracks => {
                if let Some(idx) = self.state.selected_track_index() {
                    if let Some(player) = &mut self.player {
                        // Native player: load full queue and start from selected index
                        let uris: Vec<String> =
                            self.state.tracks.iter().map(|t| t.uri.clone()).collect();
                        player.set_queue(uris, idx);

                        // Update UI with current track
                        if let Some(track) = self.state.tracks.get(idx) {
                            self.state.playback.title = track.name.clone();
                            self.state.playback.artist = track.artist.clone();
                            self.state.playback.album = track.album.clone();
                            self.state.playback.duration_ms = track.duration_ms;
                            self.state.playback.progress_ms = 0;
                            self.state.playback.is_playing = true;
                        }
                    } else {
                        // Fallback: control via Spotify API
                        let track_uri = self.state.tracks[idx].uri.clone();
                        let is_playlist = self.state.active_playlist_uri
                            .as_deref()
                            .map(|u| !u.starts_with("search:") && u != "liked_songs")
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

    /// Syncs the visual track selection with the player's current index.
    fn sync_track_selection(&mut self) {
        if let Some(player) = &self.player {
            if let Some(idx) = player.current_index() {
                self.state.track_list.select(Some(idx));
                if let Some(track) = self.state.tracks.get(idx) {
                    self.state.playback.title = track.name.clone();
                    self.state.playback.artist = track.artist.clone();
                    self.state.playback.album = track.album.clone();
                    self.state.playback.duration_ms = track.duration_ms;
                    self.state.playback.progress_ms = 0;
                }
            }
        }
    }
}
