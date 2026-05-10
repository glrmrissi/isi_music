use anyhow::{Result};
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

        #[derive(Deserialize)]
        struct SessionResp {
            session: Session,
        }
        #[derive(Deserialize)]
        struct Session {
            key: String,
        }

        let http = Client::new();
        let resp: SessionResp = http
            .get("https://ws.audioscrobbler.com/2.0/")
            .query(&params)
            .send()
            .await?
            .json()
            .await?;

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

        match self
            .http
            .post("https://ws.audioscrobbler.com/2.0/")
            .form(&params)
            .send()
            .await
        {
            Ok(_) => info!("Last.fm: updated now playing: {} - {}", artist, track),
            Err(e) => warn!("Last.fm: failed to update now playing: {e}"),
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

        match self
            .http
            .post("https://ws.audioscrobbler.com/2.0/")
            .form(&params)
            .send()
            .await
        {
            Ok(_) => info!("Last.fm: scrobbled: {} - {}", artist, track),
            Err(e) => warn!("Last.fm: failed to scrobble: {e}"),
        }
    }
}