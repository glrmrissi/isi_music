use anyhow::Result;
use tracing::{info, warn};

use super::super::types::{PlaylistSummary, TrackSummary};
use super::SpotifyClient;

pub(crate) fn playlist_item_to_track(item_wrapper: &serde_json::Value) -> Option<TrackSummary> {
    let track = if !item_wrapper["item"].is_null() {
        &item_wrapper["item"]
    } else if !item_wrapper["track"].is_null() {
        &item_wrapper["track"]
    } else {
        return None;
    };
    if track.is_null() {
        return None;
    }
    let uri = track["uri"].as_str().unwrap_or("").to_string();
    if uri.is_empty() {
        return None;
    }
    let (artist, album) = if track["type"].as_str() == Some("episode") {
        let show = track["show"]["name"].as_str().unwrap_or("").to_string();
        (show.clone(), show)
    } else {
        (
            track["artists"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["name"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
            track["album"]["name"].as_str().unwrap_or("").to_string(),
        )
    };
    Some(TrackSummary {
        name: track["name"].as_str().unwrap_or("Unknown").to_string(),
        artist,
        album,
        duration_ms: track["duration_ms"].as_u64().unwrap_or(0),
        uri,
        cover_path: None,
        added_at: item_wrapper["added_at"].as_str().map(|s| s.to_string()),
    })
}

impl SpotifyClient {
    pub async fn fetch_playlists(&self) -> Result<Vec<PlaylistSummary>> {
        if !self.authenticated {
            warn!("fetch_playlists: not authenticated");
            return Ok(Vec::new());
        }

        info!("fetch_playlists: starting to fetch playlists");
        let mut all = Vec::with_capacity(50);
        let mut offset = 0u32;
        loop {
            let token = self
                .get_access_token()
                .await
                .ok_or_else(|| anyhow::anyhow!("No access token available"))?;
            let offset_str = offset.to_string();
            super::spotify_rate_limit().await;
            info!("fetch_playlists: requesting offset={}", offset);
            let response = super::send_with_retry(
                &token,
                self.http
                    .get("https://api.spotify.com/v1/me/playlists")
                    .query(&[("limit", "50"), ("offset", &offset_str)]),
            )
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

    pub async fn stream_playlists(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        if !self.authenticated {
            let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
            return Ok(());
        }

        let mut offset = 0u32;
        let mut is_first_page = true;

        loop {
            let token = self
                .get_access_token()
                .await
                .ok_or_else(|| anyhow::anyhow!("No access token available"))?;
            let offset_str = offset.to_string();
            super::spotify_rate_limit().await;
            let response = super::send_with_retry(
                &token,
                self.http
                    .get("https://api.spotify.com/v1/me/playlists")
                    .query(&[("limit", "50"), ("offset", &offset_str)]),
            )
            .await?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                let err_msg = if status.as_u16() == 401 {
                    "SPOTIFY_UNAUTHORIZED".to_string()
                } else if status.as_u16() == 429 {
                    "SPOTIFY_RATE_LIMITED".to_string()
                } else if status.as_u16() == 403 {
                    format!("SPOTIFY_FORBIDDEN: {body}")
                } else {
                    format!("Spotify API error: status {status} body: {body}")
                };
                let _ = tx.send(crate::app::fetcher::StreamEvent::Error(err_msg));
                return Ok(());
            }

            let json: serde_json::Value = response.json().await?;
            let _total = json["total"].as_u64().unwrap_or(0) as u32;
            let fetched = json["items"]
                .as_array()
                .map(|a| a.len() as u32)
                .unwrap_or(0);

            let mut batch = Vec::with_capacity(fetched as usize);
            if let Some(items) = json["items"].as_array() {
                for p in items {
                    let art_url = p["images"]
                        .as_array()
                        .and_then(|imgs| imgs.first())
                        .and_then(|img| img["url"].as_str())
                        .map(|s| s.to_string());
                    batch.push(PlaylistSummary {
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

            if is_first_page {
                is_first_page = false;
                if tx
                    .send(crate::app::fetcher::StreamEvent::PlaylistsInitial { playlists: batch })
                    .is_err()
                {
                    return Ok(());
                }
            } else if !batch.is_empty()
                && tx
                    .send(crate::app::fetcher::StreamEvent::PlaylistsBatch { playlists: batch })
                    .is_err()
            {
                break;
            }

            if json["next"].is_null() || fetched == 0 {
                break;
            }
            offset += fetched;
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    pub async fn stream_playlist_tracks(
        &self,
        playlist_id: &str,
        tx: tokio::sync::mpsc::UnboundedSender<crate::app::fetcher::StreamEvent>,
    ) -> Result<()> {
        let (first_tracks, total, page_items) = self.fetch_playlist_tracks(playlist_id, 0).await?;
        if tx
            .send(crate::app::fetcher::StreamEvent::PlaylistTracksInitial {
                playlist_id: playlist_id.to_string(),
                tracks: first_tracks,
                total,
                page_items,
            })
            .is_err()
        {
            return Ok(());
        }

        let mut offset = page_items;
        while offset < total {
            super::spotify_rate_limit().await;
            match self.fetch_playlist_tracks(playlist_id, offset).await {
                Ok((batch, new_total, batch_items)) => {
                    if batch_items == 0 || batch.is_empty() {
                        break;
                    }
                    offset += batch_items;
                    if tx
                        .send(crate::app::fetcher::StreamEvent::PlaylistTracksBatch {
                            playlist_id: playlist_id.to_string(),
                            tracks: batch,
                            total: new_total,
                            page_items: batch_items,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    warn!("stream_playlist_tracks: failed offset {offset}: {e}");
                    break;
                }
            }
        }

        let _ = tx.send(crate::app::fetcher::StreamEvent::Done);
        Ok(())
    }

    /// Returns (tracks, total, page_items_count).
    /// `page_items_count` is the number of raw items the API returned, which
    /// callers must use to increment the offset — NOT `tracks.len()`.
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

        let tracks_url = format!("https://api.spotify.com/v1/playlists/{playlist_id}/tracks");
        info!("Fetching playlist tracks for {playlist_id} (offset={offset})");
        super::spotify_rate_limit().await;
        let mut response = super::send_with_retry(
            &token,
            self.http
                .get(&tracks_url)
                .query(&[("limit", limit_str), ("offset", &offset_str)]),
        )
        .await?;

        if !response.status().is_success() {
            let items_url = format!("https://api.spotify.com/v1/playlists/{playlist_id}/items");
            super::spotify_rate_limit().await;
            if let Ok(items_resp) = super::send_with_retry(
                &token,
                self.http
                    .get(&items_url)
                    .query(&[("limit", limit_str), ("offset", &offset_str)]),
            )
            .await
                && items_resp.status().is_success()
            {
                response = items_resp;
            }
        }

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
                info!("Got 403, trying main playlist endpoint for {playlist_id}");
                super::spotify_rate_limit().await;
                let main_url = format!("https://api.spotify.com/v1/playlists/{playlist_id}");
                let main_resp = self.http.get(&main_url).bearer_auth(&token).send().await?;
                let main_status = main_resp.status();
                info!("Main playlist endpoint returned status={main_status} for {playlist_id}");
                if main_status.is_success() {
                    let main_json: serde_json::Value = main_resp.json().await?;
                    let tracks_obj = if !main_json["items"].is_null() {
                        &main_json["items"]
                    } else {
                        &main_json["tracks"]
                    };
                    let total = tracks_obj["total"].as_u64().unwrap_or(0) as u32;
                    let mut all_tracks = Vec::new();
                    if let Some(items) = tracks_obj["items"].as_array() {
                        for item_wrapper in items {
                            if let Some(track) = playlist_item_to_track(item_wrapper) {
                                all_tracks.push(track);
                            }
                        }
                    }
                    let paged_tracks: Vec<TrackSummary> = all_tracks
                        .into_iter()
                        .skip(offset as usize)
                        .take(50)
                        .collect();
                    let page_items = paged_tracks.len() as u32;
                    self.library_cache.save_tracks(&key, &paged_tracks, total);
                    return Ok((paged_tracks, total, page_items));
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
                if let Some(track) = playlist_item_to_track(item_wrapper) {
                    tracks.push(track);
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
