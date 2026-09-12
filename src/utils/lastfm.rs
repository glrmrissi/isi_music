use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{info, warn};

include!(concat!(env!("OUT_DIR"), "/secrets.rs"));

pub fn get_api_key() -> String {
    reveal_secret(LASTFM_API_KEY)
}

pub fn get_api_secret() -> String {
    reveal_secret(LASTFM_API_SECRET)
}

#[derive(Clone)]
pub struct LastfmClient {
    api_key: String,
    api_secret: String,
    session_key: String,
    http: Client,
}

impl LastfmClient {
    pub fn new(api_key: String, api_secret: String, session_key: String) -> Self {
        Self {
            api_key,
            api_secret,
            session_key,
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .pool_max_idle_per_host(2)
                .pool_idle_timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn sign(params: &BTreeMap<&str, String>, secret: &str) -> String {
        let mut s = String::new();
        for (k, v) in params {
            if *k != "format" && *k != "callback" {
                s.push_str(k);
                s.push_str(v);
            }
        }
        s.push_str(secret);
        format!("{:x}", md5::compute(s.as_bytes()))
    }

    pub async fn get_auth_token(api_key: &str) -> Result<String> {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        #[derive(Deserialize)]
        struct TokenResp {
            token: String,
        }

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3u32 {
            let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                let resp = http
                    .get("https://ws.audioscrobbler.com/2.0/")
                    .query(&[
                        ("method", "auth.getToken"),
                        ("api_key", api_key),
                        ("format", "json"),
                    ])
                    .send()
                    .await?;
                resp.json::<TokenResp>().await
            })
            .await;
            match result {
                Ok(Ok(resp)) => return Ok(resp.token),
                Ok(Err(e)) => last_err = Some(e.into()),
                Err(_) => last_err = Some(anyhow::anyhow!("Last.fm auth token request timed out")),
            }
            if attempt < 2 {
                tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt))).await;
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Last.fm auth token failed")))
    }

    pub async fn get_session(api_key: &str, api_secret: &str, token: &str) -> Result<String> {
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", api_key.to_string());
        params.insert("method", "auth.getSession".to_string());
        params.insert("token", token.to_string());

        let api_sig = Self::sign(&params, api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3u32 {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                http.get("https://ws.audioscrobbler.com/2.0/")
                    .query(&params)
                    .send(),
            )
            .await;
            match result {
                Ok(Ok(resp)) => {
                    let text = match resp.text().await {
                        Ok(t) => t,
                        Err(e) => {
                            last_err = Some(e.into());
                            if attempt < 2 {
                                tokio::time::sleep(std::time::Duration::from_secs(
                                    2u64.pow(attempt),
                                ))
                                .await;
                            }
                            continue;
                        }
                    };
                    if text.contains("\"error\":") {
                        return Err(anyhow::anyhow!("Last.fm API returned error"));
                    }
                    #[derive(Deserialize)]
                    struct SessionResp {
                        session: Session,
                    }
                    #[derive(Deserialize)]
                    struct Session {
                        key: String,
                    }
                    let session_resp: SessionResp = serde_json::from_str(&text)
                        .with_context(|| "Failed to parse Last.fm session response")?;
                    return Ok(session_resp.session.key);
                }
                Ok(Err(e)) => last_err = Some(e.into()),
                Err(_) => last_err = Some(anyhow::anyhow!("Last.fm session request timed out")),
            }
            if attempt < 2 {
                tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt))).await;
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Last.fm session failed")))
    }

    pub async fn authenticate_with_browser(api_key: &str, api_secret: &str) -> Result<String> {
        println!("Getting auth token from Last.fm...");
        let token = Self::get_auth_token(api_key).await?;

        let auth_url = format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            api_key, token
        );

        Self::open_browser(&auth_url);

        println!("Opening Last.fm authorization in your browser...");
        println!("After authorizing, press ENTER to continue...");

        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf).ok();

        println!("Getting session key from Last.fm...");
        let session_key = Self::get_session(api_key, api_secret, &token).await?;
        println!("Session key received successfully!");

        Ok(session_key)
    }

    pub async fn authenticate_with_default() -> Result<String> {
        Self::authenticate_with_browser(&get_api_key(), &get_api_secret()).await
    }

    fn open_browser(url: &str) {
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();

        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("powershell")
            .args(["-Command", &format!("Start-Process '{}'", url)])
            .spawn();
    }

    pub async fn update_now_playing(
        &self,
        artist: &str,
        track: &str,
        album: &str,
        duration_ms: u64,
    ) {
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", self.api_key.clone());
        params.insert("artist", artist.to_string());
        if !album.trim().is_empty() {
            params.insert("album", album.to_string());
        }
        if duration_ms > 0 {
            params.insert("duration", (duration_ms / 1000).to_string());
        }
        params.insert("method", "track.updateNowPlaying".to_string());
        params.insert("sk", self.session_key.clone());
        params.insert("track", track.to_string());

        let api_sig = Self::sign(&params, &self.api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        match self.send_with_retry(&params).await {
            Ok(()) => info!("Last.fm: updated now playing: {} - {}", artist, track),
            Err(e) => warn!("Last.fm: failed to update now playing: {e}"),
        }
    }

    pub async fn scrobble(
        &self,
        artist: &str,
        track: &str,
        album: &str,
        timestamp: u64,
        duration_ms: u64,
    ) {
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", self.api_key.clone());
        params.insert("artist[0]", artist.to_string());
        if !album.trim().is_empty() {
            params.insert("album[0]", album.to_string());
        } else {
            info!(
                "Last.fm: scrobble with empty album for {} - {}",
                artist, track
            );
        }
        if duration_ms > 0 {
            params.insert("duration[0]", (duration_ms / 1000).to_string());
        }
        params.insert("method", "track.scrobble".to_string());
        params.insert("sk", self.session_key.clone());
        params.insert("timestamp[0]", timestamp.to_string());
        params.insert("track[0]", track.to_string());

        let api_sig = Self::sign(&params, &self.api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        match self.send_with_retry(&params).await {
            Ok(()) => info!("Last.fm: scrobbled: {} - {}", artist, track),
            Err(e) => warn!("Last.fm: failed to scrobble: {e}"),
        }
    }

    async fn send_with_retry(&self, params: &BTreeMap<&str, String>) -> Result<(), String> {
        const MAX_RETRIES: u32 = 3;
        let mut attempt = 0u32;
        loop {
            let result = self
                .http
                .post("https://ws.audioscrobbler.com/2.0/")
                .form(params)
                .send()
                .await;
            match result {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        return Err(format!("Last.fm HTTP {}", resp.status().as_u16()));
                    }
                    let text = resp.text().await.unwrap_or_default();
                    if text.contains("\"error\":") {
                        return Err("Last.fm API error response".to_string());
                    }
                    return Ok(());
                }
                Err(e) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(e.to_string());
                    }
                    let delay = std::time::Duration::from_secs(2u64.pow(attempt));
                    tracing::warn!(
                        "Last.fm: retrying in {:?} (attempt {}/{})",
                        delay,
                        attempt,
                        MAX_RETRIES
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_decoding() {
        let key = get_api_key();
        let secret = get_api_secret();
        // Only check format, never print or log the actual values
        assert_eq!(key.len(), 32, "API key must be 32 chars");
        assert_eq!(secret.len(), 32, "API secret must be 32 chars");
        assert!(
            key.chars().all(|c| c.is_ascii_hexdigit()),
            "API key must be hex"
        );
        assert!(
            secret.chars().all(|c| c.is_ascii_hexdigit()),
            "API secret must be hex"
        );
        // Drop explicitly to minimize time in memory
        drop(key);
        drop(secret);
    }
}
