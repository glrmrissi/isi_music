use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const OFFICIAL_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
pub const OFFICIAL_REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
pub const CUSTOM_REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppConfig {
    pub spotify: SpotifyConfig,
    #[serde(default)]
    pub lastfm: LastfmConfig,
    #[serde(default)]
    pub discord: DiscordConfig,
    #[serde(default)]
    pub local: LocalConfig,
    #[serde(default)]
    pub musixmatch: MusixMatchConfig,
    #[serde(default)]
    pub options: AppOptionsConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub session: SessionState,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SessionState {
    pub focus: Option<String>,
    pub active_content: Option<String>,
    pub compact_mode: Option<bool>,
    pub library_selected: Option<usize>,
    pub volume: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct LocalConfig {
    pub music_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DiscordConfig {
    pub enabled: Option<bool>,
    pub app_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct LastfmConfig {
    pub session_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SpotifyConfig {
    pub client_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MusixMatchConfig {
    pub musixmatch_api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppOptionsConfig {
    pub show_cover_images: Option<bool>,
    pub enable_lyrics: Option<bool>,
    pub show_visualizer: Option<bool>,
    pub default_layout: Option<String>,
    pub compact_mode_default: Option<bool>,
    /// When the queue ends, automatically fetch and queue recommended tracks (default: true)
    pub autoplay: Option<bool>,
    /// Hot-reload theme.toml and keybinds.toml on file change (default: true)
    pub hot_reload: Option<bool>,
    /// Windows System Media Transport Controls overlay (default: true)
    #[serde(default)]
    pub smtc_enabled: Option<bool>,
    /// Windows global media hotkeys (default: true)
    #[serde(default)]
    pub media_keys_enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct UiConfig {
    #[serde(default)]
    pub show_cover_images: Option<bool>,
    #[serde(default)]
    pub enable_lyrics: Option<bool>,
    #[serde(default)]
    pub show_visualizer: Option<bool>,
    #[serde(default)]
    pub default_layout: Option<String>,
    #[serde(default)]
    pub compact_mode_default: Option<bool>,
    #[serde(default)]
    pub show_breadcrumb: Option<bool>,
    #[serde(default)]
    pub autoplay: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct CacheConfig {
    pub enabled: Option<bool>,
    pub auto_cleanup: Option<bool>,
    pub max_size_mb: Option<u64>,
    pub cleanup_interval_hours: Option<u32>,
    pub keep_days: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AudioConfig {
    /// Gapless playback for Spotify (librespot). Local files still need work.
    #[serde(default = "default_true")]
    pub gapless: bool,
    /// Spotify stream bitrate in kbps: 96, 160 or 320.
    #[serde(default = "default_bitrate")]
    pub bitrate: u16,
}

fn default_true() -> bool {
    true
}

fn default_bitrate() -> u16 {
    320
}

impl AudioConfig {
    pub fn librespot_bitrate(&self) -> librespot_playback::config::Bitrate {
        match self.bitrate {
            96 => librespot_playback::config::Bitrate::Bitrate96,
            160 => librespot_playback::config::Bitrate::Bitrate160,
            _ => librespot_playback::config::Bitrate::Bitrate320,
        }
    }
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
        toml::from_str(&content).with_context(|| format!("Invalid config at {}", path.display()))
    }

    pub fn get_client_id(&self) -> Option<String> {
        std::env::var("SPOTIFY_CLIENT_ID")
            .ok()
            .filter(|s| !s.is_empty() && s != "your_client_id_here")
            .or_else(|| {
                self.spotify
                    .client_id
                    .as_ref()
                    .filter(|s| !s.is_empty() && s.as_str() != "your_client_id_here")
                    .cloned()
            })
    }

    pub fn get_musixmatch_api_key(&self) -> Option<String> {
        std::env::var("MUSIXMATCH_API_KEY")
            .ok()
            .or_else(|| self.musixmatch.musixmatch_api_key.clone())
    }

    pub fn show_cover_images(&self) -> bool {
        self.ui
            .show_cover_images
            .or(self.options.show_cover_images)
            .unwrap_or(true)
    }

    pub fn enable_lyrics(&self) -> bool {
        self.ui
            .enable_lyrics
            .or(self.options.enable_lyrics)
            .unwrap_or(true)
    }

    pub fn show_visualizer(&self) -> bool {
        self.ui
            .show_visualizer
            .or(self.options.show_visualizer)
            .unwrap_or(true)
    }

    pub fn compact_mode_default(&self) -> bool {
        self.ui
            .compact_mode_default
            .or(self.options.compact_mode_default)
            .unwrap_or(false)
    }

    pub fn show_breadcrumb(&self) -> bool {
        self.ui.show_breadcrumb.unwrap_or(false)
    }

    /// Autoplay is enabled by default; only disabled if explicitly set to false.
    pub fn autoplay_enabled(&self) -> bool {
        self.ui.autoplay.or(self.options.autoplay).unwrap_or(true)
    }

    pub fn hot_reload(&self) -> bool {
        self.options.hot_reload.unwrap_or(true)
    }

    pub fn smtc_enabled(&self) -> bool {
        self.options.smtc_enabled.unwrap_or(true)
    }

    pub fn media_keys_enabled(&self) -> bool {
        self.options.media_keys_enabled.unwrap_or(true)
    }

    /// Copies values from the legacy `[options]` section into `[ui]` when `[ui]`
    /// was not explicitly set. This keeps existing user configs working.
    pub fn normalize(&mut self) {
        if self.ui.show_cover_images.is_none() {
            self.ui.show_cover_images = self.options.show_cover_images;
        }
        if self.ui.enable_lyrics.is_none() {
            self.ui.enable_lyrics = self.options.enable_lyrics;
        }
        if self.ui.show_visualizer.is_none() {
            self.ui.show_visualizer = self.options.show_visualizer;
        }
        if self.ui.default_layout.is_none() {
            self.ui.default_layout = self.options.default_layout.clone();
        }
        if self.ui.compact_mode_default.is_none() {
            self.ui.compact_mode_default = self.options.compact_mode_default;
        }
        if self.ui.autoplay.is_none() {
            self.ui.autoplay = self.options.autoplay;
        }
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
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Could not determine config directory")?;
    Ok(base.join("isi-music").join("config.toml"))
}

pub fn env_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Could not determine config directory")?;
    Ok(base.join("isi-music").join(".env"))
}

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

pub fn clear_refresh_token() {
    if let Ok(p) = refresh_token_path() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn streaming_refresh_token_path() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("streaming_refresh_token"))
}

pub fn save_streaming_refresh_token(rt: &str) {
    if let Ok(p) = streaming_refresh_token_path() {
        let _ = std::fs::write(p, rt);
    }
}

pub fn load_streaming_refresh_token() -> Option<String> {
    streaming_refresh_token_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn clear_streaming_refresh_token() {
    if let Ok(p) = streaming_refresh_token_path() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn get_local_db_path() -> String {
    if let Some(mut path) = dirs::data_dir() {
        // Old versions used "isi_music" (underscore) — migrate to the hyphenated name
        let legacy = path.join("isi_music");
        path.push("isi-music");
        if legacy.exists() && !path.exists() {
            let _ = std::fs::rename(&legacy, &path);
        }

        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!("Erro ao criar diretório: {e}");
            return "local_files.db".into();
        }

        path.push("library.db");
        return path.to_string_lossy().to_string();
    }

    "local_files.db".into()
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

pub fn lyrics_cache_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music").join("lyrics");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn waveform_cache_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Could not determine cache directory")?;
    let dir = base.join("isi-music").join("waveforms");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
