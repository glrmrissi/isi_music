use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
};
use crate::spotify::RepeatState;
#[cfg(feature = "album-art")]
use ratatui_image::protocol::StatefulProtocol;

use super::{Focus, LIBRARY_ITEMS, LocalNode, PlaybackState, SearchPanel, Ui, UiState};

impl Ui {
    pub fn render_local_tree(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let items: Vec<ListItem> = state
                .sorted_track_indices
                .iter()
                .filter_map(|&vi| {
                    let node = state.local_tree.get_visible(vi)?;
                    let indent = "  ".repeat(node.depth());
                    let item = match node {
                        LocalNode::Folder { name, .. } => ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(
                                "  ",
                                Style::default()
                                    .fg(self.theme.accent_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                name.clone(),
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
                                    .fg(self.theme.border_active)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(self.theme.text_primary)
                            };
                            ListItem::new(Line::from(vec![
                                Span::raw(indent),
                                Span::styled(icon, Style::default().fg(self.theme.border_inactive)),
                                Span::styled(track.name.clone(), title_style),
                                if !track.artist.is_empty() {
                                    Span::styled(
                                        format!("  {}", track.artist),
                                        Style::default().fg(self.theme.border_inactive),
                                    )
                                } else {
                                    Span::raw("")
                                },
                            ]))
                        }
                    };
                    Some(item)
                })
                .collect();
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(list, area, &mut state.local_tree_list);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let total_tracks: usize = state
            .local_tree
            .all_nodes
            .iter()
            .filter(|n| !n.is_folder())
            .count();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Local Files ")
            .title_bottom(Line::from(vec![Span::styled(
                format!(
                    " {} tracks  [ENTER] play/expand  [A] queue  [Ctrl+F] search ",
                    total_tracks
                ),
                Style::default().fg(self.theme.border_inactive),
            )]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state
            .sorted_track_indices
            .iter()
            .filter_map(|&vi| {
                let node = state.local_tree.get_visible(vi)?;
                let indent = "  ".repeat(node.depth());
                let item = match node {
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
                                name.clone(),
                                Style::default()
                                    .fg(self.theme.text_primary)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("  ({} tracks)", child_count),
                                Style::default().fg(self.theme.border_inactive),
                            ),
                        ]))
                    }
                    LocalNode::Track { track, .. } => {
                        let is_playing =
                            state.playback.title == track.name && state.playback.is_local;
                        let icon = if is_playing { " " } else { " " };
                        let title_style = if is_playing {
                            Style::default()
                                .fg(self.theme.border_active)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(self.theme.text_primary)
                        };
                        let dur = fmt_duration(track.duration_ms);
                        ListItem::new(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(icon, Style::default().fg(self.theme.border_inactive)),
                            Span::styled(track.name.clone(), title_style),
                            if !track.artist.is_empty() {
                                Span::styled(
                                    format!(" - {}", track.artist),
                                    Style::default().fg(self.theme.border_inactive),
                                )
                            } else {
                                Span::raw("")
                            },
                            Span::styled(
                                format!("  {}", dur),
                                Style::default()
                                    .fg(self.theme.border_inactive)
                                    .add_modifier(Modifier::DIM),
                            ),
                        ]))
                    }
                };
                Some(item)
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut state.local_tree_list);
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

        const LEFT: [u8; 4] = [1 << 6, 1 << 2, 1 << 1, 1 << 0];
        const RIGHT: [u8; 4] = [1 << 7, 1 << 5, 1 << 4, 1 << 3];

        let n_bars = inner.width as usize;
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
            if bar_h == 0 {
                continue;
            }

            let color = if amp > 0.75 {
                self.theme.text_primary
            } else if amp > 0.25 {
                self.theme.accent_color
            } else {
                self.theme.border_inactive
            };

            for cell_y in 0..inner.height as usize {
                let bottom_idx = inner.height as usize - 1 - cell_y;
                let px_base = bottom_idx * 4;
                if px_base >= bar_h {
                    continue;
                }

                let mut bits: u8 = 0;
                for dot_row in 0..4 {
                    if px_base + dot_row < bar_h {
                        bits |= LEFT[dot_row];
                        bits |= RIGHT[dot_row];
                    }
                }
                if bits == 0 {
                    continue;
                }

                let ch = char::from_u32(0x2800 | bits as u32).unwrap_or(' ');
                if let Some(cell) = frame
                    .buffer_mut()
                    .cell_mut((inner.x + bar as u16, inner.y + cell_y as u16))
                {
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

    pub fn render_header(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let compact = area.height < 2;

        if compact {
            let bc = self.breadcrumb(state);
            let content = if state.search_active {
                let mut spans = vec![
                    Span::styled(" Search: ", Style::default().fg(self.theme.border_active)),
                    Span::styled(
                        &state.search_query,
                        Style::default().fg(self.theme.text_primary),
                    ),
                    Span::styled(
                        "█",
                        Style::default()
                            .fg(self.theme.border_active)
                            .add_modifier(Modifier::SLOW_BLINK),
                    ),
                ];
                if !bc.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        &bc,
                        Style::default().fg(self.theme.text_secondary),
                    ));
                }
                Line::from(spans)
            } else if state.quick_search_active {
                let mut spans = vec![
                    Span::styled(
                        " Quick Search: ",
                        Style::default().fg(self.theme.border_active),
                    ),
                    Span::styled(
                        &state.quick_search_query,
                        Style::default().fg(self.theme.text_primary),
                    ),
                    Span::styled(
                        "█",
                        Style::default()
                            .fg(self.theme.border_active)
                            .add_modifier(Modifier::SLOW_BLINK),
                    ),
                ];
                if !bc.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        &bc,
                        Style::default().fg(self.theme.text_secondary),
                    ));
                }
                Line::from(spans)
            } else if let Some(msg) = &state.status_msg {
                let mut spans = vec![Span::styled(
                    msg.clone(),
                    Style::default().fg(self.theme.border_active),
                )];
                if !bc.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        &bc,
                        Style::default().fg(self.theme.text_secondary),
                    ));
                }
                Line::from(spans)
            } else if state.search_results.is_some() {
                let mut spans = vec![Span::styled(
                    " Search Results",
                    Style::default()
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )];
                if !bc.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        &bc,
                        Style::default().fg(self.theme.text_secondary),
                    ));
                }
                Line::from(spans)
            } else {
                let mut spans = vec![Span::styled(
                    " isi-music ",
                    Style::default().fg(self.theme.border_inactive),
                )];
                if !bc.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        &bc,
                        Style::default().fg(self.theme.text_secondary),
                    ));
                }
                Line::from(spans)
            };
            frame.render_widget(
                Paragraph::new(content)
                    .style(Style::default().bg(self.theme.highlight_bg))
                    .alignment(Alignment::Left),
                area,
            );
            return;
        }

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(
                if state.search_active || state.quick_search_active {
                    self.theme.border_active
                } else {
                    self.theme.border_inactive
                },
            ));

        let bc = self.breadcrumb(state);
        if !bc.is_empty() {
            block = block.title(
                Line::from(Span::styled(
                    bc,
                    Style::default().fg(self.theme.text_secondary),
                ))
                .alignment(Alignment::Right),
            );
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content = if state.quick_search_active {
            Line::from(vec![
                Span::styled(
                    "   Quick Search: ",
                    Style::default().fg(self.theme.border_active),
                ),
                Span::styled(
                    &state.quick_search_query,
                    Style::default().fg(self.theme.text_primary),
                ),
                Span::styled(
                    "█",
                    Style::default()
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        } else if state.search_active {
            Line::from(vec![
                Span::styled("   Search: ", Style::default().fg(self.theme.border_active)),
                Span::styled(
                    &state.search_query,
                    Style::default().fg(self.theme.text_primary),
                ),
                Span::styled(
                    "█",
                    Style::default()
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        } else if let Some(msg) = &state.status_msg {
            Line::from(Span::styled(
                msg.clone(),
                Style::default().fg(self.theme.border_active),
            ))
        } else if state.search_results.is_some() {
            Line::from(vec![
                Span::styled(
                    "  Search Results",
                    Style::default()
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  [TAB] switch panel  [ENTER] open  [ESC] close",
                    Style::default().fg(self.theme.border_inactive),
                ),
            ])
        } else {
            Line::from(vec![Span::styled(
                "",
                Style::default().fg(self.theme.border_inactive),
            )])
        };
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Left), inner);
    }

    pub fn render_library(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Library;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![Span::raw(" Library ")]).alignment(Alignment::Left))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = LIBRARY_ITEMS
            .iter()
            .map(|name| ListItem::new(Line::from(vec![Span::raw(format!("  {name} "))])))
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

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

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![Span::raw(" Playlists ")]).alignment(Alignment::Left))
            .title_bottom(Line::from(vec![
                Span::styled(
                    format!(" Vol: {}% ", pb.volume),
                    Style::default().fg(self.theme.border_inactive),
                ),
                Span::styled(
                    format!(" {} ", status_icon),
                    Style::default().fg(self.theme.border_inactive),
                ),
                Span::styled(repeat_str, Style::default().fg(self.theme.border_active)),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state
            .playlists
            .iter()
            .map(|p| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {} ", p.name)),
                    Span::styled(
                        format!("({})", p.total_tracks),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.playlist_list);
    }

    pub fn render_welcome(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let mut items: Vec<ListItem> = Vec::new();

            items.push(ListItem::new(Line::from(Span::styled(
                " Default",
                Style::default()
                    .fg(self.theme.border_inactive)
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
                        .fg(self.theme.border_inactive)
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
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(list, area, &mut state.library_list);
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.theme.border_inactive));
        frame.render_widget(&block, area);
        let inner = block.inner(area);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                " isi-music",
                Style::default()
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Select a playlist from the Library or Playlists panel,",
                Style::default().fg(self.theme.border_inactive),
            )),
            Line::from(Span::styled(
                "or press / to search Spotify.",
                Style::default().fg(self.theme.border_inactive),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "[TAB] navigate panels   [ENTER] select   [/] search   [Ctrl+F] quick search",
                Style::default()
                    .fg(self.theme.border_inactive)
                    .add_modifier(Modifier::DIM),
            )),
        ];

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
                    .style(Style::default().fg(self.theme.border_inactive))
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
                                    .fg(self.theme.border_active)
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
                                .fg(self.theme.border_inactive)
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

        let current = lyrics.lines.get(active).map(|l| l.text.clone());
        let next = lyrics.lines.get(active + 1).map(|l| l.text.clone());

        let lines: Vec<Line> = std::iter::once(Line::from(""))
            .chain(
                current
                    .map(|t| {
                        Line::from(Span::styled(
                            t,
                            Style::default()
                                .fg(self.theme.border_active)
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
                            .fg(self.theme.border_inactive)
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
            let items: Vec<ListItem> = state
                .sorted_track_indices
                .iter()
                .enumerate()
                .filter_map(|(display_idx, &real_idx)| {
                    let t = state.tracks.get(real_idx)?;
                    let is_playing = state.playback.title == t.name;
                    let style = if is_playing {
                        Style::default()
                            .fg(self.theme.border_active)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(self.theme.text_primary)
                    };
                    Some(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:>3}. ", display_idx + 1),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                        Span::styled(t.name.clone(), style),
                        Span::styled(
                            format!("  {}", t.artist),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                    ])))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(list, area, &mut state.track_list);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let title = if state.active_playlist_uri.as_deref() == Some("liked_songs") {
            " Liked Songs ".to_string()
        } else {
            " Tracks ".to_string()
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
        let loading = if state.tracks_loading { " …" } else { "" };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title.as_str())
            .title_bottom(Line::from(vec![
                Span::styled(
                    format!(" {count}{loading} ",),
                    Style::default().fg(self.theme.border_inactive),
                ),
                Span::styled(sort_label, Style::default().fg(self.theme.accent_color)),
                Span::styled(
                    " [Ctrl+F] search  [O] sort ",
                    Style::default().fg(self.theme.border_inactive),
                ),
            ]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state
            .sorted_track_indices
            .iter()
            .enumerate()
            .filter_map(|(display_idx, &real_idx)| {
                let t = state.tracks.get(real_idx)?;
                let is_playing = state.playback.title == t.name;
                let style = if is_playing {
                    Style::default()
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.theme.text_primary)
                };
                Some(ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", display_idx + 1),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::styled(t.name.clone(), style),
                    Span::styled(
                        format!(" - {}", t.artist),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                ])))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.track_list);
    }

    pub fn render_albums(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let items: Vec<ListItem> = state
                .albums
                .iter()
                .enumerate()
                .map(|(idx, a)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:>3}. ", idx + 1),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                        Span::raw(a.name.clone()),
                        Span::styled(
                            format!("  {}", a.artist),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(list, area, &mut state.album_list);
            return;
        }

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
            .title_bottom(Line::from(vec![Span::styled(
                format!(" {count} "),
                Style::default().fg(self.theme.border_inactive),
            )]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state
            .albums
            .iter()
            .enumerate()
            .map(|(idx, a)| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", idx + 1),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::raw(a.name.clone()),
                    Span::styled(
                        format!(" - {}", a.artist),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::styled(
                        format!(" ({} tracks)", a.total_tracks),
                        Style::default()
                            .fg(self.theme.border_inactive)
                            .add_modifier(Modifier::DIM),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.album_list);
    }

    pub fn render_artists(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let items: Vec<ListItem> = state
                .artists
                .iter()
                .enumerate()
                .map(|(idx, a)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:>3}. ", idx + 1),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                        Span::raw(a.name.clone()),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(list, area, &mut state.artist_list);
            return;
        }

        let focused = state.focus == Focus::Tracks;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Artists ")
            .title_bottom(Line::from(vec![Span::styled(
                format!(" {} ", state.artists.len()),
                Style::default().fg(self.theme.border_inactive),
            )]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state
            .artists
            .iter()
            .enumerate()
            .map(|(idx, a)| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", idx + 1),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::raw(a.name.clone()),
                    Span::styled(
                        if a.genres.is_empty() {
                            String::new()
                        } else {
                            format!("  {}", a.genres)
                        },
                        Style::default().fg(self.theme.border_inactive),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.artist_list);
    }

    pub fn render_shows(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let items: Vec<ListItem> = state
                .shows
                .iter()
                .enumerate()
                .map(|(idx, s)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:>3}. ", idx + 1),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                        Span::raw(s.name.clone()),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(list, area, &mut state.show_list);
            return;
        }

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
            .title_bottom(Line::from(vec![Span::styled(
                format!(" {count} "),
                Style::default().fg(self.theme.border_inactive),
            )]))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        let items: Vec<ListItem> = state
            .shows
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>3}. ", idx + 1),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::raw(s.name.clone()),
                    Span::styled(
                        format!("  {}", s.publisher),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::styled(
                        format!(" ({} eps)", s.total_episodes),
                        Style::default()
                            .fg(self.theme.border_inactive)
                            .add_modifier(Modifier::DIM),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.show_list);
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

        let panel_style = |panel: SearchPanel| -> Style {
            if is_focused && focus_panel == panel {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            }
        };

        if state.compact_effective {
            if let Some(sr) = &mut state.search_results {
                let items: Vec<ListItem> = match focus_panel {
                    SearchPanel::Tracks => sr
                        .tracks
                        .iter()
                        .enumerate()
                        .map(|(idx, t)| {
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.border_active)),
                                Span::styled(
                                    format!("{:>3}. ", idx + 1),
                                    Style::default().fg(self.theme.border_inactive),
                                ),
                                Span::raw(t.name.clone()),
                                Span::styled(
                                    format!("  {}", t.artist),
                                    Style::default().fg(self.theme.border_inactive),
                                ),
                            ]))
                        })
                        .collect(),
                    SearchPanel::Artists => sr
                        .artists
                        .iter()
                        .map(|a| {
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.border_active)),
                                Span::raw(a.name.clone()),
                            ]))
                        })
                        .collect(),
                    SearchPanel::Albums => sr
                        .albums
                        .iter()
                        .map(|a| {
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.border_active)),
                                Span::raw(a.name.clone()),
                                Span::styled(
                                    format!("  {}", a.artist),
                                    Style::default().fg(self.theme.border_inactive),
                                ),
                            ]))
                        })
                        .collect(),
                    SearchPanel::Playlists => sr
                        .playlists
                        .iter()
                        .map(|p| {
                            ListItem::new(Line::from(vec![
                                Span::styled("", Style::default().fg(self.theme.border_active)),
                                Span::raw(p.name.clone()),
                            ]))
                        })
                        .collect(),
                };
                let label = match focus_panel {
                    SearchPanel::Tracks => " Tracks ",
                    SearchPanel::Artists => " Artists ",
                    SearchPanel::Albums => " Albums ",
                    SearchPanel::Playlists => " Playlists ",
                };
                let title = if is_loading {
                    format!("{label}…")
                } else {
                    label.to_string()
                };
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        title,
                        Style::default().fg(self.theme.border_inactive),
                    ))),
                    area,
                );
                let list = List::new(items)
                    .highlight_style(
                        Style::default()
                            .bg(self.theme.highlight_bg)
                            .fg(self.theme.border_active)
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("  ");
                let list_area = Rect {
                    x: area.x,
                    y: area.y + 1,
                    width: area.width,
                    height: area.height.saturating_sub(1),
                };
                if list_area.height > 0 {
                    match focus_panel {
                        SearchPanel::Tracks => {
                            frame.render_stateful_widget(list, list_area, &mut sr.track_list)
                        }
                        SearchPanel::Artists => {
                            frame.render_stateful_widget(list, list_area, &mut sr.artist_list)
                        }
                        SearchPanel::Albums => {
                            frame.render_stateful_widget(list, list_area, &mut sr.album_list)
                        }
                        SearchPanel::Playlists => {
                            frame.render_stateful_widget(list, list_area, &mut sr.playlist_list)
                        }
                    }
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

            let track_items: Vec<ListItem> = sr
                .tracks
                .iter()
                .enumerate()
                .map(|(idx, t)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.border_active)),
                        Span::styled(
                            format!("{:>3}. ", idx + 1),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                        Span::raw(t.name.clone()),
                        Span::styled(
                            format!(" - {}", t.artist),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                    ]))
                })
                .collect();
            let track_list = List::new(track_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(ptitle(SearchPanel::Tracks, " Tracks "))
                        .border_style(panel_style(SearchPanel::Tracks)),
                )
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(track_list, top_cols[0], &mut sr.track_list);

            let artist_items: Vec<ListItem> = sr
                .artists
                .iter()
                .map(|a| {
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.border_active)),
                        Span::raw(a.name.clone()),
                        Span::styled(
                            if a.genres.is_empty() {
                                String::new()
                            } else {
                                format!("  {}", a.genres)
                            },
                            Style::default().fg(self.theme.border_inactive),
                        ),
                    ]))
                })
                .collect();
            let artist_list = List::new(artist_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(ptitle(SearchPanel::Artists, " Artists "))
                        .border_style(panel_style(SearchPanel::Artists)),
                )
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(artist_list, top_cols[1], &mut sr.artist_list);

            let album_items: Vec<ListItem> = sr
                .albums
                .iter()
                .map(|a| {
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.border_active)),
                        Span::raw(a.name.clone()),
                        Span::styled(
                            format!(" - {}", a.artist),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                    ]))
                })
                .collect();
            let album_list = List::new(album_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(ptitle(SearchPanel::Albums, " Albums "))
                        .border_style(panel_style(SearchPanel::Albums)),
                )
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(album_list, bot_cols[0], &mut sr.album_list);

            let pl_items: Vec<ListItem> = sr
                .playlists
                .iter()
                .map(|p| {
                    ListItem::new(Line::from(vec![
                        Span::styled(" ", Style::default().fg(self.theme.border_active)),
                        Span::raw(p.name.clone()),
                        Span::styled(
                            format!("  ({})", p.total_tracks),
                            Style::default().fg(self.theme.border_inactive),
                        ),
                    ]))
                })
                .collect();
            let pl_list = List::new(pl_items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(ptitle(SearchPanel::Playlists, " Playlists "))
                        .border_style(panel_style(SearchPanel::Playlists)),
                )
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.border_active)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  ");
            frame.render_stateful_widget(pl_list, bot_cols[1], &mut sr.playlist_list);
        }
    }

    pub fn render_progress(&self, frame: &mut Frame, pb: &PlaybackState, area: Rect) {
        let ratio = if pb.duration_ms > 0 {
            (pb.progress_ms as f64 / pb.duration_ms as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let shuffle_label = if pb.shuffle { " Shuf" } else { "" };
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
                Style::default()
                    .fg(self.theme.accent_color)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::raw(" "),
            Span::styled(bar, Style::default().fg(self.theme.accent_color)),
            Span::raw(" "),
            Span::styled(
                fmt_duration(pb.duration_ms),
                Style::default()
                    .fg(self.theme.accent_color)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::styled(shuffle_label, Style::default().fg(self.theme.accent_color)),
        ]);
        frame.render_widget(Paragraph::new(content).alignment(Alignment::Center), area);
    }

    pub fn render_marquee(&self, frame: &mut Frame, pb: &PlaybackState, offset: usize, area: Rect) {
        let text = if pb.title.is_empty() {
            format!("isi-music v{}", env!("CARGO_PKG_VERSION"))
        } else {
            format!("{} • {} ", pb.title, pb.artist)
        };
        let display = if text.len() < area.width as usize {
            text
        } else {
            let combined = format!("{}   •   ", text);
            let chars: Vec<char> = combined.chars().collect();
            (0..area.width as usize)
                .map(|i| chars[(offset + i) % chars.len()])
                .collect()
        };
        frame.render_widget(
            Paragraph::new(display).style(Style::default().fg(self.theme.border_inactive)),
            area,
        );
    }

    #[cfg(feature = "album-art")]
    pub fn render_album_art(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            if let Some(art_data) = &mut state.album_art {
                if let Some(protocol_state) = &mut art_data.image_state {
                    let size = area.height.min(area.width);
                    let img_area = Rect {
                        x: area.x + (area.width.saturating_sub(size)) / 2,
                        y: area.y + (area.height.saturating_sub(size)) / 2,
                        width: size,
                        height: size,
                    };
                    if img_area.width > 2 && img_area.height > 2 {
                        frame.render_stateful_widget(
                            ratatui_image::StatefulImage::<StatefulProtocol>::default(),
                            img_area,
                            protocol_state,
                        );
                    }
                }
            }
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Cover ")
            .border_style(Style::default().fg(self.theme.border_inactive));

        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        if let Some(art_data) = &mut state.album_art {
            if let Some(protocol_state) = &mut art_data.image_state {
                frame.render_stateful_widget(
                    ratatui_image::StatefulImage::<StatefulProtocol>::default(),
                    inner,
                    protocol_state,
                );
            }
        }
    }

    pub fn render_queue(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let focused = state.focus == Focus::Queue;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Queue ")
            .title_bottom(Line::from(Span::styled(
                format!(" {} tracks ", state.queue_items.len()),
                Style::default().fg(self.theme.border_inactive),
            )))
            .border_style(if focused {
                Style::default().fg(self.theme.border_active)
            } else {
                Style::default().fg(self.theme.border_inactive)
            });

        if state.queue_items.is_empty() {
            frame.render_widget(
                Paragraph::new("  Queue empty — press [A] on a track to add")
                    .block(block)
                    .style(Style::default().fg(self.theme.border_inactive)),
                area,
            );
            return;
        }

        let items: Vec<ListItem> = state
            .queue_items
            .iter()
            .enumerate()
            .map(|(idx, (name, artist))| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>2}. ", idx + 1),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                    Span::styled(name.clone(), Style::default().fg(self.theme.text_primary)),
                    Span::styled(
                        format!(" - {}", artist),
                        Style::default().fg(self.theme.border_inactive),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.border_active)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, area, &mut state.queue_list);
    }
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

impl Ui {
    pub fn render_now_playing_widget(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if area.width < 10 || area.height < 5 {
            return;
        }

        let pb = &state.playback;

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
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .split(info_area);

        let title_area = info_grid[0];
        let artist_area = info_grid[1];
        let album_area = info_grid[2];
        let progress_area = info_grid[4];

        frame.render_widget(
            Paragraph::new(vec![Line::from(Span::styled(
                pb.title.clone(),
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
                    pb.artist.clone(),
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
                    pb.album.clone(),
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
}
