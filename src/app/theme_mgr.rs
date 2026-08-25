use std::time::Instant;

use crate::ui::Ui;
use crate::utils::theme::{Theme, ThemeWatcher};

#[allow(clippy::large_enum_variant)]
pub enum ThemeChange {
    None,
    Apply { theme: Theme },
}

pub struct ThemeManager {
    pub theme: Theme,
    pub theme_rx: ThemeWatcher,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub reactive_target: Option<Theme>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub reactive_from: Option<Theme>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub reactive_start: Option<Instant>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub reactive_swatches: Option<Vec<crate::utils::palette::Rgb>>,
    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub reactive_toggle_pending: bool,
}

impl ThemeManager {
    pub fn new(theme: Theme, theme_rx: ThemeWatcher) -> Self {
        Self {
            theme,
            theme_rx,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_target: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_from: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_start: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_swatches: None,
            #[cfg(all(feature = "palette", feature = "album-art"))]
            reactive_toggle_pending: false,
        }
    }

    pub fn poll_theme_changes(&mut self) -> ThemeChange {
        let mut change = ThemeChange::None;
        while let Ok(new_theme) = self.theme_rx.try_recv() {
            let preserve_reactive_transition = {
                #[cfg(all(feature = "palette", feature = "album-art"))]
                {
                    let preserve = self.reactive_toggle_pending || self.reactive_start.is_some();
                    self.reactive_toggle_pending = false;
                    preserve
                }
                #[cfg(not(all(feature = "palette", feature = "album-art")))]
                {
                    false
                }
            };
            #[cfg(all(feature = "palette", feature = "album-art"))]
            tracing::debug!(
                "reactive: theme_watcher fired, preserve={}, reactive_start={}",
                preserve_reactive_transition,
                self.reactive_start.is_some()
            );
            self.theme = new_theme.clone();
            if !preserve_reactive_transition {
                #[cfg(all(feature = "palette", feature = "album-art"))]
                let skip_apply = self.theme.reactive_theme
                    && self.reactive_start.is_none()
                    && self.reactive_target.is_none();
                #[cfg(not(all(feature = "palette", feature = "album-art")))]
                let skip_apply = false;

                if !skip_apply {
                    change = ThemeChange::Apply { theme: new_theme };
                }
            }
        }
        change
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn lerp_reactive(&mut self, now: Instant) -> Option<Theme> {
        if let (Some(start), Some(from), Some(target)) = (
            self.reactive_start,
            self.reactive_from.as_ref(),
            self.reactive_target.as_ref(),
        ) {
            let elapsed = now.duration_since(start).as_millis() as f32;
            let dur = self.theme.reactive_cross_fade_ms.max(1) as f32;
            let t = (elapsed / dur).min(1.0);
            let blended = Theme::lerp(from, target, t);
            if t >= 1.0 {
                self.theme = target.clone();
                self.reactive_start = None;
                self.reactive_from = None;
                self.reactive_target = None;
            }
            Some(blended)
        } else {
            None
        }
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn toggle_reactive(&mut self, enabled: bool) -> anyhow::Result<()> {
        let path = Theme::get_path().unwrap_or_else(|| std::path::PathBuf::from("theme.toml"));
        let content = std::fs::read_to_string(&path)?;
        let mut theme: Theme = toml::from_str(&content).unwrap_or_default();
        theme.reactive_theme = enabled;
        let new_content = toml::to_string_pretty(&theme)?;
        std::fs::write(&path, new_content)?;
        Ok(())
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn start_reactive(&mut self, swatches: &[crate::utils::palette::Rgb], ui: &Ui) {
        if swatches.is_empty() {
            return;
        }
        let new_theme = crate::utils::palette::derive_theme(swatches, &self.theme);
        self.reactive_from = Some(ui.theme_snapshot());
        self.reactive_target = Some(new_theme);
        self.reactive_start = Some(Instant::now());
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn disable_reactive(&mut self) -> Theme {
        self.reactive_toggle_pending = false;
        self.reactive_start = None;
        self.reactive_from = None;
        self.reactive_target = None;
        let restored = Theme::load();
        self.theme = restored.clone();
        restored
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn store_swatches(&mut self, swatches: Vec<crate::utils::palette::Rgb>) {
        self.reactive_swatches = Some(swatches);
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn swatches_clone(&self) -> Option<Vec<crate::utils::palette::Rgb>> {
        self.reactive_swatches.clone()
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn reactive_theme_enabled(&self) -> bool {
        self.theme.reactive_theme
    }

    #[cfg(all(feature = "palette", feature = "album-art"))]
    pub fn set_reactive_toggle_pending(&mut self, pending: bool) {
        self.reactive_toggle_pending = pending;
    }
}

#[cfg(test)]
#[path = "../../tests/app/theme_mgr.rs"]
mod tests;
