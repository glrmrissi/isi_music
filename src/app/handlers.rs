use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use std::time::{Duration, Instant};
use tracing::warn;

use crate::App;
use crate::player::RepeatMode;
use crate::ui::{ActiveContent, CompactItem, Focus, SearchPanel, SearchResults};
use crate::utils::debug_overlay::LogLevel;

impl App {
    pub async fn handle_quick_search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.state.cancel_quick_search(),
            KeyCode::Enter => self.state.apply_quick_filter(),
            KeyCode::Backspace => self.state.quick_search_pop(),
            KeyCode::Char(c) if c.is_alphanumeric() || c == ' ' || c == '-' => {
                self.state.quick_search_push(c);
            }
            _ => {}
        }
    }

    pub async fn handle_search_key(&mut self, code: KeyCode) -> Result<()> {
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
                            self.state.rebuild_sort_indices();
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
        Ok(())
    }

    pub async fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        self.state.status_msg = None;

        if self.state.quick_search_active {
            self.handle_quick_search_key(code).await;
            return Ok(());
        }

        if self.state.search_active {
            return self.handle_search_key(code).await;
        }

        if let Some(ref mut panel) = self.options_panel {
            if panel.visible {
                use crate::ui::options::{OptionsSection, PanelAction};
                match panel.handle_key(code) {
                    PanelAction::Close => {
                        panel.visible = false;
                        self.state.status_msg = Some("Options panel closed".to_string());
                    }
                    PanelAction::ToggleItem => match panel.focused_section {
                        OptionsSection::Features => {
                            let idx = panel.selected_item;
                            #[cfg(feature = "album-art")]
                            if idx == 0 {
                                self.state.show_album_art = !self.state.show_album_art;
                                panel.config.show_cover_images = Some(self.state.show_album_art);
                                self.state.status_msg = Some(if self.state.show_album_art {
                                    "Cover images enabled".to_string()
                                } else {
                                    "Cover images disabled".to_string()
                                });
                                return Ok(());
                            }
                            #[cfg(feature = "album-art")]
                            let idx = idx - 1;
                            match idx {
                                0 => {
                                    let v = !panel.config.enable_lyrics.unwrap_or(true);
                                    panel.config.enable_lyrics = Some(v);
                                    self.state.status_msg = Some(if v {
                                        "Lyrics fetching enabled".to_string()
                                    } else {
                                        "Lyrics fetching disabled".to_string()
                                    });
                                }
                                1 => {
                                    self.state.show_visualizer = !self.state.show_visualizer;
                                    panel.config.show_visualizer = Some(self.state.show_visualizer);
                                    self.state.status_msg = Some(if self.state.show_visualizer {
                                        "Visualizer enabled".to_string()
                                    } else {
                                        "Visualizer disabled".to_string()
                                    });
                                }
                                2 => {
                                    self.state.compact_mode = !self.state.compact_mode;
                                    panel.config.compact_mode_default =
                                        Some(self.state.compact_mode);
                                    self.state.status_msg = Some(if self.state.compact_mode {
                                        "Compact mode on".to_string()
                                    } else {
                                        "Compact mode off".to_string()
                                    });
                                }
                                3 => {
                                    self.state.show_breadcrumb = !self.state.show_breadcrumb;
                                    self.state.status_msg = Some(if self.state.show_breadcrumb {
                                        "Breadcrumb on".to_string()
                                    } else {
                                        "Breadcrumb off".to_string()
                                    });
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    },
                    PanelAction::ClearAllCache => {
                        let _ = panel.cache_manager.clear_all().await;
                        panel.cache_stats = Some(panel.cache_manager.get_stats().await);
                        self.state.status_msg = Some("All caches cleared".to_string());
                    }
                    PanelAction::CleanupExpired => {
                        let _ = panel.cache_manager.cleanup_expired().await;
                        panel.cache_stats = Some(panel.cache_manager.get_stats().await);
                        self.state.status_msg =
                            Some("Expired cache entries cleaned up".to_string());
                    }
                    PanelAction::RefreshStats => {
                        panel.load_cache_stats().await;
                        self.state.status_msg = Some("Cache stats refreshed".to_string());
                    }
                    PanelAction::RefreshPlaylists => {
                        if self.spotify.authenticated {
                            match self.spotify.fetch_playlists().await {
                                Ok(playlists) => {
                                    self.state.playlists = playlists;
                                    if !self.state.playlists.is_empty() {
                                        self.state.playlist_list.select(Some(0));
                                    }
                                    self.state.status_msg = Some("Playlists refreshed".to_string());
                                }
                                Err(e) => {
                                    self.state.status_msg =
                                        Some(format!("Failed to refresh playlists: {e}"));
                                }
                            }
                        } else {
                            self.state.status_msg =
                                Some("Not authenticated with Spotify".to_string());
                        }
                    }
                    PanelAction::None => {}
                }
                return Ok(());
            }
        }

        match code {
            KeyCode::Left | KeyCode::Right => {
                let now = Instant::now();
                let is_held = self
                    .last_seek_time
                    .map(|t| t.elapsed() < Duration::from_millis(300))
                    .unwrap_or(false);

                if is_held {
                    self.seek_hold_count += 1;
                } else {
                    self.seek_hold_count = 0;
                }
                self.last_seek_time = Some(now);

                let step_ms = if self.seek_hold_count > 4 {
                    10_000
                } else {
                    5_000
                };

                let new_pos = match code {
                    KeyCode::Right => (self.state.playback.progress_ms + step_ms)
                        .min(self.state.playback.duration_ms),
                    _ => self.state.playback.progress_ms.saturating_sub(step_ms),
                };

                self.state.playback.progress_ms = new_pos;
                self.progress_at_play_start = new_pos;
                if self.state.playback.is_playing {
                    self.playing_started_at = Some(Instant::now());
                }
                let _ = self.seek_tx.send(new_pos as u32);
                return Ok(());
            }
            _ => {}
        }

        if let Some(action) = self.keybinds.lookup(code, modifiers) {
            self.dispatch(action).await;
        }

        Ok(())
    }

    pub async fn dispatch(&mut self, action: crate::keybinds::Action) {
        use crate::keybinds::Action as A;
        match action {
            A::PlayPause => {
                if self.state.playback.is_playing {
                    if let Some(player) = &mut self.player {
                        player.pause();
                    }
                    self.state.playback.is_playing = false;
                } else if let Some(player) = &mut self.player {
                    player.play();
                    self.state.playback.is_playing = true;
                } else {
                    if !self.ensure_spotify_player().await {
                        self.ensure_local_player().await;
                    }
                    if let Some(player) = &mut self.player {
                        if !player.is_playing() {
                            player.play();
                        }
                        self.state.playback.is_playing = true;
                    } else if self.spotify.authenticated {
                        let _ = self.spotify.toggle_playback().await;
                    }
                }
            }
            A::NextTrack => {
                if self.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player {
                    if player.next() {
                        self.sync_track_selection();
                        self.sync_queue_display();
                    }
                } else if self.spotify.authenticated {
                    let _ = self.spotify.next_track().await;
                }
            }
            A::PrevTrack => {
                if self.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player {
                    if player.prev() {
                        self.sync_track_selection();
                        self.sync_queue_display();
                    }
                } else if self.spotify.authenticated {
                    let _ = self.spotify.prev_track().await;
                }
            }
            A::VolumeUp => {
                if let Some(player) = &mut self.player {
                    player.volume_up();
                    self.state.playback.volume = player.volume();
                }
                self.saved_volume = self.state.playback.volume;
            }
            A::VolumeDown => {
                if let Some(player) = &mut self.player {
                    player.volume_down();
                    self.state.playback.volume = player.volume();
                }
                self.saved_volume = self.state.playback.volume;
            }
            A::SeekForward => {}
            A::SeekBackward => {}
            A::ToggleShuffle => {
                if self.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player {
                    player.toggle_shuffle();
                    self.state.playback.shuffle = player.shuffle();
                }
            }
            A::CycleRepeat => {
                if self.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player {
                    player.cycle_repeat();
                    self.state.playback.repeat = match player.repeat() {
                        RepeatMode::Off => crate::spotify::RepeatState::Off,
                        RepeatMode::Queue => crate::spotify::RepeatState::Context,
                        RepeatMode::Track => crate::spotify::RepeatState::Track,
                    };
                }
            }
            A::ToggleRadio => {
                self.state.playback.radio_mode = !self.state.playback.radio_mode;
                if self.state.playback.radio_mode {
                    self.state.status_msg = Some("Radio mode on".to_string());
                } else {
                    self.state.status_msg = Some("Radio mode off".to_string());
                }
            }
            A::GetRecommendations => {
                self.get_similar_tracks().await;
            }
            A::LikeTrack => {
                if !self.spotify.authenticated {
                    self.state.status_msg = Some("Spotify not connected".to_string());
                } else if self.current_track_uri.is_empty() {
                    self.debug_overlay.log(
                        LogLevel::Warn,
                        "LikeTrack: no current track URI".to_string(),
                    );
                    self.state.status_msg = Some("No track to like".to_string());
                } else {
                    self.state.status_msg = Some("Liking...".to_string());
                    let Some(token) = self.spotify.get_access_token().await else {
                        self.state.status_msg = Some("Like failed: no token".to_string());
                        return;
                    };
                    let track_id = self
                        .current_track_uri
                        .split(':')
                        .last()
                        .unwrap_or("")
                        .to_string();
                    if track_id.is_empty() {
                        self.state.status_msg = Some("Like failed: empty track ID".to_string());
                        return;
                    }
                    match crate::spotify::save_track_http(&self.spotify.http, &token, &track_id)
                        .await
                    {
                        Ok(_) => {
                            self.state.status_msg = Some("Liked".to_string());
                            self.spotify.library_cache.delete_key_pattern("liked:%");
                            tracing::info!("LikeTrack: saved successfully — liked cache cleared");
                        }
                        Err(e) => {
                            self.state.status_msg = Some(format!("Like failed: {e}"));
                            tracing::error!("LikeTrack failed: {e}");
                        }
                    }
                }
            }
            A::AddToQueue => {
                let track = if self.state.active_content == ActiveContent::LocalFiles {
                    self.state.local_tree_list.selected().and_then(|vi| {
                        let actual_vi = self.state.sorted_track_indices.get(vi)?;
                        self.state
                            .local_tree
                            .get_visible(*actual_vi)
                            .and_then(|n| n.track().cloned())
                            .map(|t| (t.uri, t.name, t.artist, t.duration_ms, t.cover_path))
                    })
                } else {
                    self.state.track_list.selected().and_then(|display_idx| {
                        let actual_idx = self.state.sorted_track_indices.get(display_idx)?;
                        self.state.tracks.get(*actual_idx).map(|t| {
                            (
                                t.uri.clone(),
                                t.name.clone(),
                                t.artist.clone(),
                                t.duration_ms,
                                t.cover_path.clone(),
                            )
                        })
                    })
                };
                if let Some((uri, name, artist, duration_ms, cover_path)) = track {
                    let is_local = uri.starts_with("file://");
                    let target = if is_local == self.local_active {
                        self.player.as_mut()
                    } else {
                        self.parked_player.as_mut()
                    };
                    if let Some(player) = target {
                        player.add_to_queue(
                            uri,
                            name.clone(),
                            artist,
                            duration_ms,
                            cover_path.map(std::path::PathBuf::from),
                        );
                        self.state.status_msg = Some(format!("+ {name} added to queue"));
                        self.sync_queue_display();
                    }
                }
            }
            A::RemoveFromQueue => {
                if self.state.focus == Focus::Queue {
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
            }
            A::SortTracks => {
                if matches!(
                    self.state.active_content,
                    ActiveContent::Tracks | ActiveContent::None
                ) {
                    self.state.sort_tracks();
                    self.state.status_msg =
                        Some(format!("Sorting by: {}", self.state.track_sort_by.label()));
                }
            }
            A::NavUp => {
                if !self.state.fullscreen_player {
                    self.state.nav_up();
                }
            }
            A::NavDown => {
                if !self.state.fullscreen_player {
                    self.state.nav_down();
                    self.maybe_load_more().await;
                }
            }
            A::NavFirst => {
                if !self.state.fullscreen_player {
                    self.state.nav_first();
                }
            }
            A::NavLast => {
                if !self.state.fullscreen_player {
                    self.state.nav_last();
                    self.maybe_load_more().await;
                }
            }
            A::TabNext => {
                if self.state.fullscreen_player {
                    // no-op
                } else if self.state.focus == Focus::Search {
                    self.state.switch_search_panel();
                } else {
                    self.state.switch_focus();
                }
            }
            A::TabPrev => {
                if self.state.fullscreen_player {
                    // no-op
                } else if self.state.focus == Focus::Search {
                    self.state.switch_search_panel_prev();
                } else {
                    self.state.switch_focus_prev();
                }
            }
            A::Enter => self.handle_enter().await,
            A::Back => {
                if self.state.quick_search_active {
                    self.state.cancel_quick_search();
                } else if self.state.fullscreen_player {
                    self.state.fullscreen_player = false;
                } else if self.state.search_results.is_some() {
                    self.state.search_results = None;
                    self.state.previous_search = None;
                    self.state.active_content = ActiveContent::None;
                    self.state.focus = Focus::Library;
                } else if let Some(entry) = self.state.pop_nav() {
                    self.state.active_content = entry.active_content;
                    self.state.focus = entry.focus;
                    self.state.active_playlist_uri = entry.active_playlist_uri;
                    self.state.active_playlist_id = entry.active_playlist_id;
                    self.state.active_artist_name = entry.active_artist_name;
                    self.state.search_results = entry.search_results;
                    self.state.previous_search = entry.previous_search;
                    self.state.tracks = entry.tracks;
                    self.state.sorted_track_indices = entry.sorted_track_indices;
                    self.state.track_sort_by = entry.track_sort_by;
                } else if self.state.compact_effective
                    && self.state.active_content != ActiveContent::None
                {
                    self.state.active_content = ActiveContent::None;
                }
            }
            A::Search => self.state.start_search(),
            A::QuickSearch => {
                self.state.start_quick_search();
                self.state.apply_quick_filter();
            }
            A::Help => {
                let raw = self.keybinds.format_help_text();
                let mut lines = Vec::new();
                for (cat, entries) in &raw {
                    lines.push(format!("#{}", cat));
                    for entry in entries {
                        lines.push(format!("  {}", entry));
                    }
                    lines.push(String::new());
                }
                if let Some(ref mut panel) = self.options_panel {
                    panel.set_help_text(lines);
                    panel.focused_section = crate::ui::options::OptionsSection::Help;
                    panel.selected_item = 0;
                    if !panel.visible {
                        panel.visible = true;
                        self.state.status_msg = Some("Help — Options panel".to_string());
                    }
                }
            }
            A::ToggleCompact => {
                self.state.compact_mode = !self.state.compact_mode;
                if self.state.compact_mode
                    && matches!(
                        self.state.focus,
                        Focus::Library | Focus::Playlists | Focus::Queue
                    )
                {
                    self.state.focus = Focus::Tracks;
                }
                self.state.status_msg = Some(if self.state.compact_mode {
                    "Compact mode on".to_string()
                } else {
                    "Compact mode off".to_string()
                });
            }
            A::ToggleFullscreen => {
                if !self.state.playback.title.is_empty() {
                    self.state.fullscreen_player = !self.state.fullscreen_player;
                }
            }
            A::ToggleVisualizer => {
                self.state.show_visualizer = !self.state.show_visualizer;
                if let Some(player) = &mut self.player {
                    player.set_visualizer_enabled(self.state.show_visualizer);
                }
            }
            A::ToggleLyrics => {
                self.state.show_lyrics = !self.state.show_lyrics;
                if self.state.show_lyrics {
                    self.ensure_lyrics();
                }
                self.state.status_msg = Some(if self.state.show_lyrics {
                    "Lyrics panel on".to_string()
                } else {
                    "Lyrics panel off".to_string()
                });
            }
            A::OptionsPanel => {
                if let Some(ref mut panel) = self.options_panel {
                    panel.toggle().await;
                    self.state.status_msg = Some(if panel.visible {
                        "Options panel opened".to_string()
                    } else {
                        "Options panel closed".to_string()
                    });
                }
            }
            A::CopyTrackLink => {
                let url = self
                    .current_track_uri
                    .strip_prefix("spotify:track:")
                    .map(|id| format!("https://open.spotify.com/track/{id}"))
                    .unwrap_or_default();
                if url.is_empty() {
                    self.state.status_msg = Some("No track playing".to_string());
                    self.debug_overlay
                        .log(LogLevel::Warn, "CopyTrackLink: no track playing");
                } else {
                    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
                    let cmd = if wayland { "wl-copy" } else { "xclip" };
                    let args: &[&str] = if wayland {
                        &[]
                    } else {
                        &["-selection", "clipboard"]
                    };
                    let mut child = match std::process::Command::new(cmd)
                        .args(args)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            self.state.status_msg =
                                Some(format!("Copy failed: {cmd} not found ({e})"));
                            self.debug_overlay.log(
                                LogLevel::Error,
                                format!("CopyTrackLink: failed to spawn {cmd}: {e}"),
                            );
                            return;
                        }
                    };
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(url.as_bytes());
                    }
                    match child.wait() {
                        Ok(status) if status.success() => {
                            self.state.status_msg = Some(format!("Link copied: {url}"));
                            self.debug_overlay
                                .log(LogLevel::Info, format!("CopyTrackLink: copied {url}"));
                        }
                        Ok(status) => {
                            self.state.status_msg =
                                Some(format!("Copy failed: {cmd} exited with {status}"));
                            self.debug_overlay.log(
                                LogLevel::Error,
                                format!("CopyTrackLink: {cmd} exited with {status}"),
                            );
                        }
                        Err(e) => {
                            self.state.status_msg = Some(format!("Copy failed: {cmd} error ({e})"));
                            self.debug_overlay.log(
                                LogLevel::Error,
                                format!("CopyTrackLink: {cmd} wait error: {e}"),
                            );
                        }
                    }
                }
            }
            A::ToggleBreadcrumb => {
                self.state.show_breadcrumb = !self.state.show_breadcrumb;
                self.state.status_msg = Some(if self.state.show_breadcrumb {
                    "Breadcrumb on".to_string()
                } else {
                    "Breadcrumb off".to_string()
                });
            }
            A::ToggleDebug => {
                self.debug_overlay.toggle_visible();
            }
            A::ScrollUp => {
                if (self.state.fullscreen_player || self.state.show_lyrics)
                    && self
                        .state
                        .playback
                        .lyrics
                        .as_ref()
                        .map(|l| !l.is_synced)
                        .unwrap_or(false)
                {
                    self.state.playback.lyrics_scroll =
                        self.state.playback.lyrics_scroll.saturating_sub(4);
                }
            }
            A::ScrollDown => {
                if (self.state.fullscreen_player || self.state.show_lyrics)
                    && self
                        .state
                        .playback
                        .lyrics
                        .as_ref()
                        .map(|l| !l.is_synced)
                        .unwrap_or(false)
                {
                    self.state.playback.lyrics_scroll =
                        self.state.playback.lyrics_scroll.saturating_add(4);
                }
            }
            A::Quit => {
                self.should_quit = true;
            }
        }
    }

    pub async fn handle_enter(&mut self) {
        let mut needs_reconnect = false;

        if self.state.compact_effective && self.state.active_content == ActiveContent::None {
            if let Some(pos) = self.state.library_list.selected() {
                match self.state.compact_item_at(pos) {
                    Some(CompactItem::LibraryItem(idx)) => {
                        if self.handle_library_item(idx).await {
                            if !self.session_reconnecting {
                                self.session_reconnecting = true;
                                self.reconnect_player().await;
                            }
                        }
                    }
                    Some(CompactItem::PlaylistItem(idx)) => {
                        if self.handle_playlist_item(idx).await {
                            if !self.session_reconnecting {
                                self.session_reconnecting = true;
                                self.reconnect_player().await;
                            }
                        }
                    }
                    None => {}
                }
            }
            return;
        }

        match self.state.focus {
            Focus::Library => {
                let idx = match self.state.library_list.selected() {
                    Some(i) => i,
                    None => return,
                };
                if self.handle_library_item(idx).await {
                    needs_reconnect = true;
                }
            }

            Focus::Playlists => {
                if let Some(idx) = self.state.playlist_list.selected() {
                    if idx < self.state.playlists.len() {
                        if self.handle_playlist_item(idx).await {
                            needs_reconnect = true;
                        }
                    }
                }
            }

            Focus::Tracks => match &self.state.active_content {
                ActiveContent::Albums => {
                    if let Some(idx) = self.state.selected_album_index() {
                        if let Some(album) = self.state.albums.get(idx) {
                            let id = album.id.clone();
                            let name = album.name.clone();
                            self.state.push_nav();
                            self.state.status_msg = Some(format!("Loading {name}…"));
                            tokio::time::sleep(Duration::from_millis(100)).await;
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
                                    self.state.rebuild_sort_indices();
                                    self.state.status_msg = None;
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                        || err_str.contains("401")
                                    {
                                        warn!("Got 401 - triggering reconnect");
                                        needs_reconnect = true;
                                        self.state.status_msg = Some(
                                            "Authorization expired, reconnecting...".to_string(),
                                        );
                                    } else {
                                        self.state.status_msg = Some(format!("Error: {e}"));
                                    }
                                }
                            }
                        }
                    }
                }
                ActiveContent::Artists => {
                    if let Some(idx) = self.state.selected_artist_index() {
                        if let Some(artist) = self.state.artists.get(idx) {
                            let id = artist.uri.trim_start_matches("spotify:artist:").to_string();
                            let name = artist.name.clone();
                            self.state.push_nav();
                            self.state.status_msg = Some(format!("Loading top tracks for {name}…"));
                            tokio::time::sleep(Duration::from_millis(100)).await;
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
                                    self.state.rebuild_sort_indices();
                                    self.state.status_msg = None;
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                        || err_str.contains("401")
                                    {
                                        warn!("Got 401 - triggering reconnect");
                                        needs_reconnect = true;
                                        self.state.status_msg = Some(
                                            "Authorization expired, reconnecting...".to_string(),
                                        );
                                    } else {
                                        self.state.status_msg = Some(format!("Error: {e}"));
                                    }
                                }
                            }
                        }
                    }
                }
                ActiveContent::Shows => {
                    if let Some(idx) = self.state.selected_show_index() {
                        if let Some(show) = self.state.shows.get(idx) {
                            let id = show.id.clone();
                            let name = show.name.clone();
                            self.state.push_nav();
                            self.state.status_msg = Some(format!("Loading {name}…"));
                            tokio::time::sleep(Duration::from_millis(100)).await;
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
                                    self.state.rebuild_sort_indices();
                                    self.state.status_msg = None;
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                        || err_str.contains("401")
                                    {
                                        warn!("Got 401 - triggering reconnect");
                                        needs_reconnect = true;
                                        self.state.status_msg = Some(
                                            "Authorization expired, reconnecting...".to_string(),
                                        );
                                    } else {
                                        self.state.status_msg = Some(format!("Error: {e}"));
                                    }
                                }
                            }
                        }
                    }
                }
                ActiveContent::LocalFiles => {
                    let vi = match self.state.local_tree_list.selected() {
                        Some(i) => i,
                        None => return,
                    };

                    let actual_vi = match self.state.sorted_track_indices.get(vi) {
                        Some(&idx) => idx,
                        None => return,
                    };
                    let node = match self.state.local_tree.get_visible(actual_vi) {
                        Some(n) => n.clone(),
                        None => return,
                    };
                    match node {
                        crate::ui::LocalNode::Folder { .. } => {
                            self.state.local_tree.toggle_folder(actual_vi);
                            self.state.apply_quick_filter();
                            let new_len = self.state.sorted_track_indices.len();
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

                                self.state.playback.progress_ms = 0;
                                self.scrobble_sent = false;
                                self.track_start_unix = crate::app::metadata::unix_now();
                            }
                        }
                    }
                }
                ActiveContent::Tracks | ActiveContent::None => {
                    if let Some(display_idx) = self.state.selected_track_index() {
                        let actual_idx = match self.state.sorted_track_indices.get(display_idx) {
                            Some(&idx) => idx,
                            None => return,
                        };

                        if self.spotify_streaming_disabled {
                            self.state.status_msg =
                                Some("Spotify Premium required for streaming".to_string());
                            return;
                        }
                        self.state.cancel_quick_search();
                        self.activate_spotify_player();
                        self.ensure_spotify_player().await;
                        if self
                            .state
                            .tracks
                            .get(actual_idx)
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
                            let adjusted_idx = self.state.tracks[..actual_idx]
                                .iter()
                                .filter(|t| !t.uri.starts_with("spotify:episode:"))
                                .count();
                            player.set_queue(uris, adjusted_idx);
                            self.playing_tracks = self.state.tracks.clone();
                            if let Some(track) = self.state.tracks.get(actual_idx) {
                                self.state.playback.title = track.name.clone();
                                self.state.playback.artist = track.artist.clone();
                                self.state.playback.album = track.album.clone();
                                self.state.playback.duration_ms = track.duration_ms;
                                self.state.playback.art_url = track.cover_path.clone();
                                self.state.playback.progress_ms = 0;
                                self.state.playback.is_playing = true;
                                self.state.playback.is_local = false;
                                self.current_track_uri = track.uri.clone();
                                self.on_track_started();
                            }
                        } else if self.spotify.authenticated {
                            let track_uri = self.state.tracks[actual_idx].uri.clone();
                            let is_playlist = self
                                .state
                                .active_playlist_uri
                                .as_deref()
                                .map(|u| u != "liked_songs" && !u.starts_with("search:"))
                                .unwrap_or(false);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            let result = if is_playlist {
                                let uri = self.state.active_playlist_uri.clone().unwrap();
                                self.spotify.play_in_context(&uri, &track_uri).await
                            } else {
                                self.spotify.play_track_uri(&track_uri).await
                            };
                            if let Err(e) = result {
                                let err_str = e.to_string();
                                if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                    || err_str.contains("401")
                                {
                                    warn!("Got 401 - triggering reconnect");
                                    needs_reconnect = true;
                                    self.state.status_msg =
                                        Some("Authorization expired, reconnecting...".to_string());
                                } else {
                                    self.state.status_msg = Some(format!("Error: {e}"));
                                }
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
                                    Some("Spotify Premium required for streaming".to_string());
                                return;
                            }
                            self.activate_spotify_player();
                            self.ensure_spotify_player().await;
                            if let Some(player) = &mut self.player {
                                self.current_track_uri = track_uri.clone();
                                tokio::time::sleep(Duration::from_millis(100)).await;
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
                                                    cover_path: t.cover_path.clone(),
                                                }];
                                            self.on_track_started();
                                        }
                                    }
                                }
                            } else if self.spotify.authenticated {
                                let _ = self.spotify.play_track_uri(&track_uri).await;
                            }
                            self.state.focus = Focus::Tracks;
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
                            tokio::time::sleep(Duration::from_millis(100)).await;
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
                                    self.state.push_nav();
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.rebuild_sort_indices();
                                    self.state.previous_search = self.state.search_results.take();
                                    self.state.status_msg = None;
                                    self.state.focus = Focus::Tracks;
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                        || err_str.contains("401")
                                    {
                                        warn!("Got 401 - triggering reconnect");
                                        needs_reconnect = true;
                                        self.state.status_msg = Some(
                                            "Authorization expired, reconnecting...".to_string(),
                                        );
                                    } else {
                                        self.state.status_msg = Some(format!("Error: {e}"));
                                    }
                                }
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
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            match self.spotify.fetch_playlist_tracks(&id, 0).await {
                                Ok((tracks, total)) => {
                                    if tracks.is_empty() {
                                        self.state.status_msg = Some(
                                            "Playlist tracks not available for playlists you don't own or collaborate on".to_string(),
                                        );
                                    } else {
                                        self.state.tracks = tracks;
                                        self.state.tracks_total = total;
                                        self.state.tracks_offset = self.state.tracks.len() as u32;
                                        self.state.active_playlist_uri = Some(uri);
                                        self.state.active_playlist_id = Some(id);
                                        self.state.track_list.select(Some(0));
                                        self.state.push_nav();
                                        self.state.active_content = ActiveContent::Tracks;
                                        self.state.rebuild_sort_indices();
                                        self.state.previous_search =
                                            self.state.search_results.take();
                                        self.state.status_msg = None;
                                        self.state.focus = Focus::Tracks;
                                    }
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                        || err_str.contains("401")
                                    {
                                        warn!("Got 401 - triggering reconnect");
                                        needs_reconnect = true;
                                        self.state.status_msg = Some(
                                            "Authorization expired, reconnecting...".to_string(),
                                        );
                                    } else if err_str.contains("SPOTIFY_PLAYLIST_NOT_ACCESSIBLE") {
                                        self.state.status_msg = Some(
                                            "Playlist tracks not available for playlists you don't own or collaborate on".to_string(),
                                        );
                                    } else {
                                        self.state.status_msg = Some(format!("Error: {e}"));
                                    }
                                }
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
                            tokio::time::sleep(Duration::from_millis(100)).await;
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
                                    self.state.push_nav();
                                    self.state.active_content = ActiveContent::Tracks;
                                    self.state.rebuild_sort_indices();
                                    self.state.previous_search = self.state.search_results.take();
                                    self.state.status_msg = None;
                                    self.state.focus = Focus::Tracks;
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if err_str.contains("SPOTIFY_UNAUTHORIZED")
                                        || err_str.contains("401")
                                    {
                                        warn!("Got 401 - triggering reconnect");
                                        needs_reconnect = true;
                                        self.state.status_msg = Some(
                                            "Authorization expired, reconnecting...".to_string(),
                                        );
                                    } else {
                                        self.state.status_msg = Some(format!("Error: {e}"));
                                    }
                                }
                            }
                        }
                    }
                    None => {}
                }
            }
            Focus::Queue => {
                if let Some(idx) = self.state.queue_list.selected() {
                    let active_len = self
                        .player
                        .as_ref()
                        .map(|p| p.user_queue().len())
                        .unwrap_or(0);
                    let queued = if idx < active_len {
                        self.player
                            .as_mut()
                            .and_then(|p| p.user_queue().get(idx).cloned())
                    } else {
                        let parked_idx = idx - active_len;
                        self.parked_player
                            .as_mut()
                            .and_then(|p| p.user_queue().get(parked_idx).cloned())
                    };
                    if let Some(qt) = queued {
                        let is_local = qt.uri.starts_with("file://");
                        if is_local {
                            self.activate_local_player();
                        } else if !self.local_active {
                            // já está no spotify player
                        } else {
                            self.activate_spotify_player();
                        }
                        if let Some(player) = &mut self.player {
                            player.set_queue(vec![qt.uri.clone()], 0);
                            self.playing_tracks = vec![];
                            self.sync_track_selection();
                            self.sync_queue_display();
                        }
                    }
                }
            }
        }

        if needs_reconnect && !self.session_reconnecting {
            warn!("Triggering reconnect due to 401");
            self.session_reconnecting = true;
            self.reconnect_player().await;
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/handlers.rs"]
mod tests;
