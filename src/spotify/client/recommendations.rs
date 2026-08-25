use anyhow::Result;
use tracing::{info, warn};

use super::super::types::TrackSummary;
use super::SpotifyClient;

impl SpotifyClient {
    pub async fn fetch_recommendations(
        &self,
        seed_uris: &[String],
        limit: u8,
    ) -> Result<Vec<TrackSummary>> {
        if !self.authenticated {
            return Ok(Vec::new());
        }

        // Try the official Spotify recommendations endpoint first
        match self.fetch_spotify_recommendations(seed_uris, limit).await {
            Ok(tracks) if tracks.len() >= (limit as usize / 2).max(3) => {
                info!(
                    "Spotify /v1/recommendations returned {} tracks",
                    tracks.len()
                );
                return Ok(tracks);
            }
            Ok(few) => {
                info!(
                    "Spotify /v1/recommendations returned only {} tracks, falling back to custom",
                    few.len()
                );
            }
            Err(e) => {
                warn!("Spotify /v1/recommendations failed ({e:#}), falling back to custom logic");
            }
        }

        // Fallback: custom heuristic
        self.fetch_recommendations_fallback(seed_uris, limit).await
    }

    /// Official Spotify `/v1/recommendations` endpoint.
    /// Accepts up to 5 seed URIs (tracks and/or artists).
    async fn fetch_spotify_recommendations(
        &self,
        seed_uris: &[String],
        limit: u8,
    ) -> Result<Vec<TrackSummary>> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        // Split seeds into track IDs and artist IDs (max 5 total)
        let mut seed_track_ids: Vec<&str> = Vec::new();
        let mut seed_artist_ids: Vec<&str> = Vec::new();

        for uri in seed_uris.iter().take(5) {
            if let Some(id) = uri.strip_prefix("spotify:track:") {
                seed_track_ids.push(id);
            } else if let Some(id) = uri.strip_prefix("spotify:artist:") {
                seed_artist_ids.push(id);
            }
        }

        if seed_track_ids.is_empty() && seed_artist_ids.is_empty() {
            return Ok(Vec::new());
        }

        let limit_str = limit.to_string();
        let mut query: Vec<(&str, &str)> =
            vec![("limit", limit_str.as_str()), ("market", "from_token")];

        let seed_tracks_joined = seed_track_ids.join(",");
        let seed_artists_joined = seed_artist_ids.join(",");

        if !seed_track_ids.is_empty() {
            query.push(("seed_tracks", seed_tracks_joined.as_str()));
        }
        if !seed_artist_ids.is_empty() {
            query.push(("seed_artists", seed_artists_joined.as_str()));
        }

        super::spotify_rate_limit().await;
        let resp = self
            .http
            .get("https://api.spotify.com/v1/recommendations")
            .bearer_auth(&token)
            .query(&query)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!(
                "Spotify /v1/recommendations returned {status}: {}",
                &body[..body.len().min(500)]
            );
            anyhow::bail!("Spotify /v1/recommendations returned {status}");
        }

        let json: serde_json::Value = resp.json().await?;
        let empty: Vec<serde_json::Value> = vec![];
        let tracks: Vec<TrackSummary> = json["tracks"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|t| {
                let uri = t["uri"].as_str()?.to_string();
                if uri.is_empty() {
                    return None;
                }
                Some(TrackSummary {
                    name: t["name"].as_str().unwrap_or("Unknown").to_string(),
                    artist: t["artists"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x["name"].as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default(),
                    album: t["album"]["name"].as_str().unwrap_or_default().to_string(),
                    duration_ms: t["duration_ms"].as_u64().unwrap_or(0),
                    uri,
                    cover_path: None,
                    added_at: None,
                })
            })
            .collect();

        info!(
            "Spotify /v1/recommendations: seed_tracks={}, seed_artists={}, got {} tracks",
            seed_track_ids.len(),
            seed_artist_ids.len(),
            tracks.len()
        );

        Ok(tracks)
    }

    /// Custom heuristic recommendation logic (fallback).
    /// Discovers featured artists from seed artists' albums, then searches their tracks.
    async fn fetch_recommendations_fallback(
        &self,
        seed_uris: &[String],
        limit: u8,
    ) -> Result<Vec<TrackSummary>> {
        if !self.authenticated {
            return Ok(Vec::new());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let mut seed_artists: Vec<(String, String)> = Vec::new();

        for uri in seed_uris {
            if let Some(id) = uri.strip_prefix("spotify:artist:") {
                if !seed_artists.iter().any(|(i, _)| i == id) {
                    seed_artists.push((id.to_string(), String::new()));
                }
            } else if let Some(track_id) = uri.strip_prefix("spotify:track:")
                && let Ok(resp) = self
                    .http
                    .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
                    .bearer_auth(&token)
                    .send()
                    .await
                && let Ok(json) = resp.json::<serde_json::Value>().await
                && let (Some(a_id), Some(a_name)) = (
                    json["artists"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|a| a["id"].as_str()),
                    json["artists"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|a| a["name"].as_str()),
                )
                && !seed_artists.iter().any(|(i, _)| i == a_id)
            {
                seed_artists.push((a_id.to_string(), a_name.to_string()));
            }
        }

        if seed_artists.is_empty() {
            return Ok(vec![]);
        }

        let seed_artist_names: Vec<String> = seed_artists.iter().map(|(_, n)| n.clone()).collect();

        let mut featured_artists: Vec<String> = Vec::new();

        for (artist_id, _) in seed_artists.iter().take(2) {
            let album_query: Vec<(&str, &str)> = vec![
                ("limit", "5"),
                ("include_groups", "album,single"),
                ("market", "from_token"),
            ];
            if let Ok(resp) = self
                .http
                .get(format!(
                    "https://api.spotify.com/v1/artists/{artist_id}/albums"
                ))
                .bearer_auth(&token)
                .query(&album_query)
                .send()
                .await
                && let Ok(json) = resp.json::<serde_json::Value>().await
            {
                let album_ids: Vec<String> = json["items"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|a| a["id"].as_str())
                    .take(4)
                    .map(|s| s.to_string())
                    .collect();

                for album_id in &album_ids {
                    let track_query: Vec<(&str, &str)> =
                        vec![("limit", "10"), ("market", "from_token")];
                    if let Ok(resp2) = self
                        .http
                        .get(format!(
                            "https://api.spotify.com/v1/albums/{album_id}/tracks"
                        ))
                        .bearer_auth(&token)
                        .query(&track_query)
                        .send()
                        .await
                        && let Ok(json2) = resp2.json::<serde_json::Value>().await
                        && let Some(items) = json2["items"].as_array()
                    {
                        for track in items {
                            if let Some(artists) = track["artists"].as_array() {
                                for a in artists {
                                    if let Some(name) = a["name"].as_str() {
                                        let is_seed = seed_artist_names
                                            .iter()
                                            .any(|n| n.eq_ignore_ascii_case(name));
                                        if !is_seed
                                            && !featured_artists
                                                .iter()
                                                .any(|n| n.eq_ignore_ascii_case(name))
                                        {
                                            featured_artists.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if featured_artists.len() >= 10 {
                        break;
                    }
                }
            }
            if featured_artists.len() >= 10 {
                break;
            }
        }

        let mut pool: Vec<TrackSummary> = Vec::new();
        use rand::Rng;
        let mut rng = rand::thread_rng();

        for artist_name in featured_artists.iter().take(8) {
            let offset: u32 = rng.gen_range(0..20);
            let query = format!("artist:\"{}\"", artist_name);
            let offset_str = offset.to_string();
            let search_query: Vec<(&str, &str)> = vec![
                ("q", query.as_str()),
                ("type", "track"),
                ("limit", "3"),
                ("offset", offset_str.as_str()),
                ("market", "from_token"),
            ];
            if let Ok(resp) = self
                .http
                .get("https://api.spotify.com/v1/search")
                .bearer_auth(&token)
                .query(&search_query)
                .send()
                .await
                && let Ok(json) = resp.json::<serde_json::Value>().await
                && let Some(tracks) = json["tracks"]["items"].as_array()
            {
                for t in tracks {
                    let t_artist = t["artists"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|a| a["name"].as_str())
                        .unwrap_or("")
                        .to_string();
                    if seed_artist_names
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(&t_artist))
                    {
                        continue;
                    }
                    let name = t["name"].as_str().unwrap_or("Unknown").to_string();
                    let artist = t["artists"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x["name"].as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    let uri = t["uri"].as_str().unwrap_or_default().to_string();
                    if !uri.is_empty() {
                        pool.push(TrackSummary {
                            name,
                            artist,
                            album: t["album"]["name"].as_str().unwrap_or_default().to_string(),
                            duration_ms: t["duration_ms"].as_u64().unwrap_or(0),
                            uri,
                            cover_path: None,
                            added_at: None,
                        });
                    }
                }
            }
            if pool.len() >= (limit as usize * 2) {
                break;
            }
        }

        use rand::seq::SliceRandom;
        pool.shuffle(&mut rand::thread_rng());
        pool.truncate(limit as usize);

        info!("Generated {} manual recommendations", pool.len());
        Ok(pool)
    }
}
