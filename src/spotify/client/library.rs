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

    pub async fn fetch_followed_artists(&self) -> Result<Vec<ArtistSummary>> {
        if !self.authenticated {
            return Ok(Vec::new());
        }
        if let Some(cached) = self.library_cache.get_artists() {
            info!("Library cache hit: followed artists");
            return Ok(cached);
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        super::spotify_rate_limit().await;
        let response = self
            .http
            .get("https://api.spotify.com/v1/me/following")
            .bearer_auth(&token)
            .query(&[("type", "artist"), ("limit", "50")])
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
        let mut artists = Vec::with_capacity(50);

        if let Some(artists_obj) = json["artists"].as_object()
            && let Some(items) = artists_obj.get("items").and_then(|v| v.as_array())
        {
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
                artists.push(ArtistSummary {
                    id,
                    name,
                    uri,
                    genres,
                });
            }
        }

        self.library_cache.save_artists(&artists);
        Ok(artists)
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
                let description = item["description"].as_str().unwrap_or("").to_string();
                let artist = {
                    let chars: Vec<char> = description.chars().collect();
                    if chars.len() > 60 {
                        format!("{}…", chars[..60].iter().collect::<String>())
                    } else {
                        description
                    }
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
}
