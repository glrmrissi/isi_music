use crate::app::metadata::sanitize_control_chars;
use crate::spotify::RepeatState;
use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
};
#[cfg(feature = "album-art")]
use ratatui_image::Resize;
#[cfg(feature = "album-art")]
use ratatui_image::protocol::StatefulProtocol;
use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;

use super::{Focus, LIBRARY_ITEMS, LocalNode, PlaybackState, SearchPanel, Ui, UiState};

fn color_to_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| {
        (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

fn clamp_text(text: &str, max_width: usize) -> Cow<'_, str> {
    if text.width() <= max_width {
        Cow::Borrowed(text)
    } else {
        let mut result = String::new();
        let mut w = 0;
        for ch in text.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if w + cw + 3 > max_width {
                break;
            }
            result.push(ch);
            w += cw;
        }
        result.push_str("...");
        Cow::Owned(result)
    }
}

fn pad_right(text: &str, width: usize) -> String {
    let current = text.width();
    if current >= width {
        text.to_string()
    } else {
        format!("{}{}", text, " ".repeat(width - current))
    }
}

fn calculate_number_width(total: usize) -> usize {
    if total == 0 {
        return 1;
    }
    total.to_string().len()
}

struct ListWindow<'a> {
    items: Vec<ListItem<'a>>,
    start: usize,
    selected: Option<usize>,
}

fn build_list_window<'a, F>(
    total: usize,
    height: usize,
    list_state: &ListState,
    mut item_fn: F,
) -> ListWindow<'a>
where
    F: FnMut(usize) -> ListItem<'a>,
{
    let visible = height.max(1);
    let selected = list_state
        .selected()
        .map(|index| index.min(total.saturating_sub(1)));
    let selected_index = selected.unwrap_or(0);
    let start = selected_index
        .saturating_sub(visible / 2)
        .min(total.saturating_sub(visible));
    let end = (start + visible).min(total);
    let items = (start..end).map(|index| item_fn(index)).collect();
    let local_selected =
        selected.and_then(|index| (index >= start && index < end).then_some(index - start));

    ListWindow {
        items,
        start,
        selected: local_selected,
    }
}

fn render_list_window<'a>(
    frame: &mut Frame,
    list: List<'a>,
    area: Rect,
    global_state: &mut ListState,
    start: usize,
    selected: Option<usize>,
) {
    let mut local_state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, area, &mut local_state);
    *global_state.offset_mut() = start + local_state.offset();
}

impl Ui {
    pub fn render_local_tree(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let total = state.sorted_track_indices.len();
            let width = area.width as usize;
            let ListWindow {
                items,
                start,
                selected,
            } = build_list_window(
                total,
                area.height as usize,
                &state.local_tree_list,
                |display_idx| {
                    let vi = state.sorted_track_indices[display_idx];
                    let Some(node) = state.local_tree.get_visible(vi) else {
                        return ListItem::new(Line::default());
                    };
                    let indent = "  ".repeat(node.depth());
                    match node {
                        LocalNode::Folder { name, .. } => ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(
                                "  ",
                                Style::default()
                                    .fg(self.theme.accent_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                name.as_str(),
                                Style::default()
                                    .fg(self.theme.text_primary)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ])),
                        LocalNode::Track { track, .. } => {
                            let is_playing =
                                state.playback.title == track.name && state.playback.is_local;
                            let icon = if is_playing { " " } else { " " };
                            let title_style = if is_playing {
                                Style::default()
                                    .fg(self.theme.primary)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(self.theme.text_primary)
                            };
                            let clean_name = sanitize_control_chars(&track.name);
                            let clean_artist = sanitize_control_chars(&track.artist);
                            let dur = fmt_duration(track.duration_ms);
                            let right_w = dur.width();
                            let content_w = width;
                            let indent_w = indent.width() + 2;
                            let left_budget = content_w.saturating_sub(indent_w + 2 + right_w);
                            let artist_w = (left_budget / 3).min(22);
                            let name_w = left_budget.saturating_sub(artist_w + 2);
                            let name_text =
                                pad_right(&clamp_text(&clean_name, name_w.max(8)), name_w.max(8));
                            let artist_text = pad_right(
                                &clamp_text(&clean_artist, artist_w.max(6)),
                                artist_w.max(6),
                            );
                            ListItem::new(Line::from(vec![
                                Span::raw(indent),
                                Span::styled(icon, Style::default().fg(self.theme.text_secondary)),
                                Span::styled(name_text, title_style),
                                Span::styled(
                                    format!("  {artist_text}"),
                                    Style::default().fg(self.theme.text_secondary),
                                ),
                                Span::styled(
                                    dur,
                                    Style::default()
                                        .fg(self.theme.text_secondary)
                                        .add_modifier(Modifier::DIM),
                                ),
                            ]))
                        }
                    }
                },
            );
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(
                frame,
                list,
                area,
                &mut state.local_tree_list,
                start,
                selected,
            );
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let total_tracks: usize = state
            .local_tree
            .all_nodes
            .iter()
            .filter(|n| !n.is_folder())
            .count();

        let block = self
            .build_panel_block(UiWidget::MainContent, focused, "Local Files")
            .title_bottom(Line::from(vec![Span::styled(
                format!(
                    " {} tracks  [ENTER] play/expand  [A] queue  [Ctrl+F] search ",
                    total_tracks
                ),
                Style::default().fg(self.theme.text_secondary),
            )]));

        let inner = block.inner(area);
        let total = state.sorted_track_indices.len();
        let content_w = inner.width as usize;
        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(
            total,
            inner.height as usize,
            &state.local_tree_list,
            |display_idx| {
                let vi = state.sorted_track_indices[display_idx];
                let Some(node) = state.local_tree.get_visible(vi) else {
                    return ListItem::new(Line::default());
                };
                let indent = "  ".repeat(node.depth());
                match node {
                    LocalNode::Folder { name, expanded, .. } => {
                        let icon = if *expanded { "v " } else { "> " };
                        let child_count = state.local_tree.tracks_under_folder(vi).len();
                        ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(
                                icon,
                                Style::default()
                                    .fg(self.theme.accent_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                name.as_str(),
                                Style::default()
                                    .fg(self.theme.text_primary)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("  ({} tracks)", child_count),
                                Style::default().fg(self.theme.text_secondary),
                            ),
                        ]))
                    }
                    LocalNode::Track { track, .. } => {
                        let is_playing =
                            state.playback.title == track.name && state.playback.is_local;
                        let icon = if is_playing { " " } else { " " };
                        let title_style = if is_playing {
                            Style::default()
                                .fg(self.theme.primary)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(self.theme.text_primary)
                        };
                        let clean_name = sanitize_control_chars(&track.name);
                        let clean_artist = sanitize_control_chars(&track.artist);
                        let dur = fmt_duration(track.duration_ms);
                        let right_w = dur.width();
                        let indent_w = indent.width() + 2;
                        let left_budget = content_w.saturating_sub(indent_w + 2 + right_w);
                        let artist_w = (left_budget / 3).min(28);
                        let name_w = left_budget.saturating_sub(artist_w + 2);
                        let name_text =
                            pad_right(&clamp_text(&clean_name, name_w.max(8)), name_w.max(8));
                        let artist_text =
                            pad_right(&clamp_text(&clean_artist, artist_w.max(6)), artist_w.max(6));
                        ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(icon, Style::default().fg(self.theme.text_secondary)),
                            Span::styled(name_text, title_style),
                            Span::styled(
                                format!("  {artist_text}"),
                                Style::default().fg(self.theme.text_secondary),
                            ),
                            Span::styled(
                                dur,
                                Style::default()
                                    .fg(self.theme.text_secondary)
                                    .add_modifier(Modifier::DIM),
                            ),
                        ]))
                    }
                }
            },
        );

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.background_element)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(
            frame,
            list,
            area,
            &mut state.local_tree_list,
            start,
            selected,
        );
    }

    pub fn render_visualizer(
        &self,
        frame: &mut Frame,
        pb: &PlaybackState,
        viz_bands: &[f32],
        area: Rect,
        state: &UiState,
    ) {
        if !state.show_visualizer {
            return;
        }

        let block = Block::default();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let viz_color = self
            .theme
            .visualizer
            .color
            .unwrap_or(self.theme.accent_color);

        let effective_h = self
            .theme
            .visualizer
            .height
            .map(|h| h.min(inner.height))
            .unwrap_or(inner.height);
        if effective_h == 0 {
            return;
        }
        let viz_top = inner.y + (inner.height - effective_h);

        match self.theme.visualizer.style {
            crate::utils::theme::VisualizerStyle::BrailleBars => {
                self.render_braille_bars(
                    frame,
                    pb,
                    viz_bands,
                    inner,
                    viz_top,
                    effective_h,
                    viz_color,
                );
            }
            crate::utils::theme::VisualizerStyle::BlockBars => {
                self.render_block_bars(
                    frame,
                    pb,
                    viz_bands,
                    inner,
                    viz_top,
                    effective_h,
                    viz_color,
                );
            }
            crate::utils::theme::VisualizerStyle::Plasma => {
                self.render_plasma_wave(
                    frame,
                    pb,
                    viz_bands,
                    inner,
                    viz_top,
                    effective_h,
                    viz_color,
                );
            }
            crate::utils::theme::VisualizerStyle::AnimeArt => {
                self.render_anime_art_viz(
                    frame,
                    pb,
                    viz_bands,
                    inner,
                    viz_top,
                    effective_h,
                    viz_color,
                );
            }
        }
    }

    fn render_braille_bars(
        &self,
        frame: &mut Frame,
        pb: &PlaybackState,
        viz_bands: &[f32],
        inner: Rect,
        viz_top: u16,
        effective_h: u16,
        viz_color: ratatui::style::Color,
    ) {
        const LEFT: [u8; 4] = [1 << 6, 1 << 2, 1 << 1, 1 << 0];
        const RIGHT: [u8; 4] = [1 << 7, 1 << 5, 1 << 4, 1 << 3];

        let width = inner.width as usize;
        let height = effective_h as usize;
        if width == 0 || height == 0 {
            return;
        }

        let bar_count = self
            .theme
            .visualizer
            .bar_count
            .unwrap_or(width)
            .clamp(1, width);
        let pixel_rows = height * 4;

        for bar in 0..bar_count {
            let amp = if !pb.is_playing {
                0.0
            } else if viz_bands.is_empty() {
                0.05
            } else {
                let start = bar * viz_bands.len() / bar_count;
                let end = ((bar + 1) * viz_bands.len() / bar_count)
                    .max(start + 1)
                    .min(viz_bands.len());
                viz_bands[start..end]
                    .iter()
                    .map(|&band| f64::from(band))
                    .sum::<f64>()
                    / (end - start) as f64
            }
            .clamp(0.0, 1.0);
            let bar_height = (amp.powf(0.7) * pixel_rows as f64) as usize;
            if bar_height == 0 {
                continue;
            }

            let color = if amp > 0.75 {
                self.theme.text_primary
            } else if amp > 0.25 {
                viz_color
            } else {
                self.theme.border_subtle
            };
            let x_start = inner.x + (bar * width / bar_count) as u16;
            let x_end = inner.x + ((bar + 1) * width / bar_count) as u16;

            for cell_y in 0..height {
                let px_base = (height - 1 - cell_y) * 4;
                if px_base >= bar_height {
                    continue;
                }

                let mut bits = 0u8;
                for dot_row in 0..4 {
                    if px_base + dot_row < bar_height {
                        bits |= LEFT[dot_row] | RIGHT[dot_row];
                    }
                }
                if bits == 0 {
                    continue;
                }

                let symbol = char::from_u32(0x2800 | u32::from(bits)).unwrap_or(' ');
                let y = viz_top + cell_y as u16;
                for x in x_start..x_end {
                    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                        cell.set_char(symbol).set_fg(color);
                    }
                }
            }
        }
    }

    fn render_block_bars(
        &self,
        frame: &mut Frame,
        pb: &PlaybackState,
        viz_bands: &[f32],
        inner: Rect,
        viz_top: u16,
        effective_h: u16,
        viz_color: ratatui::style::Color,
    ) {
        let width = inner.width as usize;
        let height = effective_h as usize;
        if width == 0 || height == 0 {
            return;
        }

        let bar_count = self
            .theme
            .visualizer
            .bar_count
            .unwrap_or(width)
            .clamp(1, width);

        for bar in 0..bar_count {
            let amp = if !pb.is_playing {
                0.0
            } else if viz_bands.is_empty() {
                0.05
            } else {
                let start = bar * viz_bands.len() / bar_count;
                let end = ((bar + 1) * viz_bands.len() / bar_count)
                    .max(start + 1)
                    .min(viz_bands.len());
                viz_bands[start..end]
                    .iter()
                    .map(|&band| f64::from(band))
                    .sum::<f64>()
                    / (end - start) as f64
            }
            .clamp(0.0, 1.0);
            let filled_rows = if amp > 0.0 {
                (amp.powf(0.65) * height as f64).ceil() as usize
            } else {
                0
            };
            if filled_rows == 0 {
                continue;
            }

            let color = if amp > 0.75 {
                self.theme.text_primary
            } else if amp > 0.25 {
                viz_color
            } else {
                self.theme.border_subtle
            };
            let x_start = inner.x + (bar * width / bar_count) as u16;
            let x_end = inner.x + ((bar + 1) * width / bar_count) as u16;
            let y_start = viz_top + height.saturating_sub(filled_rows) as u16;

            for y in y_start..viz_top + effective_h {
                for x in x_start..x_end {
                    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                        cell.set_char('█').set_fg(color);
                    }
                }
            }
        }
    }

    fn render_plasma_wave(
        &self,
        frame: &mut Frame,
        pb: &PlaybackState,
        viz_bands: &[f32],
        inner: Rect,
        viz_top: u16,
        effective_h: u16,
        viz_color: ratatui::style::Color,
    ) {
        let mid = effective_h as f64 / 2.0;
        let n_bars = inner.width as usize;
        let energy: f64 = if !pb.is_playing || viz_bands.is_empty() {
            0.0
        } else {
            viz_bands.iter().map(|&b| b as f64).sum::<f64>() / viz_bands.len() as f64
        };

        for x in 0..n_bars {
            let phase = x as f64 * 0.3;
            let band_idx =
                (x * viz_bands.len() / n_bars.max(1)).min(viz_bands.len().saturating_sub(1));
            let band = if viz_bands.is_empty() {
                0.0
            } else {
                viz_bands[band_idx] as f64
            };
            let amp = (band * 0.6 + energy * 0.4).clamp(0.0, 1.0);
            let wave = (phase + (band * 6.0)).sin() * amp * mid * 0.8;
            let y = (mid + wave).round() as i64;

            let bx = inner.x + x as u16;
            let by = viz_top + y.clamp(0, effective_h as i64 - 1) as u16;
            if bx < inner.x + inner.width && by < viz_top + effective_h {
                if let Some(cell) = frame.buffer_mut().cell_mut((bx, by)) {
                    cell.set_char('●').set_fg(viz_color);
                }
            }
        }
    }

    fn render_anime_art_viz(
        &self,
        frame: &mut Frame,
        pb: &PlaybackState,
        viz_bands: &[f32],
        inner: Rect,
        viz_top: u16,
        effective_h: u16,
        viz_color: ratatui::style::Color,
    ) {
        let art: Vec<String> = if let Some(path) = &self.theme.visualizer.art_path {
            match std::fs::read_to_string(path) {
                Ok(content) => content.lines().map(|l| l.to_string()).collect(),
                Err(_) => vec!["  ♪  ".to_string()],
            }
        } else {
            vec!["  ♪  ".to_string()]
        };

        let bass = if viz_bands.is_empty() || !pb.is_playing {
            0.0
        } else {
            viz_bands.iter().take(8).map(|&b| b as f64).sum::<f64>() / 8.0
        };

        let art_h = art.len() as u16;
        let start_y = if art_h < effective_h {
            viz_top + (effective_h - art_h) / 2
        } else {
            viz_top
        };

        for (row, line) in art.iter().enumerate().take(effective_h as usize) {
            let y = start_y + row as u16;
            if y >= viz_top + effective_h {
                break;
            }
            for (col, ch) in line.chars().enumerate() {
                if ch == ' ' || ch == '\t' {
                    continue;
                }
                let x = inner.x + col as u16;
                if x >= inner.x + inner.width {
                    break;
                }
                let bright = ch != ' ';
                if !bright && bass < 0.1 {
                    continue;
                }
                let color = if bass > 0.5 {
                    self.theme.text_primary
                } else {
                    viz_color
                };
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_char(ch).set_fg(color);
                }
            }
        }
    }

    fn breadcrumb(&self, state: &UiState) -> String {
        if !state.show_breadcrumb {
            return String::new();
        }
        let mut segments: Vec<String> = Vec::new();
        for entry in &state.nav_stack {
            segments.push(entry.label.clone());
        }
        segments.push(state.current_label());
        if segments.is_empty() {
            return String::new();
        }
        format!(" {} ", segments.join(" > "))
    }

    pub fn render_search(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let active = state.search_active || state.quick_search_active || state.command_mode;
        let content = if state.command_mode {
            Line::from(vec![
                Span::styled(" : ", Style::default().fg(self.theme.accent_color)),
                Span::styled(
                    &state.command_buffer,
                    Style::default().fg(self.theme.text_primary),
                ),
                Span::styled(
                    "▏",
                    Style::default()
                        .fg(self.theme.accent_color)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        } else if state.quick_search_active {
            Line::from(vec![
                Span::styled(" Ctrl+F ", Style::default().fg(self.theme.accent_color)),
                Span::styled(
                    &state.quick_search_query,
                    Style::default().fg(self.theme.text_primary),
                ),
                Span::styled(
                    "▏",
                    Style::default()
                        .fg(self.theme.accent_color)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        } else if state.search_active {
            Line::from(vec![
                Span::styled(" / ", Style::default().fg(self.theme.accent_color)),
                Span::styled(
                    &state.search_query,
                    Style::default().fg(self.theme.text_primary),
                ),
                Span::styled(
                    "▏",
                    Style::default()
                        .fg(self.theme.accent_color)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        } else if let Some(msg) = &state.status_msg {
            Line::from(Span::styled(
                msg.as_str(),
                Style::default().fg(self.theme.info),
            ))
        } else if let Some(results) = &state.search_results {
            Line::from(vec![
                Span::styled(
                    format!(" Search: {} ", results.query),
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Tab panels  Enter open  Esc close",
                    Style::default().fg(self.theme.text_secondary),
                ),
            ])
        } else {
            let breadcrumb = self.breadcrumb(state);
            if breadcrumb.is_empty() {
                Line::from(Span::styled(
                    " / search",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ))
            } else {
                Line::from(Span::styled(
                    breadcrumb,
                    Style::default().fg(self.theme.text_secondary),
                ))
            }
        };

        let background = Style::default().bg(self.theme.background_panel);
        if area.height < 2 {
            frame.render_widget(Paragraph::new(content).style(background), area);
            return;
        }

        let border_color = if active {
            self.theme.border_active
        } else {
            self.theme.border_subtle
        };
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(border_color))
            .style(background);
        frame.render_widget(Paragraph::new(content).block(block), area);
    }

    pub fn render_library(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Library;

        let block = self.build_panel_block(UiWidget::Library, focused, "Library");

        let items: Vec<ListItem> = LIBRARY_ITEMS
            .iter()
            .map(|name| ListItem::new(Line::from(vec![Span::raw(format!("  {name} "))])))
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.background_element)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        frame.render_stateful_widget(list, area, &mut state.library_list);
    }

    pub fn render_playlists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Playlists;
        let pb = &state.playback;

        let status_icon = if pb.is_playing { "Playing" } else { "Paused" };
        let repeat_str = match pb.repeat {
            RepeatState::Off => String::new(),
            RepeatState::Context => " Rep ".to_string(),
            RepeatState::Track => " Rep1 ".to_string(),
        };

        let block = self
            .build_panel_block(UiWidget::Playlists, focused, "Playlists")
            .title_bottom(Line::from(vec![
                Span::styled(
                    format!(" Vol: {}% ", pb.volume),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::styled(
                    format!(" {} ", status_icon),
                    Style::default().fg(if pb.is_playing {
                        self.theme.success
                    } else {
                        self.theme.text_secondary
                    }),
                ),
                Span::styled(repeat_str, Style::default().fg(self.theme.accent_color)),
            ]));

        let inner = block.inner(area);
        let total = state.playlists.len();
        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(total, inner.height as usize, &state.playlist_list, |idx| {
            let p = &state.playlists[idx];
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {} ", p.name)),
                Span::styled(
                    format!("({})", p.total_tracks),
                    Style::default().fg(self.theme.text_secondary),
                ),
            ]))
        });

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.background_element)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(frame, list, area, &mut state.playlist_list, start, selected);
    }

    pub fn render_welcome(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let mut items: Vec<ListItem> = Vec::new();

            items.push(ListItem::new(Line::from(Span::styled(
                " Default",
                Style::default()
                    .fg(self.theme.text_secondary)
                    .add_modifier(Modifier::BOLD),
            ))));

            for name in LIBRARY_ITEMS {
                items.push(ListItem::new(Line::from(vec![Span::raw(format!(
                    "  {name} "
                ))])));
            }

            if !state.playlists.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(
                    " Playlists",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::BOLD),
                ))));

                for p in &state.playlists {
                    items.push(ListItem::new(Line::from(vec![Span::raw(format!(
                        "  {} ",
                        p.name
                    ))])));
                }
            }

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            frame.render_stateful_widget(list, area, &mut state.library_list);
            return;
        }

        let block = self.build_panel_block(UiWidget::MainContent, false, "");
        frame.render_widget(&block, area);
        let inner = block.inner(area);

        let lines = if state.loading {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Loading...",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::SLOW_BLINK),
                )),
                Line::from(""),
            ]
        } else if !state.spotify_authenticated && state.local_tree.visible_len() == 0 {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Welcome to isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Choose how to get started:",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " 1. Spotify streaming",
                    Style::default()
                        .fg(self.theme.text_primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "   Run: isi-music setup-spotify",
                    Style::default().fg(self.theme.accent_color),
                )),
                Line::from(Span::styled(
                    "   Then select Liked Songs or a playlist from the left panel",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " 2. Local files",
                    Style::default()
                        .fg(self.theme.text_primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "   Set [local] music_dir in config.toml",
                    Style::default().fg(self.theme.accent_color),
                )),
                Line::from(Span::styled(
                    "   Then select Local Files from the Library panel and press ENTER",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate   ENTER select   / search   ? help   q quit",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        } else if state.spotify_authenticated && state.tracks.is_empty() {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Select a playlist from the Library or Playlists panel,",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(Span::styled(
                    "or press / to search Spotify.",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate   ENTER select   / search   Ctrl+F quick search",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        } else if !state.spotify_authenticated && state.local_tree.visible_len() > 0 {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Local files loaded! Select Local Files and press ENTER to play.",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Want Spotify streaming? Run: isi-music setup-spotify",
                    Style::default().fg(self.theme.accent_color),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate   ENTER select   / search   ? help   q quit",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        } else {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Select a playlist from the Library or Playlists panel,",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(Span::styled(
                    "or press / to search Spotify.",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate panels   ENTER select   / search   Ctrl+F quick search",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        };

        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }

    pub fn render_lyrics(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let pb = &state.playback;
        let block = Block::default();
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width < 4 || inner.height < 2 {
            return;
        }

        let Some(lyrics) = &pb.lyrics else {
            let msg = if pb.lyrics_loading {
                "Loading lyrics..."
            } else if pb.title.is_empty() {
                "No track playing"
            } else {
                "No lyrics found"
            };

            let vertical_center = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(45),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .split(inner);

            frame.render_widget(
                Paragraph::new(msg)
                    .style(Style::default().fg(self.theme.text_secondary))
                    .alignment(Alignment::Center),
                vertical_center[1],
            );
            return;
        };

        let visible_rows = inner.height as usize;

        if lyrics.is_synced {
            let active = lyrics.active_idx(pb.progress_ms).unwrap_or(0);

            let half = visible_rows / 2;
            let start_idx = active.saturating_sub(half);
            let lines_to_render = lyrics.lines.iter().skip(start_idx).take(visible_rows);

            let items: Vec<ListItem> = lines_to_render
                .enumerate()
                .map(|(rel, line)| {
                    let abs = start_idx + rel;
                    if abs == active {
                        ListItem::new(
                            Line::from(Span::styled(
                                format!("{}", line.text),
                                Style::default()
                                    .fg(self.theme.primary)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .alignment(Alignment::Center),
                        )
                    } else {
                        let distance = (abs as isize - active as isize).unsigned_abs();
                        let style = if distance <= 2 {
                            Style::default().fg(self.theme.text_primary)
                        } else {
                            Style::default()
                                .fg(self.theme.text_secondary)
                                .add_modifier(Modifier::DIM)
                        };
                        ListItem::new(
                            Line::from(Span::styled(format!("{}", line.text), style))
                                .alignment(Alignment::Center),
                        )
                    }
                })
                .collect();

            let list = List::new(items);
            frame.render_widget(list, inner);
        } else {
            let total = lyrics.lines.len();
            let max_scroll = total.saturating_sub(visible_rows);
            let scroll = pb.lyrics_scroll.min(max_scroll);

            let text_lines: Vec<Line> = lyrics.lines[scroll..]
                .iter()
                .take(visible_rows)
                .map(|l| {
                    Line::from(Span::styled(
                        format!("{}", l.text),
                        Style::default().fg(self.theme.text_primary),
                    ))
                    .alignment(Alignment::Center)
                })
                .collect();

            frame.render_widget(
                Paragraph::new(text_lines)
                    .alignment(Alignment::Center)
                    .wrap(ratatui::widgets::Wrap { trim: false }),
                inner,
            );
        }
    }

    pub fn render_lyrics_compact(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let pb = &state.playback;
        if area.width < 4 || area.height < 1 {
            return;
        }

        let Some(lyrics) = &pb.lyrics else { return };
        if !lyrics.is_synced {
            return;
        }

        let active = lyrics.active_idx(pb.progress_ms).unwrap_or(0);

        let current = lyrics.lines.get(active).map(|l| l.text.as_str());
        let next = lyrics.lines.get(active + 1).map(|l| l.text.as_str());

        let lines: Vec<Line> = std::iter::once(Line::from(""))
            .chain(
                current
                    .map(|t| {
                        Line::from(Span::styled(
                            t,
                            Style::default()
                                .fg(self.theme.primary)
                                .add_modifier(Modifier::BOLD),
                        ))
                        .alignment(Alignment::Center)
                    })
                    .into_iter(),
            )
            .chain(
                next.map(|t| {
                    Line::from(Span::styled(
                        t,
                        Style::default()
                            .fg(self.theme.text_secondary)
                            .add_modifier(Modifier::DIM),
                    ))
                    .alignment(Alignment::Center)
                })
                .into_iter(),
            )
            .collect();

        if lines.len() <= 1 {
            return;
        }
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
    }

    pub fn render_tracks(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let total = state.sorted_track_indices.len();
            let num_width = calculate_number_width(total.max(state.tracks.len()));
            let content_w = area.width as usize;

            let ListWindow {
                items,
                start,
                selected,
            } = build_list_window(
                total,
                area.height as usize,
                &state.track_list,
                |display_idx| {
                    let real_idx = state.sorted_track_indices[display_idx];
                    let Some(t) = state.tracks.get(real_idx) else {
                        return ListItem::new(Line::default());
                    };
                    let is_playing = state.playback.title == t.name;
                    let style = if is_playing {
                        Style::default()
                            .fg(self.theme.success)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(self.theme.text_primary)
                    };
                    let dur = fmt_duration(t.duration_ms);
                    let added = match &t.added_at {
                        Some(dt) if dt.len() >= 10 => format!(" {}", &dt[..10]),
                        _ => String::new(),
                    };
                    let right_w = added.width() + 1 + dur.width();
                    let num_prefix_width = num_width + 2; // número + ". "
                    let left_budget = content_w.saturating_sub(num_prefix_width + 2 + right_w);
                    let artist_w = (left_budget / 3).min(22);
                    let name_w = left_budget.saturating_sub(artist_w + 2);
                    let name_text = pad_right(&clamp_text(&t.name, name_w.max(8)), name_w.max(8));
                    let artist_text =
                        pad_right(&clamp_text(&t.artist, artist_w.max(6)), artist_w.max(6));
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:>width$}. ", display_idx + 1, width = num_width),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                        Span::styled(name_text, style),
                        Span::styled(
                            format!("  {artist_text}"),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                        Span::styled(added, Style::default().fg(self.theme.text_secondary)),
                        Span::raw(" "),
                        Span::styled(dur, Style::default().fg(self.theme.text_secondary)),
                    ]))
                },
            );
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(frame, list, area, &mut state.track_list, start, selected);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let title = if state.active_playlist_uri.as_deref() == Some("liked_songs") {
            "Liked Songs"
        } else {
            "Tracks"
        };

        let sort_label = format!("[Sort: {}]", state.track_sort_by.label());
        let count = if state.tracks_total > 0 {
            format!(
                "{}/{}",
                state.sorted_track_indices.len(),
                state.tracks_total
            )
        } else {
            state.sorted_track_indices.len().to_string()
        };
        let block = self
            .build_panel_block(UiWidget::MainContent, focused, title)
            .title_bottom(Line::from(vec![
                Span::styled(
                    format!(" {count} ",),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::styled(sort_label, Style::default().fg(self.theme.accent_color)),
                Span::styled(
                    " [Ctrl+F] search  [O] sort ",
                    Style::default().fg(self.theme.text_secondary),
                ),
            ]));

        let inner = block.inner(area);
        let total = state.sorted_track_indices.len();
        let num_width = calculate_number_width(total.max(state.tracks.len()));
        let content_w = inner.width as usize;

        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(
            total,
            inner.height as usize,
            &state.track_list,
            |display_idx| {
                let real_idx = state.sorted_track_indices[display_idx];
                let Some(t) = state.tracks.get(real_idx) else {
                    return ListItem::new(Line::default());
                };
                let is_playing = state.playback.title == t.name;
                let style = if is_playing {
                    Style::default()
                        .fg(self.theme.success)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.text_primary)
                };
                let dur = fmt_duration(t.duration_ms);
                let added = match &t.added_at {
                    Some(dt) if dt.len() >= 10 => format!(" {}", &dt[..10]),
                    _ => String::new(),
                };
                let right_w = added.width() + 1 + dur.width();
                let num_prefix_width = num_width + 2; // número + ". "
                let left_budget = content_w.saturating_sub(num_prefix_width + 2 + right_w);
                let artist_w = (left_budget / 3).min(28);
                let name_w = left_budget.saturating_sub(artist_w + 2);
                let name_text = pad_right(&clamp_text(&t.name, name_w.max(8)), name_w.max(8));
                let artist_text =
                    pad_right(&clamp_text(&t.artist, artist_w.max(6)), artist_w.max(6));
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>width$}. ", display_idx + 1, width = num_width),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                    Span::styled(name_text, style),
                    Span::styled(
                        format!("  {artist_text}"),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                    Span::styled(added, Style::default().fg(self.theme.text_secondary)),
                    Span::raw(" "),
                    Span::styled(dur, Style::default().fg(self.theme.text_secondary)),
                ]))
            },
        );

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(frame, list, area, &mut state.track_list, start, selected);
    }

    pub fn render_albums(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let total = state.albums.len();
            let ListWindow {
                items,
                start,
                selected,
            } = build_list_window(total, area.height as usize, &state.album_list, |idx| {
                let a = &state.albums[idx];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", idx + 1),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                    Span::raw(clamp_text(&a.name, 30)),
                    Span::styled(
                        format!("  {}", clamp_text(&a.artist, 20)),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                ]))
            });
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(frame, list, area, &mut state.album_list, start, selected);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let count = if state.albums_total > 0 {
            format!("{}/{}", state.albums.len(), state.albums_total)
        } else {
            state.albums.len().to_string()
        };

        let block = self
            .build_panel_block(UiWidget::MainContent, focused, "Albums")
            .title_bottom(Line::from(vec![Span::styled(
                format!(" {count} "),
                Style::default().fg(self.theme.text_secondary),
            )]));

        let inner = block.inner(area);
        let total = state.albums.len();
        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(total, inner.height as usize, &state.album_list, |idx| {
            let a = &state.albums[idx];
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>3}. ", idx + 1),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::raw(clamp_text(&a.name, 35)),
                Span::styled(
                    format!(" - {}", clamp_text(&a.artist, 25)),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::styled(
                    format!(" ({} tracks)", a.total_tracks),
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ),
            ]))
        });

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(frame, list, area, &mut state.album_list, start, selected);
    }

    pub fn render_artists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let total = state.artists.len();
            let ListWindow {
                items,
                start,
                selected,
            } = build_list_window(total, area.height as usize, &state.artist_list, |idx| {
                let a = &state.artists[idx];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", idx + 1),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                    Span::raw(clamp_text(&a.name, 30)),
                ]))
            });
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(frame, list, area, &mut state.artist_list, start, selected);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let block = self
            .build_panel_block(UiWidget::MainContent, focused, "Artists")
            .title_bottom(Line::from(vec![Span::styled(
                format!(" {} ", state.artists.len()),
                Style::default().fg(self.theme.text_secondary),
            )]));

        let inner = block.inner(area);
        let total = state.artists.len();
        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(total, inner.height as usize, &state.artist_list, |idx| {
            let a = &state.artists[idx];
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>3}. ", idx + 1),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::raw(clamp_text(&a.name, 35)),
                Span::styled(
                    if a.genres.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", clamp_text(&a.genres, 25))
                    },
                    Style::default().fg(self.theme.text_secondary),
                ),
            ]))
        });

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(frame, list, area, &mut state.artist_list, start, selected);
    }

    pub fn render_shows(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let total = state.shows.len();
            let ListWindow {
                items,
                start,
                selected,
            } = build_list_window(total, area.height as usize, &state.show_list, |idx| {
                let s = &state.shows[idx];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", idx + 1),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                    Span::raw(clamp_text(&s.name, 30)),
                ]))
            });
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(frame, list, area, &mut state.show_list, start, selected);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let count = if state.shows_total > 0 {
            format!("{}/{}", state.shows.len(), state.shows_total)
        } else {
            state.shows.len().to_string()
        };

        let block = self
            .build_panel_block(UiWidget::MainContent, focused, "Podcasts")
            .title_bottom(Line::from(vec![Span::styled(
                format!(" {count} "),
                Style::default().fg(self.theme.text_secondary),
            )]));

        let inner = block.inner(area);
        let total = state.shows.len();
        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(total, inner.height as usize, &state.show_list, |idx| {
            let s = &state.shows[idx];
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>3}. ", idx + 1),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::raw(clamp_text(&s.name, 35)),
                Span::styled(
                    format!("  {}", clamp_text(&s.publisher, 25)),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::styled(
                    format!(" ({} eps)", s.total_episodes),
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ),
            ]))
        });

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(frame, list, area, &mut state.show_list, start, selected);
    }

    pub fn render_search_panels(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focus_panel = state
            .search_results
            .as_ref()
            .map(|sr| sr.panel)
            .unwrap_or(SearchPanel::Tracks);
        let is_focused = state.focus == Focus::Search;
        let is_loading = state
            .search_results
            .as_ref()
            .map(|sr| sr.loading)
            .unwrap_or(false);

        if state.compact_effective {
            if let Some(sr) = &mut state.search_results {
                let label = match focus_panel {
                    SearchPanel::Tracks => "Tracks",
                    SearchPanel::Artists => "Artists",
                    SearchPanel::Albums => "Albums",
                    SearchPanel::Playlists => "Playlists",
                };
                let title = if is_loading {
                    format!("{label} …")
                } else {
                    label.to_string()
                };
                let block = self.build_panel_block(UiWidget::Search, is_focused, &title);
                let list_area = block.inner(area);
                frame.render_widget(block, area);
                if list_area.height == 0 {
                    return;
                }
                let list_height = list_area.height as usize;
                let window = match focus_panel {
                    SearchPanel::Tracks => {
                        build_list_window(sr.tracks.len(), list_height, &sr.track_list, |idx| {
                            let t = &sr.tracks[idx];
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.primary)),
                                Span::styled(
                                    format!("{:>3}. ", idx + 1),
                                    Style::default().fg(self.theme.text_secondary),
                                ),
                                Span::styled(
                                    t.name.as_str(),
                                    Style::default().fg(self.theme.text_primary),
                                ),
                                Span::styled(
                                    format!("  {}", t.artist),
                                    Style::default().fg(self.theme.text_secondary),
                                ),
                            ]))
                        })
                    }
                    SearchPanel::Artists => {
                        build_list_window(sr.artists.len(), list_height, &sr.artist_list, |idx| {
                            let a = &sr.artists[idx];
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.primary)),
                                Span::styled(
                                    a.name.as_str(),
                                    Style::default().fg(self.theme.text_primary),
                                ),
                            ]))
                        })
                    }
                    SearchPanel::Albums => {
                        build_list_window(sr.albums.len(), list_height, &sr.album_list, |idx| {
                            let a = &sr.albums[idx];
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.primary)),
                                Span::styled(
                                    a.name.as_str(),
                                    Style::default().fg(self.theme.text_primary),
                                ),
                                Span::styled(
                                    format!("  {}", a.artist),
                                    Style::default().fg(self.theme.text_secondary),
                                ),
                            ]))
                        })
                    }
                    SearchPanel::Playlists => build_list_window(
                        sr.playlists.len(),
                        list_height,
                        &sr.playlist_list,
                        |idx| {
                            let p = &sr.playlists[idx];
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.primary)),
                                Span::styled(
                                    p.name.as_str(),
                                    Style::default().fg(self.theme.text_primary),
                                ),
                            ]))
                        },
                    ),
                };
                let ListWindow {
                    items,
                    start,
                    selected,
                } = window;
                let list = List::new(items)
                    .highlight_style(
                        Style::default()
                            .bg(self.theme.highlight_bg)
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol(self.theme.highlight_symbol.as_str());
                match focus_panel {
                    SearchPanel::Tracks => render_list_window(
                        frame,
                        list,
                        list_area,
                        &mut sr.track_list,
                        start,
                        selected,
                    ),
                    SearchPanel::Artists => render_list_window(
                        frame,
                        list,
                        list_area,
                        &mut sr.artist_list,
                        start,
                        selected,
                    ),
                    SearchPanel::Albums => render_list_window(
                        frame,
                        list,
                        list_area,
                        &mut sr.album_list,
                        start,
                        selected,
                    ),
                    SearchPanel::Playlists => render_list_window(
                        frame,
                        list,
                        list_area,
                        &mut sr.playlist_list,
                        start,
                        selected,
                    ),
                }
            }
            return;
        }

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

        if let Some(sr) = &mut state.search_results {
            let ptitle = |panel: SearchPanel, base: &'static str| -> String {
                if is_loading && focus_panel == panel {
                    format!("{base} …")
                } else {
                    base.to_string()
                }
            };

            let track_title = ptitle(SearchPanel::Tracks, "Tracks");
            let track_block = self.build_panel_block(
                UiWidget::Search,
                is_focused && focus_panel == SearchPanel::Tracks,
                &track_title,
            );
            let track_inner = track_block.inner(top_cols[0]);
            let ListWindow {
                items: track_items,
                start: track_start,
                selected: track_selected,
            } = build_list_window(
                sr.tracks.len(),
                track_inner.height as usize,
                &sr.track_list,
                |idx| {
                    let t = &sr.tracks[idx];
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.primary)),
                        Span::styled(
                            format!("{:>3}. ", idx + 1),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                        Span::styled(
                            t.name.as_str(),
                            Style::default().fg(self.theme.text_primary),
                        ),
                        Span::styled(
                            format!(" - {}", t.artist),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                    ]))
                },
            );
            let track_list = List::new(track_items)
                .block(track_block)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(
                frame,
                track_list,
                top_cols[0],
                &mut sr.track_list,
                track_start,
                track_selected,
            );

            let artist_title = ptitle(SearchPanel::Artists, "Artists");
            let artist_block = self.build_panel_block(
                UiWidget::Search,
                is_focused && focus_panel == SearchPanel::Artists,
                &artist_title,
            );
            let artist_inner = artist_block.inner(top_cols[1]);
            let ListWindow {
                items: artist_items,
                start: artist_start,
                selected: artist_selected,
            } = build_list_window(
                sr.artists.len(),
                artist_inner.height as usize,
                &sr.artist_list,
                |idx| {
                    let a = &sr.artists[idx];
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.primary)),
                        Span::styled(
                            a.name.as_str(),
                            Style::default().fg(self.theme.text_primary),
                        ),
                        Span::styled(
                            if a.genres.is_empty() {
                                String::new()
                            } else {
                                format!("  {}", a.genres)
                            },
                            Style::default().fg(self.theme.text_secondary),
                        ),
                    ]))
                },
            );
            let artist_list = List::new(artist_items)
                .block(artist_block)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(
                frame,
                artist_list,
                top_cols[1],
                &mut sr.artist_list,
                artist_start,
                artist_selected,
            );

            let album_title = ptitle(SearchPanel::Albums, "Albums");
            let album_block = self.build_panel_block(
                UiWidget::Search,
                is_focused && focus_panel == SearchPanel::Albums,
                &album_title,
            );
            let album_inner = album_block.inner(bot_cols[0]);
            let ListWindow {
                items: album_items,
                start: album_start,
                selected: album_selected,
            } = build_list_window(
                sr.albums.len(),
                album_inner.height as usize,
                &sr.album_list,
                |idx| {
                    let a = &sr.albums[idx];
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.primary)),
                        Span::styled(
                            a.name.as_str(),
                            Style::default().fg(self.theme.text_primary),
                        ),
                        Span::styled(
                            format!(" - {}", a.artist),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                    ]))
                },
            );
            let album_list = List::new(album_items)
                .block(album_block)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(
                frame,
                album_list,
                bot_cols[0],
                &mut sr.album_list,
                album_start,
                album_selected,
            );

            let playlist_title = ptitle(SearchPanel::Playlists, "Playlists");
            let pl_block = self.build_panel_block(
                UiWidget::Search,
                is_focused && focus_panel == SearchPanel::Playlists,
                &playlist_title,
            );
            let pl_inner = pl_block.inner(bot_cols[1]);
            let ListWindow {
                items: pl_items,
                start: pl_start,
                selected: pl_selected,
            } = build_list_window(
                sr.playlists.len(),
                pl_inner.height as usize,
                &sr.playlist_list,
                |idx| {
                    let p = &sr.playlists[idx];
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.primary)),
                        Span::styled(
                            p.name.as_str(),
                            Style::default().fg(self.theme.text_primary),
                        ),
                        Span::styled(
                            format!("  ({})", p.total_tracks),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                    ]))
                },
            );
            let pl_list = List::new(pl_items)
                .block(pl_block)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            render_list_window(
                frame,
                pl_list,
                bot_cols[1],
                &mut sr.playlist_list,
                pl_start,
                pl_selected,
            );
        }
    }

    pub fn render_progress(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let shuffle_label = if pb.shuffle { " Shuf" } else { "" };
        let shuffle_display_width = unicode_width::UnicodeWidthStr::width(shuffle_label);
        let width = area.width.saturating_sub(14 + shuffle_display_width as u16) as usize;
        let filled = (width as f64 * ratio) as usize;

        const WAVEFORM_BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

        let bar = if let Some(waveform) = pb.waveform.as_ref().filter(|w| !w.is_empty()) {
            (0..width)
                .map(|i| {
                    let idx = (i * waveform.len()) / width.max(1);
                    let amp = waveform[idx.min(waveform.len().saturating_sub(1))] as usize;
                    WAVEFORM_BLOCKS[amp.min(WAVEFORM_BLOCKS.len() - 1)]
                })
                .collect::<String>()
        } else {
            format!(
                "{}{}",
                "█".repeat(filled),
                "░".repeat(width.saturating_sub(filled))
            )
        };

        let chars: Vec<char> = bar.chars().collect();
        let played_chars: Vec<char> = chars.iter().take(filled).copied().collect();
        let unplayed: String = chars.iter().skip(filled).collect();

        let primary_rgb = color_to_rgb(self.theme.primary);
        let accent_rgb = color_to_rgb(self.theme.accent_color);
        let track_rgb = color_to_rgb(self.theme.border_subtle);

        let played_spans: Vec<Span> = if filled > 0 {
            played_chars
                .iter()
                .enumerate()
                .map(|(i, ch)| {
                    let t = if filled > 1 {
                        i as f32 / (filled - 1) as f32
                    } else {
                        0.0
                    };
                    let c = lerp_rgb(primary_rgb, accent_rgb, t);
                    Span::styled(
                        ch.to_string(),
                        Style::default().fg(Color::Rgb(c.0, c.1, c.2)),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut line_spans = vec![
            Span::styled(
                fmt_duration(pb.progress_ms),
                Style::default()
                    .fg(self.theme.text_secondary)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::raw(" "),
        ];
        line_spans.extend(played_spans);
        line_spans.push(Span::styled(
            unplayed,
            Style::default().fg(Color::Rgb(track_rgb.0, track_rgb.1, track_rgb.2)),
        ));
        line_spans.push(Span::raw(" "));
        line_spans.push(Span::styled(
            fmt_duration(pb.duration_ms),
            Style::default()
                .fg(self.theme.text_secondary)
                .add_modifier(Modifier::ITALIC),
        ));
        if pb.shuffle {
            line_spans.push(Span::styled(
                shuffle_label,
                Style::default().fg(self.theme.accent_color),
            ));
        }

        let content = Line::from(line_spans);
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }

    pub fn render_marquee(&self, frame: &mut Frame, pb: &PlaybackState, offset: usize, area: Rect) {
        use unicode_width::UnicodeWidthStr;
        let text = if pb.title.is_empty() {
            format!("isi-music v{}", env!("CARGO_PKG_VERSION"))
        } else {
            let t = sanitize_control_chars(&pb.title);
            let a = sanitize_control_chars(&pb.artist);
            format!("{} • {} ", t, a)
        };
        let display = if text.width() < area.width as usize {
            text
        } else {
            let combined = format!("{}   •   ", text);
            let chars: Vec<char> = combined.chars().collect();
            if chars.is_empty() {
                return;
            }
            let area_w = area.width as usize;
            let mut result = String::with_capacity(area_w);
            let mut col = 0usize;
            let mut i = offset % chars.len();
            while col < area_w {
                let ch = chars[i % chars.len()];
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if w == 0 {
                    i += 1;
                    continue;
                }
                if col + w > area_w {
                    break;
                }
                result.push(ch);
                col += w;
                i += 1;
            }
            result
        };
        frame.render_widget(
            Paragraph::new(display).style(Style::default().fg(self.theme.text_secondary)),
            area,
        );
    }

    #[cfg(feature = "album-art")]
    pub fn render_album_art(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if area.width < 2 || area.height < 2 {
            return;
        }
        let img_area = self.album_art_rect(area, state);
        if img_area.width < 2 || img_area.height < 2 {
            return;
        }

        if let Some(art_data) = &mut state.album_art {
            if let Some(protocol_state) = &mut art_data.image_state {
                frame.render_stateful_widget(
                    ratatui_image::StatefulImage::<StatefulProtocol>::default(),
                    img_area,
                    protocol_state,
                );
            }
        }
    }

    #[cfg(feature = "album-art")]
    fn album_art_rect(&self, area: Rect, state: &UiState) -> Rect {
        if area.width < 2 || area.height < 2 {
            return Rect::ZERO;
        }
        if let Some(art) = &state.album_art {
            if let Some(ps) = &art.image_state {
                let s = ps.size_for(
                    Resize::Fit(None),
                    ratatui::layout::Size::new(area.width, area.height),
                );
                if s.width < 2 || s.height < 2 {
                    return Rect::ZERO;
                }
                return Rect {
                    x: area.x + (area.width.saturating_sub(s.width)) / 2,
                    y: area.y + (area.height.saturating_sub(s.height)) / 2,
                    width: s.width,
                    height: s.height,
                };
            }
        }
        Rect::ZERO
    }

    pub fn render_album_art_with_info(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if area.width < 4 || area.height < 6 {
            return;
        }

        let info_h = 3u16.min(area.height / 4);
        let avail_h = area.height.saturating_sub(info_h);

        #[cfg(feature = "album-art")]
        let (img_w, img_h) = if state.show_album_art {
            if let Some(art) = &state.album_art {
                if let Some(ps) = &art.image_state {
                    let s = ps.size_for(
                        ratatui_image::Resize::Fit(None),
                        ratatui::layout::Size::new(area.width, avail_h),
                    );
                    (s.width.max(2), s.height.max(2))
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        #[cfg(not(feature = "album-art"))]
        let (img_w, img_h) = (0u16, 0u16);

        let total_h = img_h + info_h;
        let y0 = area.y + (area.height.saturating_sub(total_h)) / 2;
        let x0 = area.x + (area.width.saturating_sub(img_w)) / 2;

        let art_area = Rect {
            x: x0,
            y: y0,
            width: img_w,
            height: img_h,
        };
        let info_area = Rect {
            x: area.x,
            y: y0 + img_h,
            width: area.width,
            height: info_h,
        };

        #[cfg(feature = "album-art")]
        if state.show_album_art && img_w >= 2 && img_h >= 2 {
            if let Some(art) = &mut state.album_art {
                if let Some(ps) = &mut art.image_state {
                    frame.render_stateful_widget(
                        ratatui_image::StatefulImage::<StatefulProtocol>::default(),
                        art_area,
                        ps,
                    );
                }
            }
        }

        let pb = &state.playback;

        let title_line = if !pb.title.is_empty() {
            Line::from(Span::styled(
                clamp_text(&pb.title, area.width as usize),
                Style::default()
                    .fg(self.theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ))
        } else {
            Line::from("")
        };

        let artist_line = if !pb.artist.is_empty() {
            Line::from(Span::styled(
                clamp_text(&pb.artist, area.width as usize),
                Style::default().fg(self.theme.text_secondary),
            ))
        } else {
            Line::from("")
        };

        frame.render_widget(
            Paragraph::new(vec![title_line, artist_line]).alignment(Alignment::Center),
            info_area,
        );
    }

    pub fn render_queue(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Queue;

        let block = self
            .build_panel_block(UiWidget::Queue, focused, "Queue")
            .title_bottom(Line::from(Span::styled(
                format!(" {} tracks ", state.queue_items.len()),
                Style::default().fg(self.theme.text_secondary),
            )));

        if state.queue_items.is_empty() {
            frame.render_widget(
                Paragraph::new("  Queue empty — press [A] on a track to add")
                    .block(block)
                    .style(Style::default().fg(self.theme.text_secondary)),
                area,
            );
            return;
        }

        let inner = block.inner(area);
        let total = state.queue_items.len();
        let ListWindow {
            items,
            start,
            selected,
        } = build_list_window(total, inner.height as usize, &state.queue_list, |idx| {
            let (name, artist) = &state.queue_items[idx];
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>2}. ", idx + 1),
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::styled(name.as_str(), Style::default().fg(self.theme.text_primary)),
                Span::styled(
                    format!(" - {}", artist),
                    Style::default().fg(self.theme.text_secondary),
                ),
            ]))
        });

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.background_element)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        render_list_window(frame, list, area, &mut state.queue_list, start, selected);
    }
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:>2}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_window_centers_global_selection() {
        let state = ListState::default().with_selected(Some(100));
        let window = build_list_window(2_240, 10, &state, |index| ListItem::new(index.to_string()));

        assert_eq!(window.start, 95);
        assert_eq!(window.items.len(), 10);
        assert_eq!(window.selected, Some(5));
    }

    #[test]
    fn list_window_clamps_to_list_edges() {
        let state = ListState::default().with_selected(Some(2_239));
        let window = build_list_window(2_240, 10, &state, |index| ListItem::new(index.to_string()));

        assert_eq!(window.start, 2_230);
        assert_eq!(window.items.len(), 10);
        assert_eq!(window.selected, Some(9));
    }
}

impl Ui {
    pub fn render_now_playing_widget(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if area.width < 10 || area.height < 5 {
            return;
        }

        #[cfg(feature = "album-art")]
        let info_area = if state.show_album_art {
            let art_size = area.height.min(18).min(area.width / 4).max(12);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(art_size),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(area);

            let art_area = cols[0];
            if let Some(art) = &mut state.album_art {
                if let Some(img_state) = &mut art.image_state {
                    frame.render_stateful_widget(
                        ratatui_image::StatefulImage::<StatefulProtocol>::default(),
                        art_area,
                        img_state,
                    );
                }
            }
            cols[2]
        } else {
            area
        };

        #[cfg(not(feature = "album-art"))]
        let info_area = area;

        let info_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(info_area);

        let title_area = info_grid[0];
        let artist_area = info_grid[1];
        let album_area = info_grid[2];
        let progress_area = info_grid[4];
        state.store_widget_rect(UiWidget::Progress, progress_area);

        let pb = &state.playback;

        frame.render_widget(
            Paragraph::new(vec![Line::from(Span::styled(
                pb.title.as_str(),
                Style::default()
                    .fg(self.theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ))]),
            title_area,
        );

        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![
                Span::styled(
                    "Artist  ",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    pb.artist.as_str(),
                    Style::default().fg(self.theme.text_primary),
                ),
            ])]),
            artist_area,
        );

        let album_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(12)])
            .split(album_area);

        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![
                Span::styled(
                    "Album   ",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    pb.album.as_str(),
                    Style::default().fg(self.theme.text_primary),
                ),
            ])]),
            album_split[0],
        );

        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![Span::styled(
                format!(" Vol: {}% ", pb.volume),
                Style::default().fg(self.theme.text_secondary),
            )])])
            .alignment(ratatui::layout::Alignment::Right),
            album_split[1],
        );

        self.render_progress(frame, &state.playback, progress_area);
    }

    pub fn render_add_to_playlist(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![Span::raw(" Add to Playlist ")]).alignment(Alignment::Left))
            .border_style(Style::default().fg(self.theme.border_active));

        let mut items: Vec<ListItem> = state
            .playlists
            .iter()
            .map(|p| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {} ", p.name)),
                    Span::styled(
                        format!("({})", p.total_tracks),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                ]))
            })
            .collect();

        items.push(ListItem::new(Line::from(vec![Span::styled(
            " + Create new playlist",
            Style::default()
                .fg(self.theme.accent_color)
                .add_modifier(Modifier::BOLD),
        )])));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        frame.render_stateful_widget(list, area, &mut state.add_to_playlist_list);
    }

    pub fn render_delete_playlist_confirm(
        &self,
        frame: &mut Frame,
        state: &mut UiState,
        area: Rect,
    ) {
        let name = state
            .delete_playlist_target
            .as_deref()
            .unwrap_or("this playlist");
        let title = format!(" Delete Playlist — {name}? ");

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(Span::raw(title)).alignment(Alignment::Left))
            .border_style(Style::default().fg(self.theme.border_active));

        let items = vec![
            ListItem::new(Line::from(Span::styled(
                "  Yes (y)",
                Style::default().fg(self.theme.text_primary),
            ))),
            ListItem::new(Line::from(Span::styled(
                "  No (n)",
                Style::default().fg(self.theme.text_primary),
            ))),
        ];

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        frame.render_stateful_widget(list, area, &mut list_state);
    }
}
