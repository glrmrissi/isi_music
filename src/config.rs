use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub spotify: SpotifyConfig,
    #[serde(default)]
    pub lastfm: LastfmConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LastfmConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub session_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SpotifyConfig {
    pub client_id: Option<String>,
}

impl AppConfig {
    /// Reads ~/.config/isi-music/config.toml.
    /// Creates the file with empty values if it does not exist.
    pub fn load() -> Result<Self> {
        let path = config_path()?;

        if !path.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let empty = toml::to_string(&AppConfig::default())?;
            std::fs::write(&path, empty)?;
            return Ok(AppConfig::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Invalid config at {}", path.display()))
    }

    /// Environment variable takes priority over config file.
    pub fn get_client_id(&self) -> Option<String> {
        std::env::var("SPOTIFY_CLIENT_ID")
            .ok()
            .or_else(|| self.spotify.client_id.clone())
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn needs_setup(&self) -> bool {
        self.get_client_id().is_none()
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Could not determine config directory")?;
    Ok(base.join("isi-music").join("config.toml"))
}

pub fn env_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Could not determine config directory")?;
    Ok(base.join("isi-music").join(".env"))
}

pub fn cache_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("token.json"))
}

/// Separate file that persists just the refresh_token across rspotify auto-refreshes.
/// rspotify drops the refresh_token from its Token object when Spotify's refresh
/// response omits it (meaning "keep using the same one"), so we save it ourselves.
pub fn refresh_token_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("refresh_token"))
}

pub fn save_refresh_token(rt: &str) {
    if let Ok(p) = refresh_token_path() {
        let _ = std::fs::write(p, rt);
    }
}

pub fn load_refresh_token() -> Option<String> {
    refresh_token_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn volume_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("volume"))
}

pub fn load_volume() -> u8 {
    volume_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(|v| v.min(100))
        .unwrap_or(100)
}

pub fn save_volume(volume: u8) {
    if let Ok(p) = volume_path() {
        let _ = std::fs::write(p, volume.to_string());
    }
}

pub fn log_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("isi-music.log"))
}
