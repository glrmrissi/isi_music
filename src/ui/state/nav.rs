use ratatui::widgets::ListState;

use super::{ActiveContent, CompactItem, Focus, NavEntry, UiState};
use crate::ui::LIBRARY_ITEMS;

fn scroll_up(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state
        .selected()
        .map(|i| if i == 0 { len - 1 } else { i - 1 })
        .unwrap_or(0);
    state.select(Some(i));
}

fn scroll_down(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state
        .selected()
        .map(|i| if i >= len - 1 { 0 } else { i + 1 })
        .unwrap_or(0);
    state.select(Some(i));
}

impl UiState {
    pub fn push_nav(&mut self) {
        if self.nav_stack.len() >= 3 {
            self.nav_stack.remove(0);
        }
        let entry = NavEntry {
            active_content: self.active_content.clone(),
            focus: self.focus,
            active_playlist_uri: self.active_playlist_uri.clone(),
            active_playlist_id: self.active_playlist_id.clone(),
            active_artist_name: self.active_artist_name.clone(),
            search_results: self.search_results.clone(),
            previous_search: self.previous_search.clone(),
            tracks: self.tracks.clone(),
            sorted_track_indices: self.sorted_track_indices.clone(),
            track_sort_by: self.track_sort_by,
            label: self.current_label(),
        };
        self.nav_stack.push(entry);
    }

    pub fn pop_nav(&mut self) -> Option<NavEntry> {
        self.nav_stack.pop()
    }

    pub(super) fn compact_selectable_positions(&self) -> Vec<usize> {
        let mut positions: Vec<usize> = (1..=LIBRARY_ITEMS.len()).collect();
        if !self.playlists.is_empty() {
            let playlist_start = 1 + LIBRARY_ITEMS.len() + 1;
            for i in 0..self.playlists.len() {
                positions.push(playlist_start + i);
            }
        }
        positions
    }

    pub fn compact_item_at(&self, pos: usize) -> Option<CompactItem> {
        if pos >= 1 && pos < 1 + LIBRARY_ITEMS.len() {
            Some(CompactItem::LibraryItem(pos - 1))
        } else if !self.playlists.is_empty() {
            let playlist_start = 1 + LIBRARY_ITEMS.len() + 1;
            if pos >= playlist_start {
                let idx = pos - playlist_start;
                if idx < self.playlists.len() {
                    return Some(CompactItem::PlaylistItem(idx));
                }
            }
            None
        } else {
            None
        }
    }

    pub fn nav_up(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if selectable.is_empty() {
                return;
            }
            let cur = self.library_list.selected().unwrap_or(selectable[0]);
            let idx = selectable.iter().position(|&p| p == cur).unwrap_or(0);
            let next = if idx == 0 {
                selectable.len() - 1
            } else {
                idx - 1
            };
            self.library_list.select(Some(selectable[next]));
            return;
        }
        match self.focus {
            Focus::Library => {
                let i = self
                    .library_list
                    .selected()
                    .map(|i| {
                        if i == 0 {
                            LIBRARY_ITEMS.len() - 1
                        } else {
                            i - 1
                        }
                    })
                    .unwrap_or(0);
                self.library_list.select(Some(i));
            }
            Focus::Playlists => scroll_up(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => scroll_up(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists => scroll_up(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows => scroll_up(&mut self.show_list, self.shows.len()),
                ActiveContent::LocalFiles => {
                    scroll_up(&mut self.local_tree_list, self.sorted_track_indices.len())
                }
                _ => scroll_up(&mut self.track_list, self.sorted_track_indices.len()),
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    sr.nav_up();
                }
            }
            Focus::Queue => scroll_up(&mut self.queue_list, self.queue_items.len()),
        }
    }

    pub fn nav_down(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if selectable.is_empty() {
                return;
            }
            let cur = self.library_list.selected().unwrap_or(selectable[0]);
            let idx = selectable.iter().position(|&p| p == cur).unwrap_or(0);
            let next = if idx >= selectable.len() - 1 {
                0
            } else {
                idx + 1
            };
            self.library_list.select(Some(selectable[next]));
            return;
        }
        match self.focus {
            Focus::Library => {
                let i = self
                    .library_list
                    .selected()
                    .map(|i| {
                        if i >= LIBRARY_ITEMS.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    })
                    .unwrap_or(0);
                self.library_list.select(Some(i));
            }
            Focus::Playlists => scroll_down(&mut self.playlist_list, self.playlists.len()),
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => scroll_down(&mut self.album_list, self.albums.len()),
                ActiveContent::Artists => scroll_down(&mut self.artist_list, self.artists.len()),
                ActiveContent::Shows => scroll_down(&mut self.show_list, self.shows.len()),
                ActiveContent::LocalFiles => {
                    scroll_down(&mut self.local_tree_list, self.sorted_track_indices.len())
                }
                _ => scroll_down(&mut self.track_list, self.sorted_track_indices.len()),
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    sr.nav_down();
                }
            }
            Focus::Queue => scroll_down(&mut self.queue_list, self.queue_items.len()),
        }
    }

    pub fn nav_first(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if !selectable.is_empty() {
                self.library_list.select(Some(selectable[0]));
            }
            return;
        }
        match self.focus {
            Focus::Library => self.library_list.select(Some(0)),
            Focus::Playlists => {
                if !self.playlists.is_empty() {
                    self.playlist_list.select(Some(0));
                }
            }
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => {
                    if !self.albums.is_empty() {
                        self.album_list.select(Some(0));
                    }
                }
                ActiveContent::Artists => {
                    if !self.artists.is_empty() {
                        self.artist_list.select(Some(0));
                    }
                }
                ActiveContent::Shows => {
                    if !self.shows.is_empty() {
                        self.show_list.select(Some(0));
                    }
                }
                ActiveContent::LocalFiles => {
                    if self.sorted_track_indices.len() > 0 {
                        self.local_tree_list.select(Some(0));
                    }
                }
                _ => {
                    if !self.sorted_track_indices.is_empty() {
                        self.track_list.select(Some(0));
                    }
                }
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    if sr.current_len() > 0 {
                        sr.current_list_mut().select(Some(0));
                    }
                }
            }
            Focus::Queue => {
                if !self.queue_items.is_empty() {
                    self.queue_list.select(Some(0));
                }
            }
        }
    }

    pub fn nav_last(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if !selectable.is_empty() {
                self.library_list
                    .select(Some(selectable[selectable.len() - 1]));
            }
            return;
        }
        match self.focus {
            Focus::Library => self.library_list.select(Some(LIBRARY_ITEMS.len() - 1)),
            Focus::Playlists => {
                let n = self.playlists.len();
                if n > 0 {
                    self.playlist_list.select(Some(n - 1));
                }
            }
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => {
                    let n = self.albums.len();
                    if n > 0 {
                        self.album_list.select(Some(n - 1));
                    }
                }
                ActiveContent::Artists => {
                    let n = self.artists.len();
                    if n > 0 {
                        self.artist_list.select(Some(n - 1));
                    }
                }
                ActiveContent::Shows => {
                    let n = self.shows.len();
                    if n > 0 {
                        self.show_list.select(Some(n - 1));
                    }
                }
                ActiveContent::LocalFiles => {
                    let n = self.sorted_track_indices.len();
                    if n > 0 {
                        self.local_tree_list.select(Some(n - 1));
                    }
                }
                _ => {
                    let n = self.sorted_track_indices.len();
                    if n > 0 {
                        self.track_list.select(Some(n - 1));
                    }
                }
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    let n = sr.current_len();
                    if n > 0 {
                        sr.current_list_mut().select(Some(n - 1));
                    }
                }
            }
            Focus::Queue => {
                let n = self.queue_items.len();
                if n > 0 {
                    self.queue_list.select(Some(n - 1));
                }
            }
        }
    }

    pub fn nav_middle(&mut self) {
        if self.compact_effective
            && self.focus == Focus::Tracks
            && self.active_content == ActiveContent::None
        {
            let selectable = self.compact_selectable_positions();
            if !selectable.is_empty() {
                self.library_list
                    .select(Some(selectable[selectable.len() / 2]));
            }
            return;
        }
        match self.focus {
            Focus::Library => self.library_list.select(Some(LIBRARY_ITEMS.len() / 2)),
            Focus::Playlists => {
                let n = self.playlists.len();
                if n > 0 {
                    self.playlist_list.select(Some(n / 2));
                }
            }
            Focus::Tracks => match self.active_content {
                ActiveContent::Albums => {
                    let n = self.albums.len();
                    if n > 0 {
                        self.album_list.select(Some(n / 2));
                    }
                }
                ActiveContent::Artists => {
                    let n = self.artists.len();
                    if n > 0 {
                        self.artist_list.select(Some(n / 2));
                    }
                }
                ActiveContent::Shows => {
                    let n = self.shows.len();
                    if n > 0 {
                        self.show_list.select(Some(n / 2));
                    }
                }
                ActiveContent::LocalFiles => {
                    let n = self.sorted_track_indices.len();
                    if n > 0 {
                        self.local_tree_list.select(Some(n / 2));
                    }
                }
                _ => {
                    let n = self.sorted_track_indices.len();
                    if n > 0 {
                        self.track_list.select(Some(n / 2));
                    }
                }
            },
            Focus::Search => {
                if let Some(sr) = &mut self.search_results {
                    let n = sr.current_len();
                    if n > 0 {
                        sr.current_list_mut().select(Some(n / 2));
                    }
                }
            }
            Focus::Queue => {
                let n = self.queue_items.len();
                if n > 0 {
                    self.queue_list.select(Some(n / 2));
                }
            }
        }
    }
}
