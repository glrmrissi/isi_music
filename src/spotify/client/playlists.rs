use anyhow::Result;
use tracing::{info, warn};

use super::super::types::{PlaylistSummary, TrackSummary};
use super::SpotifyClient;

impl SpotifyClient {
    pub async fn fetch_playlists(&self) -> Result<Vec<PlaylistSummary>> {
        if !self.authenticated {
            warn!("fetch_playlists: not authenticated");
            return Ok(Vec::new());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        info!("fetch_playlists: starting to fetch playlists");
        let mut all = Vec::with_capacity(50);
        let mut offset = 0u32;
        loop {
            let offset_str = offset.to_string();
            super::spotify_rate_limit().await;
            info!("fetch_playlists: requesting offset={}", offset);
            let response = self
                .http
                .get("https://api.spotify.com/v1/me/playlists")
                .bearer_auth(&token)
                .query(&[("limit", "50"), ("offset", &offset_str)])
                .send()
                .await?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                warn!("fetch_playlists: API error status={}", status);
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
            let fetched = json["items"]
                .as_array()
                .map(|a| a.len() as u32)
                .unwrap_or(0);

            info!(
                "fetch_playlists: fetched {} playlists at offset={}",
                fetched, offset
            );

            if let Some(items) = json["items"].as_array() {
                for p in items {
                    let art_url = p["images"]
                        .as_array()
                        .and_then(|imgs| imgs.first())
                        .and_then(|img| img["url"].as_str())
                        .map(|s| s.to_string());
                    all.push(PlaylistSummary {
                        id: p["id"].as_str().unwrap_or("").to_string(),
                        uri: p["uri"].as_str().unwrap_or("").to_string(),
                        name: p["name"].as_str().unwrap_or("Unknown").to_string(),
                        total_tracks: p["items"]["total"]
                            .as_u64()
                            .or_else(|| p["tracks"]["total"].as_u64())
                            .unwrap_or(0) as u32,
                        art_url,
                    });
                }
            }

            if json["next"].is_null() || fetched == 0 {
                break;
            }
            offset += fetched;
        }
        info!("fetch_playlists: completed, total playlists={}", all.len());
        Ok(all)
    }

    /// Returns (tracks, total, page_items_count).
    /// `page_items_count` is the number of items the API returned (before episode filtering),
    /// which callers must use to increment the offset — NOT `tracks.len()`.
    pub async fn fetch_playlist_tracks(
        &self,
        playlist_id: &str,
        offset: u32,
    ) -> Result<(Vec<TrackSummary>, u32, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0, 0));
        }
        let key = format!("playlist:{playlist_id}:{offset}");
        if let Some(cached) = self.library_cache.get_tracks(&key)
            && !cached.0.is_empty()
        {
            info!("Library cache hit: playlist {playlist_id} offset={offset}");
            let page_items = cached.1.saturating_sub(offset).min(50);
            return Ok((cached.0, cached.1, page_items));
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let limit_str = "50";

        // Feb 2026: /tracks endpoint was deprecated and removed, use /items
        let url = format!("https://api.spotify.com/v1/playlists/{playlist_id}/items");
        info!("Fetching playlist items for {playlist_id} (offset={offset})");
        super::spotify_rate_limit().await;
        let response = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&[("limit", limit_str), ("offset", &offset_str)])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("Playlist fetch failed for {playlist_id} status={status}");

            if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                return Err(anyhow::anyhow!("SPOTIFY_UNAUTHORIZED"));
            }

            if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                return Err(anyhow::anyhow!("SPOTIFY_RATE_LIMITED"));
            }

            if status.as_u16() == 403 {
                // Fallback: try the main playlist endpoint which returns items inline
                info!("Got 403 on /items, trying main playlist endpoint for {playlist_id}");
                super::spotify_rate_limit().await;
                let main_url = format!("https://api.spotify.com/v1/playlists/{playlist_id}");
                let main_resp = self.http.get(&main_url).bearer_auth(&token).send().await?;
                let main_status = main_resp.status();
                info!("Main playlist endpoint returned status={main_status} for {playlist_id}");
                if main_status.is_success() {
                    let main_json: serde_json::Value = main_resp.json().await?;
                    // New API: "items" field (was "tracks"), items[].item (was items[].track)
                    let tracks_obj = if !main_json["items"].is_null() {
                        &main_json["items"]
                    } else {
                        &main_json["tracks"]
                    };
                    let total = tracks_obj["total"].as_u64().unwrap_or(0) as u32;
                    let items_count = tracks_obj["items"].as_array().map(|a| a.len()).unwrap_or(0);
                    info!("Main endpoint: total={total}, items={items_count} for {playlist_id}");
                    let mut tracks = Vec::new();
                    if let Some(items) = tracks_obj["items"].as_array() {
                        for item_wrapper in items {
                            // New API: "item" (was "track")
                            let track = if !item_wrapper["item"].is_null() {
                                &item_wrapper["item"]
                            } else {
                                &item_wrapper["track"]
                            };
                            if track.is_null() || track["type"].as_str() == Some("episode") {
                                continue;
                            }
                            let name = track["name"].as_str().unwrap_or("Unknown").to_string();
                            let artist = track["artists"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|x| x["name"].as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                })
                                .unwrap_or_default();
                            let album = track["album"]["name"].as_str().unwrap_or("").to_string();
                            let duration_ms = track["duration_ms"].as_u64().unwrap_or(0);
                            let uri = track["uri"].as_str().unwrap_or("").to_string();
                            let added_at = item_wrapper["added_at"].as_str().map(|s| s.to_string());
                            if !uri.is_empty() {
                                tracks.push(TrackSummary {
                                    name,
                                    artist,
                                    album,
                                    duration_ms,
                                    uri,
                                    cover_path: None,
                                    added_at,
                                });
                            }
                        }
                    }
                    info!(
                        "Main endpoint: parsed {} tracks for {playlist_id}",
                        tracks.len()
                    );
                    self.library_cache.save_tracks(&key, &tracks, total);
                    return Ok((tracks, total, items_count as u32));
                }
                let _ = main_resp.text().await.unwrap_or_default();
                warn!("Main playlist endpoint also failed: {main_status}");
                return Err(anyhow::anyhow!("SPOTIFY_PLAYLIST_NOT_ACCESSIBLE"));
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
        let page_items = json["items"].as_array().map(|a| a.len()).unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut tracks = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for item_wrapper in items {
                // New API: "item" (was "track")
                let track = if !item_wrapper["item"].is_null() {
                    &item_wrapper["item"]
                } else if !item_wrapper["track"].is_null() {
                    &item_wrapper["track"]
                } else {
                    continue;
                };

                if track.is_null() || track["type"].as_str() == Some("episode") {
                    continue;
                }

                let added_at = item_wrapper["added_at"].as_str().map(|s| s.to_string());

                let name = track["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = track["artists"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x["name"].as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let album = track["album"]["name"].as_str().unwrap_or("").to_string();
                let duration_ms = track["duration_ms"].as_u64().unwrap_or(0);
                let uri = track["uri"].as_str().unwrap_or("").to_string();
                let cover_path = None;

                if !uri.is_empty() {
                    tracks.push(TrackSummary {
                        name,
                        artist,
                        album,
                        duration_ms,
                        uri,
                        cover_path,
                        added_at,
                    });
                }
            }
        }

        info!(
            "Parsed {} tracks from playlist {playlist_id} (total={total}, page_items={page_items})",
            tracks.len()
        );

        self.library_cache.save_tracks(&key, &tracks, total);
        Ok((tracks, total, page_items))
    }

    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: &str,
        uris: &[String],
        position: Option<u32>,
    ) -> Result<String> {
        super::spotify_rate_limit().await;
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token"))?;
        let mut body = serde_json::json!({ "uris": uris });
        if let Some(pos) = position {
            body["position"] = serde_json::json!(pos);
        }
        let mut resp = self
            .http
            .post(format!(
                "https://api.spotify.com/v1/playlists/{playlist_id}/items"
            ))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;
        if resp.status().as_u16() == 404 {
            resp = self
                .http
                .post(format!(
                    "https://api.spotify.com/v1/playlists/{playlist_id}/items"
                ))
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await?;
        }
        let status = resp.status();
        let text = resp.text().await?;
        if status.is_success() {
            let snap: serde_json::Value = serde_json::from_str(&text)?;
            Ok(snap["snapshot_id"].as_str().unwrap_or("").to_string())
        } else {
            anyhow::bail!("Add to playlist failed ({}): {}", status.as_u16(), text);
        }
    }

    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: &str,
        uris: &[String],
    ) -> Result<String> {
        super::spotify_rate_limit().await;
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token"))?;
        let body = serde_json::json!({
            "items": uris.iter().map(|uri| serde_json::json!({"uri": uri})).collect::<Vec<_>>()
        });
        let mut resp = self
            .http
            .delete(format!(
                "https://api.spotify.com/v1/playlists/{playlist_id}/items"
            ))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;
        if resp.status().as_u16() == 404 {
            resp = self
                .http
                .delete(format!(
                    "https://api.spotify.com/v1/playlists/{playlist_id}/items"
                ))
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await?;
        }
        let status = resp.status();
        let text = resp.text().await?;
        if status.is_success() {
            let snap: serde_json::Value = serde_json::from_str(&text)?;
            Ok(snap["snapshot_id"].as_str().unwrap_or("").to_string())
        } else {
            anyhow::bail!(
                "Remove from playlist failed ({}): {}",
                status.as_u16(),
                text
            );
        }
    }

    pub async fn unfollow_playlist(&self, playlist_id: &str) -> Result<()> {
        super::spotify_rate_limit().await;
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token"))?;
        let resp = self
            .http
            .delete(format!(
                "https://api.spotify.com/v1/playlists/{playlist_id}/followers"
            ))
            .bearer_auth(&token)
            .send()
            .await?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Unfollow playlist failed ({}): {}", status.as_u16(), text);
        }
    }

    pub async fn create_playlist(
        &self,
        name: &str,
        public: bool,
        description: Option<&str>,
    ) -> Result<PlaylistSummary> {
        super::spotify_rate_limit().await;
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token"))?;
        let mut body = serde_json::json!({
            "name": name,
            "public": public,
        });
        if let Some(desc) = description {
            body["description"] = serde_json::json!(desc);
        }
        let resp = self
            .http
            .post("https://api.spotify.com/v1/me/playlists")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if status.is_success() {
            let v: serde_json::Value = serde_json::from_str(&text)?;
            Ok(PlaylistSummary {
                id: v["id"].as_str().unwrap_or("").to_string(),
                name: v["name"].as_str().unwrap_or("").to_string(),
                uri: v["uri"].as_str().unwrap_or("").to_string(),
                total_tracks: v["tracks"]["total"].as_u64().unwrap_or(0) as u32,
                art_url: v["images"]
                    .as_array()
                    .and_then(|imgs| imgs.first())
                    .and_then(|img| img["url"].as_str())
                    .map(|s| s.to_string()),
            })
        } else {
            anyhow::bail!("Create playlist failed ({}): {}", status.as_u16(), text);
        }
    }
}
