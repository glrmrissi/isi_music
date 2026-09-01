use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
};

use super::{Focus, SearchPanel, Ui, UiState};
use super::{ListWindow, build_list_window, render_list_window};

impl Ui {
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
                let local_only = sr.local_only;
                let label = if local_only {
                    "Local Results"
                } else {
                    match focus_panel {
                        SearchPanel::Tracks => "Tracks",
                        SearchPanel::Artists => "Artists",
                        SearchPanel::Albums => "Albums",
                        SearchPanel::Playlists => "Playlists",
                    }
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
                let window = if local_only {
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
                } else {
                    match focus_panel {
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
                        SearchPanel::Artists => build_list_window(
                            sr.artists.len(),
                            list_height,
                            &sr.artist_list,
                            |idx| {
                                let a = &sr.artists[idx];
                                ListItem::new(Line::from(vec![
                                    Span::styled("", Style::default().fg(self.theme.primary)),
                                    Span::styled(
                                        a.name.as_str(),
                                        Style::default().fg(self.theme.text_primary),
                                    ),
                                ]))
                            },
                        ),
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
                    }
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
                if local_only {
                    render_list_window(frame, list, list_area, &mut sr.track_list, start, selected);
                } else {
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
            }
            return;
        }

        if let Some(sr) = &mut state.search_results {
            let local_only = sr.local_only;

            if local_only {
                let title = if is_loading {
                    "Local Results …".to_string()
                } else {
                    "Local Results".to_string()
                };
                let block = self.build_panel_block(UiWidget::Search, is_focused, &title);
                let inner = block.inner(area);
                frame.render_widget(block, area);
                if inner.height == 0 {
                    return;
                }
                let ListWindow {
                    items,
                    start,
                    selected,
                } = build_list_window(
                    sr.tracks.len(),
                    inner.height as usize,
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
                let list = List::new(items)
                    .highlight_style(
                        Style::default()
                            .bg(self.theme.highlight_bg)
                            .fg(self.theme.primary)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol(self.theme.highlight_symbol.as_str());
                render_list_window(frame, list, inner, &mut sr.track_list, start, selected);
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
}
