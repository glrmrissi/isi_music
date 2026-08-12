pub mod layout;
pub mod local_tree;
pub mod options;
pub mod playback;
pub mod render;
pub mod search;
pub mod state;

pub use local_tree::{LIBRARY_ITEMS, LocalFileTree, LocalNode};
pub use options::SettingsPanel;
pub use playback::PlaybackState;
pub use search::SearchResults;
pub use state::{ActiveContent, CompactItem, Focus, SearchPanel, UiState};

use crate::utils::debug_overlay::DebugOverlay;
use crate::utils::theme::{BorderConfig, LayoutNode, SerializableConstraint, Theme, UiWidget};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
#[cfg(feature = "album-art")]
use ratatui_image::protocol::StatefulProtocol;
use std::sync::Arc;

#[cfg(feature = "album-art")]
pub struct AlbumArtData {
    pub image_state: Option<StatefulProtocol>,
}

pub struct Ui {
    theme: Theme,
    debug_overlay: Arc<DebugOverlay>,
    cached_fullscreen: Option<LayoutNode>,
    cached_fullscreen_show_lyrics: bool,
    cached_fullscreen_show_album_art: bool,
}

impl Ui {
    pub fn new(theme: Theme, debug_overlay: Arc<DebugOverlay>) -> Self {
        Self {
            theme,
            debug_overlay,
            cached_fullscreen: None,
            cached_fullscreen_show_lyrics: false,
            cached_fullscreen_show_album_art: false,
        }
    }

    pub fn theme_snapshot(&self) -> Theme {
        self.theme.clone()
    }

    pub fn build_panel_block(&self, widget: UiWidget, focused: bool, title: &str) -> Block<'_> {
        use crate::utils::theme::BorderStyle as B;

        let cfg: BorderConfig = self.theme.borders.get(&widget).cloned().unwrap_or_default();

        let color = if focused {
            cfg.color_focused.unwrap_or(self.theme.border_active)
        } else {
            cfg.color_unfocused.unwrap_or(self.theme.border_subtle)
        };

        let mut block = match cfg.style {
            B::LeftBar => Block::default()
                .borders(Borders::LEFT)
                .border_type(BorderType::Thick),
            B::Rounded => Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
            B::Thick => Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick),
            B::None => Block::default(),
        };

        block = block
            .border_style(Style::default().fg(color))
            .style(Style::default().bg(self.theme.background_panel))
            .title_top(
                Line::from(vec![Span::styled(
                    format!(" {} ", title),
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )])
                .alignment(Alignment::Left),
            );

        block
    }

    fn build_fullscreen_tree(&mut self, state: &UiState) {
        let rebuild = self.cached_fullscreen.is_none()
            || state.show_lyrics != self.cached_fullscreen_show_lyrics
            || state.show_album_art != self.cached_fullscreen_show_album_art;

        if !rebuild {
            return;
        }

        let mut tree = self.theme.fullscreen_layout.clone();
        if !state.show_lyrics {
            if let Some(children) = &mut tree.children {
                if let Some(constraints) = &mut tree.constraints {
                    if let Some(idx) = children.iter().position(|c| {
                        c.widget == Some(UiWidget::FullscreenLyrics)
                            || c.widget == Some(UiWidget::Lyrics)
                    }) {
                        children.remove(idx);
                        constraints.remove(idx);
                    }
                }
            }
        }
        if !state.show_album_art {
            if let Some(constraints) = &mut tree.constraints {
                if let Some(children) = &tree.children {
                    if let Some(idx) = children
                        .iter()
                        .position(|c| c.widget == Some(UiWidget::NowPlaying))
                    {
                        if idx < constraints.len() {
                            constraints[idx] = SerializableConstraint::Length(8);
                        }
                    }
                }
            }
        }
        self.cached_fullscreen = Some(tree);
        self.cached_fullscreen_show_lyrics = state.show_lyrics;
        self.cached_fullscreen_show_album_art = state.show_album_art;
    }

    pub fn render(&mut self, frame: &mut Frame, state: &mut UiState) {
        state.clear_widget_rects();
        let area = frame.area();

        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.background)),
            area,
        );

        let root_area = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        state.compact_effective = state.compact_mode || root_area.width < 100;

        if state.fullscreen_player {
            let inner = Rect {
                x: root_area.x + 2,
                y: root_area.y + 1,
                width: root_area.width.saturating_sub(4),
                height: root_area.height.saturating_sub(3),
            };
            let footer_area = Rect {
                x: root_area.x + 2,
                y: root_area.y + root_area.height.saturating_sub(2),
                width: root_area.width.saturating_sub(4),
                height: 1,
            };

            if inner.width >= 10 && inner.height >= 5 {
                self.build_fullscreen_tree(state);
                if let Some(ref layout_tree) = self.cached_fullscreen {
                    self.render_recursive(frame, state, inner, layout_tree);
                }
            }

            let footer = Line::from(vec![
                Span::styled(
                    " isi-music ",
                    Style::default().fg(self.theme.text_secondary),
                ),
                Span::styled(
                    format!("v{} ", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(self.theme.border_subtle),
                ),
            ]);
            frame.render_widget(
                Paragraph::new(footer).alignment(Alignment::Left),
                footer_area,
            );
        } else if state.compact_effective {
            let layout_tree = self.build_compact_layout(state);
            self.render_recursive(frame, state, root_area, &layout_tree);
        } else {
            let layout_tree = self.theme.layout_tree.clone();
            self.render_recursive(frame, state, root_area, &layout_tree);
        }

        self.debug_overlay.render(frame, area);
    }
}
