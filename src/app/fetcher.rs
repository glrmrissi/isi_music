use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::spotify::SpotifyClient;
use crate::ui::UiState;
use crate::utils::debug_overlay::DebugOverlay;
use crate::utils::lyrics::LyricsHandle;

type MoreTracksResult = Result<
    (
        Vec<crate::spotify::TrackSummary>,
        u32,
        Option<String>,
        Option<u32>,
    ),
    String,
>;

#[derive(Debug)]
pub enum StreamEvent {
    LikedTracksInitial {
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    LikedTracksBatch {
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    PlaylistsInitial {
        playlists: Vec<crate::spotify::PlaylistSummary>,
    },
    PlaylistsBatch {
        playlists: Vec<crate::spotify::PlaylistSummary>,
    },
    PlaylistTracksInitial {
        playlist_id: String,
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
        page_items: u32,
    },
    PlaylistTracksBatch {
        playlist_id: String,
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
        page_items: u32,
    },
    AlbumsInitial {
        albums: Vec<crate::spotify::AlbumSummary>,
        total: u32,
    },
    AlbumsBatch {
        albums: Vec<crate::spotify::AlbumSummary>,
        total: u32,
    },
    ArtistsInitial {
        artists: Vec<crate::spotify::ArtistSummary>,
    },
    ShowsInitial {
        shows: Vec<crate::spotify::ShowSummary>,
        total: u32,
    },
    ShowsBatch {
        shows: Vec<crate::spotify::ShowSummary>,
        total: u32,
    },
    ShowTracksInitial {
        show_id: String,
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    ShowTracksBatch {
        show_id: String,
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    EpisodesInitial {
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    EpisodesBatch {
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    AlbumTracksInitial {
        album_id: String,
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    AlbumTracksBatch {
        album_id: String,
        tracks: Vec<crate::spotify::TrackSummary>,
        total: u32,
    },
    SearchInitial {
        query: String,
        results: Box<crate::spotify::FullSearchResults>,
    },
    SearchMore {
        stype: String,
        results: Box<crate::spotify::FullSearchResults>,
    },
    Done,
    Error(String),
}

#[allow(clippy::enum_variant_names)]
pub enum FetchResult {
    ArtistTracks(Result<(Vec<crate::spotify::TrackSummary>, u32), String>),
    MoreTracks(MoreTracksResult),
    LocalFolderTracks(Result<Vec<crate::spotify::TrackSummary>, String>),
}

pub(crate) struct LocalScanGuard {
    running: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

impl LocalScanGuard {
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for LocalScanGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}

pub struct FetchCoordinator {
    pub pending_fetch: Option<tokio::sync::oneshot::Receiver<FetchResult>>,
    pub pending_pagination: Option<tokio::sync::oneshot::Receiver<FetchResult>>,
    pub stream_rx: Option<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>>,
    pub pending_nav_down: bool,
    pub local_scan_rx: Option<tokio::sync::oneshot::Receiver<Vec<crate::ui::LocalNode>>>,
    local_scan_running: Arc<AtomicBool>,
    local_scan_cancelled: Arc<AtomicBool>,
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
            stream_rx: None,
            pending_nav_down: false,
            local_scan_rx: None,
            local_scan_running: Arc::new(AtomicBool::new(false)),
            local_scan_cancelled: Arc::new(AtomicBool::new(false)),
            local_scan_total: 0,
            album_art_pending: None,
            last_art_uri: String::new(),
            lyrics: None,
        }
    }

    pub(crate) fn try_start_local_scan(&self) -> Option<LocalScanGuard> {
        self.local_scan_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| {
                self.local_scan_cancelled.store(false, Ordering::Release);
                LocalScanGuard {
                    running: Arc::clone(&self.local_scan_running),
                    cancelled: Arc::clone(&self.local_scan_cancelled),
                }
            })
    }

    pub fn cancel_all_pending(&mut self, state: &mut UiState) {
        self.pending_fetch = None;
        self.pending_pagination = None;
        self.pending_nav_down = false;
        self.stream_rx = None;
        self.local_scan_cancelled.store(true, Ordering::Release);
        self.local_scan_rx = None;
        state.loading = false;
        state.tracks_loading = false;
        if let Some(sr) = state.search_results.as_mut() {
            sr.loading = false;
        }
    }

    pub fn ensure_lyrics(&mut self, debug_overlay: &Arc<DebugOverlay>) {
        if self.lyrics.is_none() {
            self.lyrics = LyricsHandle::new(
                crate::config::get_local_db_path().into(),
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(8))
                    .pool_max_idle_per_host(2)
                    .pool_idle_timeout(std::time::Duration::from_secs(30))
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
                    state.tracks_loading = false;
                    needs_redraw = true;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &mut self.stream_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => match event {
                        StreamEvent::LikedTracksInitial { tracks, total } => {
                            if state.active_playlist_id.as_deref() != Some("liked_songs")
                                && state.active_playlist_id.is_some()
                            {
                                continue;
                            }
                            state.loading = false;
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
                            needs_redraw = true;
                        }
                        StreamEvent::LikedTracksBatch { mut tracks, total } => {
                            if state.active_playlist_id.as_deref() == Some("liked_songs") {
                                if state.tracks.len() >= total as usize {
                                    continue;
                                }
                                tracks.retain(|t| {
                                    !state.tracks.iter().any(|existing| existing.uri == t.uri)
                                });
                                let remaining = (total as usize).saturating_sub(state.tracks.len());
                                if tracks.len() > remaining {
                                    tracks.truncate(remaining);
                                }
                                if tracks.is_empty() {
                                    continue;
                                }
                                let selected_raw = state
                                    .track_list
                                    .selected()
                                    .and_then(|display_idx| {
                                        state.sorted_track_indices.get(display_idx)
                                    })
                                    .copied();
                                state.tracks.append(&mut tracks);
                                state.tracks_total = total;
                                state.tracks_offset = state.tracks.len() as u32;
                                state.tracks_api_offset = state.tracks.len() as u32;
                                state.rebuild_sort_indices();
                                if let Some(raw_idx) = selected_raw
                                    && let Some(pos) = state
                                        .sorted_track_indices
                                        .iter()
                                        .position(|&idx| idx == raw_idx)
                                {
                                    state.track_list.select(Some(pos));
                                }
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::PlaylistsInitial { playlists } => {
                            state.loading = false;
                            state.playlists = playlists;
                            if !state.playlists.is_empty()
                                && state.playlist_list.selected().is_none()
                            {
                                state.playlist_list.select(Some(0));
                            }
                            needs_redraw = true;
                        }
                        StreamEvent::PlaylistsBatch { mut playlists } => {
                            playlists.retain(|p| {
                                !state.playlists.iter().any(|existing| existing.id == p.id)
                            });
                            state.playlists.append(&mut playlists);
                            needs_redraw = true;
                        }
                        StreamEvent::PlaylistTracksInitial {
                            playlist_id,
                            tracks,
                            total,
                            page_items,
                        } => {
                            if state.active_playlist_id.as_deref() != Some(&playlist_id)
                                && state.active_playlist_id.is_some()
                            {
                                continue;
                            }
                            state.loading = false;
                            state.tracks = tracks;
                            state.tracks_total = total;
                            state.tracks_offset = state.tracks.len() as u32;
                            state.tracks_api_offset = page_items;
                            state.active_playlist_uri =
                                Some(format!("spotify:playlist:{playlist_id}"));
                            state.active_playlist_id = Some(playlist_id);
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
                            needs_redraw = true;
                        }
                        StreamEvent::PlaylistTracksBatch {
                            playlist_id,
                            mut tracks,
                            total,
                            page_items,
                        } => {
                            if state.active_playlist_id.as_deref() == Some(&playlist_id) {
                                if state.tracks.len() >= total as usize {
                                    continue;
                                }
                                tracks.retain(|t| {
                                    !state.tracks.iter().any(|existing| existing.uri == t.uri)
                                });
                                let remaining = (total as usize).saturating_sub(state.tracks.len());
                                if tracks.len() > remaining {
                                    tracks.truncate(remaining);
                                }
                                if tracks.is_empty() {
                                    continue;
                                }
                                let selected_raw = state
                                    .track_list
                                    .selected()
                                    .and_then(|display_idx| {
                                        state.sorted_track_indices.get(display_idx)
                                    })
                                    .copied();
                                state.tracks.append(&mut tracks);
                                state.tracks_total = total;
                                state.tracks_offset = state.tracks.len() as u32;
                                state.tracks_api_offset += page_items;
                                state.rebuild_sort_indices();
                                if let Some(raw_idx) = selected_raw
                                    && let Some(pos) = state
                                        .sorted_track_indices
                                        .iter()
                                        .position(|&idx| idx == raw_idx)
                                {
                                    state.track_list.select(Some(pos));
                                }
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::AlbumsInitial { albums, total } => {
                            state.loading = false;
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
                            needs_redraw = true;
                        }
                        StreamEvent::AlbumsBatch { mut albums, total } => {
                            if state.active_content == crate::ui::ActiveContent::Albums {
                                if state.albums.len() >= total as usize {
                                    continue;
                                }
                                albums.retain(|a| {
                                    !state.albums.iter().any(|existing| existing.id == a.id)
                                });
                                let remaining = (total as usize).saturating_sub(state.albums.len());
                                if albums.len() > remaining {
                                    albums.truncate(remaining);
                                }
                                if albums.is_empty() {
                                    continue;
                                }
                                let selected = state.album_list.selected();
                                state.albums.append(&mut albums);
                                state.albums_total = total;
                                state.albums_offset = state.albums.len() as u32;
                                state.album_list.select(selected);
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::ArtistsInitial { artists } => {
                            state.loading = false;
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
                            needs_redraw = true;
                        }
                        StreamEvent::ShowsInitial { shows, total } => {
                            state.loading = false;
                            state.shows = shows;
                            state.shows_total = total;
                            state.shows_offset = state.shows.len() as u32;
                            state.show_list.select(if state.shows.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                            state.active_content = crate::ui::ActiveContent::Shows;
                            state.search_results = None;
                            state.status_msg = None;
                            state.focus = crate::ui::Focus::Tracks;
                            needs_redraw = true;
                        }
                        StreamEvent::ShowsBatch { mut shows, total } => {
                            if state.active_content == crate::ui::ActiveContent::Shows {
                                if state.shows.len() >= total as usize {
                                    continue;
                                }
                                shows.retain(|s| {
                                    !state.shows.iter().any(|existing| existing.id == s.id)
                                });
                                let remaining = (total as usize).saturating_sub(state.shows.len());
                                if shows.len() > remaining {
                                    shows.truncate(remaining);
                                }
                                if shows.is_empty() {
                                    continue;
                                }
                                let selected = state.show_list.selected();
                                state.shows.append(&mut shows);
                                state.shows_total = total;
                                state.shows_offset = state.shows.len() as u32;
                                state.show_list.select(selected);
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::ShowTracksInitial {
                            show_id,
                            tracks,
                            total,
                        } => {
                            let expected = format!("show:{show_id}");
                            if state.active_playlist_id.as_deref() != Some(&expected)
                                && state.active_playlist_id.is_some()
                            {
                                continue;
                            }
                            state.loading = false;
                            state.tracks = tracks;
                            state.tracks_total = total;
                            state.tracks_offset = state.tracks.len() as u32;
                            state.tracks_api_offset = state.tracks.len() as u32;
                            state.active_playlist_uri = Some(format!("show:{show_id}"));
                            state.active_playlist_id = Some(format!("show:{show_id}"));
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
                            needs_redraw = true;
                        }
                        StreamEvent::ShowTracksBatch {
                            show_id,
                            mut tracks,
                            total,
                        } => {
                            if state.active_playlist_id.as_deref()
                                == Some(&format!("show:{show_id}"))
                            {
                                if state.tracks.len() >= total as usize {
                                    continue;
                                }
                                tracks.retain(|t| {
                                    !state.tracks.iter().any(|existing| existing.uri == t.uri)
                                });
                                let remaining = (total as usize).saturating_sub(state.tracks.len());
                                if tracks.len() > remaining {
                                    tracks.truncate(remaining);
                                }
                                if tracks.is_empty() {
                                    continue;
                                }
                                let selected_raw = state
                                    .track_list
                                    .selected()
                                    .and_then(|display_idx| {
                                        state.sorted_track_indices.get(display_idx)
                                    })
                                    .copied();
                                state.tracks.append(&mut tracks);
                                state.tracks_total = total;
                                state.tracks_offset = state.tracks.len() as u32;
                                state.tracks_api_offset = state.tracks.len() as u32;
                                state.rebuild_sort_indices();
                                if let Some(raw_idx) = selected_raw
                                    && let Some(pos) = state
                                        .sorted_track_indices
                                        .iter()
                                        .position(|&idx| idx == raw_idx)
                                {
                                    state.track_list.select(Some(pos));
                                }
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::EpisodesInitial { tracks, total } => {
                            if state.active_playlist_id.as_deref() != Some("saved_episodes")
                                && state.active_playlist_id.is_some()
                            {
                                continue;
                            }
                            state.loading = false;
                            state.tracks = tracks;
                            state.tracks_total = total;
                            state.tracks_offset = state.tracks.len() as u32;
                            state.tracks_api_offset = state.tracks.len() as u32;
                            state.active_playlist_uri = Some("saved_episodes".to_string());
                            state.active_playlist_id = Some("saved_episodes".to_string());
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
                            needs_redraw = true;
                        }
                        StreamEvent::EpisodesBatch { mut tracks, total } => {
                            if state.active_playlist_id.as_deref() == Some("saved_episodes") {
                                if state.tracks.len() >= total as usize {
                                    continue;
                                }
                                tracks.retain(|t| {
                                    !state.tracks.iter().any(|existing| existing.uri == t.uri)
                                });
                                let remaining = (total as usize).saturating_sub(state.tracks.len());
                                if tracks.len() > remaining {
                                    tracks.truncate(remaining);
                                }
                                if tracks.is_empty() {
                                    continue;
                                }
                                let selected_raw = state
                                    .track_list
                                    .selected()
                                    .and_then(|display_idx| {
                                        state.sorted_track_indices.get(display_idx)
                                    })
                                    .copied();
                                state.tracks.append(&mut tracks);
                                state.tracks_total = total;
                                state.tracks_offset = state.tracks.len() as u32;
                                state.tracks_api_offset = state.tracks.len() as u32;
                                state.rebuild_sort_indices();
                                if let Some(raw_idx) = selected_raw
                                    && let Some(pos) = state
                                        .sorted_track_indices
                                        .iter()
                                        .position(|&idx| idx == raw_idx)
                                {
                                    state.track_list.select(Some(pos));
                                }
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::AlbumTracksInitial {
                            album_id,
                            tracks,
                            total,
                        } => {
                            let expected = format!("album:{album_id}");
                            if state.active_playlist_id.as_deref() != Some(&expected)
                                && state.active_playlist_id.is_some()
                            {
                                continue;
                            }
                            state.loading = false;
                            state.tracks = tracks;
                            state.tracks_total = total;
                            state.tracks_offset = state.tracks.len() as u32;
                            state.tracks_api_offset = state.tracks.len() as u32;
                            state.active_playlist_uri = Some(format!("album:{album_id}"));
                            state.active_playlist_id = Some(format!("album:{album_id}"));
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
                            needs_redraw = true;
                        }
                        StreamEvent::AlbumTracksBatch {
                            album_id,
                            mut tracks,
                            total,
                        } => {
                            if state.active_playlist_id.as_deref()
                                == Some(&format!("album:{album_id}"))
                            {
                                if state.tracks.len() >= total as usize {
                                    continue;
                                }
                                tracks.retain(|t| {
                                    !state.tracks.iter().any(|existing| existing.uri == t.uri)
                                });
                                let remaining = (total as usize).saturating_sub(state.tracks.len());
                                if tracks.len() > remaining {
                                    tracks.truncate(remaining);
                                }
                                if tracks.is_empty() {
                                    continue;
                                }
                                let selected_raw = state
                                    .track_list
                                    .selected()
                                    .and_then(|display_idx| {
                                        state.sorted_track_indices.get(display_idx)
                                    })
                                    .copied();
                                state.tracks.append(&mut tracks);
                                state.tracks_total = total;
                                state.tracks_offset = state.tracks.len() as u32;
                                state.tracks_api_offset = state.tracks.len() as u32;
                                state.rebuild_sort_indices();
                                if let Some(raw_idx) = selected_raw
                                    && let Some(pos) = state
                                        .sorted_track_indices
                                        .iter()
                                        .position(|&idx| idx == raw_idx)
                                {
                                    state.track_list.select(Some(pos));
                                }
                                needs_redraw = true;
                            }
                        }
                        StreamEvent::SearchInitial { query, results } => {
                            if state.active_playlist_id.is_some() {
                                continue;
                            }
                            state.loading = false;
                            let total = results.tracks.len()
                                + results.artists.len()
                                + results.albums.len()
                                + results.playlists.len()
                                + results.shows.len()
                                + results.episodes.len();
                            state.search_results =
                                Some(crate::ui::SearchResults::new(query.clone(), *results));
                            state.tracks.clear();
                            state.rebuild_sort_indices();
                            state.active_playlist_uri = None;
                            state.focus = crate::ui::Focus::Search;
                            state.status_msg = if total == 0 {
                                Some(format!("No results for \"{query}\""))
                            } else {
                                Some(format!("{total} results for \"{query}\""))
                            };
                            needs_redraw = true;
                        }
                        StreamEvent::SearchMore { stype, results } => {
                            if let Some(sr) = state.search_results.as_mut() {
                                match stype.as_str() {
                                    "track" => {
                                        let page_size = results.tracks.len() as u32;
                                        let existing: std::collections::HashSet<&str> =
                                            sr.tracks.iter().map(|t| t.uri.as_str()).collect();
                                        let mut new_tracks = results.tracks;
                                        new_tracks.retain(|t| !existing.contains(t.uri.as_str()));
                                        sr.tracks_total = results.tracks_total;
                                        sr.tracks_api_offset =
                                            sr.tracks_api_offset.saturating_add(page_size);
                                        sr.tracks.append(&mut new_tracks);
                                    }
                                    "artist" => {
                                        let page_size = results.artists.len() as u32;
                                        let existing: std::collections::HashSet<&str> =
                                            sr.artists.iter().map(|a| a.id.as_str()).collect();
                                        let mut new_artists = results.artists;
                                        new_artists.retain(|a| !existing.contains(a.id.as_str()));
                                        sr.artists_total = results.artists_total;
                                        sr.artists_api_offset =
                                            sr.artists_api_offset.saturating_add(page_size);
                                        sr.artists.append(&mut new_artists);
                                    }
                                    "album" => {
                                        let page_size = results.albums.len() as u32;
                                        let existing: std::collections::HashSet<&str> =
                                            sr.albums.iter().map(|a| a.id.as_str()).collect();
                                        let mut new_albums = results.albums;
                                        new_albums.retain(|a| !existing.contains(a.id.as_str()));
                                        sr.albums_total = results.albums_total;
                                        sr.albums_api_offset =
                                            sr.albums_api_offset.saturating_add(page_size);
                                        sr.albums.append(&mut new_albums);
                                    }
                                    "playlist" => {
                                        let page_size = results.playlists.len() as u32;
                                        let existing: std::collections::HashSet<&str> =
                                            sr.playlists.iter().map(|p| p.id.as_str()).collect();
                                        let mut new_playlists = results.playlists;
                                        new_playlists.retain(|p| !existing.contains(p.id.as_str()));
                                        sr.playlists_total = results.playlists_total;
                                        sr.playlists_api_offset =
                                            sr.playlists_api_offset.saturating_add(page_size);
                                        sr.playlists.append(&mut new_playlists);
                                    }
                                    "show,episode" => {
                                        let page_size =
                                            results.shows.len().max(results.episodes.len()) as u32;
                                        let existing_shows: std::collections::HashSet<&str> =
                                            sr.shows.iter().map(|s| s.id.as_str()).collect();
                                        let mut new_shows = results.shows;
                                        new_shows
                                            .retain(|s| !existing_shows.contains(s.id.as_str()));
                                        let existing_eps: std::collections::HashSet<&str> =
                                            sr.episodes.iter().map(|t| t.uri.as_str()).collect();
                                        let mut new_eps = results.episodes;
                                        new_eps.retain(|t| !existing_eps.contains(t.uri.as_str()));
                                        sr.shows_total = results.shows_total;
                                        sr.episodes_total = results.episodes_total;
                                        sr.podcasts_api_offset = if page_size == 0 {
                                            results.shows_total + results.episodes_total
                                        } else {
                                            sr.podcasts_api_offset.saturating_add(page_size)
                                        };
                                        sr.shows.append(&mut new_shows);
                                        sr.episodes.append(&mut new_eps);
                                    }
                                    _ => {}
                                }
                                sr.loading = false;
                            }
                            needs_redraw = true;
                        }
                        StreamEvent::Done => {
                            self.stream_rx = None;
                            break;
                        }
                        StreamEvent::Error(e) => {
                            self.stream_rx = None;
                            state.loading = false;
                            if let Some(sr) = state.search_results.as_mut() {
                                sr.loading = false;
                            }
                            if e.contains("SPOTIFY_UNAUTHORIZED") || e.contains("401") {
                                state.status_msg =
                                    Some("Authorization expired, reconnecting...".to_string());
                                needs_reconnect = true;
                            } else {
                                state.status_msg = Some(format!("Error: {e}"));
                            }
                            needs_redraw = true;
                            break;
                        }
                    },
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        self.stream_rx = None;
                        state.loading = false;
                        state.tracks_loading = false;
                        if let Some(sr) = state.search_results.as_mut() {
                            sr.loading = false;
                        }
                        needs_redraw = true;
                        break;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                }
            }
        }

        (needs_redraw, needs_reconnect)
    }

    fn handle_fetch_result(
        &mut self,
        result: FetchResult,
        state: &mut UiState,
        _spotify: &SpotifyClient,
    ) -> bool {
        match result {
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
                state.tracks_loading = false;
                state.status_msg = None;

                if state.tracks.len() >= total as usize {
                    return false;
                }
                new_tracks.retain(|t| !state.tracks.iter().any(|existing| existing.uri == t.uri));
                if new_tracks.is_empty() {
                    return false;
                }
                let remaining = (total as usize).saturating_sub(state.tracks.len());
                if new_tracks.len() > remaining {
                    new_tracks.truncate(remaining);
                }

                let selected_display = state.track_list.selected();
                let selected_raw = selected_display
                    .and_then(|display_idx| state.sorted_track_indices.get(display_idx))
                    .copied();
                let old_track_len = state.tracks.len();

                if state.active_playlist_id.as_deref() == Some("liked_songs") {
                    if total > state.tracks_total {
                        state.tracks_total = total;
                    }
                    state.tracks_cursor = cursor;
                } else {
                    state.tracks_total = total;
                }
                state.tracks_offset = (state.tracks.len() + new_tracks.len()) as u32;
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
                    if let Some(next_display) = next_display
                        && next_display < state.sorted_track_indices.len()
                        && state.tracks.len() > old_track_len
                    {
                        state.track_list.select(Some(next_display));
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
            FetchResult::LocalFolderTracks(Ok(tracks)) => {
                let total = tracks.len() as u32;
                state.tracks = tracks;
                state.tracks_total = total;
                state.tracks_offset = total;
                state.tracks_api_offset = total;
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
            FetchResult::LocalFolderTracks(Err(e)) => {
                state.status_msg = Some(format!("Error: {e}"));
                false
            }
        }
    }
}
