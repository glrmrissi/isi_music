use crate::utils::theme::{LayoutNode, SerializableConstraint, SerializableDirection, UiWidget};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Block,
};

use super::{ActiveContent, Ui, UiState};

impl Ui {
    pub fn build_compact_layout(&self, state: &UiState) -> LayoutNode {
        let leaf = |w: UiWidget| LayoutNode {
            widget: Some(w),
            direction: None,
            constraints: None,
            children: None,
        };

        let (main_constraints, main_children) = if self.theme.show_ascii_art {
            (
                vec![
                    SerializableConstraint::Percentage(35),
                    SerializableConstraint::Fill(1),
                ],
                vec![leaf(UiWidget::AsciiArt), leaf(UiWidget::MainContent)],
            )
        } else {
            (
                vec![SerializableConstraint::Fill(1)],
                vec![leaf(UiWidget::MainContent)],
            )
        };

        let mut constraints = vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
        ];
        let mut children: Vec<LayoutNode> = vec![
            leaf(UiWidget::Header),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(main_constraints),
                widget: None,
                children: Some(main_children),
            },
        ];

        if state.show_lyrics {
            constraints.push(SerializableConstraint::Length(3));
            children.push(leaf(UiWidget::Lyrics));
        }

        constraints.push(SerializableConstraint::Length(1));
        children.push(LayoutNode {
            direction: Some(SerializableDirection::Horizontal),
            constraints: Some(vec![
                SerializableConstraint::Percentage(30),
                SerializableConstraint::Fill(1),
            ]),
            widget: None,
            children: Some(vec![leaf(UiWidget::Marquee), leaf(UiWidget::Progress)]),
        });

        LayoutNode {
            direction: Some(SerializableDirection::Vertical),
            constraints: Some(constraints),
            widget: None,
            children: Some(children),
        }
    }

    pub fn render_recursive(
        &self,
        frame: &mut Frame,
        state: &mut UiState,
        area: Rect,
        node: &LayoutNode,
    ) {
        if let Some(widget_type) = &node.widget {
            match widget_type {
                UiWidget::Header => self.render_header(frame, state, area),
                UiWidget::Library => self.render_library(frame, state, area),
                UiWidget::Playlists => self.render_playlists(frame, state, area),
                UiWidget::AlbumArt => {
                    if state.show_album_art {
                        self.render_album_art(frame, state, area);
                    }
                }
                UiWidget::MainContent => self.render_main_area_logic(frame, state, area),
                UiWidget::Queue => self.render_queue(frame, state, area),
                UiWidget::Progress => self.render_progress(frame, &state.playback, area),
                UiWidget::Marquee => {
                    self.render_marquee(frame, &state.playback, state.marquee_offset, area)
                }
                UiWidget::Visualizer => {
                    let viz_bands = state.viz_bands.clone();
                    let pb = state.playback.clone();
                    self.render_visualizer(frame, &pb, &viz_bands, area, state);
                }
                UiWidget::Help => {}
                UiWidget::AsciiArt => self.render_ascii_art(frame, area),
                UiWidget::Spacer => {}
                UiWidget::Lyrics => self.render_lyrics_compact(frame, state, area),
                UiWidget::NowPlaying => self.render_now_playing_widget(frame, state, area),
                UiWidget::FullscreenLyrics => {
                    if state.show_lyrics {
                        self.render_lyrics(frame, state, area);
                    }
                }
            }
            return;
        }

        if let (Some(dir), Some(raw_constraints), Some(children)) =
            (node.direction, &node.constraints, &node.children)
        {
            if children.is_empty() {
                return;
            }

            let parsed: Vec<Constraint> = raw_constraints
                .iter()
                .map(|&c| Constraint::from(c))
                .collect();

            let chunks = Layout::default()
                .direction(Direction::from(dir))
                .constraints(parsed)
                .split(area);

            for (i, child) in children.iter().enumerate() {
                if let Some(chunk) = chunks.get(i) {
                    self.render_recursive(frame, state, *chunk, child);
                }
            }
        }
    }

    pub fn render_main_area_logic(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.search_results.is_some() {
            self.render_search_panels(frame, state, area);
        } else {
            match &state.active_content {
                ActiveContent::None => {
                    self.render_welcome(frame, state, area);
                }
                ActiveContent::Tracks => self.render_tracks(frame, state, area),
                ActiveContent::LocalFiles => self.render_local_tree(frame, state, area),
                ActiveContent::Albums => self.render_albums(frame, state, area),
                ActiveContent::Artists => self.render_artists(frame, state, area),
                ActiveContent::Shows => self.render_shows(frame, state, area),
            }
        }
    }

    pub fn render_ascii_art(&self, frame: &mut Frame, area: Rect) {
        if !self.theme.show_ascii_art {
            return;
        }
        let Some(lines) = self.theme.load_ascii_art() else {
            return;
        };
        if lines.is_empty() {
            return;
        }

        let block = Block::default();
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let art_height = lines.len() as u16;
        if inner.width < 5 || inner.height < 1 {
            return;
        }

        let vertical_start = if inner.height > art_height {
            (inner.height - art_height) / 2
        } else {
            0
        };

        for (line_idx, line) in lines.iter().take(inner.height as usize).enumerate() {
            let y = inner.y + vertical_start + line_idx as u16;
            if y >= inner.y + inner.height {
                break;
            }

            let line_width = line.chars().count() as u16;
            let horizontal_start = if inner.width > line_width {
                (inner.width - line_width) / 2
            } else {
                0
            };

            let mut x = inner.x + horizontal_start;
            for ch in line.chars() {
                if x >= inner.x + inner.width {
                    break;
                }
                if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                    cell.set_char(ch).set_fg(self.theme.accent_color);
                }
                x = x.saturating_add(1);
            }
        }
    }
}
