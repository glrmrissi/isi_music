use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, List, ListItem, ListState, Paragraph},
    Frame,
};
use rspotify::model::RepeatState;
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};
use tracing::warn;
use crate::spotify::{AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, ShowSummary, TrackSummary};
use crate::theme::Theme;
use crate::theme::{LayoutNode, UiWidget};


pub struct AlbumArtData {
    pub image_state: Option<StatefulProtocol>,
}

#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub path: Option<String>,
    pub is_playing: bool,
    pub shuffle: bool,
    pub repeat: RepeatState,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    pub art_url: Option<String>,
    pub cover_path: Option<String>, 
    pub is_local: bool,
    pub radio_mode: bool,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            path: None,
            is_playing: false,
            shuffle: false,
            repeat: RepeatState::Off,
            progress_ms: 0,
            duration_ms: 0,
            volume: 100,
            art_url: None,
            cover_path: None, 
            is_local: false,
            radio_mode: false,
        }
    }
}

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
    fn next(self) -> Self {
        match self {
            Self::Tracks    => Self::Artists,
            Self::Artists   => Self::Albums,
            Self::Albums    => Self::Playlists,
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

/// A node in the local file tree.
#[derive(Clone)]
pub enum LocalNode {
    Folder {
        name: String,
        depth: usize,
        expanded: bool,
        children_start: usize,
        children_count: usize,
    },
    Track {
        track: TrackSummary,
        depth: usize,
    },
}

impl LocalNode {
    pub fn depth(&self) -> usize {
        match self {
            LocalNode::Folder { depth, .. } => *depth,
            LocalNode::Track { depth, .. } => *depth,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, LocalNode::Folder { .. })
    }

    pub fn is_expanded(&self) -> bool {
        match self {
            LocalNode::Folder { expanded, .. } => *expanded,
            LocalNode::Track { .. } => false,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            LocalNode::Folder { name, .. } => name,
            LocalNode::Track { track, .. } => &track.name,
        }
    }

    pub fn track(&self) -> Option<&TrackSummary> {
        match self {
            LocalNode::Track { track, .. } => Some(track),
            _ => None,
        }
    }
}

#[derive(Default, Clone)]
pub struct LocalFileTree {
    pub all_nodes: Vec<LocalNode>,
    pub visible: Vec<usize>,
}

impl LocalFileTree {
    pub fn new(nodes: Vec<LocalNode>) -> Self {
        let mut tree = Self { all_nodes: nodes, visible: Vec::new() };
        tree.rebuild_visible();
        tree
    }

    pub fn rebuild_visible(&mut self) {
        self.visible.clear();
        let mut skip_depth: Option<usize> = None;

        for (i, node) in self.all_nodes.iter().enumerate() {
            if let Some(depth) = skip_depth {
                if node.depth() > depth {
                    continue;
                }
                skip_depth = None;
            }

            self.visible.push(i);
            
            if let LocalNode::Folder { expanded: false, depth, .. } = node {
                skip_depth = Some(*depth);
            }
        }
    }

    pub fn toggle_folder(&mut self, visible_idx: usize) {
        let Some(&node_idx) = self.visible.get(visible_idx) else { return };
        if let LocalNode::Folder { expanded, .. } = &mut self.all_nodes[node_idx] {
            *expanded = !*expanded;
        }
        self.rebuild_visible();
    }

    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    pub fn get_visible(&self, visible_idx: usize) -> Option<&LocalNode> {
        self.visible.get(visible_idx).and_then(|&i| self.all_nodes.get(i))
    }

    pub fn all_tracks_flat(&self) -> Vec<TrackSummary> {
        self.all_nodes.iter().filter_map(|n| n.track().cloned()).collect()
    }

    pub fn tracks_under_folder(&self, visible_idx: usize) -> Vec<TrackSummary> {
        let Some(&node_idx) = self.visible.get(visible_idx) else { return vec![] };
        let folder_depth = self.all_nodes[node_idx].depth();
        self.all_nodes[node_idx + 1..]
            .iter()
            .take_while(|n| n.depth() > folder_depth)
            .filter_map(|n| n.track().cloned())
            .collect()
    }

    pub fn flat_track_index(&self, visible_idx: usize) -> Option<usize> {
        let Some(&node_idx) = self.visible.get(visible_idx) else { return None };
        let node = self.all_nodes.get(node_idx)?;
        if node.is_folder() { return None; }
        let target_uri = node.track()?.uri.as_str();
        self.all_nodes.iter()
            .filter_map(|n| n.track())
            .position(|t| t.uri == target_uri)
    }
}

const LIBRARY_ITEMS: &[&str] = &[
    "Liked Songs",
    "Albums",
    "Artists",
    "Podcasts",
    "Local Files",
];

pub struct SearchResults {
    pub tracks:   Vec<TrackSummary>,
    pub artists:  Vec<ArtistSummary>,
    pub albums:   Vec<AlbumSummary>,
    pub playlists: Vec<PlaylistSummary>,
    pub track_list:    ListState,
    pub artist_list:   ListState,
    pub album_list:    ListState,
    pub playlist_list: ListState,
    pub panel: SearchPanel,
    pub query: String,
    pub tracks_total:    u32,
    pub artists_total:   u32,
    pub albums_total:    u32,
    pub playlists_total: u32,
    pub loading: bool,
}

impl SearchResults {
    pub fn new(query: String, r: FullSearchResults) -> Self {
        let mut tl = ListState::default();
        if !r.tracks.is_empty() { tl.select(Some(0)); }
        Self {
            tracks: r.tracks,
            artists: r.artists,
            albums: r.albums,
            playlists: r.playlists,
            track_list: tl,
            artist_list: ListState::default(),
            album_list: ListState::default(),
            playlist_list: ListState::default(),
            panel: SearchPanel::Tracks,
            query,
            tracks_total:    r.tracks_total,
            artists_total:   r.artists_total,
            albums_total:    r.albums_total,
            playlists_total: r.playlists_total,
            loading: false,
        }
    }

    fn current_len(&self) -> usize {
        match self.panel {
            SearchPanel::Tracks    => self.tracks.len(),
            SearchPanel::Artists   => self.artists.len(),
            SearchPanel::Albums    => self.albums.len(),
            SearchPanel::Playlists => self.playlists.len(),
        }
    }

    fn current_list_mut(&mut self) -> &mut ListState {
        match self.panel {
            SearchPanel::Tracks    => &mut self.track_list,
            SearchPanel::Artists   => &mut self.artist_list,
            SearchPanel::Albums    => &mut self.album_list,
            SearchPanel::Playlists => &mut self.playlist_list,
        }
    }

    pub fn nav_up(&mut self) {
        let len = self.current_len();
        if len == 0 { return; }
        let list = self.current_list_mut();
        let i = list.selected().map(|i| if i == 0 { len - 1 } else { i - 1 }).unwrap_or(0);
        list.select(Some(i));
    }

    pub fn nav_down(&mut self) {
        let len = self.current_len();
        if len == 0 { return; }
        let list = self.current_list_mut();
        let i = list.selected().map(|i| if i >= len - 1 { 0 } else { i + 1 }).unwrap_or(0);
        list.select(Some(i));
    }

    pub fn next_panel(&mut self) {
        self.panel = self.panel.next();
    }

    pub fn selected_track_uri(&self) -> Option<&str> {
        self.track_list.selected().and_then(|i| self.tracks.get(i)).map(|t| t.uri.as_str())
    }

    pub fn selected_album(&self) -> Option<&AlbumSummary> {
        self.album_list.selected().and_then(|i| self.albums.get(i))
    }

    pub fn selected_artist(&self) -> Option<&ArtistSummary> {
        self.artist_list.selected().and_then(|i| self.artists.get(i))
    }

    pub fn selected_playlist(&self) -> Option<&PlaylistSummary> {
        self.playlist_list.selected().and_then(|i| self.playlists.get(i))
    }
}

pub struct UiState {
    pub focus: Focus,
    pub library_list: ListState,
    pub playlists: Vec<PlaylistSummary>,
    pub playlist_list: ListState,
    pub active_content: ActiveContent,
    pub tracks: Vec<TrackSummary>,
    pub track_list: ListState,
    pub local_tree: LocalFileTree,
    pub local_tree_list: ListState,
    pub active_playlist_uri: Option<String>,
    pub active_playlist_id: Option<String>,
    pub tracks_offset: u32,
    pub tracks_total: u32,
    pub tracks_loading: bool,
    pub albums: Vec<AlbumSummary>,
    pub album_list: ListState,
    pub albums_offset: u32,
    pub albums_total: u32,
    pub artists: Vec<ArtistSummary>,
    pub artist_list: ListState,
    pub active_artist_name: Option<String>,
    pub shows: Vec<ShowSummary>,
    pub show_list: ListState,
    pub shows_offset: u32,
    pub shows_total: u32,
    pub search_results: Option<SearchResults>,
    pub previous_search: Option<SearchResults>,
    pub fullscreen_player: bool,
    pub queue_items: Vec<(String, String)>,
    pub queue_list: ListState,
    pub show_album_art: bool,
    pub album_art: Option<AlbumArtData>,
    pub playback: PlaybackState,
    pub status_msg: Option<String>,
    pub search_query: String,
    pub search_active: bool,
    pub spin_angle: f64,
    pub marquee_offset: usize,
    pub marquee_ms: u64,
    pub viz_bands: Vec<f32>,
    pub art_url: Option<String>,
    pub show_visualizer: bool,
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
            local_tree: LocalFileTree::default(),
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
            playback: PlaybackState::default(),
            status_msg: None,
            search_query: String::new(),
            search_active: false,
            spin_angle: 0.0,
            marquee_offset: 0,
            marquee_ms: 0,
            viz_bands: Vec::new(),
            art_url: None,
            show_visualizer: true,
        }
    }

    pub fn selected_playlist(&self) -> Option<&PlaylistSummary> {
        self.playlist_list.selected().and_then(|i| self.playlists.get(i))
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

    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
    }

    pub fn nav_up(&mut self) {
        match self.focus {
            Focus::Library => {
                let i = self.library_list.selected()
                    .map(|i| if i == 0 { LIBRARY_ITEMS.len() - 1 } else { i - 1 })
                    .unwrap_or(0);
                self.library_list.select(Some(i));
            }
            Focus::Playlists => scroll_up(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums    => scroll_up(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists   => scroll_up(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows     => scroll_up(&mut self.show_list, self.shows.len()),
                ActiveContent::LocalFiles => scroll_up(&mut self.local_tree_list, self.local_tree.visible_len()),
                _ => scroll_up(&mut self.track_list, self.tracks.len()),
            },
            Focus::Search    => { if let Some(sr) = &mut self.search_results { sr.nav_up(); } }
            Focus::Queue     => scroll_up(&mut self.queue_list, self.queue_items.len()),
        }
    }

    pub fn nav_down(&mut self) {
        match self.focus {
            Focus::Library => {
                let i = self.library_list.selected()
                    .map(|i| if i >= LIBRARY_ITEMS.len() - 1 { 0 } else { i + 1 })
                    .unwrap_or(0);
                self.library_list.select(Some(i));
            }
            Focus::Playlists => scroll_down(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums    => scroll_down(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists   => scroll_down(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows     => scroll_down(&mut self.show_list, self.shows.len()),
                ActiveContent::LocalFiles => scroll_down(&mut self.local_tree_list, self.local_tree.visible_len()),
                _ => scroll_down(&mut self.track_list, self.tracks.len()),
            },
            Focus::Search    => { if let Some(sr) = &mut self.search_results { sr.nav_down(); } }
            Focus::Queue     => scroll_down(&mut self.queue_list, self.queue_items.len()),
        }
    }

    pub fn nav_first(&mut self) {
        match self.focus {
            Focus::Library   => self.library_list.select(Some(0)),
            Focus::Playlists => { if !self.playlists.is_empty() { self.playlist_list.select(Some(0)); } }
            Focus::Tracks    => match self.active_content {
                ActiveContent::Albums    => { if !self.albums.is_empty()  { self.album_list.select(Some(0));  } }
                ActiveContent::Artists   => { if !self.artists.is_empty() { self.artist_list.select(Some(0)); } }
                ActiveContent::Shows     => { if !self.shows.is_empty()   { self.show_list.select(Some(0));   } }
                ActiveContent::LocalFiles => { if self.local_tree.visible_len() > 0 { self.local_tree_list.select(Some(0)); } }
                _ => { if !self.tracks.is_empty() { self.track_list.select(Some(0)); } }
            },
            Focus::Search => { if let Some(sr) = &mut self.search_results { if sr.current_len() > 0 { sr.current_list_mut().select(Some(0)); } } }
            Focus::Queue  => { if !self.queue_items.is_empty() { self.queue_list.select(Some(0)); } }
        }
    }

    pub fn nav_last(&mut self) {
        match self.focus {
            Focus::Library   => self.library_list.select(Some(LIBRARY_ITEMS.len() - 1)),
            Focus::Playlists => { let n = self.playlists.len(); if n > 0 { self.playlist_list.select(Some(n - 1)); } }
            Focus::Tracks    => match self.active_content {
                ActiveContent::Albums    => { let n = self.albums.len();  if n > 0 { self.album_list.select(Some(n - 1));  } }
                ActiveContent::Artists   => { let n = self.artists.len(); if n > 0 { self.artist_list.select(Some(n - 1)); } }
                ActiveContent::Shows     => { let n = self.shows.len();   if n > 0 { self.show_list.select(Some(n - 1));   } }
                ActiveContent::LocalFiles => { let n = self.local_tree.visible_len(); if n > 0 { self.local_tree_list.select(Some(n - 1)); } }
                _ => { let n = self.tracks.len(); if n > 0 { self.track_list.select(Some(n - 1)); } }
            },
            Focus::Search => { if let Some(sr) = &mut self.search_results { let n = sr.current_len(); if n > 0 { sr.current_list_mut().select(Some(n - 1)); } } }
            Focus::Queue  => { let n = self.queue_items.len(); if n > 0 { self.queue_list.select(Some(n - 1)); } }
        }
    }

    pub fn switch_focus(&mut self) {
        self.search_active = false;
        self.focus = match self.focus {
            Focus::Library   => Focus::Playlists,
            Focus::Playlists => if self.search_results.is_some() { Focus::Search } else { Focus::Tracks },
            Focus::Tracks    => Focus::Queue,
            Focus::Queue | Focus::Search => Focus::Library,
        };
    }

    pub fn switch_focus_prev(&mut self) {
        self.search_active = false;
        self.focus = match self.focus {
            Focus::Library   => Focus::Queue,
            Focus::Playlists => Focus::Library,
            Focus::Tracks    => Focus::Playlists,
            Focus::Queue     => Focus::Tracks,
            Focus::Search    => Focus::Playlists,
        };
    }

    pub fn switch_search_panel(&mut self) {
        if let Some(sr) = &mut self.search_results {
            sr.next_panel();
        }
    }
}

fn scroll_up(state: &mut ListState, len: usize) {
    if len == 0 { return; }
    let i = state.selected().map(|i| if i == 0 { len - 1 } else { i - 1 }).unwrap_or(0);
    state.select(Some(i));
}

fn scroll_down(state: &mut ListState, len: usize) {
    if len == 0 { return; }
    let i = state.selected().map(|i| if i >= len - 1 { 0 } else { i + 1 }).unwrap_or(0);
    state.select(Some(i));
}

pub struct Ui {
    theme: Theme,
}

impl Ui {
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }

    pub fn with_default_theme() -> Self {
        Self { theme: Theme::default() }
    }

    pub fn render(&self, frame: &mut Frame, state: &mut UiState) {
        let area = frame.area();
        let root_area = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        if state.fullscreen_player {
            self.render_now_playing(frame, state, root_area);
            return;
        }

        let layout_tree = self.theme.layout_tree.clone();
        self.render_recursive(frame, state, root_area, &layout_tree);
    }

    fn render_recursive(&self, frame: &mut Frame, state: &mut UiState, area: Rect, node: &LayoutNode) {
        if let Some(widget_type) = &node.widget {
            match widget_type {
                UiWidget::Header      => self.render_header(frame, state, area),
                UiWidget::Library     => self.render_library(frame, state, area),
                UiWidget::Playlists   => self.render_playlists(frame, state, area),
                UiWidget::AlbumArt    => {
                    if state.show_album_art {
                        self.render_album_art(frame, state, area);
                    }
                }
                UiWidget::MainContent => self.render_main_area_logic(frame, state, area),
                UiWidget::Queue       => self.render_queue(frame, state, area),
                UiWidget::Progress    => self.render_progress(frame, &state.playback, area),
                UiWidget::Marquee     => self.render_marquee(frame, &state.playback, state.marquee_offset, area),
                UiWidget::Visualizer  => {
                    let viz_bands = state.viz_bands.clone();
                    let pb = state.playback.clone();
                    self.render_visualizer(frame, &pb, &viz_bands, area, state);
                }
                UiWidget::Help        => self.render_help(frame, state, area),
                UiWidget::Spacer      => {}
            }
            return;
        }

        if let (Some(dir), Some(raw_constraints), Some(children)) =
            (node.direction, &node.constraints, &node.children)
        {
            if children.is_empty() { return; }

            let parsed: Vec<Constraint> = raw_constraints
                .iter()
                .map(|s| self.parse_constraint(s))
                .collect();

            let chunks = Layout::default()
                .direction(Direction::from(dir))
                .constraints(parsed)
                .split(area);

            for (i, child) in children.iter().enumerate() {
                if let Some(chunk) = chunks.get(i) {
                    self.render_recursive(frame, state, *chunk, child);
                }
            }
        }
    }

    fn render_main_area_logic(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.search_results.is_some() {
            self.render_search_panels(frame, state, area);
        } else {
            match &state.active_content {
                ActiveContent::None => {
                    if state.playback.title.is_empty() {
                        self.render_welcome(frame, area);
                    } else {
                        self.render_now_playing(frame, state, area);
                    }
                }
                ActiveContent::Tracks    => self.render_tracks(frame, state, area),
                ActiveContent::LocalFiles => self.render_local_tree(frame, state, area),
                ActiveContent::Albums    => self.render_albums(frame, state, area),
                ActiveContent::Artists   => self.render_artists(frame, state, area),
                ActiveContent::Shows     => self.render_shows(frame, state, area),
            }
        }
    }

    fn parse_constraint(&self, s: &str) -> Constraint {
        let s = s.trim().to_lowercase().replace(' ', "");
        if s.ends_with('%') {
            if let Ok(p) = s.trim_end_matches('%').parse::<u16>() {
                return Constraint::Percentage(p);
            }
        }
        if s.starts_with("min(") && s.ends_with(')') {
            if let Ok(v) = s[4..s.len() - 1].parse::<u16>() {
                return Constraint::Min(v);
            }
        }
        if let Ok(l) = s.parse::<u16>() {
            return Constraint::Length(l);
        }
        Constraint::Min(0)
    }

    fn render_local_tree(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let total_tracks: usize = state.local_tree.all_nodes.iter()
            .filter(|n| !n.is_folder())
            .count();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰈣 Local Files ")
            .title_bottom(Line::from(vec![
                Span::styled(
                    format!(" {} tracks  [ENTER] play/expand  [A] queue ", total_tracks),
                    Style::default().fg(self.theme.border_inactive),
                ),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = (0..state.local_tree.visible_len())
            .filter_map(|vi| {
                let node = state.local_tree.get_visible(vi)?;
                let indent = "  ".repeat(node.depth());
                let item = match node {
                    LocalNode::Folder { name, expanded, .. } => {
                        let icon = if *expanded { "󰝰 " } else { "󰉋 " };
                        let child_count = state.local_tree.tracks_under_folder(vi).len();
                        ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(icon, Style::default().fg(self.theme.accent_color).add_modifier(Modifier::BOLD)),
                            Span::styled(name.clone(), Style::default().fg(self.theme.text_primary).add_modifier(Modifier::BOLD)),
                            Span::styled(
                                format!("  ({} tracks)", child_count),
                                Style::default().fg(self.theme.border_inactive),
                            ),
                        ]))
                    }
                    LocalNode::Track { track, .. } => {
                        let is_playing = state.playback.title == track.name
                            && state.playback.is_local;
                        let icon = if is_playing { "󰎈 " } else { "󰝚 " };
                        let title_style = if is_playing {
                            Style::default().fg(self.theme.border_active).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(self.theme.text_primary)
                        };
                        let dur = fmt_duration(track.duration_ms);
                        ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(icon, Style::default().fg(self.theme.border_inactive)),
                            Span::styled(track.name.clone(), title_style),
                            if !track.artist.is_empty() {
                                Span::styled(
                                    format!("  󰠃 {}", track.artist),
                                    Style::default().fg(self.theme.border_inactive),
                                )
                            } else {
                                Span::raw("")
                            },
                            Span::styled(
                                format!("  {}", dur),
                                Style::default().fg(self.theme.border_inactive).add_modifier(Modifier::DIM),
                            ),
                        ]))
                    }
                };
                Some(item)
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut state.local_tree_list);
    }

    fn render_visualizer(
        &self,
        frame: &mut Frame,
        pb: &PlaybackState,
        viz_bands: &[f32],
        area: Rect,
        state: &UiState,
    ) {
        if !state.show_visualizer { return; }

        let block = Block::default();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 { return; }

        const LEFT:  [u8; 4] = [1 << 6, 1 << 2, 1 << 1, 1 << 0];
        const RIGHT: [u8; 4] = [1 << 7, 1 << 5, 1 << 4, 1 << 3];

        let n_bars  = inner.width as usize;
        let px_rows = inner.height as usize * 4;

        for bar in 0..n_bars {
            let amp: f64 = if !pb.is_playing {
                0.0
            } else if viz_bands.is_empty() {
                0.05
            } else {
                let band_idx = (bar * viz_bands.len() / n_bars).min(viz_bands.len() - 1);
                (viz_bands[band_idx] as f64).clamp(0.0, 1.0)
            };

            let bar_h = ((amp * px_rows as f64) as usize).min(px_rows);
            if bar_h == 0 { continue; }

            let color = if amp > 0.75      { self.theme.text_primary }
                        else if amp > 0.25 { self.theme.accent_color }
                        else               { self.theme.border_inactive };

            for cell_y in 0..inner.height as usize {
                let bottom_idx = inner.height as usize - 1 - cell_y;
                let px_base    = bottom_idx * 4;
                if px_base >= bar_h { continue; }

                let mut bits: u8 = 0;
                for dot_row in 0..4 {
                    if px_base + dot_row < bar_h {
                        bits |= LEFT[dot_row];
                        bits |= RIGHT[dot_row];
                    }
                }
                if bits == 0 { continue; }

                let ch = char::from_u32(0x2800 | bits as u32).unwrap_or(' ');
                if let Some(cell) = frame.buffer_mut().cell_mut((
                    inner.x + bar as u16,
                    inner.y + cell_y as u16,
                )) {
                    cell.set_char(ch).set_fg(color);
                }
            }
        }
    }

    fn render_header(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(
                if state.search_active { self.theme.border_active } else { self.theme.border_inactive }
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content = if state.search_active {
            Line::from(vec![
                Span::styled("   Search: ", Style::default().fg(self.theme.border_active)),
                Span::styled(&state.search_query, Style::default().fg(self.theme.text_primary)),
                Span::styled("█", Style::default().fg(self.theme.border_active).add_modifier(Modifier::SLOW_BLINK)),
            ])
        } else if state.search_results.is_some() {
            Line::from(vec![
                Span::styled(" 󰍉  Search Results", Style::default().fg(self.theme.border_active).add_modifier(Modifier::BOLD)),
                Span::styled("  [TAB] switch panel  [ENTER] open  [ESC] close", Style::default().fg(self.theme.border_inactive)),
            ])
        } else {
            Line::from(vec![
                Span::styled("", Style::default().fg(self.theme.border_inactive)),
            ])
        };
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Left), inner);
    }

    fn render_library(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Library;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![Span::raw(" 󰋑 Library ")]).alignment(Alignment::Left))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = LIBRARY_ITEMS.iter().map(|name| {
            ListItem::new(Line::from(vec![Span::raw(format!("  {name} "))]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.library_list);
    }

    fn render_playlists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Playlists;
        let pb = &state.playback;

        let status_icon = if pb.is_playing { "Playing" } else { "Paused" };
        let repeat_str = match pb.repeat {
            RepeatState::Off     => String::new(),
            RepeatState::Context => " 󰑖 Rep ".to_string(),
            RepeatState::Track   => " 󰑘 Rep1 ".to_string(),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![Span::raw(" 󰲚 Playlists ")]).alignment(Alignment::Left))
            .title_bottom(Line::from(vec![
                Span::styled(format!(" Vol: {}% ", pb.volume), Style::default().fg(self.theme.border_inactive)),
                Span::styled(format!(" {} ", status_icon), Style::default().fg(self.theme.border_inactive)),
                Span::styled(repeat_str, Style::default().fg(self.theme.border_active)),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state.playlists.iter().map(|p| {
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {} ", p.name)),
                Span::styled(format!("({})", p.total_tracks), Style::default().fg(self.theme.border_inactive)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.playlist_list);
    }

    fn render_now_playing(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.playback.is_local {
            return self.render_local_now_playing(frame, state, area);
        }

        let focused = state.focus == Focus::Tracks;
        let accent = if focused { self.theme.border_active } else { self.theme.border_inactive };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰎈 Now Playing ")
            .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(accent));

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 { return; }

        let viz_h: u16 = inner.height.min(8).max(4);
        let info_min: u16 = 8;
        let art_h = inner.height
            .saturating_sub(info_min)
            .saturating_sub(viz_h)
            .min(inner.width / 2);
        let art_w = art_h * 2;
        let info_h = inner.height.saturating_sub(art_h).saturating_sub(viz_h);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(art_h),
                Constraint::Length(info_h),
                Constraint::Length(viz_h),
            ])
            .split(inner);

        let art_area  = sections[0];
        let info_area = sections[1];
        let viz_area  = sections[2];

        let padding = art_area.width.saturating_sub(art_w) / 2;
        let art_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(padding),
                Constraint::Length(art_w),
                Constraint::Min(0),
            ])
            .split(art_area);

        if let Some(art) = &mut state.album_art {
            if let Some(img_state) = &mut art.image_state {
                frame.render_stateful_widget(
                    StatefulImage::<StatefulProtocol>::default(),
                    art_cols[1],
                    img_state,
                );
            }
        }

        if info_h > 0 {
            let pb = &state.playback;
            let repeat_icon = match pb.repeat {
                RepeatState::Off     => "󰑗",
                RepeatState::Context => "󰑖",
                RepeatState::Track   => "󰑘",
            };
            let shuffle_icon = if pb.shuffle { "󰒝" } else { "󰒞" };
            let play_icon    = if pb.is_playing { "󰏦" } else { "󰐍" };
            let radio_icon   = if pb.radio_mode { "  󰐇" } else { "" };

            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(pb.title.clone(), Style::default().fg(self.theme.text_primary).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(Span::styled(pb.artist.clone(), Style::default().fg(self.theme.border_inactive))),
                Line::from(Span::styled(pb.album.clone(),  Style::default().fg(self.theme.border_inactive))),
                Line::from(""),
                Line::from(Span::styled(
                    format!("{}  {}  {}  vol {}%{}", play_icon, shuffle_icon, repeat_icon, pb.volume, radio_icon),
                    Style::default().fg(self.theme.border_inactive),
                )),
            ];

            frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), info_area);
        }

        let viz_bands = state.viz_bands.clone();
        let pb = state.playback.clone();
        self.render_visualizer(frame, &pb, &viz_bands, viz_area, state);
    }

    fn render_local_now_playing(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;
        let accent = if focused { self.theme.border_active } else { self.theme.border_inactive };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![
                Span::styled("  Local Files ", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            ]))
            .border_style(Style::default().fg(accent));

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 { return; }

        let pb = &state.playback;
        let viz_h  = (inner.height / 3).max(4);
        let info_h = inner.height.saturating_sub(viz_h);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(info_h), Constraint::Length(viz_h)])
            .split(inner);

        let repeat_icon = match pb.repeat {
            RepeatState::Off     => "󰑗",
            RepeatState::Context => "󰑖",
            RepeatState::Track   => "󰑘",
        };
        let shuffle_icon = if pb.shuffle { "󰒝" } else { "󰒞" };
        let play_icon    = if pb.is_playing { "󰏦" } else { "󰐍" };
        let ext_hint = pb.path
            .as_ref()
            .map(|p| format!("  {}", p))
            .unwrap_or_default();

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(pb.title.clone(), Style::default().fg(self.theme.text_primary).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled(
                if pb.artist.is_empty() { "Unknown Artist".to_string() } else { pb.artist.clone() },
                Style::default().fg(accent),
            )),
            Line::from(Span::styled(ext_hint, Style::default().fg(self.theme.border_inactive))),
            Line::from(""),
            Line::from(Span::styled(
                format!("{}  {}  {}  vol {}%", play_icon, shuffle_icon, repeat_icon, pb.volume),
                Style::default().fg(self.theme.border_inactive),
            )),
        ];

        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), sections[0]);
        self.render_visualizer(frame, &state.playback, &state.viz_bands, sections[1], state);
    }

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.theme.border_inactive));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(" 󰓇  isi-music", Style::default().fg(self.theme.border_active).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled("Select a playlist from the Library or Playlists panel,", Style::default().fg(self.theme.border_inactive))),
            Line::from(Span::styled("or press / to search Spotify.", Style::default().fg(self.theme.border_inactive))),
            Line::from(""),
            Line::from(Span::styled("[TAB] navigate panels   [ENTER] select   [/] search", Style::default().fg(self.theme.border_inactive).add_modifier(Modifier::DIM))),
        ];

        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }

    fn render_tracks(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let title = if state.active_playlist_uri.as_deref() == Some("liked_songs") {
            " Liked Songs ".to_string()
        } else {
            " Tracks ".to_string()
        };

        let count = if state.tracks_total > 0 {
            format!("{}/{}", state.tracks.len(), state.tracks_total)
        } else {
            state.tracks.len().to_string()
        };
        let loading = if state.tracks_loading { " …" } else { "" };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title.as_str())
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count}{loading} "), Style::default().fg(self.theme.border_inactive)),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state.tracks.iter().enumerate().map(|(idx, t)| {
            let is_playing = state.playback.title == t.name;
            let style = if is_playing {
                Style::default().fg(self.theme.border_active).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.text_primary)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(self.theme.border_inactive)),
                Span::styled(t.name.clone(), style),
                Span::styled(format!("  󰠃 {}", t.artist), Style::default().fg(self.theme.border_inactive)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.track_list);
    }

    fn render_albums(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let count = if state.albums_total > 0 {
            format!("{}/{}", state.albums.len(), state.albums_total)
        } else {
            state.albums.len().to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Albums ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count} "), Style::default().fg(self.theme.border_inactive)),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state.albums.iter().enumerate().map(|(idx, a)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(self.theme.border_inactive)),
                Span::raw(a.name.clone()),
                Span::styled(format!("  󰠃 {}", a.artist), Style::default().fg(self.theme.border_inactive)),
                Span::styled(format!(" ({} tracks)", a.total_tracks), Style::default().fg(self.theme.border_inactive).add_modifier(Modifier::DIM)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.album_list);
    }

    fn render_artists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Artists ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {} ", state.artists.len()), Style::default().fg(self.theme.border_inactive)),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state.artists.iter().enumerate().map(|(idx, a)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(self.theme.border_inactive)),
                Span::raw(a.name.clone()),
                Span::styled(
                    if a.genres.is_empty() { String::new() } else { format!("  {}", a.genres) },
                    Style::default().fg(self.theme.border_inactive),
                ),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.artist_list);
    }

    fn render_shows(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let count = if state.shows_total > 0 {
            format!("{}/{}", state.shows.len(), state.shows_total)
        } else {
            state.shows.len().to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Podcasts ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count} "), Style::default().fg(self.theme.border_inactive)),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state.shows.iter().enumerate().map(|(idx, s)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(self.theme.border_inactive)),
                Span::raw(s.name.clone()),
                Span::styled(format!("  {}", s.publisher), Style::default().fg(self.theme.border_inactive)),
                Span::styled(format!(" ({} eps)", s.total_episodes), Style::default().fg(self.theme.border_inactive).add_modifier(Modifier::DIM)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.show_list);
    }

    fn render_search_panels(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);

        let bot_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);

        let focused_panel   = state.search_results.as_ref().map(|sr| sr.panel).unwrap_or(SearchPanel::Tracks);
        let is_search_focus = state.focus == Focus::Search;
        let is_loading      = state.search_results.as_ref().map(|sr| sr.loading).unwrap_or(false);

        let panel_border = |panel: SearchPanel| -> Style {
            if is_search_focus && focused_panel == panel {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            }
        };

        let panel_title = |panel: SearchPanel, base: &'static str| -> String {
            if is_loading && focused_panel == panel { format!("{base} …") } else { base.to_string() }
        };

        if let Some(sr) = &mut state.search_results {
            let track_items: Vec<ListItem> = sr.tracks.iter().enumerate().map(|(idx, t)| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰓇 ", Style::default().fg(self.theme.border_active)),
                    Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(self.theme.border_inactive)),
                    Span::raw(t.name.clone()),
                    Span::styled(format!("  󰠃 {}", t.artist), Style::default().fg(self.theme.border_inactive)),
                ]))
            }).collect();
            let track_list = List::new(track_items)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(panel_title(SearchPanel::Tracks, " 󰎆 Tracks "))
                    .border_style(panel_border(SearchPanel::Tracks)))
                .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(track_list, top_cols[0], &mut sr.track_list);

            let artist_items: Vec<ListItem> = sr.artists.iter().map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰋌 ", Style::default().fg(self.theme.border_active)),
                    Span::raw(a.name.clone()),
                    Span::styled(
                        if a.genres.is_empty() { String::new() } else { format!("  {}", a.genres) },
                        Style::default().fg(self.theme.border_inactive),
                    ),
                ]))
            }).collect();
            let artist_list = List::new(artist_items)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(panel_title(SearchPanel::Artists, " 󰋌 Artists "))
                    .border_style(panel_border(SearchPanel::Artists)))
                .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(artist_list, top_cols[1], &mut sr.artist_list);

            let album_items: Vec<ListItem> = sr.albums.iter().map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰀥 ", Style::default().fg(self.theme.border_active)),
                    Span::raw(a.name.clone()),
                    Span::styled(format!("  󰠃 {}", a.artist), Style::default().fg(self.theme.border_inactive)),
                ]))
            }).collect();
            let album_list = List::new(album_items)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(panel_title(SearchPanel::Albums, " 󰀥 Albums "))
                    .border_style(panel_border(SearchPanel::Albums)))
                .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(album_list, bot_cols[0], &mut sr.album_list);

            let pl_items: Vec<ListItem> = sr.playlists.iter().map(|p| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰲚 ", Style::default().fg(self.theme.border_active)),
                    Span::raw(p.name.clone()),
                    Span::styled(format!("  ({})", p.total_tracks), Style::default().fg(self.theme.border_inactive)),
                ]))
            }).collect();
            let pl_list = List::new(pl_items)
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                    .title(panel_title(SearchPanel::Playlists, " 󰲚 Playlists "))
                    .border_style(panel_border(SearchPanel::Playlists)))
                .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(pl_list, bot_cols[1], &mut sr.playlist_list);
        }
    }

    fn render_progress(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let shuffle_label = if pb.shuffle { "  󰒝 Shuf" } else { "" };
        let shuffle_width = if pb.shuffle { 9u16 } else { 0u16 };
        let width  = area.width.saturating_sub(14 + shuffle_width) as usize;
        let filled = (width as f64 * ratio) as usize;

        let bar = format!(
            "{}{}{}",
            "⣿".repeat(filled),
            "⡷",
            "⠶".repeat(width.saturating_sub(filled))
        );

        let content = Line::from(vec![
            Span::styled(fmt_duration(pb.progress_ms), Style::default().fg(self.theme.accent_color).add_modifier(Modifier::ITALIC)),
            Span::raw(" "),
            Span::styled(bar, Style::default().fg(self.theme.accent_color)),
            Span::raw(" "),
            Span::styled(fmt_duration(pb.duration_ms), Style::default().fg(self.theme.accent_color).add_modifier(Modifier::ITALIC)),
            Span::styled(shuffle_label, Style::default().fg(self.theme.accent_color)),
        ]);
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }

    fn render_marquee(&self, frame: &mut Frame, pb: &PlaybackState, offset: usize, area: Rect) {
        let text = if pb.title.is_empty() {
            "isi-music v0.2.7".to_string()
        } else {
            format!("{} • {} ", pb.title, pb.artist)
        };
        let display = if text.len() < area.width as usize {
            text
        } else {
            let combined = format!("{}   •   ", text);
            let chars: Vec<char> = combined.chars().collect();
            (0..area.width as usize).map(|i| chars[(offset + i) % chars.len()]).collect()
        };
        frame.render_widget(
            Paragraph::new(display).style(Style::default().fg(self.theme.border_inactive)),
            area,
        );
    }

    fn render_album_art(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰋩 Cover ")
            .border_style(Style::default().fg(self.theme.border_inactive));

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 { return; }

        let img_h = inner.height.min(inner.width / 2);
        let img_w = img_h * 2;
        let padding = inner.width.saturating_sub(img_w) / 2;

        let img_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(padding),
                Constraint::Length(img_w),
                Constraint::Min(0),
            ])
            .split(inner);

        if let Some(art) = &mut state.album_art {
            if let Some(img_state) = &mut art.image_state {
                frame.render_stateful_widget(
                    StatefulImage::<StatefulProtocol>::default(),
                    img_cols[1],
                    img_state,
                );
            }
        }
    }

    fn render_queue(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Queue;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰲸 Queue ")
            .title_bottom(Line::from(Span::styled(
                format!(" {} tracks ", state.queue_items.len()),
                Style::default().fg(self.theme.border_inactive),
            )))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        if state.queue_items.is_empty() {
            frame.render_widget(
                Paragraph::new("  Queue empty — press [A] on a track to add")
                    .block(block)
                    .style(Style::default().fg(self.theme.border_inactive)),
                area,
            );
            return;
        }

        let items: Vec<ListItem> = state.queue_items.iter().enumerate().map(|(idx, (name, artist))| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>2}. ", idx + 1), Style::default().fg(self.theme.border_inactive)),
                Span::styled(name.clone(), Style::default().fg(self.theme.text_primary)),
                Span::styled(format!("  󰠃 {}", artist), Style::default().fg(self.theme.border_inactive)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(self.theme.highlight_bg).fg(self.theme.border_active).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.queue_list);
    }

    fn render_help(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let content = if let Some(msg) = &state.status_msg {
            Line::from(Span::styled(msg.clone(), Style::default().fg(self.theme.border_active)))
        } else if state.focus == Focus::Search {
            Line::from(Span::styled(
                " [TAB] Switch panel  [↑↓] Navigate  [ENTER] Select  [ESC] Close search ",
                Style::default().fg(self.theme.border_inactive),
            ))
        } else if state.search_active {
            Line::from(Span::styled(
                " [ESC] Cancel  [ENTER] Search  [Type] Query ",
                Style::default().fg(self.theme.border_inactive),
            ))
        } else if state.focus == Focus::Queue {
            Line::from(Span::styled(
                " [↑↓] Navigate  [DEL] Remove from queue  [TAB] Focus  [A] Add track ",
                Style::default().fg(self.theme.border_inactive),
            ))
        } else if state.active_content == ActiveContent::LocalFiles {
            Line::from(Span::styled(
                " [↑↓] Navigate  [ENTER] play/expand folder  [A] Add to queue  [N/P] Skip  [SPACE] Pause ",
                Style::default().fg(self.theme.border_inactive),
            ))
        } else if state.previous_search.is_some() {
            Line::from(Span::styled(
                " [hjkl/↑↓] Nav  [SPACE] Play/Pause  [N/P] Skip  [A] Queue  [←→] Seek  [BACKSPACE] Back to search ",
                Style::default().fg(self.theme.border_inactive),
            ))
        } else {
            Line::from(Span::styled(
                " [hjkl/↑↓] Nav  [SPACE] Play/Pause  [N/P] Skip  [S] Shuffle  [R] Repeat  [A] Queue  [C] Cover  [Z] Player  [←→] Seek  [L] Like  [+/-] Vol  [/] Search  [Q] Quit ",
                Style::default().fg(self.theme.border_inactive),
            ))
        };

        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}