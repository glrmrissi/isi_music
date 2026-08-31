mod nav;
mod search;
mod session;
mod sort;

use ratatui::widgets::ListState;
use std::collections::HashMap;

use super::SearchResults;
use crate::utils::theme::UiWidget;

#[cfg(test)]
pub use crate::ui::LIBRARY_ITEMS;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Library,
    Playlists,
    Tracks,
    Search,
    Queue,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum SearchPanel {
    Tracks,
    Artists,
    Albums,
    Playlists,
}

impl SearchPanel {
    pub fn next(self) -> Self {
        match self {
            Self::Tracks => Self::Artists,
            Self::Artists => Self::Albums,
            Self::Albums => Self::Playlists,
            Self::Playlists => Self::Tracks,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Tracks => Self::Playlists,
            Self::Artists => Self::Tracks,
            Self::Albums => Self::Artists,
            Self::Playlists => Self::Albums,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CompactItem {
    LibraryItem(usize),
    PlaylistItem(usize),
}

#[derive(Clone)]
pub struct NavEntry {
    pub active_content: ActiveContent,
    pub focus: Focus,
    pub active_playlist_uri: Option<String>,
    pub active_playlist_id: Option<String>,
    pub active_artist_name: Option<String>,
    pub search_results: Option<SearchResults>,
    pub previous_search: Option<SearchResults>,
    pub tracks: Vec<crate::spotify::TrackSummary>,
    pub sorted_track_indices: Vec<usize>,
    pub track_sort_by: TrackSortBy,
    pub label: String,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub enum ActiveContent {
    #[default]
    None,
    Tracks,
    Albums,
    Artists,
    Shows,
    LocalFiles,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackSortBy {
    Default,
    Title,
    Artist,
    Album,
    Duration,
    DateAdded,
}

impl TrackSortBy {
    pub fn next(self) -> Self {
        match self {
            TrackSortBy::Default => TrackSortBy::Title,
            TrackSortBy::Title => TrackSortBy::Artist,
            TrackSortBy::Artist => TrackSortBy::Album,
            TrackSortBy::Album => TrackSortBy::Duration,
            TrackSortBy::Duration => TrackSortBy::DateAdded,
            TrackSortBy::DateAdded => TrackSortBy::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrackSortBy::Default => "Default",
            TrackSortBy::Title => "Title",
            TrackSortBy::Artist => "Artist",
            TrackSortBy::Album => "Album",
            TrackSortBy::Duration => "Duration",
            TrackSortBy::DateAdded => "Date Added",
        }
    }
}

pub struct UiState {
    pub focus: Focus,
    pub library_list: ListState,
    pub playlists: Vec<crate::spotify::PlaylistSummary>,
    pub playlist_list: ListState,
    pub active_content: ActiveContent,
    pub tracks: Vec<crate::spotify::TrackSummary>,
    pub track_list: ListState,
    pub local_tree: super::LocalFileTree,
    pub local_tree_list: ListState,
    pub active_playlist_uri: Option<String>,
    pub active_playlist_id: Option<String>,
    pub tracks_offset: u32,
    pub tracks_api_offset: u32,
    pub tracks_total: u32,
    pub tracks_loading: bool,
    pub tracks_cursor: Option<String>,
    pub albums: Vec<crate::spotify::AlbumSummary>,
    pub album_list: ListState,
    pub albums_offset: u32,
    pub albums_total: u32,
    pub artists: Vec<crate::spotify::ArtistSummary>,
    pub artist_list: ListState,
    pub active_artist_name: Option<String>,
    pub shows: Vec<crate::spotify::ShowSummary>,
    pub show_list: ListState,
    pub shows_offset: u32,
    pub shows_total: u32,
    pub search_results: Option<SearchResults>,
    pub previous_search: Option<SearchResults>,
    pub fullscreen_player: bool,
    pub queue_items: Vec<(String, String)>,
    pub queue_list: ListState,
    pub show_album_art: bool,
    #[cfg(feature = "album-art")]
    pub album_art: Option<super::AlbumArtData>,
    pub playback: super::PlaybackState,
    pub status_msg: Option<String>,
    pub search_query: String,
    pub search_active: bool,
    pub quick_search_active: bool,
    pub quick_search_query: String,
    pub spin_angle: f64,
    pub marquee_offset: usize,
    pub show_breadcrumb: bool,
    pub marquee_ms: u64,
    pub viz_bands: Vec<f32>,
    pub art_url: Option<String>,
    pub show_visualizer: bool,
    pub reactive_theme_enabled: bool,
    pub track_sort_by: TrackSortBy,
    pub sorted_track_indices: Vec<usize>,
    pub show_lyrics: bool,
    pub compact_mode: bool,
    pub compact_effective: bool,
    pub nav_stack: Vec<NavEntry>,
    pub add_to_playlist_mode: bool,
    pub add_to_playlist_list: ListState,
    pub command_mode: bool,
    pub command_buffer: String,
    pub loading: bool,
    pub delete_playlist_confirm: bool,
    pub delete_playlist_target: Option<String>,
    pub lastfm_connected: bool,
    pub lastfm_pending: bool,
    pub first_run: bool,
    pub spotify_authenticated: bool,
    pub spotify_enabled: bool,
    /// Library entries shown in the left panel (filtered when Spotify is disabled).
    pub library_items: &'static [&'static str],
    pub widget_rects: HashMap<UiWidget, ratatui::layout::Rect>,
}

impl UiState {
    pub fn new() -> Self {
        let mut library_list = ListState::default();
        library_list.select(Some(0));
        Self {
            focus: Focus::Library,
            library_list,
            playlists: Vec::new(),
            playlist_list: ListState::default(),
            active_content: ActiveContent::None,
            tracks: Vec::new(),
            track_list: ListState::default(),
            local_tree: super::LocalFileTree::default(),
            local_tree_list: ListState::default(),
            active_playlist_uri: None,
            active_playlist_id: None,
            tracks_offset: 0,
            tracks_api_offset: 0,
            tracks_total: 0,
            tracks_loading: false,
            tracks_cursor: None,
            albums: Vec::new(),
            album_list: ListState::default(),
            albums_offset: 0,
            albums_total: 0,
            artists: Vec::new(),
            artist_list: ListState::default(),
            active_artist_name: None,
            shows: Vec::new(),
            show_list: ListState::default(),
            shows_offset: 0,
            shows_total: 0,
            search_results: None,
            previous_search: None,
            fullscreen_player: false,
            queue_items: Vec::new(),
            queue_list: ListState::default(),
            show_album_art: true,
            #[cfg(feature = "album-art")]
            album_art: None,
            playback: super::PlaybackState::default(),
            status_msg: None,
            search_query: String::new(),
            search_active: false,
            quick_search_active: false,
            quick_search_query: String::new(),
            spin_angle: 0.0,
            marquee_offset: 0,
            show_breadcrumb: false,
            marquee_ms: 0,
            viz_bands: Vec::new(),
            art_url: None,
            show_visualizer: true,
            reactive_theme_enabled: false,
            track_sort_by: TrackSortBy::Default,
            sorted_track_indices: Vec::new(),
            show_lyrics: false,
            compact_mode: false,
            compact_effective: false,
            nav_stack: Vec::new(),
            add_to_playlist_mode: false,
            add_to_playlist_list: ListState::default(),
            command_mode: false,
            command_buffer: String::new(),
            loading: false,
            delete_playlist_confirm: false,
            delete_playlist_target: None,
            lastfm_connected: false,
            lastfm_pending: false,
            first_run: false,
            spotify_authenticated: false,
            spotify_enabled: true,
            library_items: crate::ui::LIBRARY_ITEMS,
            widget_rects: HashMap::new(),
        }
    }

    pub fn selected_track_index(&self) -> Option<usize> {
        self.track_list.selected()
    }

    pub fn clear_widget_rects(&mut self) {
        self.widget_rects.clear();
    }

    pub fn store_widget_rect(&mut self, widget: UiWidget, rect: ratatui::layout::Rect) {
        self.widget_rects.insert(widget, rect);
    }

    pub fn widget_at(&self, x: u16, y: u16) -> Option<UiWidget> {
        self.widget_rects
            .iter()
            .find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(w, _)| *w)
    }

    pub fn selected_album_index(&self) -> Option<usize> {
        self.album_list.selected()
    }

    pub fn selected_artist_index(&self) -> Option<usize> {
        self.artist_list.selected()
    }

    pub fn selected_show_index(&self) -> Option<usize> {
        self.show_list.selected()
    }
}

#[cfg(test)]
#[path = "../../../tests/ui/state.rs"]
mod tests;
