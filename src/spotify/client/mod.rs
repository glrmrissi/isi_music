use anyhow::Result;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::library_cache::LibraryCache;
use super::search_cache::SearchCache;
use super::token::TokenManager;
use super::types::TrackSummary;

mod auth;
mod library;
mod liked;
mod playback;
mod playlists;
mod recommendations;
mod search;

#[cfg(test)]
pub(crate) use playlists::playlist_item_to_track;

/// Build a reqwest client with a sane timeout so network drops don't hang the TUI.
pub(super) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

static SPOTIFY_RATE_LIMITER: LazyLock<Mutex<Instant>> =
    LazyLock::new(|| Mutex::new(Instant::now()));

pub(super) async fn spotify_rate_limit() {
    let min_interval = Duration::from_millis(250);
    let sleep_time = {
        let last_request = SPOTIFY_RATE_LIMITER.lock().await;
        let elapsed = last_request.elapsed();
        if elapsed < min_interval {
            min_interval - elapsed
        } else {
            Duration::ZERO
        }
    };
    if sleep_time > Duration::ZERO {
        sleep(sleep_time).await;
    }
    let mut last_request = SPOTIFY_RATE_LIMITER.lock().await;
    *last_request = Instant::now();
}

const MAX_429_RETRIES: u32 = 3;

pub(super) async fn send_with_retry(
    token: &str,
    req_builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        let resp = req_builder
            .try_clone()
            .ok_or_else(|| anyhow::anyhow!("Request cannot be retried"))?
            .bearer_auth(token)
            .send()
            .await?;
        if resp.status().as_u16() != 429 {
            return Ok(resp);
        }
        attempt += 1;
        if attempt >= MAX_429_RETRIES {
            return Ok(resp);
        }
        let retry_after = resp
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(2);
        tracing::warn!(
            "Spotify 429 rate limited, retrying after {}s (attempt {}/{})",
            retry_after,
            attempt,
            MAX_429_RETRIES
        );
        sleep(Duration::from_secs(retry_after.min(30))).await;
    }
}

pub struct SpotifyClient {
    pub(super) token_manager: TokenManager,
    pub http: reqwest::Client,
    pub(super) shuffle_state: std::sync::atomic::AtomicBool,
    pub(super) is_playing: std::sync::atomic::AtomicBool,
    pub(super) repeat_state: std::sync::RwLock<super::RepeatState>,
    pub authenticated: bool,
    pub(super) search_cache: SearchCache,
    pub library_cache: LibraryCache,
}

impl SpotifyClient {
    pub fn http_client(&self) -> reqwest::Client {
        self.http.clone()
    }

    pub async fn fetch_track_art_url(&self, track_uri: &str) -> Option<String> {
        if !self.authenticated {
            return None;
        }
        let track_id = track_uri.strip_prefix("spotify:track:")?;
        let token = self.get_access_token().await?;
        let json: serde_json::Value = self
            .http
            .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        json["album"]["images"]
            .as_array()?
            .last()
            .and_then(|img| img["url"].as_str())
            .map(|s| s.to_string())
    }

    pub async fn fetch_track_summary(&self, track_id: &str) -> Result<TrackSummary> {
        if !self.authenticated {
            anyhow::bail!("Not authenticated");
        }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;
        spotify_rate_limit().await;
        let resp = self
            .http
            .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
            .bearer_auth(&token)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("Spotify track fetch failed: {}", resp.status());
        }
        let json: serde_json::Value = resp.json().await?;
        let name = json["name"].as_str().unwrap_or("Unknown").to_string();
        let artist = json["artists"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a["name"].as_str())
            .unwrap_or("Unknown")
            .to_string();
        let album = json["album"]["name"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string();
        let duration_ms = json["duration_ms"].as_u64().unwrap_or(0);
        let uri = json["uri"]
            .as_str()
            .unwrap_or(&format!("spotify:track:{track_id}"))
            .to_string();
        let cover_path = json["album"]["images"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|img| img["url"].as_str())
            .map(|s| s.to_string());

        Ok(TrackSummary {
            name,
            artist,
            album,
            duration_ms,
            uri,
            cover_path,
            added_at: None,
        })
    }

    pub async fn check_uri_saved(&self, uri: &str) -> Result<bool> {
        spotify_rate_limit().await;
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token"))?;
        let resp = send_with_retry(
            &token,
            self.http
                .get("https://api.spotify.com/v1/me/library/contains")
                .query(&[("uris", uri)]),
        )
        .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if status.is_success() {
            let arr: Vec<bool> = serde_json::from_str(&text)?;
            Ok(arr.first().copied().unwrap_or(false))
        } else {
            anyhow::bail!("Check track saved failed ({}): {}", status.as_u16(), text);
        }
    }
}

pub async fn remove_uri_http(http: &reqwest::Client, token: &str, uri: &str) -> Result<()> {
    spotify_rate_limit().await;

    let resp = send_with_retry(
        token,
        http.delete("https://api.spotify.com/v1/me/library")
            .query(&[("uris", &uri)])
            .header("Content-Length", "0"),
    )
    .await?;

    let status = resp.status();
    if status.is_success() {
        tracing::info!("Unlike track: OK ({})", status.as_u16());
        Ok(())
    } else {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Unlike failed ({}): {}", status.as_u16(), text);
    }
}

pub async fn save_uri_http(http: &reqwest::Client, token: &str, uri: &str) -> Result<()> {
    spotify_rate_limit().await;

    let resp = send_with_retry(
        token,
        http.put("https://api.spotify.com/v1/me/library")
            .query(&[("uris", &uri)])
            .header("Content-Length", "0"),
    )
    .await?;

    let status = resp.status();
    if status.is_success() {
        tracing::info!("Like track: OK ({})", status.as_u16());
        Ok(())
    } else {
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Like failed ({}): {}", status.as_u16(), text);
    }
}
