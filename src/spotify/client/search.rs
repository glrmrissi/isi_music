use anyhow::Result;
use tracing::{info, warn};

use super::super::types::{
    AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, TrackSummary,
};
use super::SpotifyClient;

impl SpotifyClient {
    pub async fn search_all(&self, query: &str) -> Result<FullSearchResults> {
        if !self.authenticated {
            return Ok(FullSearchResults {
                tracks: vec![],
                artists: vec![],
                albums: vec![],
                playlists: vec![],
                tracks_total: 0,
                artists_total: 0,
                albums_total: 0,
                playlists_total: 0,
            });
        }
        self.search_internal(query, "track,artist,album,playlist", 0, 10)
            .await
    }

    pub async fn search_more(
        &self,
        query: &str,
        search_type: &str,
        offset: u32,
    ) -> Result<FullSearchResults> {
        if !self.authenticated {
            return Ok(FullSearchResults {
                tracks: vec![],
                artists: vec![],
                albums: vec![],
                playlists: vec![],
                tracks_total: 0,
                artists_total: 0,
                albums_total: 0,
                playlists_total: 0,
            });
        }
        self.search_internal(query, search_type, offset, 10).await
    }

    async fn search_internal(
        &self,
        query: &str,
        search_type: &str,
        offset: u32,
        limit: u32,
    ) -> Result<FullSearchResults> {
        let cache_key = format!("{}:{}:{}:{}", query, search_type, offset, limit);

        if let Some(cached) = self.search_cache.get(&cache_key).await {
            info!("Search cache hit: {}", query);
            return Ok(cached);
        }

        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let limit_str = limit.to_string();
        super::spotify_rate_limit().await;
        let query_params: Vec<(&str, &str)> = vec![
            ("q", query),
            ("type", search_type),
            ("limit", limit_str.as_str()),
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

        let mut tracks = Vec::new();
        let mut tracks_total = 0u32;
        if let Some(obj) = json["tracks"].as_object() {
            tracks_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                tracks.reserve(items.len());
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
        }

        let mut artists = Vec::new();
        let mut artists_total = 0u32;
        if let Some(obj) = json["artists"].as_object() {
            artists_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                artists.reserve(items.len());
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    let genres = item["genres"]
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
        }

        let mut albums = Vec::new();
        let mut albums_total = 0u32;
        if let Some(obj) = json["albums"].as_object() {
            albums_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                albums.reserve(items.len());
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
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
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    let total_tracks = item["total_tracks"].as_u64().unwrap_or(0) as u32;
                    albums.push(AlbumSummary {
                        id,
                        name,
                        artist,
                        uri,
                        total_tracks,
                    });
                }
            }
        }

        let mut playlists = Vec::new();
        let mut playlists_total = 0u32;
        if let Some(obj) = json["playlists"].as_object() {
            playlists_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                playlists.reserve(items.len());
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    let total_tracks = item["items"]["total"]
                        .as_u64()
                        .or_else(|| item["tracks"]["total"].as_u64())
                        .unwrap_or(0) as u32;
                    let art_url = item["images"]
                        .as_array()
                        .and_then(|imgs| imgs.first())
                        .and_then(|img| img["url"].as_str())
                        .map(|s| s.to_string());
                    playlists.push(PlaylistSummary {
                        id,
                        name,
                        uri,
                        total_tracks,
                        art_url,
                    });
                }
            }
        }

        let results = FullSearchResults {
            tracks,
            artists,
            albums,
            playlists,
            tracks_total,
            artists_total,
            albums_total,
            playlists_total,
        };
        self.search_cache.insert(cache_key, results.clone()).await;
        Ok(results)
    }
}
