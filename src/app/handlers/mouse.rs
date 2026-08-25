use anyhow::Result;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::time::Instant;

use crate::App;

impl App {
    pub async fn handle_mouse(&mut self, event: MouseEvent) -> Result<()> {
        use crate::ui::Focus;
        use crate::utils::theme::UiWidget;

        match event.kind {
            MouseEventKind::ScrollDown => {
                for _ in 0..3 {
                    self.dispatch(crate::keybinds::Action::ScrollDown).await;
                }
            }
            MouseEventKind::ScrollUp => {
                for _ in 0..3 {
                    self.dispatch(crate::keybinds::Action::ScrollUp).await;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let (cx, cy) = (event.column, event.row);

                // Don't process clicks when settings panel is open
                if self.settings_panel.as_ref().is_some_and(|p| p.visible) {
                    return Ok(());
                }

                // Detect double-click: two left clicks within 500ms and ~2 cells
                let now = Instant::now();
                let is_double_click = self
                    .last_click_time
                    .map(|t| now.duration_since(t).as_millis() < 500)
                    .unwrap_or(false)
                    && (self.last_click_pos.0 as i16 - cx as i16).abs() <= 2
                    && (self.last_click_pos.1 as i16 - cy as i16).abs() <= 2;

                let widget = self.state.widget_at(cx, cy);

                // Click on progress bar → seek (no double-click here)
                if matches!(widget, Some(UiWidget::Progress)) {
                    if let Some(rect) = self.state.widget_rects.get(&UiWidget::Progress) {
                        let duration = self.state.playback.duration_ms;
                        if duration > 0 {
                            let bar_start = rect.x + 7;
                            let bar_end = rect.x + rect.width.saturating_sub(7);
                            let bar_w = bar_end.saturating_sub(bar_start);
                            if bar_w > 0 {
                                let click_x = cx.clamp(bar_start, bar_end);
                                let ratio = (click_x - bar_start) as f64 / bar_w as f64;
                                let new_pos = (duration as f64 * ratio) as u64;
                                self.state.playback.progress_ms = new_pos;
                                self.player_mgr.progress_at_play_start = new_pos;
                                if self.state.playback.is_playing {
                                    self.player_mgr.playing_started_at = Some(Instant::now());
                                }
                                let _ = self.seek_tx.send(new_pos as u32);
                                self.state.status_msg =
                                    Some(format!("Seek to {}", super::fmt_seek(new_pos)));
                            }
                        }
                    }
                    self.last_click_time = None;
                    return Ok(());
                }

                // Click on header → toggle play/pause (no double-click)
                if matches!(widget, Some(UiWidget::Header | UiWidget::Search)) {
                    self.dispatch(crate::keybinds::Action::PlayPause).await;
                    self.last_click_time = None;
                    return Ok(());
                }

                // For list panels: single click = focus + select, double click = Enter
                let (focus, should_enter) = match widget {
                    Some(UiWidget::Library) => (Focus::Library, is_double_click),
                    Some(UiWidget::Playlists) => (Focus::Playlists, is_double_click),
                    Some(UiWidget::Queue) => (Focus::Queue, is_double_click),
                    Some(UiWidget::MainContent) => (Focus::Tracks, is_double_click),
                    _ => {
                        self.last_click_time = None;
                        return Ok(());
                    }
                };

                self.state.focus = focus;

                // Select the item under the cursor
                match widget {
                    Some(UiWidget::Library) => {
                        if let Some(rect) = self.state.widget_rects.get(&UiWidget::Library) {
                            let offset = self.state.library_list.offset();
                            if let Some(idx) =
                                super::click_to_list_index(rect, cx, cy, super::LIBRARY_LEN, offset)
                            {
                                self.state.library_list.select(Some(idx));
                            }
                        }
                    }
                    Some(UiWidget::Playlists) => {
                        if let Some(rect) = self.state.widget_rects.get(&UiWidget::Playlists) {
                            let total = self.state.playlists.len();
                            let offset = self.state.playlist_list.offset();
                            if let Some(idx) =
                                super::click_to_list_index(rect, cx, cy, total, offset)
                            {
                                self.state.playlist_list.select(Some(idx));
                            }
                        }
                    }
                    Some(UiWidget::Queue) => {
                        if let Some(rect) = self.state.widget_rects.get(&UiWidget::Queue) {
                            let total = self.state.queue_items.len();
                            let offset = self.state.queue_list.offset();
                            if let Some(idx) =
                                super::click_to_list_index(rect, cx, cy, total, offset)
                            {
                                self.state.queue_list.select(Some(idx));
                            }
                        }
                    }
                    Some(UiWidget::MainContent) => {
                        if let Some(rect) =
                            self.state.widget_rects.get(&UiWidget::MainContent).copied()
                        {
                            match self.state.active_content {
                                crate::ui::ActiveContent::Tracks => {
                                    let total = self.state.sorted_track_indices.len();
                                    let offset = self.state.track_list.offset();
                                    if let Some(idx) =
                                        super::click_to_list_index(&rect, cx, cy, total, offset)
                                    {
                                        self.state.track_list.select(Some(idx));
                                    }
                                }
                                crate::ui::ActiveContent::Albums => {
                                    let total = self.state.albums.len();
                                    let offset = self.state.album_list.offset();
                                    if let Some(idx) =
                                        super::click_to_list_index(&rect, cx, cy, total, offset)
                                    {
                                        self.state.album_list.select(Some(idx));
                                    }
                                }
                                crate::ui::ActiveContent::Artists => {
                                    let total = self.state.artists.len();
                                    let offset = self.state.artist_list.offset();
                                    if let Some(idx) =
                                        super::click_to_list_index(&rect, cx, cy, total, offset)
                                    {
                                        self.state.artist_list.select(Some(idx));
                                    }
                                }
                                crate::ui::ActiveContent::Shows => {
                                    let total = self.state.shows.len();
                                    let offset = self.state.show_list.offset();
                                    if let Some(idx) =
                                        super::click_to_list_index(&rect, cx, cy, total, offset)
                                    {
                                        self.state.show_list.select(Some(idx));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }

                // Double-click → activate (Enter)
                if should_enter {
                    self.handle_enter().await;
                    self.last_click_time = None;
                } else {
                    self.last_click_time = Some(now);
                    self.last_click_pos = (cx, cy);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
