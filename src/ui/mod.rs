use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
};
use rspotify::model::RepeatState;

use crate::spotify::{PlaylistSummary, TrackSummary};

// ── Playback state ────────────────────────────────────────────────────────────

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

// ── Panel focus ───────────────────────────────────────────────────────────────

#[derive(PartialEq)]
pub enum Focus {
    Playlists,
    Tracks,
}

// ── UI state ──────────────────────────────────────────────────────────────────

pub struct UiState {
    pub focus: Focus,
    pub playlists: Vec<PlaylistSummary>,
    pub playlist_list: ListState,
    pub tracks: Vec<TrackSummary>,
    pub track_list: ListState,
    pub playback: PlaybackState,
    pub active_playlist_uri: Option<String>,
    pub status_msg: Option<String>,
    pub search_query: String,
    pub search_active: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            focus: Focus::Playlists,
            playlists: Vec::new(),
            playlist_list: ListState::default(),
            tracks: Vec::new(),
            track_list: ListState::default(),
            playback: PlaybackState::default(),
            active_playlist_uri: None,
            status_msg: None,
            search_query: String::new(),
            search_active: false,
        }
    }

    /// Returns (filtered_index → real_index, &item) pairs for playlists.
    pub fn filtered_playlists(&self) -> Vec<(usize, &PlaylistSummary)> {
        if !self.search_active || self.search_query.is_empty() {
            return self.playlists.iter().enumerate().collect();
        }
        let q = self.search_query.to_lowercase();
        self.playlists
            .iter()
            .enumerate()
            .filter(|(_, p)| p.name.to_lowercase().contains(&q))
            .collect()
    }

    /// Returns (filtered_index → real_index, &item) pairs for tracks.
    pub fn filtered_tracks(&self) -> Vec<(usize, &TrackSummary)> {
        if !self.search_active || self.search_query.is_empty() {
            return self.tracks.iter().enumerate().collect();
        }
        let q = self.search_query.to_lowercase();
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.name.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn selected_playlist(&self) -> Option<&PlaylistSummary> {
        let filtered = self.filtered_playlists();
        self.playlist_list.selected().and_then(|i| filtered.get(i)).map(|(_, p)| *p)
    }

    /// Returns the *real* index in `self.tracks` of the selected track.
    pub fn selected_track_index(&self) -> Option<usize> {
        let filtered = self.filtered_tracks();
        self.track_list.selected().and_then(|i| filtered.get(i)).map(|(idx, _)| *idx)
    }

    pub fn nav_up(&mut self) {
        match self.focus {
            Focus::Playlists => {
                let len = self.filtered_playlists().len();
                scroll_up(&mut self.playlist_list, len);
            }
            Focus::Tracks => {
                let len = self.filtered_tracks().len();
                scroll_up(&mut self.track_list, len);
            }
        }
    }

    pub fn nav_down(&mut self) {
        match self.focus {
            Focus::Playlists => {
                let len = self.filtered_playlists().len();
                scroll_down(&mut self.playlist_list, len);
            }
            Focus::Tracks => {
                let len = self.filtered_tracks().len();
                scroll_down(&mut self.track_list, len);
            }
        }
    }

    pub fn switch_focus(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.focus = match self.focus {
            Focus::Playlists => Focus::Tracks,
            Focus::Tracks => Focus::Playlists,
        };
    }

    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        // Reset selection to top of filtered list
        match self.focus {
            Focus::Playlists => self.playlist_list.select(Some(0)),
            Focus::Tracks => self.track_list.select(Some(0)),
        }
    }

    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
        // Reset to first result when query changes
        match self.focus {
            Focus::Playlists => self.playlist_list.select(Some(0)),
            Focus::Tracks => self.track_list.select(Some(0)),
        }
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
        match self.focus {
            Focus::Playlists => self.playlist_list.select(Some(0)),
            Focus::Tracks => self.track_list.select(Some(0)),
        }
    }
}

fn scroll_up(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let next = state.selected().map(|i| if i == 0 { len - 1 } else { i - 1 }).unwrap_or(0);
    state.select(Some(next));
}

fn scroll_down(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let next = state.selected().map(|i| if i >= len - 1 { 0 } else { i + 1 }).unwrap_or(0);
    state.select(Some(next));
}

// ── Rendering ─────────────────────────────────────────────────────────────────

pub struct Ui;

impl Ui {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, frame: &mut Frame, state: &mut UiState) {
        let area = frame.area();

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        // Main panel: playlists | tracks
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(rows[0]);

        self.render_playlists(frame, state, cols[0].into());
        self.render_tracks(frame, state, cols[1].into());
        self.render_player(frame, &state.playback, rows[1].into());
        self.render_help(
            frame,
            &state.focus,
            &state.status_msg,
            state.search_active,
            &state.search_query.clone(),
            rows[2].into(),
        );
    }

    fn render_playlists(&self, frame: &mut Frame, state: &mut UiState, area: ratatui::layout::Rect) {
        let focused = state.focus == Focus::Playlists;
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Playlists ")
            .style(border_style(focused));

        if state.playlists.is_empty() {
            let msg = Paragraph::new("Loading...")
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        // Collect owned items first to release the immutable borrow before render_stateful_widget
        let items: Option<Vec<ListItem>> = {
            let filtered = state.filtered_playlists();
            if filtered.is_empty() {
                None
            } else {
                Some(
                    filtered
                        .iter()
                        .map(|(_, p)| {
                            ListItem::new(Line::from(vec![
                                Span::raw(p.name.clone()),
                                Span::styled(
                                    format!(" ({})", p.total_tracks),
                                    Style::default().fg(Color::DarkGray),
                                ),
                            ]))
                        })
                        .collect(),
                )
            }
        };

        let Some(items) = items else {
            let msg = Paragraph::new("No results")
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        };

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut state.playlist_list);
    }

    fn render_tracks(&self, frame: &mut Frame, state: &mut UiState, area: ratatui::layout::Rect) {
        let focused = state.focus == Focus::Tracks;

        let title = state
            .selected_playlist()
            .map(|p| format!(" {} ", p.name))
            .unwrap_or_else(|| " Tracks ".to_string());

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(border_style(focused));

        if state.tracks.is_empty() {
            let msg = if state.active_playlist_uri.is_some() {
                "No tracks"
            } else {
                "← Select a playlist"
            };
            let p = Paragraph::new(msg)
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(p, area);
            return;
        }

        let filtered = state.filtered_tracks();

        if filtered.is_empty() {
            let msg = Paragraph::new("No results")
                .block(block)
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }

        let items: Vec<ListItem> = state.filtered_tracks()
            .iter()
            .map(|(_, t)| {
                let name = t.name.clone(); 
                let detail = format!("  {} — {}", t.artist, fmt_duration(t.duration_ms));

                ListItem::new(Line::from(vec![
                    Span::styled(name, Style::default().fg(Color::White)),
                    Span::styled(detail, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(" ");

        frame.render_stateful_widget(list, area, &mut state.track_list);
    }

    fn render_player(&self, frame: &mut Frame, pb: &PlaybackState, area: ratatui::layout::Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Length(2)])
            .split(area);

        let status = if pb.is_playing { "▶" } else { "⏸" };
        let shuffle_icon = if pb.shuffle { " ⇄" } else { "" };
        let repeat_icon = match pb.repeat {
            RepeatState::Off => "",
            RepeatState::Context => " ↻",
            RepeatState::Track => " ↺1",
        };

        let vol_icon = match pb.volume {
            0 => " 🔇",
            1..=50 => " 🔉",
            _ => " 🔊",
        };
        let vol_str = format!("{vol_icon} {}%", pb.volume);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {status} Now Playing{shuffle_icon}{repeat_icon}  |{vol_str} "))
            .style(Style::default().fg(Color::Cyan));

        let content = if pb.title.is_empty() {
            vec![
                Line::from(Span::styled("No active playback", Style::default().fg(Color::DarkGray))),
                Line::from(""),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    &pb.title,
                    Style::default().add_modifier(Modifier::BOLD).fg(Color::White),
                )),
                Line::from(vec![
                    Span::styled(&pb.artist, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!("  —  {}", pb.album),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ]
        };

        let para = Paragraph::new(content).block(block).alignment(Alignment::Center);
        frame.render_widget(para, chunks[0]);

        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Green).bg(Color::Black))
            .ratio(ratio)
            .label(format!(
                "{} / {}",
                fmt_duration(pb.progress_ms),
                fmt_duration(pb.duration_ms)
            ));
        frame.render_widget(gauge, chunks[1]);
    }

    fn render_help(
        &self,
        frame: &mut Frame,
        focus: &Focus,
        status: &Option<String>,
        search_active: bool,
        search_query: &str,
        area: ratatui::layout::Rect,
    ) {
        if search_active {
            let bar = Paragraph::new(Line::from(vec![
                Span::styled("/ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(search_query, Style::default().fg(Color::White)),
                Span::styled("█", Style::default().fg(Color::Yellow)),
                Span::styled("   [esc] cancel  [↑↓] nav  [enter] select", Style::default().fg(Color::DarkGray)),
            ]));
            frame.render_widget(bar, area);
            return;
        }

        let base = match focus {
            Focus::Playlists => "[↑↓] nav  [enter] open  [tab] tracks  [/] search  [space] play/pause  [n/p] skip  [-/+] vol  [s] shuffle  [r] repeat  [l] like  [q] quit",
            Focus::Tracks    => "[↑↓] nav  [enter] play  [tab] playlists  [/] search  [space] play/pause  [n/p] skip  [-/+] vol  [s] shuffle  [r] repeat  [l] like  [q] quit",
        };

        let text = if let Some(msg) = status {
            format!("{msg}  |  {base}")
        } else {
            base.to_string()
        };

        let help = Paragraph::new(text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Left);
        frame.render_widget(help, area);
    }
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn fmt_duration(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}
