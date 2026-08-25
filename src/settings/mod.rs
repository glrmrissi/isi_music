use crate::config::AppConfig;
use anyhow::Result;

/// Runtime wrapper around all user settings.
///
/// In the MVP it only owns `AppConfig` (`config.toml`). Future phases will also
/// own `Theme` and `Keybinds` so the Settings panel can edit them in one place.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub config: AppConfig,
    pub dirty: bool,
}

impl Settings {
    pub fn load() -> Result<Self> {
        let mut config = AppConfig::load()?;
        config.normalize();
        Ok(Self {
            config,
            dirty: false,
        })
    }

    pub fn save(&self) -> Result<()> {
        self.config.save()
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}
