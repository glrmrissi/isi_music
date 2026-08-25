use anyhow::Result;
use tracing::{info, warn};

use super::super::types::TrackSummary;
use super::SpotifyClient;

impl SpotifyClient {
    pub async fn fetch_liked_tracks(
        &self,
        offset: u32,
        force_refresh: bool,
    ) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }

        let key = format!("liked:{offset}");
        if !force_refresh && let Some(cached) = self.library_cache.get_tracks(&key) {
            info!("Library cache hit: liked songs offset={offset}");
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
            .get("https://api.spotify.com/v1/me/tracks")
            .bearer_auth(&token)
            .query(&[("limit", "50"), ("offset", &offset_str)])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(if status.as_u16() == 401 {
                warn!("Got 401 Unauthorized - token may have expired");
                anyhow::anyhow!("SPOTIFY_UNAUTHORIZED")
            } else if status.as_u16() == 429 {
                warn!("Rate limited on Spotify API");
                anyhow::anyhow!("SPOTIFY_RATE_LIMITED")
            } else if status.as_u16() == 403 {
                warn!("Got 403 Forbidden on /me/tracks. Body: {body}");
                anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}")
            } else {
                anyhow::anyhow!("Spotify API error: status {} body: {}", status, body)
            });
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let items_len = json["items"].as_array().map_or(0, |a| a.len());
        let mut tracks = Vec::with_capacity(items_len);
        let mut added_ats = Vec::with_capacity(items_len);

        if let Some(items) = json["items"].as_array() {
            for saved in items {
                let added_at_val = saved["added_at"].as_str().unwrap_or("").to_string();
                let added_at = Some(added_at_val.clone()).filter(|s| !s.is_empty());
                let track = &saved["track"];
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
                    added_ats.push(added_at_val);
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

        self.library_cache.save_tracks(&key, &tracks, total);
        if !tracks.is_empty() && added_ats.len() == tracks.len() {
            self.library_cache
                .append_liked_tracks_batch(&tracks, &added_ats);
        }
        Ok((tracks, total))
    }

    pub async fn fetch_liked_tracks_page(
        &self,
        after: Option<&str>,
        fallback_offset: u32,
    ) -> Result<(Vec<TrackSummary>, u32, Option<String>)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0, None));
        }

        if let Some((tracks, total, next_cursor)) =
            self.library_cache.get_liked_tracks_page(after, 50)
        {
            info!("Cursor cache hit: liked songs after={:?}", after);
            return Ok((tracks, total, next_cursor));
        }

        let (_, total) = self.fetch_liked_tracks(fallback_offset, true).await?;
        let (tracks, _, next_cursor) = self
            .library_cache
            .get_liked_tracks_page(after, 50)
            .unwrap_or_default();
        Ok((tracks, total, next_cursor))
    }

    pub async fn sync_liked_tracks(&self) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated {
            return Ok((Vec::new(), 0));
        }

        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        super::spotify_rate_limit().await;
        let response = self
            .http
            .get("https://api.spotify.com/v1/me/tracks")
            .bearer_auth(&token)
            .query(&[("limit", "50"), ("offset", "0")])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(if status.as_u16() == 401 {
                anyhow::anyhow!("SPOTIFY_UNAUTHORIZED")
            } else if status.as_u16() == 429 {
                anyhow::anyhow!("SPOTIFY_RATE_LIMITED")
            } else if status.as_u16() == 403 {
                warn!("Got 403 Forbidden on /me/tracks/contains. Body: {body}");
                anyhow::anyhow!("SPOTIFY_FORBIDDEN: {body}")
            } else {
                anyhow::anyhow!("Spotify API error: status {} body: {}", status, body)
            });
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;

        if let Some((tracks, cached_total, _)) =
            self.library_cache.get_liked_tracks_page(None, u32::MAX)
        {
            if !tracks.is_empty() && cached_total == total {
                info!("sync_liked_tracks: cache hit ({} tracks)", tracks.len());
                return Ok((tracks, total));
            }
            info!(
                "sync_liked_tracks: cache has {} but API total is {} — refetching",
                cached_total, total
            );
        }

        let mut all_tracks: Vec<TrackSummary> = Vec::with_capacity(total as usize);
        let mut all_added_ats: Vec<String> = Vec::with_capacity(total as usize);

        if let Some(items) = json["items"].as_array() {
            for saved in items {
                let added_at = saved["added_at"].as_str().unwrap_or("").to_string();
                let track = &saved["track"];
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

                if !uri.is_empty() {
                    all_added_ats.push(added_at.clone());
                    all_tracks.push(TrackSummary {
                        name,
                        artist,
                        album,
                        duration_ms,
                        uri,
                        cover_path: None,
                        added_at: Some(added_at),
                    });
                }
            }
        }

        // Fetch remaining pages sequentially
        let total_pages = total.div_ceil(50);
        if total_pages > 1 {
            info!(
                "sync_liked_tracks: fetching {} remaining pages ({} total tracks)",
                total_pages - 1,
                total
            );

            for page in 1..total_pages {
                let offset = page * 50;
                super::spotify_rate_limit().await;
                let resp = self
                    .http
                    .get("https://api.spotify.com/v1/me/tracks")
                    .bearer_auth(&token)
                    .query(&[("limit", "50"), ("offset", &offset.to_string())])
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    warn!("Failed to fetch liked tracks page {page}");
                    continue;
                }

                let json: serde_json::Value = resp.json().await?;
                if let Some(items) = json["items"].as_array() {
                    for saved in items {
                        let added_at = saved["added_at"].as_str().unwrap_or("").to_string();
                        let track = &saved["track"];
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

                        if !uri.is_empty() {
                            all_added_ats.push(added_at.clone());
                            all_tracks.push(TrackSummary {
                                name,
                                artist,
                                album,
                                duration_ms,
                                uri,
                                cover_path: None,
                                added_at: Some(added_at),
                            });
                        }
                    }
                }
            }
        }

        info!(
            "sync_liked_tracks: loaded {} tracks total",
            all_tracks.len()
        );

        // Save to cache
        self.library_cache
            .reset_liked_tracks_cache(&all_tracks, &all_added_ats);

        Ok((all_tracks, total))
    }
}
