use std::sync::Arc;

use crate::spotify::SpotifyClient;
use crate::ui::UiState;
use crate::utils::debug_overlay::DebugOverlay;
use crate::utils::lyrics::LyricsHandle;

pub enum FetchResult {
    LikedTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    Albums(Result<(Vec<crate::spotify::AlbumSummary>, u32), String>),
    Artists(Result<Vec<crate::spotify::ArtistSummary>, String>),
    PlaylistTracks(Result<(Vec<crate::spotify::TrackSummary>, u32, u32), String>),
    AlbumTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    ArtistTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    MoreTracks(
        Result<
            (
                Vec<crate::spotify::TrackSummary>,
                u32,
                Option<String>,
                Option<u32>,
            ),
            String,
        >,
    ),
}

pub struct FetchCoordinator {
    pub pending_fetch: Option<tokio::sync::oneshot::Receiver<FetchResult>>,
    pub pending_pagination: Option<tokio::sync::oneshot::Receiver<FetchResult>>,
    pub pending_nav_down: bool,
    pub local_scan_rx: Option<tokio::sync::oneshot::Receiver<Vec<crate::ui::LocalNode>>>,
    pub local_scan_total: usize,
    pub album_art_pending: Option<tokio::sync::oneshot::Receiver<Vec<u8>>>,
    pub last_art_uri: String,
    pub lyrics: Option<LyricsHandle>,
}

impl FetchCoordinator {
    pub fn new() -> Self {
        Self {
            pending_fetch: None,
            pending_pagination: None,
            pending_nav_down: false,
            local_scan_rx: None,
            local_scan_total: 0,
            album_art_pending: None,
            last_art_uri: String::new(),
            lyrics: None,
        }
    }

    pub fn ensure_lyrics(&mut self, debug_overlay: &Arc<DebugOverlay>) {
        if self.lyrics.is_none() {
            self.lyrics = LyricsHandle::new(
                crate::config::get_local_db_path().into(),
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(8))
                    .build()
                    .unwrap_or_default(),
                debug_overlay.clone(),
            )
            .ok();
        }
    }

    pub fn poll_lyrics(&mut self, state: &mut UiState) -> bool {
        if let Some(ref lyrics) = self.lyrics {
            match (lyrics.poll(), lyrics.is_loading()) {
                (Some(data), _) => {
                    state.playback.lyrics_loading = false;
                    state.playback.lyrics = if data.is_empty() { None } else { Some(data) };
                    return true;
                }
                (None, true) => {
                    state.playback.lyrics_loading = true;
                }
                (None, false) => {
                    state.playback.lyrics_loading = false;
                }
            }
        }
        false
    }

    pub fn poll_pending_fetch(
        &mut self,
        state: &mut UiState,
        spotify: &SpotifyClient,
    ) -> (bool, bool) {
        let mut needs_redraw = false;
        let mut needs_reconnect = false;

        if let Some(rx) = &mut self.pending_fetch {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending_fetch = None;
                    state.loading = false;
                    needs_reconnect |= self.handle_fetch_result(result, state, spotify);
                    needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.pending_fetch = None;
                    state.loading = false;
                    state.status_msg = Some("Fetch task failed".to_string());
                    needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &mut self.pending_pagination {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending_pagination = None;
                    needs_reconnect |= self.handle_fetch_result(result, state, spotify);
                    needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.pending_pagination = None;
                    self.pending_nav_down = false;
                    needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }

        (needs_redraw, needs_reconnect)
    }

    fn handle_fetch_result(
        &mut self,
        result: FetchResult,
        state: &mut UiState,
        spotify: &SpotifyClient,
    ) -> bool {
        match result {
            FetchResult::LikedTracks(Ok((tracks, total))) => {
                state.tracks = tracks;
                state.tracks_total = total;
                state.tracks_offset = state.tracks.len() as u32;
                state.tracks_api_offset = state.tracks.len() as u32;
                state.active_playlist_uri = Some("liked_songs".to_string());
                state.active_playlist_id = Some("liked_songs".to_string());
                state.track_list.select(if state.tracks.is_empty() {
                    None
                } else {
                    Some(0)
                });
                state.active_content = crate::ui::ActiveContent::Tracks;
                state.search_results = None;
                state.rebuild_sort_indices();
                state.status_msg = None;
                state.focus = crate::ui::Focus::Tracks;
                state.tracks_cursor = spotify
                    .library_cache
                    .get_liked_tracks_page(None, 50)
                    .and_then(|(_, _, next)| next);
                false
            }
            FetchResult::LikedTracks(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    state.status_msg = Some("Authorization expired, reconnecting...".to_string());
                    true
                } else {
                    state.status_msg = Some(format!("Error: {e}"));
                    false
                }
            }
            FetchResult::Albums(Ok((albums, total))) => {
                state.albums = albums;
                state.albums_total = total;
                state.albums_offset = state.albums.len() as u32;
                state.album_list.select(if state.albums.is_empty() {
                    None
                } else {
                    Some(0)
                });
                state.active_content = crate::ui::ActiveContent::Albums;
                state.search_results = None;
                state.status_msg = None;
                state.focus = crate::ui::Focus::Tracks;
                false
            }
            FetchResult::Albums(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    state.status_msg = Some("Authorization expired, reconnecting...".to_string());
                    true
                } else {
                    state.status_msg = Some(format!("Error: {e}"));
                    false
                }
            }
            FetchResult::Artists(Ok(artists)) => {
                state.artists = artists;
                state.artist_list.select(if state.artists.is_empty() {
                    None
                } else {
                    Some(0)
                });
                state.active_content = crate::ui::ActiveContent::Artists;
                state.search_results = None;
                state.status_msg = None;
                state.focus = crate::ui::Focus::Tracks;
                false
            }
            FetchResult::Artists(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    state.status_msg = Some("Authorization expired, reconnecting...".to_string());
                    true
                } else {
                    state.status_msg = Some(format!("Error: {e}"));
                    false
                }
            }
            FetchResult::PlaylistTracks(Ok((tracks, total, page_items))) => {
                state.tracks = tracks;
                state.tracks_total = total;
                state.tracks_offset = state.tracks.len() as u32;
                state.tracks_api_offset = page_items;
                state.track_list.select(if state.tracks.is_empty() {
                    None
                } else {
                    Some(0)
                });
                state.active_content = crate::ui::ActiveContent::Tracks;
                state.search_results = None;
                state.rebuild_sort_indices();
                state.status_msg = None;
                state.focus = crate::ui::Focus::Tracks;
                false
            }
            FetchResult::PlaylistTracks(Err(e)) => {
                if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                    state.status_msg = Some("Authorization expired, reconnecting...".to_string());
                    true
                } else {
                    state.status_msg = Some(format!("Error: {e}"));
                    false
                }
            }
            FetchResult::AlbumTracks(Ok((tracks, total))) => {
                state.tracks = tracks;
                state.tracks_total = total;
                state.tracks_offset = state.tracks.len() as u32;
                state.tracks_api_offset = state.tracks.len() as u32;
                state.track_list.select(if state.tracks.is_empty() {
                    None
                } else {
                    Some(0)
                });
                state.active_content = crate::ui::ActiveContent::Tracks;
                state.search_results = None;
                state.rebuild_sort_indices();
                state.status_msg = None;
                state.focus = crate::ui::Focus::Tracks;
                false
            }
            FetchResult::AlbumTracks(Err(e)) => {
                state.status_msg = Some(format!("Error: {e}"));
                false
            }
            FetchResult::ArtistTracks(Ok((tracks, total))) => {
                state.tracks = tracks;
                state.tracks_total = total;
                state.tracks_offset = state.tracks.len() as u32;
                state.tracks_api_offset = state.tracks.len() as u32;
                state.track_list.select(if state.tracks.is_empty() {
                    None
                } else {
                    Some(0)
                });
                state.active_content = crate::ui::ActiveContent::Tracks;
                state.search_results = None;
                state.rebuild_sort_indices();
                state.status_msg = None;
                state.focus = crate::ui::Focus::Tracks;
                false
            }
            FetchResult::ArtistTracks(Err(e)) => {
                state.status_msg = Some(format!("Error: {e}"));
                false
            }
            FetchResult::MoreTracks(Ok((mut new_tracks, total, cursor, page_items))) => {
                let advance_selection = self.pending_nav_down;
                self.pending_nav_down = false;
                let selected_display = state.track_list.selected();
                let selected_raw = selected_display
                    .and_then(|display_idx| state.sorted_track_indices.get(display_idx))
                    .copied();
                let old_track_len = state.tracks.len();
                state.tracks_loading = false;
                state.status_msg = None;
                if state.active_playlist_id.as_deref() == Some("liked_songs") {
                    if total > state.tracks_total {
                        state.tracks_total = total;
                    }
                    state.tracks_cursor = cursor;
                } else {
                    state.tracks_total = total;
                }
                state.tracks_offset += new_tracks.len() as u32;
                if let Some(pi) = page_items {
                    state.tracks_api_offset += pi;
                } else {
                    state.tracks_api_offset += new_tracks.len() as u32;
                }
                state.tracks.append(&mut new_tracks);
                state.rebuild_sort_indices();
                if advance_selection {
                    let next_display = selected_raw
                        .and_then(|raw_idx| {
                            state
                                .sorted_track_indices
                                .iter()
                                .position(|&idx| idx == raw_idx)
                        })
                        .map(|display_idx| display_idx + 1)
                        .or_else(|| selected_display.map(|display_idx| display_idx + 1));
                    if let Some(next_display) = next_display {
                        if next_display < state.sorted_track_indices.len()
                            && state.tracks.len() > old_track_len
                        {
                            state.track_list.select(Some(next_display));
                        }
                    }
                }
                false
            }
            FetchResult::MoreTracks(Err(e)) => {
                self.pending_nav_down = false;
                state.tracks_loading = false;
                state.status_msg = Some(format!("Load more error: {e}"));
                false
            }
        }
    }
}
