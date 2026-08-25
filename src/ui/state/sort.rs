use super::{ActiveContent, TrackSortBy, UiState};

impl UiState {
    pub fn sort_tracks(&mut self) {
        self.track_sort_by = self.track_sort_by.next();
        self.rebuild_sort_indices();
    }

    pub fn rebuild_sort_indices(&mut self) {
        match self.active_content {
            ActiveContent::Tracks | ActiveContent::None => {
                let selected_real_idx = self
                    .track_list
                    .selected()
                    .and_then(|i| self.sorted_track_indices.get(i).copied());

                self.sorted_track_indices = (0..self.tracks.len()).collect();

                let is_liked_songs = self.active_playlist_id.as_deref() == Some("liked_songs");

                if self.track_sort_by != TrackSortBy::Default || is_liked_songs {
                    self.sorted_track_indices.sort_by(|&a, &b| {
                        let track_a = &self.tracks[a];
                        let track_b = &self.tracks[b];

                        match self.track_sort_by {
                            TrackSortBy::Title => track_a.name.cmp(&track_b.name),
                            TrackSortBy::Artist => track_a.artist.cmp(&track_b.artist),
                            TrackSortBy::Album => track_a.album.cmp(&track_b.album),
                            TrackSortBy::Duration => track_a.duration_ms.cmp(&track_b.duration_ms),
                            TrackSortBy::DateAdded | TrackSortBy::Default => {
                                match (&track_a.added_at, &track_b.added_at) {
                                    (Some(a), Some(b)) => b.cmp(a),
                                    (Some(_), None) => std::cmp::Ordering::Less,
                                    (None, Some(_)) => std::cmp::Ordering::Greater,
                                    (None, None) => std::cmp::Ordering::Equal,
                                }
                            }
                        }
                    });
                }

                if !self.sorted_track_indices.is_empty() {
                    if let Some(real_idx) = selected_real_idx {
                        if let Some(new_pos) = self
                            .sorted_track_indices
                            .iter()
                            .position(|&x| x == real_idx)
                        {
                            self.track_list.select(Some(new_pos));
                        } else {
                            self.track_list.select(Some(0));
                        }
                    } else {
                        self.track_list.select(Some(0));
                    }
                } else {
                    self.track_list.select(None);
                }
            }
            _ => {}
        }
    }
}
