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
            should_quit: false,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal).await;

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
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

        match (code, modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => self.state.nav_up(),
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => self.state.nav_down(),
            (KeyCode::Tab, _) => self.state.switch_focus(),

            (KeyCode::Enter, _) => self.handle_enter().await,

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
                    self.state.status_msg = Some(format!("Loading {name}..."));
                    match self.spotify.fetch_playlist_tracks(&id).await {
                        Ok(tracks) => {
                            self.state.tracks = tracks;
                            if !self.state.tracks.is_empty() {
                                self.state.track_list.select(Some(0));
                            }
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
                        if let Some(playlist_uri) = self.state.active_playlist_uri.clone() {
                            let track_uri = self.state.tracks[idx].uri.clone();
                            if let Err(e) = self.spotify.play_in_context(&playlist_uri, &track_uri).await {
                                self.state.status_msg = Some(format!("Error: {e}"));
                            }
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
