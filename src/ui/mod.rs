use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, List, ListItem, ListState, Paragraph},
    Frame,
};
use rspotify::model::RepeatState;
use std::f64::consts::TAU;

use crate::spotify::{PlaylistSummary, TrackSummary};

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
    Playlists,
    Tracks,
}

// ── UI State ──────────────────────────────────────────────────────────────────

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
        let len = match self.focus {
            Focus::Playlists => self.filtered_playlists().len(),
            Focus::Tracks => self.filtered_tracks().len(),
        };
        match self.focus {
            Focus::Playlists => scroll_up(&mut self.playlist_list, len),
            Focus::Tracks => scroll_up(&mut self.track_list, len),
        }
    }

    pub fn nav_down(&mut self) {
        let len = match self.focus {
            Focus::Playlists => self.filtered_playlists().len(),
            Focus::Tracks => self.filtered_tracks().len(),
        };
        match self.focus {
            Focus::Playlists => scroll_down(&mut self.playlist_list, len),
            Focus::Tracks => scroll_down(&mut self.track_list, len),
        }
    }

    pub fn switch_focus(&mut self) {
        self.search_active = false;
        self.focus = match self.focus {
            Focus::Playlists => Focus::Tracks,
            Focus::Tracks => Focus::Playlists,
        };
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

        self.render_visualizer(frame, &state.playback, top_cols[0]);
        self.render_header(frame, state, top_cols[1]);
        self.render_playlists(frame, state, main_cols[0]);
        self.render_tracks(frame, state, main_cols[1]);
        
        let playback_row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(root[2]);

        self.render_progress(frame, &state.playback, playback_row[1]);
        self.render_marquee(frame, &state.playback, state.marquee_offset, playback_row[0]);
        self.render_help(frame, state, root[3]);
    }

    fn render_visualizer(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 { return; }

        let title_seed = pb.title.chars().map(|c| c as u32).sum::<u32>() as f64;
        let t = (pb.progress_ms as f64 / 60.0);

        for x in 0..inner.width {
            let x_f = x as f64;
            
            let mut amplitude = if pb.is_playing {
                let wave1 = (t * 1.2 + x_f * 0.8 + (title_seed * 0.1)).sin().abs();
                let wave2 = (t * 2.5 + x_f * 0.3 + (title_seed * 0.5)).cos().abs();
                let wave3 = (t * 0.5 + x_f * 1.2).sin().abs();
                
                ((wave1 * 0.4) + (wave2 * 0.4) + (wave3 * 0.2))
            } else {
                0.05
            };

            let total_pixels = (inner.height * 4) as f64;
            let target_h = (amplitude * total_pixels).clamp(1.0, total_pixels) as u16;

            for y in 0..inner.height {
                let pos_x = inner.x + x;
                let pos_y = inner.y + inner.height - 1 - y;
                
                let cell_bottom_pixel = (y * 4) as u16;
                let pixels_in_this_cell = target_h.saturating_sub(cell_bottom_pixel).clamp(0, 4);

                let char = match pixels_in_this_cell {
                    4 => '⣿',
                    3 => '⡷',
                    2 => '⠶',
                    1 => '⠤',
                    _ => ' ',
                };

                if char != ' ' {
                    if let Some(cell) = frame.buffer_mut().cell_mut((pos_x, pos_y)) {
                        cell.set_char(char).set_fg(Color::Green);
                    }
                }
            }
        }
    }

    fn render_header(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if state.search_active { Color::Yellow } else { Color::DarkGray }));
        
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content = if state.search_active {
            Line::from(vec![
                Span::styled("   Search: ", Style::default().fg(Color::Yellow)),
                Span::styled(&state.search_query, Style::default().fg(Color::White)),
                Span::styled("█", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
            ])
        } else if !state.playback.title.is_empty() {
            Line::from(vec![
                Span::styled(" 󰓇  ", Style::default().fg(Color::Green)),
                Span::styled(&state.playback.title, Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("  󰠃 {}", state.playback.artist), Style::default().fg(Color::DarkGray)),
            ])
        } else {
            Line::from(vec![
                Span::styled(" 󰓇  ", Style::default().fg(Color::DarkGray)),
                Span::styled("No music playing", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
            ])
        };
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Left), inner);
    }

    fn render_playlists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Playlists;
        let pb = &state.playback;
        
        let status_icon = if pb.is_playing { "Playing" } else { "Paused" };
        let status_color = if pb.is_playing { Color::DarkGray } else { Color::DarkGray };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![
                Span::raw(" 󰲚 Playlists "),
            ]).alignment(Alignment::Left))
            .title_bottom(Line::from(vec![ 
                Span::styled(format!(" Vol: {}% ", pb.volume), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" {} ", status_icon), Style::default().fg(status_color)),
            ]))
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.filtered_playlists().iter().map(|(_, p)| {
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {} ", p.name)),
                Span::styled(format!("({})", p.total_tracks), Style::default().fg(Color::DarkGray)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block) 
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.playlist_list);
    }

    fn render_tracks(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Tracks;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" 󰎆 Tracks ")
            .border_style(if focused { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) });

        let items: Vec<ListItem> = state.filtered_tracks().iter().map(|(_, t)| {
            let is_playing = state.playback.title == t.name;
            let style = if is_playing { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) };
            ListItem::new(Line::from(vec![
                Span::styled(if is_playing { " 󰓇 " } else { "   " }, Style::default().fg(Color::Green)),
                Span::styled(t.name.clone(), style),
                Span::styled(format!("  󰠃 {}", t.artist), Style::default().fg(Color::DarkGray)),
            ]))
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(Color::Rgb(40, 40, 40)).fg(Color::Green).add_modifier(Modifier::BOLD))
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.track_list);
    }

    fn render_progress(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let ratio = if pb.duration_ms > 0 { (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0) } else { 0.0 };
        let width = area.width.saturating_sub(14) as usize;
        let filled = (width as f64 * ratio) as usize;
        
        let bar = format!("{}{}{}", "⣿".repeat(filled), "⡷", "⠶".repeat(width.saturating_sub(filled)));
        let content = Line::from(vec![
            Span::styled(fmt_duration(pb.progress_ms), Style::default().fg(Color::Green)),
            Span::raw(" "),
            Span::styled(bar, Style::default().fg(Color::Green)),
            Span::raw(" "),
            Span::styled(fmt_duration(pb.duration_ms), Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }

    fn render_marquee(&self, frame: &mut Frame, pb: &PlaybackState, offset: usize, area: Rect) {
        let text = if pb.title.is_empty() { "isi-music v0.1.0".to_string() } else { format!("{} • {} ", pb.title, pb.artist) };
        let display = if text.len() < area.width as usize { text } else {
            let combined = format!("{}   •   ", text);
            let chars: Vec<char> = combined.chars().collect();
            (0..area.width as usize).map(|i| chars[(offset + i) % chars.len()]).collect()
        };
        frame.render_widget(Paragraph::new(display).style(Style::default().fg(Color::DarkGray)), area);
    }

    fn render_help(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let help_text = if state.search_active {
            " [ESC] Cancel  [ENTER] Search  [Type] Filter "
        } else {
            " [hjkl / ↑↓] Navigate  [SPACE] Play/Pause  [N/P] Skip  [L] Like  [+/-] Vol  [/] Search  [TAB] Focus  [Q] Quit "
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);

        frame.render_widget(help, area);
    }
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}