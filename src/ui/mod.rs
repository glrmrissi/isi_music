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
        }
    }

    pub fn selected_playlist(&self) -> Option<&PlaylistSummary> {
        self.playlist_list.selected().and_then(|i| self.playlists.get(i))
    }

    pub fn selected_track_index(&self) -> Option<usize> {
        self.track_list.selected()
    }

    pub fn nav_up(&mut self) {
        match self.focus {
            Focus::Playlists => scroll_up(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => scroll_up(&mut self.track_list, self.tracks.len()),
        }
    }

    pub fn nav_down(&mut self) {
        match self.focus {
            Focus::Playlists => scroll_down(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => scroll_down(&mut self.track_list, self.tracks.len()),
        }
    }

    pub fn switch_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Playlists => Focus::Tracks,
            Focus::Tracks => Focus::Playlists,
        };
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
                Constraint::Length(6),
                Constraint::Length(2),
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
        self.render_help(frame, &state.focus, &state.status_msg, rows[2].into());
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

        let items: Vec<ListItem> = state
            .playlists
            .iter()
            .map(|p| {
                ListItem::new(Line::from(vec![
                    Span::raw(&p.name),
                    Span::styled(
                        format!(" ({})", p.total_tracks),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

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

        let items: Vec<ListItem> = state
            .tracks
            .iter()
            .map(|t| {
                ListItem::new(Line::from(vec![
                    Span::styled(&t.name, Style::default().fg(Color::White)),
                    Span::styled(
                        format!("  {} — {}", t.artist, fmt_duration(t.duration_ms)),
                        Style::default().fg(Color::DarkGray),
                    ),
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
            .highlight_symbol("▶ ");

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

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {status} Now Playing{shuffle_icon}{repeat_icon} "))
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
        area: ratatui::layout::Rect,
    ) {
        let base = match focus {
            Focus::Playlists => "[↑↓] nav  [enter] open  [tab] tracks  [space] play/pause  [n/p] skip  [s] shuffle  [r] repeat  [l] like  [q] quit",
            Focus::Tracks    => "[↑↓] nav  [enter] play  [tab] playlists  [space] play/pause  [n/p] skip  [s] shuffle  [r] repeat  [l] like  [q] quit",
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
