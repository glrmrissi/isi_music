use anyhow::Result;
use tracing::{info, warn};

use super::super::auth::SpotifyAuth;
use super::super::library_cache::LibraryCache;
use super::super::search_cache::SearchCache;
use super::super::token::TokenManager;
use super::SpotifyClient;
use crate::config;

impl SpotifyClient {
    pub async fn new_unauthenticated() -> Result<Self> {
        let http = super::http_client();
        let dummy_token = TokenManager::new(String::new(), http.clone());
        Ok(Self {
            token_manager: dummy_token,
            http,
            shuffle_state: std::sync::atomic::AtomicBool::new(false),
            is_playing: std::sync::atomic::AtomicBool::new(false),
            repeat_state: std::sync::RwLock::new(super::super::RepeatState::Off),
            authenticated: false,
            search_cache: SearchCache::new(600)?,
            library_cache: LibraryCache::new().await?,
        })
    }

    pub async fn new() -> Result<Self> {
        let cfg = config::AppConfig::load()?;
        let Some(client_id) = cfg.get_client_id() else {
            warn!("Spotify Web API Client ID is not configured; starting in local-only mode");
            return Self::new_unauthenticated().await;
        };

        let http = super::http_client();
        let token_manager = TokenManager::new(client_id.clone(), http.clone());

        let saved_rt = config::load_refresh_token();

        if let Some(ref rt) = saved_rt {
            match Self::exchange_refresh_token(&client_id, rt, &http).await {
                Ok((access_token, expires_in_secs, new_rt)) => {
                    let effective_rt = new_rt.as_deref().unwrap_or(rt.as_str());
                    config::save_refresh_token(effective_rt);
                    token_manager.set_token(&access_token, Some(effective_rt), expires_in_secs);
                    info!("Authenticated with Spotify via refresh token");
                    return Ok(Self {
                        token_manager,
                        http: http.clone(),
                        shuffle_state: std::sync::atomic::AtomicBool::new(false),
                        is_playing: std::sync::atomic::AtomicBool::new(false),
                        repeat_state: std::sync::RwLock::new(super::super::RepeatState::Off),
                        authenticated: true,
                        search_cache: SearchCache::new(600)?,
                        library_cache: LibraryCache::new().await?,
                    });
                }
                Err(e) => {
                    warn!("Refresh token exchange failed ({e}), re-authenticating...");
                }
            }
        }

        let (access_token, refresh_token, expires_in) = SpotifyAuth::authenticate().await?;

        config::save_refresh_token(&refresh_token);
        token_manager.set_token(&access_token, Some(&refresh_token), expires_in);

        info!("Authenticated with Spotify");
        Ok(Self {
            token_manager,
            http,
            shuffle_state: std::sync::atomic::AtomicBool::new(false),
            is_playing: std::sync::atomic::AtomicBool::new(false),
            repeat_state: std::sync::RwLock::new(super::super::RepeatState::Off),
            authenticated: true,
            search_cache: SearchCache::new(600)?,
            library_cache: LibraryCache::new().await?,
        })
    }

    pub async fn get_access_token(&self) -> Option<String> {
        if !self.authenticated {
            return None;
        }
        self.token_manager.get_access_token().await
    }

    async fn exchange_refresh_token(
        client_id: &str,
        refresh_token: &str,
        http: &reqwest::Client,
    ) -> Result<(String, u64, Option<String>)> {
        let resp = http
            .post("https://accounts.spotify.com/api/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", client_id),
            ])
            .send()
            .await?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let body = serde_json::to_string(&json).unwrap_or_default();
            if status.as_u16() == 403 {
                config::clear_refresh_token();
                anyhow::bail!("SPOTIFY_FORBIDDEN: token refresh returned 403. Details: {body}");
            }
            anyhow::bail!("token endpoint {status}: {body}");
        }

        let access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no access_token in response"))?
            .to_string();
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
        let new_rt = json["refresh_token"].as_str().map(|s| s.to_string());

        Ok((access_token, expires_in, new_rt))
    }
}
