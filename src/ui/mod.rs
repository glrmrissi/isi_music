pub mod layout;
pub mod local_tree;
pub mod options;
pub mod playback;
pub mod render;
pub mod search;
pub mod state;

pub use local_tree::{LIBRARY_ITEMS, LocalFileTree, LocalNode};
pub use options::OptionsPanel;
pub use playback::PlaybackState;
pub use search::SearchResults;
pub use state::{ActiveContent, Focus, SearchPanel, UiState};

use crate::utils::debug_overlay::DebugOverlay;
use crate::utils::theme::{LayoutNode, SerializableConstraint, Theme, UiWidget};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders},
};
use ratatui_image::protocol::StatefulProtocol;
use std::sync::Arc;

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
        let area = frame.area();

        let root_area = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        state.compact_effective = state.compact_mode || root_area.width < 100;

        if state.compact_effective
            && matches!(
                state.focus,
                Focus::Library | Focus::Playlists | Focus::Queue
            )
        {
            state.focus = Focus::Tracks;
        }

        if state.fullscreen_player {
            let accent = self.theme.border_active;
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent));
            let inner = block.inner(root_area);
            frame.render_widget(block, root_area);

            if inner.width >= 10 && inner.height >= 5 {
                self.build_fullscreen_tree(state);
                if let Some(ref layout_tree) = self.cached_fullscreen {
                    self.render_recursive(frame, state, inner, layout_tree);
                }
            }
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
