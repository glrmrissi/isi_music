use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
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
            http: Client::new(),
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
        let http = Client::new();
        let url = format!(
            "https://ws.audioscrobbler.com/2.0/?method=auth.getToken&api_key={}&format=json",
            api_key
        );

        #[derive(Deserialize)]
        struct TokenResp {
            token: String,
        }

        let resp: TokenResp = http.get(url).send().await?.json().await?;
        Ok(resp.token)
    }

    pub async fn get_session(api_key: &str, api_secret: &str, token: &str) -> Result<String> {
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", api_key.to_string());
        params.insert("method", "auth.getSession".to_string());
        params.insert("token", token.to_string());

        let api_sig = Self::sign(&params, api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        let http = Client::new();
        let resp = http
            .get("https://ws.audioscrobbler.com/2.0/")
            .query(&params)
            .send()
            .await?;

        let text = resp.text().await?;

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

        Ok(session_resp.session.key)
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

        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(url).spawn();

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

        match self
            .http
            .post("https://ws.audioscrobbler.com/2.0/")
            .form(&params)
            .send()
            .await
        {
            Ok(resp) => {
                let text = resp.text().await.unwrap_or_default();
                if text.contains("\"error\":") {
                    // Don't log full response - may contain sensitive params
                    warn!("Last.fm: updateNowPlaying error response received");
                } else {
                    info!("Last.fm: updated now playing: {} - {}", artist, track);
                }
            }
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

        match self
            .http
            .post("https://ws.audioscrobbler.com/2.0/")
            .form(&params)
            .send()
            .await
        {
            Ok(resp) => {
                let text = resp.text().await.unwrap_or_default();
                if text.contains("\"error\":") {
                    warn!("Last.fm: scrobble error response received");
                } else {
                    info!("Last.fm: scrobbled: {} - {}", artist, track);
                }
            }
            Err(e) => warn!("Last.fm: failed to scrobble: {e}"),
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
