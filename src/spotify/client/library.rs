use anyhow::Result;
use tracing::{info, warn};

use super::super::types::{AlbumSummary, ArtistSummary, ShowSummary, TrackSummary};
use super::SpotifyClient;

impl SpotifyClient {
    pub async fn fetch_album_tracks(
        &self,
        album_id: &str,
        offset: u32,
    ) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }
        let key = format!("album:{album_id}:{offset}");
        if let Some(cached) = self.library_cache.get_tracks(&key)
            && !cached.0.is_empty()
        {
            info!("Library cache hit: album {album_id} offset={offset}");
            return Ok(cached);
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        super::spotify_rate_limit().await;
        let response = self
            .http
            .get(format!(
                "https://api.spotify.com/v1/albums/{album_id}/tracks"
            ))
            .bearer_auth(&token)
            .query(&[
                ("limit", "50"),
                ("offset", &offset_str),
                ("market", "from_token"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            if status.as_u16() == 403 {
                warn!("Got 403 Forbidden. Body: {body}");
                return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
            }
            return Err(anyhow::anyhow!(
                "Spotify API error: status {} body: {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut tracks = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let cover_path = None;
                tracks.push(TrackSummary {
                    name,
                    artist,
                    album: String::new(),
                    duration_ms,
                    uri,
                    cover_path,
                    added_at: None,
                });
            }
        }

        self.library_cache.save_tracks(&key, &tracks, total);
        Ok((tracks, total))
    }

    pub async fn stream_album_tracks(
        &self,
        album_id: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let (first_tracks, total) = self.fetch_album_tracks(album_id, 0).await?;
        if tx
            .send(crate::app::fetcher::StreamEvent::AlbumTracksInitial {
                album_id: album_id.to_string(),
                tracks: first_tracks,
                total,
            })
            .is_err()
        {
            return Ok(());
        }

        let mut offset = 50;
        while offset < total {
            super::spotify_rate_limit().await;
            match self.fetch_album_tracks(album_id, offset).await {
                Ok((batch, new_total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    offset += batch.len() as u32;
                    if tx
                        .send(crate::app::fetcher::StreamEvent::AlbumTracksBatch {
                            album_id: album_id.to_string(),
                            tracks: batch,
                            total: new_total,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("stream_album_tracks: failed at offset {offset}: {e}");
                    break;
                }
            }
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn fetch_saved_albums(&self, offset: u32) -> Result<(Vec<AlbumSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }
        if offset == 0
            && let Some(cached) = self.library_cache.get_albums()
        {
            info!("Library cache hit: saved albums");
            return Ok(cached);
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        super::spotify_rate_limit().await;
        let response = self
            .http
            .get("https://api.spotify.com/v1/me/albums")
            .bearer_auth(&token)
            .query(&[("limit", "20"), ("offset", &offset_str)])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            if status.as_u16() == 403 {
                warn!("Got 403 Forbidden. Body: {body}");
                return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
            }
            return Err(anyhow::anyhow!(
                "Spotify API error: status {} body: {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut albums = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for saved in items {
                let album = &saved["album"];
                let id = album["id"].as_str().unwrap_or("").to_string();
                let name = album["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = album["artists"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let uri = album["uri"].as_str().unwrap_or("").to_string();
                let total_tracks = album["total_tracks"].as_u64().unwrap_or(0) as u32;
                albums.push(AlbumSummary {
                    id,
                    name,
                    artist,
                    uri,
                    total_tracks,
                });
            }
        }

        if offset == 0 {
            self.library_cache.save_albums(&albums, total);
        }
        Ok((albums, total))
    }

    pub async fn stream_saved_albums(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let (first_albums, total) = self.fetch_saved_albums(0).await?;
        if tx
            .send(crate::app::fetcher::StreamEvent::AlbumsInitial {
                albums: first_albums,
                total,
            })
            .is_err()
        {
            return Ok(());
        }

        let mut offset = 20;
        while offset < total {
            super::spotify_rate_limit().await;
            match self.fetch_saved_albums(offset).await {
                Ok((batch, new_total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    offset += batch.len() as u32;
                    if tx
                        .send(crate::app::fetcher::StreamEvent::AlbumsBatch {
                            albums: batch,
                            total: new_total,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("stream_saved_albums: failed at offset {offset}: {e}");
                    break;
                }
            }
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn stream_followed_artists(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let artists = self.fetch_followed_artists().await?;
        let _ = tx.send(crate::app::fetcher::StreamEvent::ArtistsInitial { artists });
        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn fetch_followed_artists(&self) -> Result<Vec<ArtistSummary>> {
        if !self.authenticated {
            return Ok(Vec::new());
        }
        if let Some(cached) = self.library_cache.get_artists() {
            info!("Library cache hit: followed artists");
            return Ok(cached);
        }

        let mut all_artists = Vec::with_capacity(50);
        let mut after: Option<String> = None;

        loop {
            let token = self
                .get_access_token()
                .await
                .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

            super::spotify_rate_limit().await;
            let mut query = vec![("type", "artist"), ("limit", "50")];
            if let Some(ref a) = after {
                query.push(("after", a));
            }
            let response = super::send_with_retry(
                &token,
                self.http
                    .get("https://api.spotify.com/v1/me/following")
                    .query(&query),
            )
            .await?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();

                if status.as_u16() == 401 {
                    warn!("Got 401 Unauthorized - token may have expired");
                    return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
                }

                if status.as_u16() == 429 {
                    warn!("Rate limited on Spotify API");
                    return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
                }

                if status.as_u16() == 403 {
                    warn!("Got 403 Forbidden. Body: {body}");
                    return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
                }
                return Err(anyhow::anyhow!(
                    "Spotify API error: status {} body: {}",
                    status,
                    body
                ));
            }

            let json: serde_json::Value = response.json().await?;
            let mut page_count = 0usize;

            if let Some(artists_obj) = json["artists"].as_object()
                && let Some(items) = artists_obj.get("items").and_then(|v| v.as_array())
            {
                page_count = items.len();
                for artist in items {
                    let id = artist["id"].as_str().unwrap_or("").to_string();
                    let name = artist["name"].as_str().unwrap_or("Unknown").to_string();
                    let uri = artist["uri"].as_str().unwrap_or("").to_string();
                    let genres = artist["genres"]
                        .as_array()
                        .map(|g| {
                            g.iter()
                                .filter_map(|x| x.as_str())
                                .take(2)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    all_artists.push(ArtistSummary {
                        id,
                        name,
                        uri,
                        genres,
                    });
                }
            }

            let next_cursor = json["artists"]["cursors"]["after"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let has_next = json["artists"]["next"]
                .as_str()
                .is_some_and(|s| !s.is_empty());

            if page_count == 0 || !has_next || next_cursor.is_none() {
                break;
            }
            after = next_cursor;
        }

        self.library_cache.save_artists(&all_artists);
        Ok(all_artists)
    }

    pub async fn fetch_artist_name(&self, artist_id: &str) -> Option<String> {
        if !self.authenticated {
            return None;
        }
        let token = self.get_access_token().await?;
        super::spotify_rate_limit().await;
        let json: serde_json::Value = self
            .http
            .get(format!("https://api.spotify.com/v1/artists/{artist_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        json["name"].as_str().map(|s| s.to_string())
    }

    pub async fn fetch_show_name(&self, show_id: &str) -> Option<String> {
        if !self.authenticated {
            return None;
        }
        let token = self.get_access_token().await?;
        super::spotify_rate_limit().await;
        let json: serde_json::Value = self
            .http
            .get(format!("https://api.spotify.com/v1/shows/{show_id}"))
            .bearer_auth(&token)
            .query(&[("market", "from_token")])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        json["name"].as_str().map(|s| s.to_string())
    }

    pub async fn fetch_artist_tracks(
        &self,
        artist_name: &str,
        offset: u32,
    ) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }
        let key = format!("artist:{artist_name}:{offset}");
        if let Some(cached) = self.library_cache.get_tracks(&key)
            && !cached.0.is_empty()
        {
            info!("Library cache hit: artist {artist_name} offset={offset}");
            return Ok(cached);
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let query = format!("artist:\"{}\"", artist_name);
        let offset_str = offset.to_string();
        super::spotify_rate_limit().await;
        let query_params: Vec<(&str, &str)> = vec![
            ("q", query.as_str()),
            ("type", "track"),
            ("limit", "10"),
            ("offset", offset_str.as_str()),
            ("market", "from_token"),
        ];
        let response = self
            .http
            .get("https://api.spotify.com/v1/search")
            .bearer_auth(&token)
            .query(&query_params)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            if status.as_u16() == 403 {
                warn!("Got 403 Forbidden. Body: {body}");
                return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
            }
            return Err(anyhow::anyhow!(
                "Spotify API error: status {} body: {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["tracks"]["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["tracks"]["items"].as_array().map_or(0, |a| a.len());
        let mut tracks = Vec::with_capacity(items_len);

        if let Some(items) = json["tracks"]["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let album = item["album"]["name"].as_str().unwrap_or("").to_string();
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let cover_path = None;
                tracks.push(TrackSummary {
                    name,
                    artist,
                    album,
                    duration_ms,
                    uri,
                    cover_path,
                    added_at: None,
                });
            }
        }

        self.library_cache.save_tracks(&key, &tracks, total);
        Ok((tracks, total))
    }

    pub async fn stream_saved_shows(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let (first_shows, total) = self.fetch_saved_shows(0).await?;
        if tx
            .send(crate::app::fetcher::StreamEvent::ShowsInitial {
                shows: first_shows,
                total,
            })
            .is_err()
        {
            return Ok(());
        }

        let mut offset = 20;
        while offset < total {
            super::spotify_rate_limit().await;
            match self.fetch_saved_shows(offset).await {
                Ok((batch, new_total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    offset += batch.len() as u32;
                    if tx
                        .send(crate::app::fetcher::StreamEvent::ShowsBatch {
                            shows: batch,
                            total: new_total,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("stream_saved_shows: failed at offset {offset}: {e}");
                    break;
                }
            }
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn stream_show_episodes(
        &self,
        show_id: &str,
        show_name: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let (first_tracks, total) = self.fetch_show_episodes(show_id, show_name, 0).await?;
        if tx
            .send(crate::app::fetcher::StreamEvent::ShowTracksInitial {
                show_id: show_id.to_string(),
                tracks: first_tracks,
                total,
            })
            .is_err()
        {
            return Ok(());
        }

        let mut offset = 50;
        while offset < total {
            super::spotify_rate_limit().await;
            match self.fetch_show_episodes(show_id, show_name, offset).await {
                Ok((batch, new_total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    offset += batch.len() as u32;
                    if tx
                        .send(crate::app::fetcher::StreamEvent::ShowTracksBatch {
                            show_id: show_id.to_string(),
                            tracks: batch,
                            total: new_total,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("stream_show_episodes: failed at offset {offset}: {e}");
                    break;
                }
            }
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn fetch_saved_shows(&self, offset: u32) -> Result<(Vec<ShowSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        super::spotify_rate_limit().await;
        let response = self
            .http
            .get("https://api.spotify.com/v1/me/shows")
            .bearer_auth(&token)
            .query(&[
                ("limit", "20"),
                ("offset", &offset_str),
                ("market", "from_token"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            if status.as_u16() == 403 {
                warn!("Got 403 Forbidden. Body: {body}");
                return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
            }
            return Err(anyhow::anyhow!(
                "Spotify API error: status {} body: {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut shows = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let show = &item["show"];
                let id = show["id"].as_str().unwrap_or("").to_string();
                let name = show["name"].as_str().unwrap_or("Unknown").to_string();
                let publisher = show["publisher"].as_str().unwrap_or("").to_string();
                let total_episodes = show["total_episodes"].as_u64().unwrap_or(0) as u32;
                shows.push(ShowSummary {
                    id,
                    name,
                    publisher,
                    total_episodes,
                });
            }
        }

        Ok((shows, total))
    }

    pub async fn fetch_show_episodes(
        &self,
        show_id: &str,
        show_name: &str,
        offset: u32,
    ) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let query: Vec<(&str, &str)> = vec![
            ("limit", "50"),
            ("offset", &offset_str),
            ("market", "from_token"),
        ];

        super::spotify_rate_limit().await;
        let response = self
            .http
            .get(format!(
                "https://api.spotify.com/v1/shows/{show_id}/episodes"
            ))
            .bearer_auth(&token)
            .query(&query)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            tracing::error!("fetch_show_episodes API error: status {}", status);
            if status.as_u16() == 403 {
                warn!("Got 403 Forbidden. Body: {body}");
                return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
            }
            return Err(anyhow::anyhow!(
                "Spotify API error: status {} body: {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut tracks = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = if show_name.is_empty() {
                    let description = item["description"].as_str().unwrap_or("");
                    let chars: Vec<char> = description.chars().collect();
                    if chars.len() > 60 {
                        format!("{}…", chars[..60].iter().collect::<String>())
                    } else {
                        description.to_string()
                    }
                } else {
                    show_name.to_string()
                };
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let cover_path = None;
                tracks.push(TrackSummary {
                    name,
                    artist,
                    album: String::new(),
                    duration_ms,
                    uri,
                    cover_path,
                    added_at: None,
                });
            }
        }

        Ok((tracks, total))
    }

    pub async fn stream_saved_episodes(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let (first_tracks, total) = self.fetch_saved_episodes(0).await?;
        if tx
            .send(crate::app::fetcher::StreamEvent::EpisodesInitial {
                tracks: first_tracks,
                total,
            })
            .is_err()
        {
            return Ok(());
        }

        let mut offset = 20;
        while offset < total {
            super::spotify_rate_limit().await;
            match self.fetch_saved_episodes(offset).await {
                Ok((batch, new_total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    offset += batch.len() as u32;
                    if tx
                        .send(crate::app::fetcher::StreamEvent::EpisodesBatch {
                            tracks: batch,
                            total: new_total,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("stream_saved_episodes: failed at offset {offset}: {e}");
                    break;
                }
            }
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn fetch_saved_episodes(&self, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        super::spotify_rate_limit().await;
        let response = self
            .http
            .get("https://api.spotify.com/v1/me/episodes")
            .bearer_auth(&token)
            .query(&[
                ("limit", "20"),
                ("offset", &offset_str),
                ("market", "from_token"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            if status.as_u16() == 403 {
                warn!("Got 403 Forbidden. Body: {body}");
                return Err(anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}"));
            }
            return Err(anyhow::anyhow!(
                "Spotify API error: status {} body: {}",
                status,
                body
            ));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut tracks = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let ep = &item["episode"];
                let show_name = ep["show"]["name"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary {
                    name: ep["name"].as_str().unwrap_or("Unknown").to_string(),
                    artist: show_name.clone(),
                    album: show_name,
                    duration_ms: ep["duration_ms"].as_u64().unwrap_or(0),
                    uri: ep["uri"].as_str().unwrap_or("").to_string(),
                    cover_path: None,
                    added_at: item["added_at"].as_str().map(|s| s.to_string()),
                });
            }
        }

        Ok((tracks, total))
    }
}
