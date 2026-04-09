use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, List, ListItem, ListState, Paragraph},
    Frame,
};
use rspotify::model::RepeatState;
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};
use crate::spotify::{AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, ShowSummary, TrackSummary};


pub struct AlbumArtData {
    pub image_state: Option<StatefulProtocol>,
}

#[derive(Clone, Debug)]
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
    pub art_url: Option<String>,
    pub is_local: bool,
    pub radio_mode: bool,
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
            art_url: None,
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
    pub queue_items: Vec<(String, String)>, // (name, artist)
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

pub struct Ui;

impl Ui {
    pub fn new() -> Self { Self }

    pub fn render(&self, frame: &mut Frame, state: &mut UiState) {
        let area = frame.area();

        if state.fullscreen_player && !state.playback.title.is_empty() {
            let root = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(12),  // art + compact info strip
                    Constraint::Min(5),      // visualizer — gets all remaining height
                    Constraint::Length(1),   // help
                ])
                .split(area);
            self.render_player_compact(frame, state, root[0]);
            self.render_visualizer(frame, &state.playback, &state.viz_bands, root[1]);
            self.render_help(frame, state, root[2]);
            return;
        }

        let showing_now_playing = state.search_results.is_none()
            && state.active_content == ActiveContent::None
            && !state.playback.title.is_empty();

        if showing_now_playing {
            let root = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),   // header (full width)
                    Constraint::Min(10),     // main panels
                    Constraint::Length(2),   // progress bar + volume
                    Constraint::Length(1),   // help
                ])
                .split(area);

            let main_cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                .split(root[1]);

            let left_rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(7), Constraint::Min(0)])
                .split(main_cols[0]);

            let right_rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(8)])
                .split(main_cols[1]);

            self.render_header(frame, state, root[0]);
            self.render_library(frame, state, left_rows[0]);
            self.render_playlists(frame, state, left_rows[1]);
            self.render_now_playing(frame, state, right_rows[0]);
            self.render_queue(frame, state, right_rows[1]);
            self.render_progress(frame, &state.playback, root[2]);
            self.render_help(frame, state, root[3]);
            return;
        }

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

        let art_h = if state.show_album_art { Constraint::Length(16) } else { Constraint::Length(0) };
        let left_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Min(0), art_h])
            .split(main_cols[0]);

        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(8)])
            .split(main_cols[1]);

        self.render_visualizer(frame, &state.playback, &state.viz_bands, top_cols[0]);
        self.render_header(frame, state, top_cols[1]);
        self.render_library(frame, state, left_rows[0]);
        self.render_playlists(frame, state, left_rows[1]);
        if state.show_album_art {
            self.render_album_art(frame, state, left_rows[2]);
        }

        if state.search_results.is_some() {
            self.render_search_panels(frame, state, right_rows[0]);
        } else {
            match &state.active_content {
                ActiveContent::None => {
                    if state.playback.title.is_empty() {
                        self.render_welcome(frame, right_rows[0])
                    } else {
                        self.render_now_playing(frame, state, right_rows[0])
                    }
                }
                ActiveContent::Tracks | ActiveContent::LocalFiles => self.render_tracks(frame, state, right_rows[0]),
                ActiveContent::Albums  => self.render_albums(frame, state, right_rows[0]),
                ActiveContent::Artists => self.render_artists(frame, state, right_rows[0]),
                ActiveContent::Shows   => self.render_shows(frame, state, right_rows[0]),
            }
        }
        self.render_queue(frame, state, right_rows[1]);

        let playback_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(root[2]);
        self.render_progress(frame, &state.playback, playback_row[1]);
        self.render_marquee(frame, &state.playback, state.marquee_offset, playback_row[0]);
        self.render_help(frame, state, root[3]);
    }

    fn render_player_compact(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let art_h = area.height;
        let art_w = (art_h * 2).min(area.width * 2 / 5);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(art_w), Constraint::Min(0)])
            .split(area);

        if let Some(art) = &mut state.album_art {
            if let Some(img_state) = &mut art.image_state {
                let img_h = cols[0].height.min(cols[0].width / 2);
                let img_w = img_h * 2;
                let x_pad = cols[0].width.saturating_sub(img_w) / 2;
                let art_cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(x_pad),
                        Constraint::Length(img_w),
                        Constraint::Min(0),
                    ])
                    .split(cols[0]);
                frame.render_stateful_widget(
                    StatefulImage::<StatefulProtocol>::default(),
                    art_cols[1],
                    img_state,
                );
            }
        }

        let pb = &state.playback;
        let info = cols[1];

        let repeat_icon = match pb.repeat {
            RepeatState::Off     => "󰑗",
            RepeatState::Context => "󰑖",
            RepeatState::Track   => "󰑘",
        };
        let shuffle_icon = if pb.shuffle { "󰒝" } else { "󰒞" };
        let play_icon    = if pb.is_playing { "󰏦" } else { "󰐍" };

        // Inline progress bar sized to the info column
        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bar_w = info.width.saturating_sub(14) as usize;
        let filled = (bar_w as f64 * ratio) as usize;
        let bar = format!(
            "{}⡷{}",
            "⣿".repeat(filled),
            "⠶".repeat(bar_w.saturating_sub(filled))
        );

        let lines: Vec<Line> = vec![
            Line::from(""),
            Line::from(Span::styled(
                truncate(&pb.title, info.width as usize),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                truncate(&pb.artist, info.width as usize),
                Style::default().fg(Color::Green),
            )),
            Line::from(Span::styled(
                truncate(&pb.album, info.width as usize),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(fmt_duration(pb.progress_ms), Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(bar, Style::default().fg(Color::Green)),
                Span::raw(" "),
                Span::styled(fmt_duration(pb.duration_ms), Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(Span::styled(
                format!("{}  {}  {}  vol {}%", play_icon, shuffle_icon, repeat_icon, pb.volume),
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let text_h = lines.len() as u16;
        let y_offset = info.height.saturating_sub(text_h) / 2;
        let text_rect = Rect {
            x: info.x + 2,
            y: info.y + y_offset,
            width: info.width.saturating_sub(2),
            height: text_h.min(info.height),
        };

        frame.render_widget(Paragraph::new(lines), text_rect);
    }

    fn render_visualizer(&self, frame: &mut Frame, pb: &PlaybackState, viz_bands: &[f32], area: Rect) {
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

            let color = if amp > 0.75 { Color::White }
                        else if amp > 0.5 { Color::LightGreen }
                        else if amp > 0.25 { Color::Green }
                        else { Color::DarkGray };

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
        let pb = &state.playback;

        let _repeat_label = match pb.repeat {
            RepeatState::Off     => "",
            RepeatState::Context => "  󰑖 Rep",
            RepeatState::Track   => "  󰑘 Rep1",
        };
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
        } else if state.search_results.is_some() {
            Line::from(vec![
                Span::styled(" 󰍉  Search Results", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("  [TAB] switch panel  [ENTER] open  [ESC] close", Style::default().fg(Color::DarkGray)),
            ])
        } else {
            Line::from(vec![
                Span::styled("", Style::default().fg(Color::DarkGray)),
            ])
        };
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Left), inner);
    }

    fn render_library(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Library;
        let _pb = &state.playback;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![
                Span::raw(" 󰋑 Library "),
            ]).alignment(Alignment::Left))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = LIBRARY_ITEMS.iter().map(|name| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("  {name} ")),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
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

    fn render_now_playing(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.playback.is_local {
            return self.render_local_now_playing(frame, state, area);
        }

        let focused = state.focus == Focus::Tracks;
        let accent = if focused { Color::Green } else { Color::DarkGray };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰎈 Now Playing ")
            .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(accent));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 { return; }

        // Reserve space: art on top, text info in middle, visualizer at bottom
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

        // ── Album art ────────────────────────────────────────────────────────
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
                rspotify::model::RepeatState::Off     => "󰑗",
                rspotify::model::RepeatState::Context => "󰑖",
                rspotify::model::RepeatState::Track   => "󰑘",
            };
            let shuffle_icon = if pb.shuffle { "󰒝" } else { "󰒞" };
            let play_icon    = if pb.is_playing { "󰏦" } else { "󰐍" };
            let radio_icon   = if pb.radio_mode { "  󰐇" } else { "" };

            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    pb.title.clone(),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    pb.artist.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    pb.album.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("{}  {}  {}  vol {}%{}", play_icon, shuffle_icon, repeat_icon, pb.volume, radio_icon),
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            frame.render_widget(
                Paragraph::new(lines).alignment(Alignment::Center),
                info_area,
            );
        }

        let viz_bands = state.viz_bands.clone();
        let pb = state.playback.clone();
        self.render_visualizer(frame, &pb, &viz_bands, viz_area);
    }

    fn render_local_now_playing(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;
        let accent = if focused { Color::Green } else { Color::DarkGray };

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

        let viz_h = (inner.height / 3).max(4);
        let info_h = inner.height.saturating_sub(viz_h);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(info_h), Constraint::Length(viz_h)])
            .split(inner);

        let info_area = sections[0];
        let viz_area  = sections[1];

        let repeat_icon = match pb.repeat {
            rspotify::model::RepeatState::Off     => "󰑗",
            rspotify::model::RepeatState::Context => "󰑖",
            rspotify::model::RepeatState::Track   => "󰑘",
        };
        let shuffle_icon = if pb.shuffle { "󰒝" } else { "󰒞" };
        let play_icon    = if pb.is_playing { "󰏦" } else { "󰐍" };

        let ext_hint = if pb.album.is_empty() { String::new() } else { format!("  {}", pb.album) };

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                pb.title.clone(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                if pb.artist.is_empty() { "Unknown Artist".to_string() } else { pb.artist.clone() },
                Style::default().fg(accent),
            )),
            Line::from(Span::styled(
                ext_hint,
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("{}  {}  {}  vol {}%", play_icon, shuffle_icon, repeat_icon, pb.volume),
                Style::default().fg(Color::DarkGray),
            )),
        ];

        frame.render_widget(
            Paragraph::new(lines).alignment(Alignment::Center),
            info_area,
        );

        self.render_visualizer(frame, &state.playback, &state.viz_bands, viz_area);
    }

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
                Span::styled(format!(" {count} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.albums.iter().enumerate().map(|(idx, a)| {
            ListItem::new(Line::from(vec![
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

    fn render_artists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let count = state.artists.len().to_string();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Artists ")
            .title_bottom(Line::from(vec![
                Span::styled(format!(" {count} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.artists.iter().enumerate().map(|(idx, a)| {
            ListItem::new(Line::from(vec![
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
                Span::styled(format!(" {count} "), Style::default().fg(Color::DarkGray)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.shows.iter().enumerate().map(|(idx, s)| {
            ListItem::new(Line::from(vec![
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

        let focused_panel = state.search_results.as_ref().map(|sr| sr.panel).unwrap_or(SearchPanel::Tracks);
        let is_search_focus = state.focus == Focus::Search;
        let is_loading = state.search_results.as_ref().map(|sr| sr.loading).unwrap_or(false);

        let panel_border = |panel: SearchPanel| -> Style {
            if is_search_focus && focused_panel == panel {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            }
        };

        let panel_title = |panel: SearchPanel, base: &'static str| -> String {
            if is_loading && focused_panel == panel {
                format!("{base} …")
            } else {
                base.to_string()
            }
        };

        if let Some(sr) = &mut state.search_results {
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
                .title(panel_title(SearchPanel::Tracks, " 󰎆 Tracks ")).border_style(panel_border(SearchPanel::Tracks));
            let track_list = List::new(track_items).block(track_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(track_list, top_cols[0], &mut sr.track_list);

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
                .title(panel_title(SearchPanel::Artists, " 󰋌 Artists ")).border_style(panel_border(SearchPanel::Artists));
            let artist_list = List::new(artist_items).block(artist_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(artist_list, top_cols[1], &mut sr.artist_list);

            let album_items: Vec<ListItem> = sr.albums.iter().map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰀥 ", Style::default().fg(Color::Green)),
                    Span::raw(a.name.clone()),
                    Span::styled(format!("  󰠃 {}", a.artist), Style::default().fg(Color::DarkGray)),
                ]))
            }).collect();
            let album_block = Block::default()
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .title(panel_title(SearchPanel::Albums, " 󰀥 Albums ")).border_style(panel_border(SearchPanel::Albums));
            let album_list = List::new(album_items).block(album_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
                .highlight_symbol("  ");
            frame.render_stateful_widget(album_list, bot_cols[0], &mut sr.album_list);

            let playlist_items: Vec<ListItem> = sr.playlists.iter().map(|p| {
                ListItem::new(Line::from(vec![
                    Span::styled(" 󰲚 ", Style::default().fg(Color::Green)),
                    Span::raw(p.name.clone()),
                    Span::styled(format!("  ({})", p.total_tracks), Style::default().fg(Color::DarkGray)),
                ]))
            }).collect();
            let pl_block = Block::default()
                .borders(Borders::ALL).border_type(BorderType::Rounded)
                .title(panel_title(SearchPanel::Playlists, " 󰲚 Playlists ")).border_style(panel_border(SearchPanel::Playlists));
            let pl_list = List::new(playlist_items).block(pl_block)
                .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
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
        let width = area.width.saturating_sub(14 + shuffle_width) as usize;
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
            Span::styled(shuffle_label, Style::default().fg(Color::Green)),
        ]);
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }

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

    fn render_album_art(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰋩 Cover ")
            .border_style(Style::default().fg(Color::DarkGray));

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
        let img_rect = img_cols[1];

        if let Some(art) = &mut state.album_art {
            if let Some(img_state) = &mut art.image_state {
                frame.render_stateful_widget(
                    StatefulImage::<StatefulProtocol>::default(),
                    img_rect,
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
        } else if state.previous_search.is_some() {
            Line::from(Span::styled(
                " [hjkl/↑↓] Nav  [SPACE] Play/Pause  [N/P] Skip  [A] Queue  [←→] Seek  [BACKSPACE] Back to search ",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::styled(
                " [hjkl/↑↓] Nav  [SPACE] Play/Pause  [N/P] Skip  [S] Shuffle  [R] Repeat  [A] Queue  [C] Cover  [Z] Player  [←→] Seek  [L] Like  [+/-] Vol  [/] Search  [Q] Quit ",
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

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}
