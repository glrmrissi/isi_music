use super::{ActiveContent, UiState};
use crate::ui::LocalNode;

impl UiState {
    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
    }

    pub fn start_quick_search(&mut self) {
        self.quick_search_active = true;
        self.quick_search_query.clear();
    }

    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
    }

    pub fn cancel_quick_search(&mut self) {
        self.quick_search_active = false;
        self.quick_search_query.clear();
    }

    pub fn search_push(&mut self, c: char) {
        self.search_query.push(c);
    }

    pub fn quick_search_push(&mut self, c: char) {
        self.quick_search_query.push(c);
        self.apply_quick_filter();
    }

    pub fn search_pop(&mut self) {
        self.search_query.pop();
    }

    pub fn quick_search_pop(&mut self) {
        self.quick_search_query.pop();
        self.apply_quick_filter();
    }

    pub fn apply_quick_filter(&mut self) {
        let query = self.quick_search_query.to_lowercase();

        match self.active_content {
            ActiveContent::Tracks | ActiveContent::None => {
                if query.is_empty() {
                    self.sorted_track_indices = (0..self.tracks.len()).collect();
                } else {
                    self.sorted_track_indices = (0..self.tracks.len())
                        .filter(|&i| {
                            if let Some(t) = self.tracks.get(i) {
                                t.name.to_lowercase().contains(&query)
                                    || t.artist.to_lowercase().contains(&query)
                                    || t.album.to_lowercase().contains(&query)
                            } else {
                                false
                            }
                        })
                        .collect();
                }
                if !self.sorted_track_indices.is_empty() {
                    self.track_list.select(Some(0));
                } else {
                    self.track_list.select(None);
                }
            }
            ActiveContent::LocalFiles => {
                if query.is_empty() {
                    self.sorted_track_indices = (0..self.local_tree.visible_len()).collect();
                } else {
                    let query_lower = query.to_lowercase();

                    self.sorted_track_indices = (0..self.local_tree.visible_len())
                        .filter(|&vi| {
                            self.local_tree
                                .get_visible(vi)
                                .map_or(false, |node| match node {
                                    LocalNode::Folder { name, .. } => {
                                        name.to_lowercase().contains(&query_lower)
                                    }
                                    LocalNode::Track { track, .. } => {
                                        track.name.to_lowercase().contains(&query_lower)
                                            || track.artist.to_lowercase().contains(&query_lower)
                                    }
                                })
                        })
                        .collect();
                }
                if !self.sorted_track_indices.is_empty() {
                    self.local_tree_list.select(Some(0));
                } else {
                    self.local_tree_list.select(None);
                }
            }
            _ => {}
        }
    }
}
