use crate::App;
use crate::app::FetchResult;
use crate::app::fetcher::StreamEvent;
use crate::ui::{ActiveContent, Focus, SearchPanel};
use std::sync::Arc;

impl App {
    pub async fn maybe_load_more(&mut self) {
        if self.state.focus == Focus::Search {
            let should_load = self.state.search_results.as_ref().and_then(|sr| {
                if sr.loading {
                    return None;
                }
                let (selected, len, total, api_offset, stype) = match sr.panel {
                    SearchPanel::Tracks => (
                        sr.track_list.selected().unwrap_or(0),
                        sr.tracks.len(),
                        sr.tracks_total,
                        sr.tracks_api_offset,
                        "track",
                    ),
                    SearchPanel::Artists => (
                        sr.artist_list.selected().unwrap_or(0),
                        sr.artists.len(),
                        sr.artists_total,
                        sr.artists_api_offset,
                        "artist",
                    ),
                    SearchPanel::Albums => (
                        sr.album_list.selected().unwrap_or(0),
                        sr.albums.len(),
                        sr.albums_total,
                        sr.albums_api_offset,
                        "album",
                    ),
                    SearchPanel::Playlists => (
                        sr.playlist_list.selected().unwrap_or(0),
                        sr.playlists.len(),
                        sr.playlists_total,
                        sr.playlists_api_offset,
                        "playlist",
                    ),
                    SearchPanel::Podcasts => (
                        sr.podcast_list.selected().unwrap_or(0),
                        sr.shows.len() + sr.episodes.len(),
                        sr.shows_total + sr.episodes_total,
                        sr.podcasts_api_offset,
                        "show,episode",
                    ),
                };
                if len == 0
                    || selected < len.saturating_sub(3)
                    || api_offset >= total
                    || len >= total as usize
                {
                    return None;
                }
                Some((sr.query.clone(), api_offset, stype))
            });

            if let Some((query, offset, stype)) = should_load {
                if self.fetcher.stream_rx.is_some() {
                    return;
                }
                if let Some(sr) = self.state.search_results.as_mut() {
                    sr.loading = true;
                }
                let spotify = Arc::clone(&self.spotify);
                let stype_owned = stype.to_string();
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                self.fetcher.stream_rx = Some(rx);
                tokio::spawn(async move {
                    match spotify.search_more(&query, &stype_owned, offset).await {
                        Ok(results) => {
                            let _ = tx.send(StreamEvent::SearchMore {
                                stype: stype_owned,
                                results: Box::new(results),
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(StreamEvent::Error(format!("{e:#}")));
                        }
                    }
                    let _ = tx.send(StreamEvent::Done);
                });
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

        let display_len = self.state.sorted_track_indices.len();

        if display_len == 0 || selected < display_len.saturating_sub(3) {
            return;
        }

        let track_len = self.state.tracks.len();
        if track_len < self.state.tracks_total as usize {
            if self.fetcher.pending_pagination.is_some() || self.fetcher.stream_rx.is_some() {
                return;
            }

            self.state.tracks_loading = true;
            self.state.status_msg = Some("Loading more tracks…".to_string());
            // Use tracks_api_offset (raw API item count) for pagination, not
            // tracks_offset (list length) — they diverge when items are deduped
            let offset = self.state.tracks_api_offset;
            let id = self.state.active_playlist_id.clone();

            if id.as_deref() == Some("liked_songs") && self.state.tracks_cursor.is_none() {
                self.state.tracks_loading = false;
                self.state.status_msg = None;
                return;
            }

            let spotify = Arc::clone(&self.spotify);
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.fetcher.pending_pagination = Some(rx);

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
                Some(id) if id.starts_with("show:") => {
                    let show_id = id["show:".len()..].to_string();
                    let show_name = self
                        .state
                        .shows
                        .iter()
                        .find(|s| s.id == show_id)
                        .map(|s| s.name.clone())
                        .or_else(|| self.state.tracks.first().map(|t| t.artist.clone()))
                        .unwrap_or_default();
                    tokio::spawn(async move {
                        let result = spotify
                            .fetch_show_episodes(&show_id, &show_name, offset)
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
                Some("saved_episodes") => {
                    tokio::spawn(async move {
                        let result = spotify
                            .fetch_saved_episodes(offset)
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
                None => {
                    self.state.tracks_loading = false;
                    self.state.status_msg = None;
                    self.fetcher.pending_pagination = None;
                }
            }
        }
    }

    pub(crate) fn current_list_at_end(&self) -> bool {
        let at_end = |selected: Option<usize>, len: usize| {
            len > 0 && selected.unwrap_or(0) >= len.saturating_sub(1)
        };

        match self.state.focus {
            Focus::Search => self
                .state
                .search_results
                .as_ref()
                .map(|sr| {
                    let selected = match sr.panel {
                        SearchPanel::Tracks => sr.track_list.selected(),
                        SearchPanel::Artists => sr.artist_list.selected(),
                        SearchPanel::Albums => sr.album_list.selected(),
                        SearchPanel::Playlists => sr.playlist_list.selected(),
                        SearchPanel::Podcasts => sr.podcast_list.selected(),
                    };
                    at_end(selected, sr.current_len())
                })
                .unwrap_or(false),
            Focus::Tracks => match self.state.active_content {
                ActiveContent::Albums => {
                    at_end(self.state.album_list.selected(), self.state.albums.len())
                }
                ActiveContent::Shows => {
                    at_end(self.state.show_list.selected(), self.state.shows.len())
                }
                ActiveContent::LocalFiles => false,
                ActiveContent::Tracks | ActiveContent::None => {
                    self.state.active_playlist_id.is_some()
                        && at_end(
                            self.state.track_list.selected(),
                            self.state.sorted_track_indices.len(),
                        )
                }
                ActiveContent::Artists => false,
            },
            Focus::Library | Focus::Playlists | Focus::Queue => false,
        }
    }

    pub(crate) fn current_list_loading(&self) -> bool {
        match self.state.focus {
            Focus::Search => self
                .state
                .search_results
                .as_ref()
                .map(|sr| sr.loading)
                .unwrap_or(false),
            Focus::Tracks => self.state.tracks_loading || self.fetcher.pending_pagination.is_some(),
            Focus::Library | Focus::Playlists | Focus::Queue => false,
        }
    }

    #[cfg(feature = "album-art")]
    pub async fn maybe_fetch_album_art(&mut self) {
        #[cfg(windows)]
        let smtc_needs_art = self.integrations.smtc.is_some();
        #[cfg(not(windows))]
        let smtc_needs_art = false;

        if !self.state.show_album_art && self.integrations.discord.is_none() && !smtc_needs_art {
            return;
        }

        if self.current_track_uri.is_empty()
            || self.current_track_uri == self.fetcher.last_art_uri
            || self.fetcher.album_art_pending.is_some()
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
        self.fetcher.last_art_uri = uri.clone();

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.fetcher.album_art_pending = Some(rx);

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
            if let Ok(resp) = http.get(&url).send().await
                && let Ok(bytes) = resp.bytes().await
            {
                let _ = tx.send((Some(url), bytes.to_vec()));
            }
        });
    }

    #[cfg(feature = "album-art")]
    pub fn fetch_local_album_art(&mut self) {
        if self.current_track_uri == self.fetcher.last_art_uri
            || self.fetcher.album_art_pending.is_some()
        {
            return;
        }
        self.fetcher.last_art_uri = self.current_track_uri.clone();

        if let Some(cover_str) = &self.state.playback.cover_path {
            let path = std::path::PathBuf::from(cover_str);
            if path.exists() {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.fetcher.album_art_pending = Some(rx);

                tokio::spawn(async move {
                    if let Ok(bytes) = tokio::fs::read(&path).await {
                        let _ = tx.send((None, bytes));
                    }
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/ui.rs"]
mod tests;
