use crate::App;
use crate::app::FetchResult;
use crate::ui::{ActiveContent, Focus, SearchPanel};
use std::sync::Arc;

impl App {
    pub async fn maybe_load_more(&mut self) {
        if self.state.focus == Focus::Search {
            let should_load = self
                .state
                .search_results
                .as_ref()
                .map(|sr| {
                    if sr.loading {
                        return None;
                    }
                    let (selected, len, total, stype) = match sr.panel {
                        SearchPanel::Tracks => (
                            sr.track_list.selected().unwrap_or(0),
                            sr.tracks.len(),
                            sr.tracks_total,
                            "track",
                        ),
                        SearchPanel::Artists => (
                            sr.artist_list.selected().unwrap_or(0),
                            sr.artists.len(),
                            sr.artists_total,
                            "artist",
                        ),
                        SearchPanel::Albums => (
                            sr.album_list.selected().unwrap_or(0),
                            sr.albums.len(),
                            sr.albums_total,
                            "album",
                        ),
                        SearchPanel::Playlists => (
                            sr.playlist_list.selected().unwrap_or(0),
                            sr.playlists.len(),
                            sr.playlists_total,
                            "playlist",
                        ),
                    };
                    if len == 0 || selected < len.saturating_sub(3) || len >= total as usize {
                        return None;
                    }
                    Some((sr.query.clone(), len as u32, stype))
                })
                .flatten();

            if let Some((query, offset, stype)) = should_load {
                self.state.search_results.as_mut().unwrap().loading = true;
                match self.spotify.search_more(&query, stype, offset).await {
                    Ok(more) => {
                        let sr = self.state.search_results.as_mut().unwrap();
                        match stype {
                            "track" => {
                                sr.tracks_total = more.tracks_total;
                                sr.tracks.extend(more.tracks);
                            }
                            "artist" => {
                                sr.artists_total = more.artists_total;
                                sr.artists.extend(more.artists);
                            }
                            "album" => {
                                sr.albums_total = more.albums_total;
                                sr.albums.extend(more.albums);
                            }
                            "playlist" => {
                                sr.playlists_total = more.playlists_total;
                                sr.playlists.extend(more.playlists);
                            }
                            _ => {}
                        }
                        sr.loading = false;
                    }
                    Err(e) => {
                        if let Some(sr) = self.state.search_results.as_mut() {
                            sr.loading = false;
                        }
                        self.state.status_msg = Some(format!("Load more error: {e}"));
                    }
                }
            }
            return;
        }

        if self.state.active_content == ActiveContent::Albums {
            let selected = self.state.album_list.selected().unwrap_or(0);
            let len = self.state.albums.len();
            if len > 0
                && selected >= len.saturating_sub(3)
                && len < self.state.albums_total as usize
            {
                let offset = self.state.albums_offset;
                match self.spotify.fetch_saved_albums(offset).await {
                    Ok((mut new_albums, total)) => {
                        self.state.albums_total = total;
                        self.state.albums_offset += new_albums.len() as u32;
                        self.state.albums.append(&mut new_albums);
                    }
                    Err(e) => self.state.status_msg = Some(format!("Load more error: {e}")),
                }
            }
            return;
        }

        if self.state.active_content == ActiveContent::Shows {
            let selected = self.state.show_list.selected().unwrap_or(0);
            let len = self.state.shows.len();
            if len > 0 && selected >= len.saturating_sub(3) && len < self.state.shows_total as usize
            {
                let offset = self.state.shows_offset;
                match self.spotify.fetch_saved_shows(offset).await {
                    Ok((mut new_shows, total)) => {
                        self.state.shows_total = total;
                        self.state.shows_offset += new_shows.len() as u32;
                        self.state.shows.append(&mut new_shows);
                    }
                    Err(e) => self.state.status_msg = Some(format!("Load more error: {e}")),
                }
            }
            return;
        }

        if self.state.tracks_loading {
            return;
        }
        let selected = self.state.track_list.selected().unwrap_or(0);

        if self.state.tracks_loading {
            return;
        }

        let display_len = self.state.sorted_track_indices.len();

        if display_len == 0 || selected < display_len.saturating_sub(3) {
            return;
        }

        let track_len = self.state.tracks.len();
        if (self.state.tracks_offset as usize) >= track_len
            && track_len < self.state.tracks_total as usize
        {
            if self.pending_pagination.is_some() {
                return;
            }

            self.state.tracks_loading = true;
            self.state.status_msg = Some("Loading more tracks…".to_string());
            // Use tracks_api_offset (raw API item count) for pagination, not
            // tracks_offset (filtered track count) — they diverge when episodes are present
            let offset = self.state.tracks_api_offset;
            let id = self.state.active_playlist_id.clone();

            if id.as_deref() == Some("liked_songs") && self.state.tracks_cursor.is_none() {
                self.state.tracks_loading = false;
                self.state.status_msg = None;
                return;
            }

            let spotify = Arc::clone(&self.spotify);
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.pending_pagination = Some(rx);

            match id.as_deref() {
                Some("liked_songs") => {
                    let after = self.state.tracks_cursor.clone();
                    tokio::spawn(async move {
                        let result = spotify
                            .fetch_liked_tracks_page(after.as_deref(), offset)
                            .await
                            .map(|(t, total, next)| (t, total, next, None))
                            .map_err(|e| e.to_string());
                        let _ = tx.send(FetchResult::MoreTracks(result));
                    });
                }
                Some(id) if id.starts_with("album:") => {
                    let album_id = id["album:".len()..].to_string();
                    tokio::spawn(async move {
                        let result = spotify
                            .fetch_album_tracks(&album_id, offset)
                            .await
                            .map(|(t, total)| (t, total, None, None))
                            .map_err(|e| e.to_string());
                        let _ = tx.send(FetchResult::MoreTracks(result));
                    });
                }
                Some(id) if id.starts_with("artist:") => {
                    let name = self.state.active_artist_name.clone().unwrap_or_default();
                    tokio::spawn(async move {
                        let result = spotify
                            .fetch_artist_tracks(&name, offset)
                            .await
                            .map(|(t, total)| (t, total, None, None))
                            .map_err(|e| e.to_string());
                        let _ = tx.send(FetchResult::MoreTracks(result));
                    });
                }
                Some(id) => {
                    let playlist_id = id.to_string();
                    tokio::spawn(async move {
                        let result = spotify
                            .fetch_playlist_tracks(&playlist_id, offset)
                            .await
                            .map(|(t, total, page_items)| (t, total, None, Some(page_items)))
                            .map_err(|e| e.to_string());
                        let _ = tx.send(FetchResult::MoreTracks(result));
                    });
                }
                None => return,
            }
        }
    }

    #[cfg(feature = "album-art")]
    pub async fn maybe_fetch_album_art(&mut self) {
        if !self.state.show_album_art && self.discord.is_none() {
            return;
        }

        if self.current_track_uri.is_empty()
            || self.current_track_uri == self.last_art_uri
            || self.album_art_pending.is_some()
        {
            return;
        }

        if self.current_track_uri.starts_with("file://") {
            self.fetch_local_album_art();
            return;
        }

        if !self.spotify.authenticated {
            return;
        }

        let uri = self.current_track_uri.clone();
        let Some(token) = self.spotify.get_access_token().await else {
            return;
        };
        let http = self.spotify.http_client();
        self.last_art_uri = uri.clone();

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.album_art_pending = Some(rx);

        tokio::spawn(async move {
            let Some(track_id) = uri.strip_prefix("spotify:track:").map(|s| s.to_string()) else {
                return;
            };
            let Ok(resp) = http
                .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
                .bearer_auth(&token)
                .send()
                .await
            else {
                return;
            };
            let Ok(json) = resp.json::<serde_json::Value>().await else {
                return;
            };
            let Some(url) = json["album"]["images"]
                .as_array()
                .and_then(|imgs| imgs.first())
                .and_then(|img| img["url"].as_str())
                .map(|s| s.to_string())
            else {
                return;
            };
            if let Ok(resp) = http.get(&url).send().await {
                if let Ok(bytes) = resp.bytes().await {
                    let _ = tx.send(bytes.to_vec());
                }
            }
        });
    }

    #[cfg(feature = "album-art")]
    pub fn fetch_local_album_art(&mut self) {
        if self.current_track_uri == self.last_art_uri || self.album_art_pending.is_some() {
            return;
        }
        self.last_art_uri = self.current_track_uri.clone();

        if let Some(cover_str) = &self.state.playback.cover_path {
            let path = std::path::PathBuf::from(cover_str);
            if path.exists() {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.album_art_pending = Some(rx);

                tokio::spawn(async move {
                    if let Ok(bytes) = tokio::fs::read(&path).await {
                        let _ = tx.send(bytes);
                    }
                });
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/ui.rs"]
mod tests;
