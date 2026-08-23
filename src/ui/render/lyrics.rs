use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph, Wrap},
};

use super::{Ui, UiState};

impl Ui {
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
                    .wrap(Wrap { trim: false }),
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
}
