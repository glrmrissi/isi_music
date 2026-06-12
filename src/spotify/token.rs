use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::sync::RwLock;
use tracing::warn;

use crate::config;

pub struct TokenManager {
    access_token: RwLock<String>,
    refresh_token: RwLock<Option<String>>,
    expires_at: RwLock<Option<DateTime<Utc>>>,
    client_id: String,
    http: reqwest::Client,
}

impl TokenManager {
    pub fn new(client_id: String) -> Self {
        Self {
            access_token: RwLock::new(String::new()),
            refresh_token: RwLock::new(None),
            expires_at: RwLock::new(None),
            client_id,
            http: reqwest::Client::new(),
        }
    }

    pub fn set_token(&self, access_token: &str, refresh_token: Option<&str>, expires_in_secs: u64) {
        if let Ok(mut at) = self.access_token.write() {
            *at = access_token.to_string();
        }
        if let Ok(mut rt) = self.refresh_token.write() {
            *rt = refresh_token.map(|s| s.to_string());
        }
        if let Ok(mut ea) = self.expires_at.write() {
            *ea = Some(
                Utc::now()
                    + Duration::try_seconds(expires_in_secs as i64)
                        .unwrap_or_else(|| Duration::try_seconds(3600).unwrap()),
            );
        }
    }

    pub async fn get_access_token(&self) -> Option<String> {
        let token = self.access_token.read().ok()?.clone();
        if token.is_empty() {
            return None;
        }

        let expires_at = self.expires_at.read().ok()?.clone();
        let needs_refresh = expires_at.map(|e| e <= Utc::now()).unwrap_or(true);

        if needs_refresh {
            match self.do_refresh().await {
                Ok(new_token) => Some(new_token),
                Err(e) => {
                    warn!("Token refresh failed: {e}, forcing re-auth");
                    None
                }
            }
        } else {
            Some(token)
        }
    }

    async fn do_refresh(&self) -> Result<String> {
        let rt = self
            .refresh_token
            .read()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;

        let resp = self
            .http
            .post("https://accounts.spotify.com/api/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &rt),
                ("client_id", &self.client_id),
            ])
            .send()
            .await?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let body = serde_json::to_string(&json).unwrap_or_default();
            if status.as_u16() == 403 {
                anyhow::bail!("SPOTIFY_FORBIDDEN: token refresh returned 403. Details: {body}");
            }
            anyhow::bail!("token endpoint {status}: {body}");
        }

        let new_access = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no access_token in response"))?
            .to_string();
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
        let new_rt = json["refresh_token"].as_str().map(|s| s.to_string());

        if let Ok(mut at) = self.access_token.write() {
            *at = new_access.clone();
        }
        if let Some(ref rt) = new_rt {
            if let Ok(mut r) = self.refresh_token.write() {
                *r = Some(rt.clone());
            }
            config::save_refresh_token(rt);
        }
        if let Ok(mut ea) = self.expires_at.write() {
            *ea = Some(
                Utc::now()
                    + Duration::try_seconds(expires_in as i64)
                        .unwrap_or_else(|| Duration::try_seconds(3600).unwrap()),
            );
        }

        Ok(new_access)
    }
}
