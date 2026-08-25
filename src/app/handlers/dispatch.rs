use std::time::Instant;

use crate::App;
use crate::player::RepeatMode;
use crate::ui::{ActiveContent, Focus};

impl App {
    pub async fn dispatch(&mut self, action: crate::keybinds::Action) {
        use crate::keybinds::Action as A;
        match action {
            A::PlayPause => {
                if self.state.playback.is_playing {
                    if let Some(player) = &mut self.player_mgr.player {
                        player.pause();
                    }
                    self.state.playback.is_playing = false;
                } else if let Some(player) = &mut self.player_mgr.player {
                    player.play();
                    self.state.playback.is_playing = true;
                } else {
                    if !self.ensure_spotify_player().await {
                        self.ensure_local_player().await;
                    }
                    if let Some(player) = &mut self.player_mgr.player {
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
                if self.player_mgr.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player_mgr.player {
                    if player.next() {
                        self.sync_track_selection();
                        self.sync_queue_display();
                    }
                } else if self.spotify.authenticated {
                    let _ = self.spotify.next_track().await;
                }
            }
            A::PrevTrack => {
                if self.player_mgr.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player_mgr.player {
                    if player.prev() {
                        self.sync_track_selection();
                        self.sync_queue_display();
                    }
                } else if self.spotify.authenticated {
                    let _ = self.spotify.prev_track().await;
                }
            }
            A::VolumeUp => {
                if let Some(player) = &mut self.player_mgr.player {
                    player.volume_up();
                    self.state.playback.volume = player.volume();
                }
                self.player_mgr.saved_volume = self.state.playback.volume;
            }
            A::VolumeDown => {
                if let Some(player) = &mut self.player_mgr.player {
                    player.volume_down();
                    self.state.playback.volume = player.volume();
                }
                self.player_mgr.saved_volume = self.state.playback.volume;
            }
            A::SeekForward => {}
            A::SeekBackward => {}
            A::SeekMiddle => {
                let new_pos = self.state.playback.duration_ms / 2;
                self.state.playback.progress_ms = new_pos;
                self.player_mgr.progress_at_play_start = new_pos;
                if self.state.playback.is_playing {
                    self.player_mgr.playing_started_at = Some(Instant::now());
                }
                let _ = self.seek_tx.send(new_pos as u32);
            }
            A::ToggleShuffle => {
                if self.player_mgr.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player_mgr.player {
                    player.toggle_shuffle();
                    self.state.playback.shuffle = player.shuffle();
                }
            }
            A::CycleRepeat => {
                if self.player_mgr.player.is_none() {
                    self.ensure_spotify_player().await;
                }
                if let Some(player) = &mut self.player_mgr.player {
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
                self.dispatch_like_track().await;
            }
            A::AddToQueue => {
                let track = if self.state.active_content == ActiveContent::LocalFiles {
                    self.state.local_tree_list.selected().and_then(|vi| {
                        let actual_vi = self.state.sorted_track_indices.get(vi)?;
                        self.state
                            .local_tree
                            .get_visible(*actual_vi)
                            .and_then(|n| n.track().cloned())
                            .map(|t| {
                                (
                                    t.uri,
                                    t.name,
                                    t.artist,
                                    t.album,
                                    t.duration_ms,
                                    t.cover_path,
                                )
                            })
                    })
                } else {
                    self.state.track_list.selected().and_then(|display_idx| {
                        let actual_idx = self.state.sorted_track_indices.get(display_idx)?;
                        self.state.tracks.get(*actual_idx).map(|t| {
                            (
                                t.uri.clone(),
                                t.name.clone(),
                                t.artist.clone(),
                                t.album.clone(),
                                t.duration_ms,
                                t.cover_path.clone(),
                            )
                        })
                    })
                };
                if let Some((uri, name, artist, album, duration_ms, cover_path)) = track {
                    let is_local = uri.starts_with("file://");
                    let target = if is_local == self.player_mgr.local_active {
                        self.player_mgr.player.as_mut()
                    } else {
                        self.player_mgr.parked_player.as_mut()
                    };
                    if let Some(player) = target {
                        player.add_to_queue(
                            uri,
                            name.clone(),
                            artist,
                            album,
                            duration_ms,
                            cover_path.map(std::path::PathBuf::from),
                        );
                        self.state.status_msg = Some(format!("+ {name} added to queue"));
                        self.sync_queue_display();
                    }
                }
            }
            A::RemoveFromQueue => {
                if self.state.focus == Focus::Queue
                    && let Some(idx) = self.state.queue_list.selected()
                {
                    let active_len = self
                        .player_mgr
                        .player
                        .as_ref()
                        .map(|p| p.user_queue().len())
                        .unwrap_or(0);
                    if idx < active_len {
                        if let Some(player) = &mut self.player_mgr.player {
                            player.remove_from_user_queue(idx);
                        }
                    } else {
                        let parked_idx = idx - active_len;
                        if let Some(player) = &mut self.player_mgr.parked_player
                            && parked_idx < player.user_queue().len()
                        {
                            player.remove_from_user_queue(parked_idx);
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
                    let at_end = self.current_list_at_end();
                    if at_end {
                        if self.current_list_loading() {
                            return;
                        }
                        self.maybe_load_more().await;
                        if self.fetcher.pending_pagination.is_some() {
                            self.fetcher.pending_nav_down = true;
                            return;
                        }
                    }
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
            A::NavMiddle => {
                if !self.state.fullscreen_player {
                    self.state.nav_middle();
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
                if let Some(ref mut panel) = self.settings_panel {
                    panel.set_help_text(lines);
                    panel.focused_section = crate::ui::options::SettingsSection::Help;
                    panel.selected_item = 0;
                    if !panel.visible {
                        panel.visible = true;
                        self.state.status_msg = Some("Help — Settings panel".to_string());
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
                if let Some(player) = &mut self.player_mgr.player {
                    player.set_visualizer_enabled(self.state.show_visualizer);
                }
                self.player_mgr.band_energies = self
                    .player_mgr
                    .player
                    .as_ref()
                    .and_then(|p| p.band_energies());
            }
            A::ToggleLyrics => {
                self.state.show_lyrics = !self.state.show_lyrics;
                if self.state.show_lyrics {
                    self.fetcher.ensure_lyrics(&self.debug_overlay);
                }
                self.state.status_msg = Some(if self.state.show_lyrics {
                    "Lyrics panel on".to_string()
                } else {
                    "Lyrics panel off".to_string()
                });
            }
            A::OptionsPanel => {
                if let Some(ref mut panel) = self.settings_panel {
                    panel.toggle().await;
                    if panel.visible {
                        let raw = self.keybinds.format_help_text();
                        let mut lines = Vec::new();
                        for (cat, entries) in &raw {
                            lines.push(format!("#{}", cat));
                            for entry in entries {
                                lines.push(format!("  {}", entry));
                            }
                            lines.push(String::new());
                        }
                        panel.set_help_text(lines);
                    }
                    self.state.status_msg = Some(if panel.visible {
                        "Settings panel opened".to_string()
                    } else {
                        "Settings panel closed".to_string()
                    });
                }
            }
            A::CopyTrackLink => {
                self.dispatch_copy_track_link().await;
            }
            A::ToggleBreadcrumb => {
                self.state.show_breadcrumb = !self.state.show_breadcrumb;
                self.state.status_msg = Some(if self.state.show_breadcrumb {
                    "Breadcrumb on".to_string()
                } else {
                    "Breadcrumb off".to_string()
                });
            }
            A::AddToPlaylist => {
                if self.state.playlists.is_empty() {
                    self.state.status_msg = Some("No playlists available".to_string());
                } else if self.current_track_uri.is_empty() {
                    self.state.status_msg = Some("No track playing".to_string());
                } else {
                    self.state.add_to_playlist_mode = true;
                    self.state.add_to_playlist_list.select(Some(0));
                }
            }
            A::RemoveFromPlaylist => {
                self.dispatch_remove_from_playlist().await;
            }
            A::CommandPrompt => {
                self.state.command_mode = true;
                self.state.command_buffer.clear();
            }
            A::DeletePlaylist => {
                if !self.spotify.authenticated {
                    self.state.status_msg =
                        Some("Spotify not connected - run: isi-music setup-spotify".to_string());
                } else if self.state.focus == crate::ui::Focus::Playlists {
                    let idx = self.state.playlist_list.selected();
                    let name = idx
                        .and_then(|i| self.state.playlists.get(i))
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    if name.is_empty() {
                        self.state.status_msg = Some("No playlist selected".to_string());
                    } else {
                        self.state.delete_playlist_confirm = true;
                        self.state.delete_playlist_target = Some(name);
                    }
                } else {
                    self.state.status_msg = Some("Focus on Playlists tab to delete".to_string());
                }
            }
            A::ToggleDebug => {
                self.debug_overlay.toggle_visible();
                self.needs_redraw = true;
                self.force_clear = true;
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
                } else if !self.state.fullscreen_player {
                    self.state.nav_up();
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
                } else if !self.state.fullscreen_player {
                    self.state.nav_down();
                    self.maybe_load_more().await;
                }
            }
            A::FocusLibrary => self.state.focus = crate::ui::Focus::Library,
            A::FocusPlaylists => self.state.focus = crate::ui::Focus::Playlists,
            A::FocusTracks => self.state.focus = crate::ui::Focus::Tracks,
            A::FocusQueue => self.state.focus = crate::ui::Focus::Queue,
            A::JumpToPlaying => self.jump_to_playing().await,
            A::Quit => {
                self.should_quit = true;
            }
        }
    }
}
