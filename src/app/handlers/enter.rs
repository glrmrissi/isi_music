use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

use crate::App;
use crate::app::FetchResult;
use crate::ui::{ActiveContent, CompactItem, Focus};

impl App {
    pub async fn handle_enter(&mut self) {
        let mut needs_reconnect = false;

        if self.state.compact_effective && self.state.active_content == ActiveContent::None {
            if let Some(pos) = self.state.library_list.selected() {
                match self.state.compact_item_at(pos) {
                    Some(CompactItem::LibraryItem(idx)) => {
                        if self.handle_library_item(idx).await {
                            if !self.player_mgr.session_reconnecting {
                                self.player_mgr.session_reconnecting = true;
                                self.reconnect_player().await;
                            }
                        }
                    }
                    Some(CompactItem::PlaylistItem(idx)) => {
                        if self.handle_playlist_item(idx).await {
                            if !self.player_mgr.session_reconnecting {
                                self.player_mgr.session_reconnecting = true;
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
                            self.state.loading = true;
                            self.state.active_playlist_uri = Some(format!("album:{id}"));
                            self.state.active_playlist_id = Some(format!("album:{id}"));
                            let spotify = Arc::clone(&self.spotify);
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            self.fetcher.pending_fetch = Some(rx);
                            tokio::spawn(async move {
                                let result = spotify
                                    .fetch_album_tracks(&id, 0)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = tx.send(FetchResult::AlbumTracks(result));
                            });
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
                            self.state.loading = true;
                            self.state.active_artist_name = Some(name.clone());
                            self.state.active_playlist_uri = Some(format!("artist:{id}"));
                            self.state.active_playlist_id = Some(format!("artist:{id}"));
                            let spotify = Arc::clone(&self.spotify);
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            self.fetcher.pending_fetch = Some(rx);
                            tokio::spawn(async move {
                                let result = spotify
                                    .fetch_artist_tracks(&name, 0)
                                    .await
                                    .map_err(|e| e.to_string());
                                let _ = tx.send(FetchResult::ArtistTracks(result));
                            });
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
                                    self.state.tracks_api_offset = self.state.tracks.len() as u32;
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
                            let new_pos = self
                                .state
                                .sorted_track_indices
                                .iter()
                                .position(|&idx| idx == actual_vi)
                                .unwrap_or(0);
                            self.state
                                .local_tree_list
                                .select(Some(new_pos.min(new_len.saturating_sub(1))));
                        }
                        crate::ui::LocalNode::Track { track, .. } => {
                            self.activate_local_player();
                            if !self.ensure_local_player().await {
                                self.state.status_msg =
                                    Some("Failed to initialize local player".to_string());
                                return;
                            }
                            let all_tracks = self.state.local_tree.all_tracks_flat();
                            let start_idx = all_tracks
                                .iter()
                                .position(|t| t.uri == track.uri)
                                .unwrap_or(0);
                            if let Some(player) = &mut self.player_mgr.player {
                                player.set_queue_tracks(&all_tracks, start_idx);
                                self.player_mgr.playing_tracks = all_tracks;
                                self.state.status_msg = Some(format!("Playing {}…", track.name));
                                self.state.playback.title = track.name.clone();
                                self.state.playback.artist = track.artist.clone();
                                self.state.playback.album = track.album.clone();
                                self.state.playback.duration_ms = track.duration_ms;
                                self.state.playback.progress_ms = 0;
                                self.state.playback.is_playing = true;
                                self.state.playback.is_local = true;
                                self.state.playback.cover_path = track.cover_path.clone();
                                self.current_track_uri = track.uri.clone();
                                self.on_track_started();

                                self.state.playback.progress_ms = 0;
                                self.integrations.reset_scrobble();
                                self.integrations
                                    .set_track_start(crate::app::metadata::unix_now());
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

                        if self.player_mgr.spotify_streaming_disabled {
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
                        } else if let Some(player) = &mut self.player_mgr.player {
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
                            if let Some(track) = self.state.tracks.get(actual_idx) {
                                self.state.status_msg = Some(format!("Playing {}…", track.name));
                            }
                            player.set_queue(uris, adjusted_idx);
                            self.player_mgr.playing_tracks = self.state.tracks.clone();
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
                                if let Some(uri) = self.state.active_playlist_uri.clone() {
                                    self.spotify.play_in_context(&uri, &track_uri).await
                                } else {
                                    self.spotify.play_track_uri(&track_uri).await
                                }
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
                self.handle_enter_search(&mut needs_reconnect).await;
            }
            Focus::Queue => {
                if let Some(idx) = self.state.queue_list.selected() {
                    let active_len = self
                        .player_mgr
                        .player
                        .as_ref()
                        .map(|p| p.user_queue().len())
                        .unwrap_or(0);
                    let is_active = idx < active_len;
                    let queue_idx = if is_active { idx } else { idx - active_len };

                    let played = if is_active {
                        self.player_mgr
                            .player
                            .as_mut()
                            .map(|p| p.play_from_user_queue(queue_idx))
                            .unwrap_or(false)
                    } else {
                        self.player_mgr
                            .parked_player
                            .as_mut()
                            .map(|p| p.play_from_user_queue(queue_idx))
                            .unwrap_or(false)
                    };

                    if played {
                        self.player_mgr.playing_tracks = vec![];
                        self.sync_track_selection();
                        self.sync_queue_display();
                    }
                }
            }
        }

        if needs_reconnect && !self.player_mgr.session_reconnecting {
            warn!("Triggering reconnect due to 401");
            self.player_mgr.session_reconnecting = true;
            self.reconnect_player().await;
        }
    }
}
