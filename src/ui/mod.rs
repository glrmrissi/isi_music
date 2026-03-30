use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use rspotify::model::RepeatState;
use std::f64::consts::TAU;

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
    pub spin_angle: f64,
    pub marquee_offset: usize,
    pub marquee_ms: u64,
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
            spin_angle: 0.0,
            marquee_offset: 0,
            marquee_ms: 0,
        }
    }

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

        // Rows: top bar | main panels | progress | marquee | help
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Visualizer | Search — same height, no blank space
                Constraint::Fill(1),   // Playlists | Musics — fills all remaining space
                Constraint::Length(1), // Progress bar (dots)
                Constraint::Length(1), // Marquee ticker
                Constraint::Length(1), // Help / status bar
            ])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(rows[0]);

        let main_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(rows[1]);

        self.render_visualizer(frame, &state.playback, top_cols[0]);
        self.render_search_bar(frame, state, top_cols[1]);
        self.render_playlists(frame, state, main_cols[0]);
        self.render_tracks(frame, state, main_cols[1]);
        self.render_progress(frame, &state.playback, rows[2]);
        self.render_marquee(frame, &state.playback, state.marquee_offset, rows[3]);
        self.render_help(frame, state, rows[4]);
    }

    // ── Beat-reactive visualizer bars ────────────────────────────────────────

    fn render_visualizer(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let status = if pb.is_playing { "▶" } else { "⏸" };
        let vol_bar = {
            let filled = (pb.volume / 10) as usize;
            let empty = 10usize.saturating_sub(filled);
            format!("{}{}",  "█".repeat(filled), "░".repeat(empty))
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {status}  {vol_bar} {}% ", pb.volume))
            .style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let cols = inner.width as usize;
        let rows = inner.height as usize;
        let t = pb.progress_ms as f64 / 1000.0;

        // Compute height for each column using overlapping sine waves
        // Each bar gets fully independent phases using golden ratio spread.
        // abs(sin) gives 0→1→0 bounce motion — no spatial wave between neighbors.
        let heights: Vec<usize> = (0..cols)
            .map(|col| {
                if !pb.is_playing {
                    return 0;
                }
                let c = col as f64;
                // Golden ratio phase offsets — adjacent bars have unrelated phases
                let ph1 = c * 1.618_034 * TAU;
                let ph2 = c * 2.414_214 * TAU;

                let h = 0.10
                    + 0.50 * (t * 2.3 + ph1).sin().abs()
                    + 0.25 * (t * 5.7 + ph2).sin().abs()
                    + 0.15 * (t * 9.1 + ph1 + ph2).cos().abs();

                let max = rows.max(1);
                let scaled = h.clamp(0.05, 1.0) * max as f64;
                (scaled as usize).clamp(1, max)
            })
            .collect();

        // Render top-to-bottom: row 0 = top
        let lines: Vec<Line> = (0..rows as u16)
            .map(|row| {
                let row_from_bottom = rows - 1 - row as usize;
                let spans: Vec<Span> = heights
                    .iter()
                    .map(|&h| {
                        if row_from_bottom < h {
                            let color = if h >= rows { Color::Red } else { Color::Green };
                            Span::styled("░", Style::default().fg(color))
                        } else {
                            Span::raw(" ")
                        }
                    })
                    .collect();
                Line::from(spans)
            })
            .collect();

        frame.render_widget(Paragraph::new(lines), inner);
    }

    // ── Search bar ────────────────────────────────────────────────────────────

    fn render_search_bar(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let focused = state.search_active;
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Search: ")
            .style(if focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Vertically center the query line
        let vpad = inner.height.saturating_sub(1) / 2;
        let query_area = Rect { y: inner.y + vpad, height: 1, ..inner };

        let query_line = if state.search_active {
            Line::from(vec![
                Span::styled(
                    state.search_query.as_str(),
                    Style::default().fg(Color::White),
                ),
                Span::styled("█", Style::default().fg(Color::Yellow)),
            ])
        } else if state.search_query.is_empty() {
            Line::from(Span::styled(
                "Press / to search",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ))
        } else {
            Line::from(Span::styled(
                state.search_query.as_str(),
                Style::default().fg(Color::White).add_modifier(Modifier::DIM),
            ))
        };

        frame.render_widget(Paragraph::new(query_line), query_area);

        // Show status message if any (line below query)
        if let Some(msg) = &state.status_msg {
            if vpad + 1 < inner.height {
                let msg_area = Rect { y: inner.y + vpad + 1, height: 1, ..inner };
                let msg_line = Paragraph::new(Span::styled(
                    msg.as_str(),
                    Style::default().fg(Color::Yellow),
                ));
                frame.render_widget(msg_line, msg_area);
            }
        }
    }

    // ── Playlists panel ───────────────────────────────────────────────────────

    fn render_playlists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Playlists;
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Playlists ")
            .style(border_style(focused));

        if state.playlists.is_empty() {
            frame.render_widget(
                Paragraph::new("Loading...")
                    .block(block)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }

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
            frame.render_widget(
                Paragraph::new("No results")
                    .block(block)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
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

    // ── Musics panel ──────────────────────────────────────────────────────────

    fn render_tracks(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;

        let title = if let Some(uri) = &state.active_playlist_uri {
            if uri.starts_with("search:") {
                format!(" Search results: {} ", &uri[7..])
            } else {
                state.selected_playlist()
                    .map(|p| format!(" {} ", p.name))
                    .unwrap_or_else(|| " Musics: ".to_string())
            }
        } else {
            " Musics: ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(border_style(focused));

        if state.tracks.is_empty() {
            let msg = if state.active_playlist_uri.is_some() {
                "No tracks"
            } else {
                "Select a playlist or search for music"
            };
            frame.render_widget(
                Paragraph::new(msg)
                    .block(block)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }

        let filtered = state.filtered_tracks();

        if filtered.is_empty() {
            frame.render_widget(
                Paragraph::new("No results")
                    .block(block)
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }

        let items: Vec<ListItem> = filtered
            .iter()
            .map(|(_, t)| {
                let detail = format!("  {} — {}", t.artist, fmt_duration(t.duration_ms));
                ListItem::new(Line::from(vec![
                    Span::styled(t.name.clone(), Style::default().fg(Color::White)),
                    Span::styled(detail, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        drop(filtered);

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

    // ── Progress bar (dot style) ──────────────────────────────────────────────

    fn render_progress(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let left = fmt_duration(pb.progress_ms);
        let right = fmt_duration(pb.duration_ms);
        // " 0:00 " + " 0:00 " = 12 chars minimum; bar fills the rest
        let label_width = left.len() + right.len() + 4; // spaces around each label
        let bar_width = (area.width as usize).saturating_sub(label_width);
        let filled = ((bar_width as f64) * ratio) as usize;
        let empty = bar_width.saturating_sub(filled);

        let line = Line::from(vec![
            Span::styled(format!(" {left} "), Style::default().fg(Color::DarkGray)),
            Span::styled("░".repeat(filled), Style::default().fg(Color::Green)),
            Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" {right} "), Style::default().fg(Color::DarkGray)),
        ]);

        frame.render_widget(Paragraph::new(line), area);
    }

    // ── Marquee ticker ────────────────────────────────────────────────────────

    fn render_marquee(&self, frame: &mut Frame, pb: &PlaybackState, offset: usize, area: Rect) {
        if pb.title.is_empty() {
            frame.render_widget(
                Paragraph::new("isi-music")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center),
                area,
            );
            return;
        }

        let entry = format!("{} — {}", pb.artist, pb.title);
        let full = format!("{entry}   •   {entry}   •   ");
        let chars: Vec<char> = full.chars().collect();
        let total = chars.len();
        let width = area.width as usize;
        let start = offset % total;

        let visible: String = (0..width).map(|i| chars[(start + i) % total]).collect();

        frame.render_widget(
            Paragraph::new(visible).style(Style::default().fg(Color::Yellow)),
            area,
        );
    }

    // ── Help / status bar (bottom) ────────────────────────────────────────────

    fn render_help(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let text = if let Some(msg) = &state.status_msg {
            msg.clone()
        } else if state.search_active {
            "[esc] cancel  [enter] search Spotify  [↑↓] nav  [type] filter current list".to_string()
        } else {
            match state.focus {
                Focus::Playlists => "[↑↓] nav  [enter] open  [tab] tracks  [/] search  [space] play/pause  [n/p] skip  [-/+] vol  [s] shuffle  [r] repeat  [l] like  [q] quit",
                Focus::Tracks    => "[↑↓] nav  [enter] play  [tab] playlists  [/] search  [space] play/pause  [n/p] skip  [-/+] vol  [s] shuffle  [r] repeat  [l] like  [q] quit",
            }.to_string()
        };

        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
            area,
        );
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
