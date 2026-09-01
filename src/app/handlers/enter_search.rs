use std::time::Duration;
use tracing::{info, warn};

use crate::App;
use crate::ui::{ActiveContent, Focus, SearchPanel};

impl App {
    pub(super) async fn handle_enter_search(&mut self, needs_reconnect: &mut bool) {
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
                    let is_local = track_uri.starts_with("file://");
                    if is_local {
                        self.play_local_search_track(&track_uri).await;
                    } else {
                        if self.player_mgr.spotify_streaming_disabled {
                            self.state.status_msg =
                                Some("Spotify Premium required for streaming".to_string());
                            return;
                        }
                        self.activate_spotify_player();
                        self.ensure_spotify_player().await;
                        if let Some(player) = &mut self.player_mgr.player {
                            self.current_track_uri = track_uri.clone();
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            player.set_queue(vec![track_uri], 0);
                            if let Some(sr) = &self.state.search_results
                                && let Some(idx) = sr.track_list.selected()
                                && let Some(t) = sr.tracks.get(idx)
                            {
                                self.state.playback.title = t.name.clone();
                                self.state.playback.artist = t.artist.clone();
                                self.state.playback.album = t.album.clone();
                                self.state.playback.duration_ms = t.duration_ms;
                                self.state.playback.progress_ms = 0;
                                self.state.playback.is_playing = true;
                                self.state.playback.is_local = false;
                                self.player_mgr.playing_tracks =
                                    vec![crate::spotify::TrackSummary {
                                        uri: t.uri.clone(),
                                        name: t.name.clone(),
                                        artist: t.artist.clone(),
                                        album: t.album.clone(),
                                        duration_ms: t.duration_ms,
                                        cover_path: t.cover_path.clone(),
                                        added_at: None,
                                    }];
                                self.on_track_started();
                            }
                        } else if self.spotify.authenticated {
                            let _ = self.spotify.play_track_uri(&track_uri).await;
                        }
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
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    match self.spotify.fetch_album_tracks(&id, 0).await {
                        Ok((tracks, total)) => {
                            self.state.push_nav();
                            self.state.tracks = tracks;
                            self.state.tracks_total = total;
                            self.state.tracks_offset = self.state.tracks.len() as u32;
                            self.state.tracks_api_offset = self.state.tracks.len() as u32;
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
                            self.state.rebuild_sort_indices();
                            self.state.previous_search = self.state.search_results.take();
                            self.state.status_msg = None;
                            self.state.focus = Focus::Tracks;
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                                warn!("Got 401 - triggering reconnect");
                                *needs_reconnect = true;
                                self.state.status_msg =
                                    Some("Authorization expired, reconnecting...".to_string());
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
                    let web_result = self.spotify.fetch_playlist_tracks(&id, 0).await;
                    let loaded = match web_result {
                        Ok((t, total, pi)) if !t.is_empty() => Some((t, total, pi)),
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                                warn!("Got 401 - triggering reconnect");
                                *needs_reconnect = true;
                                self.state.status_msg =
                                    Some("Authorization expired, reconnecting...".to_string());
                                None
                            } else {
                                self.mercury_playlist_fallback(&name, &uri, &id).await
                            }
                        }
                        Ok(_) => self.mercury_playlist_fallback(&name, &uri, &id).await,
                    };
                    let Some((tracks, total, page_items)) = loaded else {
                        return;
                    };
                    self.state.push_nav();
                    self.state.tracks = tracks;
                    self.state.tracks_total = total;
                    self.state.tracks_offset = self.state.tracks.len() as u32;
                    self.state.tracks_api_offset = page_items;
                    self.state.active_playlist_uri = Some(uri);
                    self.state.active_playlist_id = Some(id);
                    self.state.track_list.select(Some(0));
                    self.state.active_content = ActiveContent::Tracks;
                    self.state.rebuild_sort_indices();
                    self.state.previous_search = self.state.search_results.take();
                    self.state.status_msg = None;
                    self.state.focus = Focus::Tracks;
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
                            self.state.push_nav();
                            self.state.tracks = tracks;
                            self.state.tracks_total = total;
                            self.state.tracks_offset = self.state.tracks.len() as u32;
                            self.state.tracks_api_offset = self.state.tracks.len() as u32;
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
                            self.state.previous_search = self.state.search_results.take();
                            self.state.status_msg = None;
                            self.state.focus = Focus::Tracks;
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            if err_str.contains("SPOTIFY_UNAUTHORIZED") || err_str.contains("401") {
                                warn!("Got 401 - triggering reconnect");
                                *needs_reconnect = true;
                                self.state.status_msg =
                                    Some("Authorization expired, reconnecting...".to_string());
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

    async fn play_local_search_track(&mut self, track_uri: &str) {
        self.activate_local_player();
        if !self.ensure_local_player().await {
            self.state.status_msg = Some("Failed to initialize local player".to_string());
            return;
        }
        let track = self
            .state
            .search_results
            .as_ref()
            .and_then(|sr| sr.tracks.iter().find(|t| t.uri == track_uri))
            .cloned();
        let Some(track) = track else {
            return;
        };
        let all_tracks = self.state.local_tree.all_tracks_flat();
        let start_idx = all_tracks
            .iter()
            .position(|t| t.uri == track_uri)
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
            self.integrations.reset_scrobble();
            self.integrations
                .set_track_start(crate::app::metadata::unix_now());
        }
    }

    async fn mercury_playlist_fallback(
        &mut self,
        name: &str,
        uri: &str,
        playlist_id: &str,
    ) -> Option<(Vec<crate::spotify::TrackSummary>, u32, u32)> {
        self.state.status_msg = Some(format!("Trying Mercury protocol for {name}…"));
        self.activate_spotify_player();
        self.ensure_spotify_player().await;
        let mercury_fut = self
            .player_mgr
            .player
            .as_ref()
            .map(|p| p.fetch_playlist_via_mercury(uri));
        match mercury_fut {
            Some(fut) => match fut.await {
                Ok(tracks) if !tracks.is_empty() => {
                    let total = tracks.len() as u32;
                    let key = format!("playlist:{playlist_id}:0");
                    self.spotify.library_cache.save_tracks(&key, &tracks, total);
                    info!(
                        "Mercury playlist saved to cache: {playlist_id} ({} tracks)",
                        tracks.len()
                    );
                    Some((tracks, total, total))
                }
                Ok(_) => {
                    self.state.status_msg = Some("Playlist is empty or not accessible".to_string());
                    None
                }
                Err(e) => {
                    warn!("Mercury playlist fetch failed: {e:#}");
                    self.state.status_msg = Some(format!("Could not load playlist: {e:#}"));
                    None
                }
            },
            None => {
                self.state.status_msg = Some("Spotify player not available".to_string());
                None
            }
        }
    }
}
