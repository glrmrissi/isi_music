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
    pub load_failed: bool,
}

impl Settings {
    pub fn load() -> Result<Self> {
        let mut config = AppConfig::load()?;
        config.normalize();
        Ok(Self {
            config,
            dirty: false,
            load_failed: false,
        })
    }

    pub fn save(&self) -> Result<()> {
        anyhow::ensure!(
            !self.load_failed,
            "config.toml failed to load; refusing to overwrite it with defaults"
        );
        self.config.save()
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_refuses_to_overwrite_when_config_failed_to_load() {
        let settings = Settings {
            load_failed: true,
            ..Default::default()
        };
        assert!(settings.save().is_err());
    }
}
