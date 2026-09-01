use crate::spotify::RepeatState;
use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::{Focus, Ui, UiState};
use super::{ListWindow, build_list_window, render_list_window};

impl Ui {
    pub(super) fn breadcrumb(&self, state: &UiState) -> String {
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

        let items: Vec<ListItem> = state
            .library_items
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
}
