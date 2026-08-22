use crate::App;

impl App {
    pub(super) async fn jump_to_playing(&mut self) {
        let playing_title = self.state.playback.title.clone();
        let playing_artist = self.state.playback.artist.clone();
        let playing_uri = self.current_track_uri.clone();
        let is_local = self.state.playback.is_local;

        if playing_title.is_empty() && playing_uri.is_empty() {
            self.state.status_msg = Some("No track playing".to_string());
            return;
        }

        // Match function: try URI first, then title+artist fallback
        let matches = |t: &crate::spotify::TrackSummary| -> bool {
            if !playing_uri.is_empty() && t.uri == playing_uri {
                return true;
            }
            t.name == playing_title && (playing_artist.is_empty() || t.artist == playing_artist)
        };

        // First try the current view
        if let Some(target_vi) = self.find_track_in_current_view(&matches) {
            self.select_track_in_view(target_vi, is_local);
            self.state.status_msg = Some("Jumped to playing track".to_string());
            return;
        }

        // Not in current view — try to load the source context
        if is_local {
            // Switch to Local Files and try there
            self.state.active_content = crate::ui::ActiveContent::LocalFiles;
            self.state.active_playlist_uri = Some("local_files".to_string());
            self.state.active_playlist_id = Some("local_files".to_string());
            if let Some(target_vi) = self.find_track_in_current_view(&matches) {
                self.select_track_in_view(target_vi, true);
                self.state.status_msg = Some("Jumped to playing track".to_string());
                return;
            }
        } else if self.spotify.authenticated {
            self.state.status_msg = Some("Searching...".to_string());
            self.needs_redraw = true;

            // Fetch fresh playback to get context_uri
            let context_uri = if let Some(ref ctx) = self.state.playback.context_uri {
                Some(ctx.clone())
            } else if let Ok(pb) = self.spotify.fetch_playback().await {
                pb.context_uri
            } else {
                None
            };

            if let Some(ctx) = context_uri {
                if ctx.starts_with("spotify:playlist:") {
                    if let Some(p) = self
                        .state
                        .playlists
                        .iter()
                        .find(|p| p.uri == ctx || p.id == ctx)
                        .cloned()
                    {
                        self.load_and_search_playlist(&p.id, &p.uri, &p.name, &matches)
                            .await;
                        return;
                    }
                } else if ctx.starts_with("spotify:album:") {
                    let album_id = ctx.strip_prefix("spotify:album:").unwrap_or(&ctx);
                    self.load_and_search_album(album_id, &matches).await;
                    return;
                } else if ctx.starts_with("spotify:artist:") {
                    // Artist context uses artist name, not ID — skip for now
                    // Fall through to liked songs fallback
                }
            }

            // Fallback: try liked songs
            self.load_and_search_liked(&matches).await;
            return;
        }

        self.state.status_msg = Some("Playing track not found".to_string());
    }

    async fn load_and_search_playlist(
        &mut self,
        playlist_id: &str,
        playlist_uri: &str,
        playlist_name: &str,
        matches: &impl Fn(&crate::spotify::TrackSummary) -> bool,
    ) {
        self.state.active_playlist_uri = Some(playlist_uri.to_string());
        self.state.active_playlist_id = Some(playlist_id.to_string());
        self.state.active_content = crate::ui::ActiveContent::Tracks;
        self.state.search_results = None;

        if let Ok((tracks, total, _)) = self.spotify.fetch_playlist_tracks(playlist_id, 0).await {
            let tracks_len = tracks.len();
            self.state.tracks = tracks;
            self.state.tracks_total = total;
            self.state.tracks_offset = tracks_len as u32;
            self.state.tracks_api_offset = tracks_len as u32;
            self.state.rebuild_sort_indices();
            self.state.track_list.select(Some(0));
            self.state.status_msg = Some(format!("Loaded: {playlist_name}"));
            self.needs_redraw = true;

            if let Some(target_real) = self.state.tracks.iter().position(|t| matches(t)) {
                if let Some(target_vi) = self
                    .state
                    .sorted_track_indices
                    .iter()
                    .position(|&r| r == target_real)
                {
                    self.select_track_in_view(target_vi, false);
                    self.state.status_msg = Some("Jumped to playing track".to_string());
                }
            }
        }
    }

    async fn load_and_search_album(
        &mut self,
        album_id: &str,
        matches: &impl Fn(&crate::spotify::TrackSummary) -> bool,
    ) {
        self.state.active_content = crate::ui::ActiveContent::Tracks;
        self.state.search_results = None;

        if let Ok((tracks, total)) = self.spotify.fetch_album_tracks(album_id, 0).await {
            let tracks_len = tracks.len();
            self.state.tracks = tracks;
            self.state.tracks_total = total;
            self.state.tracks_offset = tracks_len as u32;
            self.state.tracks_api_offset = tracks_len as u32;
            self.state.rebuild_sort_indices();
            self.state.track_list.select(Some(0));
            self.needs_redraw = true;

            if let Some(target_real) = self.state.tracks.iter().position(|t| matches(t)) {
                if let Some(target_vi) = self
                    .state
                    .sorted_track_indices
                    .iter()
                    .position(|&r| r == target_real)
                {
                    self.select_track_in_view(target_vi, false);
                    self.state.status_msg = Some("Jumped to playing track".to_string());
                }
            }
        }
    }

    async fn load_and_search_liked(
        &mut self,
        matches: &impl Fn(&crate::spotify::TrackSummary) -> bool,
    ) {
        self.state.active_content = crate::ui::ActiveContent::Tracks;
        self.state.active_playlist_uri = Some("liked_songs".to_string());
        self.state.active_playlist_id = Some("liked_songs".to_string());
        self.state.search_results = None;

        if let Ok((tracks, total)) = self.spotify.sync_liked_tracks().await {
            let tracks_len = tracks.len();
            self.state.tracks = tracks;
            self.state.tracks_total = total;
            self.state.tracks_offset = tracks_len as u32;
            self.state.tracks_api_offset = tracks_len as u32;
            self.state.rebuild_sort_indices();
            self.state.track_list.select(Some(0));
            self.needs_redraw = true;

            if let Some(target_real) = self.state.tracks.iter().position(|t| matches(t)) {
                if let Some(target_vi) = self
                    .state
                    .sorted_track_indices
                    .iter()
                    .position(|&r| r == target_real)
                {
                    self.select_track_in_view(target_vi, false);
                    self.state.status_msg = Some("Jumped to playing track".to_string());
                }
            }
        }
    }

    fn find_track_in_current_view(
        &self,
        matches: &impl Fn(&crate::spotify::TrackSummary) -> bool,
    ) -> Option<usize> {
        if self.state.active_content == crate::ui::ActiveContent::LocalFiles {
            (0..self.state.sorted_track_indices.len()).find(|&vi| {
                let real_vi = self.state.sorted_track_indices[vi];
                self.state
                    .local_tree
                    .get_visible(real_vi)
                    .and_then(|n| n.track())
                    .map(matches)
                    .unwrap_or(false)
            })
        } else {
            let target_real = self.state.tracks.iter().position(|t| matches(t));
            target_real.and_then(|real| {
                self.state
                    .sorted_track_indices
                    .iter()
                    .position(|&r| r == real)
            })
        }
    }

    fn select_track_in_view(&mut self, target_vi: usize, is_local: bool) {
        if is_local || self.state.active_content == crate::ui::ActiveContent::LocalFiles {
            self.state.local_tree_list.select(Some(target_vi));
            *self.state.local_tree_list.offset_mut() = target_vi.saturating_sub(5);
        } else {
            self.state.track_list.select(Some(target_vi));
            *self.state.track_list.offset_mut() = target_vi.saturating_sub(5);
        }
        self.state.focus = crate::ui::Focus::Tracks;
        self.needs_redraw = true;
    }
}
