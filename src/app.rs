use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Terminal;
use std::time::{Duration, Instant};
use tracing::warn;

use crate::player::{NativePlayer, PlayerNotification};
use crate::spotify::SpotifyClient;
use crate::ui::{Focus, SearchPanel, SearchResults, Ui, UiState};

pub struct App {
    spotify: SpotifyClient,
    player: Option<NativePlayer>,
    ui: Ui,
    state: UiState,
    last_tick: Instant,
    should_quit: bool,
    // Seek hold detection
    last_seek_time: Option<Instant>,
    seek_hold_count: u32,
}

impl App {
    pub async fn new() -> Result<Self> {
        let mut spotify = SpotifyClient::new().await?;

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

        state.playback = spotify.fetch_playback().await.unwrap_or_default();

        Ok(Self {
            spotify,
            player,
            ui: Ui::new(),
            state,
            last_tick: Instant::now(),
            should_quit: false,
            last_seek_time: None,
            seek_hold_count: 0,
        })
    }

    pub async fn run<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        self.last_tick = Instant::now();

        loop {
            let now = Instant::now();
            let delta_ms = now.duration_since(self.last_tick).as_millis() as u64;
            self.last_tick = now;

            let mut needs_sync = false;

            if let Some(player) = &mut self.player {
                while let Ok(notif) = player.event_rx.try_recv() {
                    match notif {
                        PlayerNotification::TrackEnded => {
                            if player.next() { needs_sync = true; }
                            else { self.state.playback.is_playing = false; }
                        }
                        PlayerNotification::Playing  => self.state.playback.is_playing = true,
                        PlayerNotification::Paused   => self.state.playback.is_playing = false,
                        PlayerNotification::TrackUnavailable => {
                            self.state.status_msg = Some("Track unavailable (Premium required)".to_string());
                            self.state.playback.is_playing = false;
                        }
                    }
                }
                self.state.playback.is_playing = player.is_playing;
                self.state.playback.volume = player.volume;
            }

            if needs_sync { self.sync_track_selection(); }

            terminal.draw(|f| self.ui.render(f, &mut self.state))?;

            if crossterm::event::poll(Duration::from_millis(100))? {
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
                                self.state.search_results = Some(SearchResults::new(results));
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
                KeyCode::Up | KeyCode::Char('k')   => self.state.nav_up(),
                KeyCode::Down | KeyCode::Char('j')  => self.state.nav_down(),
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

            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => self.state.nav_up(),
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.state.nav_down();
                self.maybe_load_more().await;
            }

            // Tab: if in Search focus → cycle search panels; else cycle focus
            (KeyCode::Tab, _) => {
                if self.state.focus == Focus::Search {
                    self.state.switch_search_panel();
                } else {
                    self.state.switch_focus();
                }
            }

            (KeyCode::Enter, _) => self.handle_enter().await,

            (KeyCode::Char('/'), _) => self.state.start_search(),

            // Esc from search panel → back to normal tracks view
            (KeyCode::Esc, _) => {
                if self.state.search_results.is_some() {
                    self.state.search_results = None;
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
            (KeyCode::Char('s'), _) => { let _ = self.spotify.toggle_shuffle().await; }
            (KeyCode::Char('r'), _) => { let _ = self.spotify.cycle_repeat().await; }
            (KeyCode::Char('l'), _) => {
                match self.spotify.save_current_track().await {
                    Ok(_)  => self.state.status_msg = Some("♥ Liked!".to_string()),
                    Err(e) => self.state.status_msg = Some(format!("Error liking track: {e}")),
                }
            }
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
                                self.state.search_results = None;
                                self.state.status_msg = None;
                                self.state.focus = Focus::Tracks;
                            }
                            Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                        }
                    }
                    _ => {
                        let names = ["Albums", "Artists", "Podcasts"];
                        self.state.status_msg = Some(format!("{} — coming soon", names[idx - 1]));
                    }
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
                            self.state.search_results = None;
                            self.state.status_msg = None;
                            self.state.focus = Focus::Tracks;
                        }
                        Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                    }
                }
            }

            Focus::Tracks => {
                if let Some(idx) = self.state.selected_track_index() {
                    if let Some(player) = &mut self.player {
                        let uris: Vec<String> = self.state.tracks.iter().map(|t| t.uri.clone()).collect();
                        player.set_queue(uris, idx);
                        if let Some(track) = self.state.tracks.get(idx) {
                            self.state.playback.title = track.name.clone();
                            self.state.playback.artist = track.artist.clone();
                            self.state.playback.album = track.album.clone();
                            self.state.playback.duration_ms = track.duration_ms;
                            self.state.playback.progress_ms = 0;
                            self.state.playback.is_playing = true;
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

            Focus::Search => {
                let panel = self.state.search_results.as_ref().map(|sr| sr.panel);
                match panel {
                    Some(SearchPanel::Tracks) => {
                        let uri = self.state.search_results.as_ref()
                            .and_then(|sr| sr.selected_track_uri())
                            .map(|s| s.to_string());
                        if let Some(track_uri) = uri {
                            if let Some(player) = &mut self.player {
                                // load just this track into queue
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
                                    self.state.search_results = None;
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
                                    self.state.search_results = None;
                                    self.state.status_msg = None;
                                    self.state.focus = Focus::Tracks;
                                }
                                Err(e) => self.state.status_msg = Some(format!("Error: {e}")),
                            }
                        }
                    }
                    Some(SearchPanel::Artists) => {
                        self.state.status_msg = Some("Artist browse — coming soon".to_string());
                    }
                    None => {}
                }
            }
        }
    }

    /// Load next page of tracks for the active context.
    async fn maybe_load_more(&mut self) {
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
