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
use crate::utils::theme::Theme;
use ratatui::{Frame, layout::Rect};
use ratatui_image::protocol::StatefulProtocol;
use std::sync::Arc;

pub struct AlbumArtData {
    pub image_state: Option<StatefulProtocol>,
}

pub struct Ui {
    theme: Theme,
    debug_overlay: Arc<DebugOverlay>,
}

impl Ui {
    pub fn new(theme: Theme, debug_overlay: Arc<DebugOverlay>) -> Self {
        Self {
            theme,
            debug_overlay,
        }
    }

    pub fn render(&self, frame: &mut Frame, state: &mut UiState) {
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
            self.render_now_playing(frame, state, root_area);
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
