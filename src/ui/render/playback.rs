use crate::app::metadata::sanitize_control_chars;
use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};
#[cfg(feature = "album-art")]
use ratatui_image::Resize;
#[cfg(feature = "album-art")]
use ratatui_image::protocol::StatefulProtocol;

use super::{Focus, PlaybackState, Ui, UiState};
use super::{ListWindow, build_list_window, clamp_text, fmt_duration, render_list_window};

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

impl Ui {
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
