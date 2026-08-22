use anyhow::Result;
use tracing::warn;

use super::super::types::Device;
use super::SpotifyClient;
use crate::ui::PlaybackState;

impl SpotifyClient {
    pub async fn play_track_uri(&self, track_uri: &str) -> Result<()> {
        if !self.authenticated {
            return Ok(());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        super::spotify_rate_limit().await;
        let body = serde_json::json!({ "uris": [track_uri] });
        let response = self
            .http
            .put("https://api.spotify.com/v1/me/player/play")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Spotify {status}: {body_text}");
        }
        Ok(())
    }

    pub async fn play_in_context(&self, playlist_uri: &str, track_uri: &str) -> Result<()> {
        if !self.authenticated {
            return Ok(());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        super::spotify_rate_limit().await;
        let body = serde_json::json!({
            "context_uri": playlist_uri,
            "offset": { "uri": track_uri }
        });
        let response = self
            .http
            .put("https://api.spotify.com/v1/me/player/play")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Spotify {status}: {body_text}");
        }
        Ok(())
    }

    pub async fn fetch_playback(&self) -> Result<PlaybackState> {
        if !self.authenticated {
            return Ok(PlaybackState::default());
        }
        let token = match self.get_access_token().await {
            Some(t) => t,
            None => {
                warn!("No access token for playback fetch");
                return Ok(PlaybackState::default());
            }
        };

        super::spotify_rate_limit().await;
        let response = match self
            .http
            .get("https://api.spotify.com/v1/me/player")
            .bearer_auth(&token)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to fetch playback: {e}");
                return Ok(PlaybackState::default());
            }
        };

        if response.status() == 204 {
            return Ok(PlaybackState::default());
        }

        let json: serde_json::Value = match response.json().await {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to parse playback response: {e}");
                return Ok(PlaybackState::default());
            }
        };

        if json.is_null() {
            return Ok(PlaybackState::default());
        }

        self.shuffle_state.store(
            json["shuffle_state"].as_bool().unwrap_or(false),
            std::sync::atomic::Ordering::Relaxed,
        );
        let repeat = json["repeat_state"].as_str().unwrap_or("off");
        if let Ok(mut rp) = self.repeat_state.write() {
            *rp = match repeat {
                "context" => super::super::RepeatState::Context,
                "track" => super::super::RepeatState::Track,
                _ => super::super::RepeatState::Off,
            };
        }

        let is_playing = json["is_playing"].as_bool().unwrap_or(false);
        self.is_playing
            .store(is_playing, std::sync::atomic::Ordering::Relaxed);
        let progress_ms = json["progress_ms"].as_u64().unwrap_or(0);
        let context_uri = json["context"]["uri"].as_str().map(|s| s.to_string());

        let item = &json["item"];
        if item.is_null() {
            return Ok(PlaybackState {
                is_playing,
                progress_ms,
                context_uri,
                ..Default::default()
            });
        }

        let item_type = item["type"].as_str().unwrap_or("");

        let (title, artist, album, duration_ms, art_url) = if item_type == "track" {
            let track_name = item["name"].as_str().unwrap_or("Unknown").to_string();
            let track_artist = item["artists"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["name"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let track_album = item["album"]["name"].as_str().unwrap_or("").to_string();
            let track_duration = item["duration_ms"].as_u64().unwrap_or(0);
            let url = if let Some(images) = item["album"]["images"].as_array() {
                images
                    .first()
                    .and_then(|img| img["url"].as_str())
                    .map(|s| s.to_string())
            } else {
                let uri = item["uri"].as_str().unwrap_or("");
                if !uri.is_empty() {
                    self.fetch_track_art_url(uri).await
                } else {
                    None
                }
            };
            (track_name, track_artist, track_album, track_duration, url)
        } else if item_type == "episode" {
            let ep_name = item["name"].as_str().unwrap_or("Unknown").to_string();
            let show_name = item["show"]["name"].as_str().unwrap_or("").to_string();
            let ep_duration = item["duration_ms"].as_u64().unwrap_or(0);
            let url = item["images"]
                .as_array()
                .and_then(|imgs| imgs.first())
                .and_then(|img| img["url"].as_str())
                .map(|s| s.to_string());
            (ep_name, show_name, String::new(), ep_duration, url)
        } else {
            return Ok(PlaybackState::default());
        };

        Ok(PlaybackState {
            title,
            artist,
            album,
            is_playing,
            shuffle: self
                .shuffle_state
                .load(std::sync::atomic::Ordering::Relaxed),
            repeat: self
                .repeat_state
                .read()
                .map(|r| *r)
                .unwrap_or(super::super::RepeatState::Off),
            progress_ms,
            duration_ms,
            volume: 100,
            art_url,
            cover_path: None,
            is_local: false,
            radio_mode: false,
            lyrics: None,
            lyrics_scroll: 0,
            lyrics_loading: false,
            waveform: None,
            context_uri,
        })
    }

    pub async fn toggle_playback(&self) -> Result<()> {
        if !self.authenticated {
            return Ok(());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let was_playing = self.is_playing.load(std::sync::atomic::Ordering::Relaxed);

        super::spotify_rate_limit().await;

        let url = if was_playing {
            "https://api.spotify.com/v1/me/player/pause"
        } else {
            "https://api.spotify.com/v1/me/player/play"
        };

        let resp = self.http.put(url).bearer_auth(&token).send().await?;
        let s = resp.status();
        if !s.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to toggle playback: {s}: {body}");
        }

        self.is_playing
            .store(!was_playing, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    pub async fn next_track(&self) -> Result<()> {
        if !self.authenticated {
            return Ok(());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        super::spotify_rate_limit().await;
        let resp = self
            .http
            .post("https://api.spotify.com/v1/me/player/next")
            .bearer_auth(&token)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to skip to next track: {status}: {body}");
        }
        self.is_playing
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    pub async fn prev_track(&self) -> Result<()> {
        if !self.authenticated {
            return Ok(());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        super::spotify_rate_limit().await;
        let resp = self
            .http
            .post("https://api.spotify.com/v1/me/player/previous")
            .bearer_auth(&token)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to skip to previous track: {status}: {body}");
        }
        self.is_playing
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    pub async fn fetch_devices(&self) -> Result<Vec<Device>> {
        if !self.authenticated {
            return Ok(Vec::new());
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No token"))?;

        super::spotify_rate_limit().await;
        let resp = self
            .http
            .get("https://api.spotify.com/v1/me/player/devices")
            .bearer_auth(&token)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("fetch_devices failed ({}): {}", status.as_u16(), text);
        }

        let v: serde_json::Value = serde_json::from_str(&text)?;
        let devices = v["devices"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("no devices array"))?
            .iter()
            .filter_map(|d| {
                Some(Device {
                    id: d["id"].as_str()?.to_string(),
                    name: d["name"].as_str()?.to_string(),
                    device_type: d["type"].as_str().unwrap_or("unknown").to_string(),
                    is_active: d["is_active"].as_bool().unwrap_or(false),
                })
            })
            .collect();
        Ok(devices)
    }

    pub async fn transfer_playback(&self, device_id: &str) -> Result<()> {
        if !self.authenticated {
            anyhow::bail!("not authenticated");
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No token"))?;

        super::spotify_rate_limit().await;
        let body = serde_json::json!({ "device_ids": [device_id], "play": false });
        let resp = self
            .http
            .put("https://api.spotify.com/v1/me/player")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("transfer_playback failed ({}): {}", status.as_u16(), text);
        }
        Ok(())
    }
}
