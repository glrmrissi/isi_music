use ratatui::widgets::ListState;

use super::{LIBRARY_ITEMS, LocalNode, SearchResults};

#[derive(PartialEq)]
pub enum Focus {
    Library,
    Playlists,
    Tracks,
    Search,
    Queue,
}

#[derive(PartialEq, Clone, Copy)]
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
}

#[derive(Default, PartialEq)]
pub enum ActiveContent {
    #[default]
    None,
    Tracks,
    Albums,
    Artists,
    Shows,
    LocalFiles,
}

#[derive(Clone, Copy, PartialEq)]
pub enum TrackSortBy {
    Default,
    Title,
    Artist,
    Album,
    Duration,
}

impl TrackSortBy {
    pub fn next(self) -> Self {
        match self {
            TrackSortBy::Default => TrackSortBy::Title,
            TrackSortBy::Title => TrackSortBy::Artist,
            TrackSortBy::Artist => TrackSortBy::Album,
            TrackSortBy::Album => TrackSortBy::Duration,
            TrackSortBy::Duration => TrackSortBy::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TrackSortBy::Default => "Default",
            TrackSortBy::Title => "Title",
            TrackSortBy::Artist => "Artist",
            TrackSortBy::Album => "Album",
            TrackSortBy::Duration => "Duration",
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
    pub tracks_total: u32,
    pub tracks_loading: bool,
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
    pub album_art: Option<super::AlbumArtData>,
    pub playback: super::PlaybackState,
    pub status_msg: Option<String>,
    pub search_query: String,
    pub search_active: bool,
    pub quick_search_active: bool,
    pub quick_search_query: String,
    pub spin_angle: f64,
    pub marquee_offset: usize,
    pub marquee_ms: u64,
    pub viz_bands: Vec<f32>,
    pub art_url: Option<String>,
    pub show_visualizer: bool,
    pub track_sort_by: TrackSortBy,
    pub sorted_track_indices: Vec<usize>,
    pub show_lyrics: bool,
    pub compact_mode: bool,
    pub compact_effective: bool,
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
            tracks_total: 0,
            tracks_loading: false,
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
            album_art: None,
            playback: super::PlaybackState::default(),
            status_msg: None,
            search_query: String::new(),
            search_active: false,
            quick_search_active: false,
            quick_search_query: String::new(),
            spin_angle: 0.0,
            marquee_offset: 0,
            marquee_ms: 0,
            viz_bands: Vec::new(),
            art_url: None,
            show_visualizer: true,
            track_sort_by: TrackSortBy::Default,
            sorted_track_indices: Vec::new(),
            show_lyrics: false,
            compact_mode: false,
            compact_effective: false,
        }
    }

    #[allow(dead_code)]
    pub fn selected_playlist(&self) -> Option<&crate::spotify::PlaylistSummary> {
        self.playlist_list
            .selected()
            .and_then(|i| self.playlists.get(i))
    }

    pub fn selected_track_index(&self) -> Option<usize> {
        self.track_list.selected()
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

    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
    }

    pub fn start_quick_search(&mut self) {
        self.quick_search_active = true;
        self.quick_search_query.clear();
    }

    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }

    pub fn cancel_quick_search(&mut self) {
        self.quick_search_active = false;
        self.quick_search_query.clear();
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
    }

    pub fn quick_search_push(&mut self, c: char) {
        self.quick_search_query.push(c);
        self.apply_quick_filter();
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
    }

    pub fn quick_search_pop(&mut self) {
        self.quick_search_query.pop();
        self.apply_quick_filter();
    }

    pub fn apply_quick_filter(&mut self) {
        let query = self.quick_search_query.to_lowercase();

        match self.active_content {
            ActiveContent::Tracks | ActiveContent::None => {
                if query.is_empty() {
                    self.sorted_track_indices = (0..self.tracks.len()).collect();
                } else {
                    self.sorted_track_indices = (0..self.tracks.len())
                        .filter(|&i| {
                            if let Some(t) = self.tracks.get(i) {
                                t.name.to_lowercase().contains(&query)
                                    || t.artist.to_lowercase().contains(&query)
                                    || t.album.to_lowercase().contains(&query)
                            } else {
                                false
                            }
                        })
                        .collect();
                }
                if !self.sorted_track_indices.is_empty() {
                    self.track_list.select(Some(0));
                } else {
                    self.track_list.select(None);
                }
            }
            ActiveContent::LocalFiles => {
                if query.is_empty() {
                    self.sorted_track_indices = (0..self.local_tree.visible_len()).collect();
                } else {
                    let query_lower = query.to_lowercase();

                    self.sorted_track_indices = (0..self.local_tree.visible_len())
                        .filter(|&vi| {
                            self.local_tree
                                .get_visible(vi)
                                .map_or(false, |node| match node {
                                    LocalNode::Folder { name, .. } => {
                                        name.to_lowercase().contains(&query_lower)
                                    }
                                    LocalNode::Track { track, .. } => {
                                        track.name.to_lowercase().contains(&query_lower)
                                            || track.artist.to_lowercase().contains(&query_lower)
                                    }
                                })
                        })
                        .collect();
                }
                if !self.sorted_track_indices.is_empty() {
                    self.local_tree_list.select(Some(0));
                } else {
                    self.local_tree_list.select(None);
                }
            }
            _ => {}
        }
    }

    pub fn sort_tracks(&mut self) {
        self.track_sort_by = self.track_sort_by.next();
        self.rebuild_sort_indices();
    }

    pub fn rebuild_sort_indices(&mut self) {
        match self.active_content {
            ActiveContent::Tracks | ActiveContent::None => {
                let selected_real_idx = self
                    .track_list
                    .selected()
                    .and_then(|i| self.sorted_track_indices.get(i).copied());

                self.sorted_track_indices = (0..self.tracks.len()).collect();

                if self.track_sort_by != TrackSortBy::Default {
                    self.sorted_track_indices.sort_by(|&a, &b| {
                        let track_a = &self.tracks[a];
                        let track_b = &self.tracks[b];

                        match self.track_sort_by {
                            TrackSortBy::Title => track_a.name.cmp(&track_b.name),
                            TrackSortBy::Artist => track_a.artist.cmp(&track_b.artist),
                            TrackSortBy::Album => track_a.album.cmp(&track_b.album),
                            TrackSortBy::Duration => track_a.duration_ms.cmp(&track_b.duration_ms),
                            TrackSortBy::Default => std::cmp::Ordering::Equal,
                        }
                    });
                }

                if !self.sorted_track_indices.is_empty() {
                    if let Some(real_idx) = selected_real_idx {
                        if let Some(new_pos) = self
                            .sorted_track_indices
                            .iter()
                            .position(|&x| x == real_idx)
                        {
                            self.track_list.select(Some(new_pos));
                        } else {
                            self.track_list.select(Some(0));
                        }
                    } else {
                        self.track_list.select(Some(0));
                    }
                } else {
                    self.track_list.select(None);
                }
            }
            _ => {}
        }
    }

    fn compact_selectable_positions(&self) -> Vec<usize> {
        let mut positions: Vec<usize> = (1..=LIBRARY_ITEMS.len()).collect();
        if !self.playlists.is_empty() {
            let playlist_start = 1 + LIBRARY_ITEMS.len() + 1;
            for i in 0..self.playlists.len() {
                positions.push(playlist_start + i);
            }
        }
        positions
    }

    pub fn nav_up(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if selectable.is_empty() {
                return;
            }
            let cur = self.library_list.selected().unwrap_or(selectable[0]);
            let idx = selectable.iter().position(|&p| p == cur).unwrap_or(0);
            let next = if idx == 0 {
                selectable.len() - 1
            } else {
                idx - 1
            };
            self.library_list.select(Some(selectable[next]));
            return;
        }
        match self.focus {
            Focus::Library => {
                let i = self
                    .library_list
                    .selected()
                    .map(|i| {
                        if i == 0 {
                            LIBRARY_ITEMS.len() - 1
                        } else {
                            i - 1
                        }
                    })
                    .unwrap_or(0);
                self.library_list.select(Some(i));
            }
            Focus::Playlists => scroll_up(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => scroll_up(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists => scroll_up(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows => scroll_up(&mut self.show_list, self.shows.len()),
                ActiveContent::LocalFiles => {
                    scroll_up(&mut self.local_tree_list, self.sorted_track_indices.len())
                }
                _ => scroll_up(&mut self.track_list, self.sorted_track_indices.len()),
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    sr.nav_up();
                }
            }
            Focus::Queue => scroll_up(&mut self.queue_list, self.queue_items.len()),
        }
    }

    pub fn nav_down(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if selectable.is_empty() {
                return;
            }
            let cur = self.library_list.selected().unwrap_or(selectable[0]);
            let idx = selectable.iter().position(|&p| p == cur).unwrap_or(0);
            let next = if idx >= selectable.len() - 1 {
                0
            } else {
                idx + 1
            };
            self.library_list.select(Some(selectable[next]));
            return;
        }
        match self.focus {
            Focus::Library => {
                let i = self
                    .library_list
                    .selected()
                    .map(|i| {
                        if i >= LIBRARY_ITEMS.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    })
                    .unwrap_or(0);
                self.library_list.select(Some(i));
            }
            Focus::Playlists => scroll_down(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => scroll_down(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists => scroll_down(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows => scroll_down(&mut self.show_list, self.shows.len()),
                ActiveContent::LocalFiles => {
                    scroll_down(&mut self.local_tree_list, self.sorted_track_indices.len())
                }
                _ => scroll_down(&mut self.track_list, self.sorted_track_indices.len()),
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    sr.nav_down();
                }
            }
            Focus::Queue => scroll_down(&mut self.queue_list, self.queue_items.len()),
        }
    }

    pub fn nav_first(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if !selectable.is_empty() {
                self.library_list.select(Some(selectable[0]));
            }
            return;
        }
        match self.focus {
            Focus::Library => self.library_list.select(Some(0)),
            Focus::Playlists => {
                if !self.playlists.is_empty() {
                    self.playlist_list.select(Some(0));
                }
            }
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => {
                    if !self.albums.is_empty() {
                        self.album_list.select(Some(0));
                    }
                }
                ActiveContent::Artists => {
                    if !self.artists.is_empty() {
                        self.artist_list.select(Some(0));
                    }
                }
                ActiveContent::Shows => {
                    if !self.shows.is_empty() {
                        self.show_list.select(Some(0));
                    }
                }
                ActiveContent::LocalFiles => {
                    if self.sorted_track_indices.len() > 0 {
                        self.local_tree_list.select(Some(0));
                    }
                }
                _ => {
                    if !self.sorted_track_indices.is_empty() {
                        self.track_list.select(Some(0));
                    }
                }
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    if sr.current_len() > 0 {
                        sr.current_list_mut().select(Some(0));
                    }
                }
            }
            Focus::Queue => {
                if !self.queue_items.is_empty() {
                    self.queue_list.select(Some(0));
                }
            }
        }
    }

    pub fn nav_last(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if !selectable.is_empty() {
                self.library_list
                    .select(Some(selectable[selectable.len() - 1]));
            }
            return;
        }
        match self.focus {
            Focus::Library => self.library_list.select(Some(LIBRARY_ITEMS.len() - 1)),
            Focus::Playlists => {
                let n = self.playlists.len();
                if n > 0 {
                    self.playlist_list.select(Some(n - 1));
                }
            }
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => {
                    let n = self.albums.len();
                    if n > 0 {
                        self.album_list.select(Some(n - 1));
                    }
                }
                ActiveContent::Artists => {
                    let n = self.artists.len();
                    if n > 0 {
                        self.artist_list.select(Some(n - 1));
                    }
                }
                ActiveContent::Shows => {
                    let n = self.shows.len();
                    if n > 0 {
                        self.show_list.select(Some(n - 1));
                    }
                }
                ActiveContent::LocalFiles => {
                    let n = self.sorted_track_indices.len();
                    if n > 0 {
                        self.local_tree_list.select(Some(n - 1));
                    }
                }
                _ => {
                    let n = self.sorted_track_indices.len();
                    if n > 0 {
                        self.track_list.select(Some(n - 1));
                    }
                }
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    let n = sr.current_len();
                    if n > 0 {
                        sr.current_list_mut().select(Some(n - 1));
                    }
                }
            }
            Focus::Queue => {
                let n = self.queue_items.len();
                if n > 0 {
                    self.queue_list.select(Some(n - 1));
                }
            }
        }
    }

    pub fn switch_focus(&mut self) {
        self.search_active = false;
        if self.compact_effective {
            self.focus = match self.focus {
                Focus::Search => Focus::Tracks,
                _ => {
                    if self.search_results.is_some() {
                        Focus::Search
                    } else {
                        Focus::Tracks
                    }
                }
            };
            return;
        }
        self.focus = match self.focus {
            Focus::Library => Focus::Playlists,
            Focus::Playlists => {
                if self.search_results.is_some() {
                    Focus::Search
                } else {
                    Focus::Tracks
                }
            }
            Focus::Tracks => Focus::Queue,
            Focus::Queue | Focus::Search => Focus::Library,
        };
    }

    pub fn switch_focus_prev(&mut self) {
        self.search_active = false;
        if self.compact_effective {
            self.focus = match self.focus {
                Focus::Search => Focus::Tracks,
                _ => {
                    if self.search_results.is_some() {
                        Focus::Search
                    } else {
                        Focus::Tracks
                    }
                }
            };
            return;
        }
        self.focus = match self.focus {
            Focus::Library => Focus::Queue,
            Focus::Playlists => Focus::Library,
            Focus::Tracks => Focus::Playlists,
            Focus::Queue => Focus::Tracks,
            Focus::Search => Focus::Playlists,
        };
    }

    pub fn switch_search_panel(&mut self) {
        if let Some(sr) = &mut self.search_results {
            sr.next_panel();
        }
    }
}

fn scroll_up(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state
        .selected()
        .map(|i| if i == 0 { len - 1 } else { i - 1 })
        .unwrap_or(0);
    state.select(Some(i));
}

fn scroll_down(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state
        .selected()
        .map(|i| if i >= len - 1 { 0 } else { i + 1 })
        .unwrap_or(0);
    state.select(Some(i));
}
