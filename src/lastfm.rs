use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
use tracing::{info, warn};

#[derive(Clone)]
pub struct LastfmClient {
    api_key: String,
    api_secret: String,
    session_key: String,
    http: Client,
}

impl LastfmClient {
    pub fn new(api_key: String, api_secret: String, session_key: String) -> Self {
        Self { api_key, api_secret, session_key, http: Client::new() }
    }

    fn sign(params: &BTreeMap<&str, String>, secret: &str) -> String {
        let mut s = String::new();
        for (k, v) in params {
            s.push_str(k);
            s.push_str(v);
        }
        s.push_str(secret);
        format!("{:x}", md5::compute(s.as_bytes()))
    }

    /// Authenticate via username/password (getMobileSession).
    /// Returns the session key — store this, never ask again.
    pub async fn authenticate(api_key: &str, api_secret: &str, username: &str, password: &str) -> Result<String> {
        let http = Client::new();
        let password_md5 = format!("{:x}", md5::compute(password.as_bytes()));

        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", api_key.to_string());
        params.insert("method", "auth.getMobileSession".to_string());
        params.insert("password", password_md5);
        params.insert("username", username.to_string());

        let api_sig = Self::sign(&params, api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        #[derive(Deserialize)]
        struct Resp { session: Session }
        #[derive(Deserialize)]
        struct Session { key: String }

        let resp = http
            .post("https://ws.audioscrobbler.com/2.0/")
            .form(&params)
            .send()
            .await?
            .json::<Resp>()
            .await
            .context("Last.fm auth failed — check your API key/secret and credentials")?;

        Ok(resp.session.key)
    }

    pub async fn update_now_playing(&self, artist: &str, track: &str, duration_ms: u64) {
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", self.api_key.clone());
        params.insert("artist", artist.to_string());
        params.insert("duration", (duration_ms / 1000).to_string());
        params.insert("method", "track.updateNowPlaying".to_string());
        params.insert("sk", self.session_key.clone());
        params.insert("track", track.to_string());

        let api_sig = Self::sign(&params, &self.api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        match self.http.post("https://ws.audioscrobbler.com/2.0/").form(&params).send().await {
            Ok(_) => info!("Last.fm: now playing {} - {}", artist, track),
            Err(e) => warn!("Last.fm now playing failed: {e}"),
        }
    }

    pub async fn scrobble(&self, artist: &str, track: &str, timestamp: u64, duration_ms: u64) {
        let mut params: BTreeMap<&str, String> = BTreeMap::new();
        params.insert("api_key", self.api_key.clone());
        params.insert("artist[0]", artist.to_string());
        params.insert("duration[0]", (duration_ms / 1000).to_string());
        params.insert("method", "track.scrobble".to_string());
        params.insert("sk", self.session_key.clone());
        params.insert("timestamp[0]", timestamp.to_string());
        params.insert("track[0]", track.to_string());

        let api_sig = Self::sign(&params, &self.api_secret);
        params.insert("api_sig", api_sig);
        params.insert("format", "json".to_string());

        match self.http.post("https://ws.audioscrobbler.com/2.0/").form(&params).send().await {
            Ok(_) => info!("Last.fm: scrobbled {} - {}", artist, track),
            Err(e) => warn!("Last.fm scrobble failed: {e}"),
        }
    }
}
