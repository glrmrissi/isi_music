use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub spotify: SpotifyConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SpotifyConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
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

    pub fn get_client_secret(&self) -> Option<String> {
        std::env::var("SPOTIFY_CLIENT_SECRET")
            .ok()
            .or_else(|| self.spotify.client_secret.clone())
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Could not determine config directory")?;
    Ok(base.join("isi-music").join("config.toml"))
}

pub fn cache_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("token.json"))
}

pub fn log_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("isi-music.log"))
}
