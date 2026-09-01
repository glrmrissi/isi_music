use anyhow::Result;
use crossterm::event::KeyCode;

use crate::App;
use crate::spotify::FullSearchResults;
use crate::ui::{Focus, SearchResults};

impl App {
    pub async fn handle_quick_search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.state.cancel_quick_search(),
            KeyCode::Enter => self.state.apply_quick_filter(),
            KeyCode::Backspace => self.state.quick_search_pop(),
            KeyCode::Char(c) if c.is_alphanumeric() || c == ' ' || c == '-' => {
                self.state.quick_search_push(c);
            }
            _ => {}
        }
    }

    pub async fn handle_search_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Esc => self.state.cancel_search(),
            KeyCode::Enter => {
                let query = self.state.search_query.trim().to_string();
                if query.is_empty() {
                    self.state.cancel_search();
                } else if !self.state.spotify_enabled {
                    self.search_local_files(&query).await;
                } else if !self.spotify.authenticated {
                    self.state.status_msg = Some("Search requires Spotify".to_string());
                    self.state.search_active = false;
                } else {
                    self.state.status_msg = Some(format!("Searching \"{query}\"..."));
                    match self.spotify.search_all(&query).await {
                        Ok(results) => {
                            let total = results.tracks.len()
                                + results.artists.len()
                                + results.albums.len()
                                + results.playlists.len();
                            self.state.search_results =
                                Some(SearchResults::new(query.clone(), results));
                            self.state.tracks.clear();
                            self.state.rebuild_sort_indices();
                            self.state.active_playlist_uri = None;
                            self.state.search_active = false;
                            self.state.focus = Focus::Search;
                            self.state.status_msg = if total == 0 {
                                Some(format!("No results for \"{query}\""))
                            } else {
                                Some(format!("{total} results for \"{query}\""))
                            };
                        }
                        Err(e) => {
                            self.state.status_msg = Some(format!("Search error: {e:#}"));
                            self.state.search_active = false;
                            tracing::error!("Search failed for \"{query}\": {e:#}");
                        }
                    }
                }
            }
            KeyCode::Up => self.state.nav_up(),
            KeyCode::Down => self.state.nav_down(),
            KeyCode::Backspace => self.state.search_pop(),
            KeyCode::Tab => self.state.switch_focus(),
            KeyCode::Char(c) => self.state.search_push(c),
            _ => {}
        }
        Ok(())
    }

    async fn ensure_local_tree_loaded(&mut self) {
        let cfg = crate::config::AppConfig::load().unwrap_or_default();
        let raw_dir = match cfg.local.music_dir {
            Some(d) => d,
            None => return,
        };
        let dir = if raw_dir.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(&raw_dir[2..])
            } else {
                std::path::PathBuf::from(&raw_dir)
            }
        } else {
            std::path::PathBuf::from(&raw_dir)
        };
        if !dir.exists() {
            return;
        }

        let nodes =
            tokio::task::spawn_blocking(move || crate::app::library::scan_local_files(&dir))
                .await
                .unwrap_or_default();

        let tree = crate::ui::LocalFileTree::new(nodes);
        self.state.local_tree = tree;
    }

    async fn search_local_files(&mut self, query: &str) {
        if self.state.local_tree.all_nodes.is_empty() {
            self.ensure_local_tree_loaded().await;
        }
        let query_lower = query.to_lowercase();
        let all_tracks = self.state.local_tree.all_tracks_flat();
        let matched: Vec<_> = all_tracks
            .into_iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&query_lower)
                    || t.artist.to_lowercase().contains(&query_lower)
                    || t.album.to_lowercase().contains(&query_lower)
            })
            .collect();
        let total = matched.len() as u32;
        let results = FullSearchResults {
            tracks: matched,
            artists: vec![],
            albums: vec![],
            playlists: vec![],
            tracks_total: total,
            artists_total: 0,
            albums_total: 0,
            playlists_total: 0,
        };
        self.state.search_results = Some(SearchResults::new(query.to_string(), results));
        if let Some(sr) = &mut self.state.search_results {
            sr.local_only = true;
        }
        self.state.tracks.clear();
        self.state.rebuild_sort_indices();
        self.state.active_playlist_uri = None;
        self.state.search_active = false;
        self.state.focus = Focus::Search;
        self.state.status_msg = if total == 0 {
            Some(format!("No local results for \"{query}\""))
        } else {
            Some(format!("{total} local results for \"{query}\""))
        };
    }

    pub async fn handle_delete_playlist_confirm_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let playlist_id = self
                    .state
                    .playlist_list
                    .selected()
                    .and_then(|i| self.state.playlists.get(i))
                    .map(|p| p.id.clone());

                let playlist_id = match playlist_id {
                    Some(id) => id,
                    None => {
                        self.state.delete_playlist_confirm = false;
                        self.state.delete_playlist_target = None;
                        return;
                    }
                };

                if self
                    .state
                    .playlists
                    .iter()
                    .any(|p| p.id == playlist_id && p.uri.starts_with("local:folder:"))
                {
                    self.state.delete_playlist_confirm = false;
                    self.state.delete_playlist_target = None;
                    self.state.status_msg = Some("Local folders cannot be deleted".to_string());
                    return;
                }

                self.state.status_msg = Some("Deleting playlist...".to_string());
                match self.spotify.unfollow_playlist(&playlist_id).await {
                    Ok(_) => {
                        self.state.playlists.retain(|p| p.id != playlist_id);
                        self.spotify
                            .library_cache
                            .delete_key_pattern(&format!("playlist:{}:%", playlist_id));
                        if self.state.active_playlist_id.as_deref() == Some(&playlist_id) {
                            self.state.active_playlist_id = None;
                            self.state.active_playlist_uri = None;
                            self.state.tracks.clear();
                            self.state.sorted_track_indices.clear();
                            self.state.track_list.select(None);
                            if let Some(entry) = self.state.pop_nav() {
                                self.state.active_content = entry.active_content;
                                self.state.focus = entry.focus;
                            }
                        }
                        let new_len = self.state.playlists.len();
                        let sel = self.state.playlist_list.selected().unwrap_or(0);
                        if sel >= new_len && new_len > 0 {
                            self.state.playlist_list.select(Some(new_len - 1));
                        }
                        self.state.status_msg = Some("Playlist deleted".to_string());
                    }
                    Err(e) => {
                        self.state.status_msg = Some(format!("Delete failed: {e}"));
                    }
                }
                self.state.delete_playlist_confirm = false;
                self.state.delete_playlist_target = None;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.state.delete_playlist_confirm = false;
                self.state.delete_playlist_target = None;
                self.state.status_msg = Some("Cancelled".to_string());
            }
            _ => {}
        }
    }
}
