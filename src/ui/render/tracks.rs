use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
};
use unicode_width::UnicodeWidthStr;

use super::{Focus, Ui, UiState};
use super::{
    ListWindow, build_list_window, calculate_number_width, clamp_text, fmt_duration, pad_right,
    render_list_window,
};

impl Ui {
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
                    let num_prefix_width = num_width + 2;
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
                let num_prefix_width = num_width + 2;
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
}
