use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, List, ListItem, ListState, Paragraph},
    Frame,
};
use rspotify::model::RepeatState;
use std::f64::consts::TAU;

use crate::spotify::{AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, ShowSummary, TrackSummary};

// ── Playback State ────────────────────────────────────────────────────────────

pub struct PlaybackState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub is_playing: bool,
    pub shuffle: bool,
    pub repeat: RepeatState,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            is_playing: false,
            shuffle: false,
            repeat: RepeatState::Off,
            progress_ms: 0,
            duration_ms: 0,
            volume: 100,
        }
    }
}

// ── Panel Focus ───────────────────────────────────────────────────────────────

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

// ── Active Content ────────────────────────────────────────────────────────────

#[derive(Default, PartialEq)]
pub enum ActiveContent {
    #[default]
    None,
    Tracks,
    Albums,
    Artists,
    Shows,
}

// ── Library items (fixed) ─────────────────────────────────────────────────────

const LIBRARY_ITEMS: &[(&str, &str)] = &[
    ("󱍙", "Liked Songs"),
    ("󰀥", "Albums"),
    ("󰋌", "Artists"),
    ("󰦔", "Podcasts"),
];

// ── Search Results ────────────────────────────────────────────────────────────

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
}

impl SearchResults {
    pub fn new(r: FullSearchResults) -> Self {
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

    pub fn selected_playlist(&self) -> Option<&PlaylistSummary> {
        self.playlist_list.selected().and_then(|i| self.playlists.get(i))
    }
}

// ── UI State ──────────────────────────────────────────────────────────────────

pub struct UiState {
    pub focus: Focus,
    // Left panel: Library (fixed 4 items)
    pub library_list: ListState,
    // Left panel: Playlists
    pub playlists: Vec<PlaylistSummary>,
    pub playlist_list: ListState,
    // Right panel: Active content type
    pub active_content: ActiveContent,
    // Right panel: Tracks
    pub tracks: Vec<TrackSummary>,
    pub track_list: ListState,
    pub active_playlist_uri: Option<String>,
    pub active_playlist_id: Option<String>,
    pub tracks_offset: u32,
    pub tracks_total: u32,
    pub tracks_loading: bool,
    // Right panel: Albums
    pub albums: Vec<AlbumSummary>,
    pub album_list: ListState,
    pub albums_offset: u32,
    pub albums_total: u32,
    // Right panel: Artists
    pub artists: Vec<ArtistSummary>,
    pub artist_list: ListState,
    // Right panel: Shows/Podcasts
    pub shows: Vec<ShowSummary>,
    pub show_list: ListState,
    pub shows_offset: u32,
    pub shows_total: u32,
    // Right panel: Search
    pub search_results: Option<SearchResults>,
    // Queue panel
    pub queue_items: Vec<(String, String)>, // (name, artist)
    pub queue_list: ListState,
    // Playback
    pub playback: PlaybackState,
    pub status_msg: Option<String>,
    pub search_query: String,
    pub search_active: bool,
    // Animation
    pub spin_angle: f64,
    pub marquee_offset: usize,
    pub marquee_ms: u64,
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
            shows: Vec::new(),
            show_list: ListState::default(),
            shows_offset: 0,
            shows_total: 0,
            search_results: None,
            queue_items: Vec::new(),
            queue_list: ListState::default(),
            playback: PlaybackState::default(),
            status_msg: None,
            search_query: String::new(),
            search_active: false,
            spin_angle: 0.0,
            marquee_offset: 0,
            marquee_ms: 0,
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
                ActiveContent::Albums  => scroll_up(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists => scroll_up(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows   => scroll_up(&mut self.show_list, self.shows.len()),
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
                ActiveContent::Albums  => scroll_down(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists => scroll_down(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows   => scroll_down(&mut self.show_list, self.shows.len()),
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
                ActiveContent::Albums  => { if !self.albums.is_empty()  { self.album_list.select(Some(0));  } }
                ActiveContent::Artists => { if !self.artists.is_empty() { self.artist_list.select(Some(0)); } }
                ActiveContent::Shows   => { if !self.shows.is_empty()   { self.show_list.select(Some(0));   } }
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
                ActiveContent::Albums  => { let n = self.albums.len();  if n > 0 { self.album_list.select(Some(n - 1));  } }
                ActiveContent::Artists => { let n = self.artists.len(); if n > 0 { self.artist_list.select(Some(n - 1)); } }
                ActiveContent::Shows   => { let n = self.shows.len();   if n > 0 { self.show_list.select(Some(n - 1));   } }
                _ => { let n = self.tracks.len(); if n > 0 { self.track_list.select(Some(n - 1)); } }
            },
            Focus::Search => { if let Some(sr) = &mut self.search_results { let n = sr.current_len(); if n > 0 { sr.current_list_mut().select(Some(n - 1)); } } }
            Focus::Queue  => { let n = self.queue_items.len(); if n > 0 { self.queue_list.select(Some(n - 1)); } }
        }
    }

    /// Tab: Library → Playlists → Tracks → Queue → Library
    pub fn switch_focus(&mut self) {
        self.search_active = false;
        self.focus = match self.focus {
            Focus::Library   => Focus::Playlists,
            Focus::Playlists => if self.search_results.is_some() { Focus::Search } else { Focus::Tracks },
            Focus::Tracks    => Focus::Queue,
            Focus::Queue | Focus::Search => Focus::Library,
        };
    }

    /// Cycle search panel (Tab when focused on Search)
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

// ── UI Implementation ─────────────────────────────────────────────────────────

pub struct Ui;

impl Ui {
    pub fn new() -> Self { Self }

    pub fn render(&self, frame: &mut Frame, state: &mut UiState) {
        let area = frame.area();

        let root = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(10),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(root[0]);

        let main_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(root[1]);

        // Left panel: Library (top 6 rows) + Playlists (rest) + Queue (bottom 8 rows)
        let left_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0), Constraint::Length(8)])
            .split(main_cols[0]);

        self.render_visualizer(frame, &state.playback, top_cols[0]);
        self.render_header(frame, state, top_cols[1]);
        self.render_library(frame, state, left_rows[0]);
        self.render_playlists(frame, state, left_rows[1]);
        self.render_queue(frame, state, left_rows[2]);

        if state.search_results.is_some() {
            self.render_search_panels(frame, state, main_cols[1]);
        } else {
            match &state.active_content {
                ActiveContent::None    => self.render_welcome(frame, main_cols[1]),
                ActiveContent::Tracks  => self.render_tracks(frame, state, main_cols[1]),
                ActiveContent::Albums  => self.render_albums(frame, state, main_cols[1]),
                ActiveContent::Artists => self.render_artists(frame, state, main_cols[1]),
                ActiveContent::Shows   => self.render_shows(frame, state, main_cols[1]),
            }
        }

        let playback_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(root[2]);

        self.render_progress(frame, &state.playback, playback_row[1]);
        self.render_marquee(frame, &state.playback, state.marquee_offset, playback_row[0]);
        self.render_help(frame, state, root[3]);
    }

    // ── Visualizer ────────────────────────────────────────────────────────────

    fn render_visualizer(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 { return; }

        let title_seed = pb.title.chars().map(|c| c as u32).sum::<u32>() as f64;
        let t = pb.progress_ms as f64 / 400.0;
        let w = inner.width as f64;

        // Sub-cell precision: each cell = 8 units (▁▂▃▄▅▆▇█)
        let partial_chars = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇'];

        for x in 0..inner.width {
            let x_f = x as f64;
            // Each column has a fixed frequency/phase from its position — no traveling wave
            let freq  = 0.5 + (x_f / w) * 3.0 + (x_f * 0.37 + title_seed * 0.01).sin() * 0.5;
            let phase = x_f * 2.1 + title_seed * 0.07;

            let amplitude = if pb.is_playing {
                let a = (t * freq + phase).sin().abs();
                let b = (t * freq * 0.53 + phase + 1.3).cos().abs();
                let c = (t * freq * 1.7 + phase * 0.4).sin().abs();
                (a * 0.5 + b * 0.3 + c * 0.2).clamp(0.03, 1.0)
            } else {
                0.03
            };

            // Convert amplitude to units (height * 8 for sub-cell resolution)
            let max_units = inner.height * 8;
            let bar_units = ((amplitude * max_units as f64) as u16).min(max_units);
            let full_cells = bar_units / 8;
            let partial    = (bar_units % 8) as usize;

            for y in 0..inner.height {
                let pos_x = inner.x + x;
                let pos_y = inner.y + inner.height - 1 - y;

                let ch = if y < full_cells {
                    '█'
                } else if y == full_cells && partial > 0 {
                    partial_chars[partial]
                } else {
                    continue; // empty — skip writing to buffer
                };

                if let Some(cell) = frame.buffer_mut().cell_mut((pos_x, pos_y)) {
                    cell.set_char(ch).set_fg(Color::Green);
                }
            }
        }
    }

    // ── Header ────────────────────────────────────────────────────────────────

    fn render_header(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let pb = &state.playback;

        let repeat_label = match pb.repeat {
            RepeatState::Off     => "",
            RepeatState::Context => "  󰑖 Rep",
            RepeatState::Track   => "  󰑘 Rep1",
        };
        let shuffle_label = if pb.shuffle { "  󰒝 Shuf" } else { "" };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if state.search_active { Color::Green } else { Color::DarkGray }));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content = if state.search_active {
            Line::from(vec![
                Span::styled("   Search: ", Style::default().fg(Color::Green)),
                Span::styled(&state.search_query, Style::default().fg(Color::White)),
                Span::styled("█", Style::default().fg(Color::Green).add_modifier(Modifier::SLOW_BLINK)),
            ])
        } else if !pb.title.is_empty() {
            Line::from(vec![
                Span::styled(" 󰓇  ", Style::default().fg(Color::Green)),
                Span::styled(&pb.title, Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("  󰠃 {}", pb.artist), Style::default().fg(Color::DarkGray)),
                Span::styled(repeat_label, Style::default().fg(Color::Green)),
                Span::styled(shuffle_label, Style::default().fg(Color::Green)),
            ])
        } else {
            Line::from(vec![
                Span::styled(" 󰓇  ", Style::default().fg(Color::DarkGray)),
                Span::styled("No music playing", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            ])
        };
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Left), inner);
    }

    // ── Library ───────────────────────────────────────────────────────────────

    fn render_library(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Library;
        let pb = &state.playback;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![
                Span::raw(" 󰋑 Library "),
            ]).alignment(Alignment::Left))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = LIBRARY_ITEMS.iter().map(|(icon, name)| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("  {icon} {name} ")),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.library_list);
    }

    // ── Playlists ─────────────────────────────────────────────────────────────

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
            .title(Line::from(vec![
                Span::raw(" 󰲚 Playlists "),
            ]).alignment(Alignment::Left))
            .title_bottom(Line::from(vec![
                Span::styled(format!(" Vol: {}% ", pb.volume), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" {} ", status_icon), Style::default().fg(Color::DarkGray)),
                Span::styled(repeat_str, Style::default().fg(Color::Green)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.playlists.iter().map(|p| {
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {} ", p.name)),
                Span::styled(format!("({})", p.total_tracks), Style::default().fg(Color::DarkGray)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.playlist_list);
    }

    // ── Welcome ───────────────────────────────────────────────────────────────

    fn render_welcome(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                " 󰓇  isi-music",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Select a playlist from the Library or Playlists panel,",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "or press / to search Spotify.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "[TAB] navigate panels   [ENTER] select   [/] search",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            )),
        ];

        frame.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            inner,
        );
    }

    // ── Tracks ────────────────────────────────────────────────────────────────

    fn render_tracks(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let title = if state.active_playlist_uri.as_deref() == Some("liked_songs") {
            " 󱍙 Liked Songs ".to_string()
        } else {
            " 󰎆 Tracks ".to_string()
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
                Span::styled(format!(" {count}{loading} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.tracks.iter().enumerate().map(|(idx, t)| {
            let is_playing = state.playback.title == t.name;
            let style = if is_playing {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(if is_playing { " 󰓇 " } else { "   " }, Style::default().fg(Color::Green)),
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(t.name.clone(), style),
                Span::styled(format!("  󰠃 {}", t.artist), Style::default().fg(Color::DarkGray)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.track_list);
    }

    // ── Albums ────────────────────────────────────────────────────────────────

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
            .title(" 󰀥 Albums ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.albums.iter().enumerate().map(|(idx, a)| {
            ListItem::new(Line::from(vec![
                Span::styled(" 󰀥 ", Style::default().fg(Color::Green)),
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                Span::raw(a.name.clone()),
                Span::styled(format!("  󰠃 {}", a.artist), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" ({} tracks)", a.total_tracks), Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.album_list);
    }

    // ── Artists ───────────────────────────────────────────────────────────────

    fn render_artists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let count = state.artists.len().to_string();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰋌 Artists ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.artists.iter().enumerate().map(|(idx, a)| {
            ListItem::new(Line::from(vec![
                Span::styled(" 󰋌 ", Style::default().fg(Color::Green)),
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                Span::raw(a.name.clone()),
                Span::styled(
                    if a.genres.is_empty() { String::new() } else { format!("  {}", a.genres) },
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.artist_list);
    }

    // ── Shows/Podcasts ────────────────────────────────────────────────────────

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
            .title(" 󰦔 Podcasts ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.shows.iter().enumerate().map(|(idx, s)| {
            ListItem::new(Line::from(vec![
                Span::styled(" 󰦔 ", Style::default().fg(Color::Green)),
                Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                Span::raw(s.name.clone()),
                Span::styled(format!("  {}", s.publisher), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" ({} eps)", s.total_episodes), Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.show_list);
    }

    // ── Search Panels (4 columns) ─────────────────────────────────────────────

    fn render_search_panels(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(area);

        let focused_panel = state.search_results.as_ref().map(|sr| sr.panel).unwrap_or(SearchPanel::Tracks);
        let is_search_focus = state.focus == Focus::Search;

        let panel_border = |panel: SearchPanel| -> Style {
            if is_search_focus && focused_panel == panel {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            }
        };

        if let Some(sr) = &mut state.search_results {
            // Tracks
            let track_items: Vec<ListItem> = sr.tracks.iter().enumerate().map(|(idx, t)| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰓇 ", Style::default().fg(Color::Green)),
                    Span::styled(format!("{:>3}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                    Span::raw(t.name.clone()),
                    Span::styled(format!("  󰠃 {}", t.artist), Style::default().fg(Color::DarkGray)),
                ]))
            }).collect();
            let track_block = Block::default()
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .title(" 󰎆 Tracks ").border_style(panel_border(SearchPanel::Tracks));
            let track_list = List::new(track_items).block(track_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(track_list, cols[0], &mut sr.track_list);

            // Artists
            let artist_items: Vec<ListItem> = sr.artists.iter().map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰋌 ", Style::default().fg(Color::Green)),
                    Span::raw(a.name.clone()),
                    Span::styled(
                        if a.genres.is_empty() { String::new() } else { format!("  {}", a.genres) },
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            }).collect();
            let artist_block = Block::default()
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .title(" 󰋌 Artists ").border_style(panel_border(SearchPanel::Artists));
            let artist_list = List::new(artist_items).block(artist_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(artist_list, cols[1], &mut sr.artist_list);

            // Albums
            let album_items: Vec<ListItem> = sr.albums.iter().map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰀥 ", Style::default().fg(Color::Green)),
                    Span::raw(a.name.clone()),
                    Span::styled(format!("  󰠃 {}", a.artist), Style::default().fg(Color::DarkGray)),
                ]))
            }).collect();
            let album_block = Block::default()
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .title(" 󰀥 Albums ").border_style(panel_border(SearchPanel::Albums));
            let album_list = List::new(album_items).block(album_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(album_list, cols[2], &mut sr.album_list);

            // Playlists
            let playlist_items: Vec<ListItem> = sr.playlists.iter().map(|p| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰲚 ", Style::default().fg(Color::Green)),
                    Span::raw(p.name.clone()),
                    Span::styled(format!("  ({})", p.total_tracks), Style::default().fg(Color::DarkGray)),
                ]))
            }).collect();
            let pl_block = Block::default()
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .title(" 󰲚 Playlists ").border_style(panel_border(SearchPanel::Playlists));
            let pl_list = List::new(playlist_items).block(pl_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(pl_list, cols[3], &mut sr.playlist_list);
        }
    }

    // ── Progress bar ──────────────────────────────────────────────────────────

    fn render_progress(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let width = area.width.saturating_sub(14) as usize;
        let filled = (width as f64 * ratio) as usize;

        let bar = format!(
            "{}{}{}",
            "⣿".repeat(filled),
            "⡷",
            "⠶".repeat(width.saturating_sub(filled))
        );

        let content = Line::from(vec![
            Span::styled(
                fmt_duration(pb.progress_ms),
                Style::default().fg(Color::Green).add_modifier(Modifier::ITALIC),
            ),
            Span::raw(" "),
            Span::styled(bar, Style::default().fg(Color::Green)),
            Span::raw(" "),
            Span::styled(
                fmt_duration(pb.duration_ms),
                Style::default().fg(Color::Green).add_modifier(Modifier::ITALIC),
            ),
        ]);
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }

    // ── Marquee ───────────────────────────────────────────────────────────────

    fn render_marquee(&self, frame: &mut Frame, pb: &PlaybackState, offset: usize, area: Rect) {
        let text = if pb.title.is_empty() {
            "isi-music v0.1.0".to_string()
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
            Paragraph::new(display).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }

    // ── Queue ─────────────────────────────────────────────────────────────────

    fn render_queue(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Queue;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰲸 Queue ")
            .title_bottom(Line::from(Span::styled(
                format!(" {} tracks ", state.queue_items.len()),
                Style::default().fg(Color::DarkGray),
            )))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        if state.queue_items.is_empty() {
            frame.render_widget(
                Paragraph::new("  Queue empty — press [A] on a track to add")
                    .block(block)
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }

        let items: Vec<ListItem> = state.queue_items.iter().enumerate().map(|(idx, (name, artist))| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>2}. ", idx + 1), Style::default().fg(Color::DarkGray)),
                Span::styled(name.clone(), Style::default().fg(Color::White)),
                Span::styled(format!("  󰠃 {}", artist), Style::default().fg(Color::DarkGray)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.queue_list);
    }

    // ── Help / Status ─────────────────────────────────────────────────────────

    fn render_help(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let content = if let Some(msg) = &state.status_msg {
            Line::from(Span::styled(msg.clone(), Style::default().fg(Color::Green)))
        } else if state.focus == Focus::Search {
            Line::from(Span::styled(
                " [TAB] Switch panel  [↑↓] Navigate  [ENTER] Select  [ESC] Close search ",
                Style::default().fg(Color::DarkGray),
            ))
        } else if state.search_active {
            Line::from(Span::styled(
                " [ESC] Cancel  [ENTER] Search  [Type] Query ",
                Style::default().fg(Color::DarkGray),
            ))
        } else if state.focus == Focus::Queue {
            Line::from(Span::styled(
                " [↑↓] Navigate  [DEL] Remove from queue  [TAB] Focus  [A] Add track ",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::styled(
                " [hjkl/↑↓] Navigate  [SPACE] Play/Pause  [N/P] Skip  [A] Queue  [←→] Seek  [L] Like  [+/-] Vol  [/] Search  [TAB] Focus  [Q] Quit ",
                Style::default().fg(Color::DarkGray),
            ))
        };

        frame.render_widget(
            Paragraph::new(content).alignment(Alignment::Center),
            area,
        );
    }
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}
