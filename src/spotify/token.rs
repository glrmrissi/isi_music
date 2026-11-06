use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::sync::RwLock;
use tracing::warn;

use crate::config;
use crate::utils::lock::{read_or_recover, write_or_recover};

pub struct TokenManager {
    access_token: RwLock<String>,
    refresh_token: RwLock<Option<String>>,
    expires_at: RwLock<Option<DateTime<Utc>>>,
    client_id: RwLock<String>,
    http: reqwest::Client,
}

impl TokenManager {
    pub fn new(client_id: String, http: reqwest::Client) -> Self {
        Self {
            access_token: RwLock::new(String::new()),
            refresh_token: RwLock::new(None),
            expires_at: RwLock::new(None),
            client_id: RwLock::new(client_id),
            http,
        }
    }

    pub fn set_client_id(&self, client_id: &str) {
        *write_or_recover(&self.client_id) = client_id.to_string();
    }

    pub fn set_token(&self, access_token: &str, refresh_token: Option<&str>, expires_in_secs: u64) {
        *write_or_recover(&self.access_token) = access_token.to_string();
        *write_or_recover(&self.refresh_token) = refresh_token.map(|s| s.to_string());
        *write_or_recover(&self.expires_at) = Some(
            Utc::now()
                + Duration::try_seconds(expires_in_secs as i64)
                    .or_else(|| Duration::try_seconds(3600))
                    .unwrap_or_default(),
        );
    }

    pub async fn get_access_token(&self) -> Option<String> {
        let token = read_or_recover(&self.access_token).clone();
        if token.is_empty() {
            return None;
        }

        let expires_at = *read_or_recover(&self.expires_at);
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
        let rt = read_or_recover(&self.refresh_token)
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;
        let client_id = read_or_recover(&self.client_id).clone();

        let resp = self
            .http
            .post("https://accounts.spotify.com/api/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &rt),
                ("client_id", &client_id),
            ])
            .send()
            .await?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let body = serde_json::to_string(&json).unwrap_or_default();
            if status.as_u16() == 403 {
                config::clear_refresh_token();
                *write_or_recover(&self.refresh_token) = None;
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

        *write_or_recover(&self.access_token) = new_access.clone();
        if let Some(ref rt) = new_rt {
            *write_or_recover(&self.refresh_token) = Some(rt.clone());
            config::save_refresh_token(rt);
        }
        *write_or_recover(&self.expires_at) = Some(
            Utc::now()
                + Duration::try_seconds(expires_in as i64)
                    .or_else(|| Duration::try_seconds(3600))
                    .unwrap_or_default(),
        );

        Ok(new_access)
    }
}
