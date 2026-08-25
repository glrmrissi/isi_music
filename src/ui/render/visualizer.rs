use ratatui::{Frame, layout::Rect, widgets::Block};

use super::{PlaybackState, Ui, UiState};

impl Ui {
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

    #[allow(clippy::too_many_arguments)]
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

    #[allow(clippy::too_many_arguments)]
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

    #[allow(clippy::too_many_arguments)]
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
            if bx < inner.x + inner.width
                && by < viz_top + effective_h
                && let Some(cell) = frame.buffer_mut().cell_mut((bx, by))
            {
                cell.set_char('●').set_fg(viz_color);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
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
}
