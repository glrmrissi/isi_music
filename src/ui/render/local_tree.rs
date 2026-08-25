use crate::app::metadata::sanitize_control_chars;
use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
};
use unicode_width::UnicodeWidthStr;

use super::{Focus, LocalNode, Ui, UiState};
use super::{build_list_window, clamp_text, fmt_duration, pad_right, render_list_window};

impl Ui {
    pub fn render_local_tree(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let total = state.sorted_track_indices.len();
            let width = area.width as usize;
            let super::ListWindow {
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
                            let icon = " ";
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
        let super::ListWindow {
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
                        let icon = " ";
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
}
