mod color_serde;
mod layout;
mod serde_types;
mod watcher;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::warn;

pub use layout::LayoutNode;
pub use serde_types::{
    BorderConfig, BorderStyle, SerializableConstraint, SerializableDirection, UiWidget,
    VisualizerConfig, VisualizerStyle, WidgetStyle,
};
pub use watcher::{ThemeWatcher, watch_theme};

#[cfg(test)]
pub use color_serde::{color_to_string, parse_color_from_str};
#[cfg(test)]
pub use ratatui::style::Style;

fn default_cross_fade_ms() -> u64 {
    800
}

fn default_highlight_symbol() -> String {
    "> ".to_string()
}

fn default_options_panel_symbol() -> String {
    "▶ ".to_string()
}

fn default_true() -> bool {
    true
}

fn default_bg_panel() -> Color {
    Color::Rgb(0x1b, 0x1b, 0x1b)
}
fn default_bg_element() -> Color {
    Color::Rgb(0x24, 0x24, 0x24)
}
fn default_border_subtle() -> Color {
    Color::Rgb(0x5a, 0x5a, 0x5a)
}
fn default_border_dimmest() -> Color {
    Color::Rgb(0x30, 0x30, 0x30)
}
fn default_primary() -> Color {
    Color::Rgb(0xd8, 0xd8, 0xd8)
}
fn default_success() -> Color {
    Color::Rgb(0xc3, 0xe8, 0x8d)
}
fn default_error() -> Color {
    Color::Rgb(0xff, 0x75, 0x7f)
}
fn default_warning() -> Color {
    Color::Rgb(0xff, 0x96, 0x6c)
}
fn default_info() -> Color {
    Color::Rgb(0xb8, 0xb8, 0xb8)
}

fn default_compact_layout() -> LayoutNode {
    use SerializableConstraint::*;
    LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![Length(1), Fill(1), Length(1)]),
        widget: None,
        children: Some(vec![
            LayoutNode {
                widget: Some(UiWidget::Header),
                direction: None,
                constraints: None,
                children: None,
            },
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![Percentage(35), Fill(1)]),
                widget: None,
                children: Some(vec![
                    LayoutNode {
                        widget: Some(UiWidget::AsciiArt),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                    LayoutNode {
                        widget: Some(UiWidget::MainContent),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                ]),
            },
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![Percentage(30), Fill(1)]),
                widget: None,
                children: Some(vec![
                    LayoutNode {
                        widget: Some(UiWidget::Marquee),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                    LayoutNode {
                        widget: Some(UiWidget::Progress),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                ]),
            },
        ]),
    }
}

fn default_fullscreen_layout() -> LayoutNode {
    use SerializableConstraint::*;
    LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![Length(18), Length(8), Min(0)]),
        widget: None,
        children: Some(vec![
            LayoutNode {
                widget: Some(UiWidget::NowPlaying),
                direction: None,
                constraints: None,
                children: None,
            },
            LayoutNode {
                widget: Some(UiWidget::FullscreenLyrics),
                direction: None,
                constraints: None,
                children: None,
            },
            LayoutNode {
                widget: Some(UiWidget::Visualizer),
                direction: None,
                constraints: None,
                children: None,
            },
        ]),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Theme {
    #[serde(with = "color_serde")]
    pub border_active: Color,
    #[serde(with = "color_serde")]
    pub border_inactive: Color,
    #[serde(with = "color_serde")]
    pub highlight_bg: Color,
    #[serde(with = "color_serde")]
    pub text_primary: Color,
    #[serde(with = "color_serde")]
    pub accent_color: Color,

    #[serde(default)]
    pub widget_styles: HashMap<UiWidget, WidgetStyle>,

    #[serde(default)]
    pub layout_tree: LayoutNode,

    #[serde(default)]
    pub ascii_art: Option<String>,

    #[serde(default)]
    pub ascii_art_inline: Option<Vec<String>>,

    #[serde(default)]
    pub ascii_art_path: Option<PathBuf>,

    #[serde(default = "default_true")]
    pub show_ascii_art: bool,

    #[serde(default = "default_compact_layout")]
    pub compact_layout: LayoutNode,

    #[serde(default = "default_fullscreen_layout")]
    pub fullscreen_layout: LayoutNode,

    #[serde(with = "color_serde")]
    pub background: Color,

    #[serde(with = "color_serde")]
    pub text_secondary: Color,

    #[serde(with = "color_serde")]
    pub status_bar: Color,

    #[serde(default = "default_highlight_symbol")]
    pub highlight_symbol: String,

    #[serde(default = "default_options_panel_symbol")]
    pub options_panel_symbol: String,

    #[serde(default = "default_bg_panel", with = "color_serde")]
    pub background_panel: Color,
    #[serde(default = "default_bg_element", with = "color_serde")]
    pub background_element: Color,

    #[serde(default = "default_border_subtle", with = "color_serde")]
    pub border_subtle: Color,
    #[serde(default = "default_border_dimmest", with = "color_serde")]
    pub border_dimmest: Color,

    #[serde(default = "default_primary", with = "color_serde")]
    pub primary: Color,
    #[serde(default = "default_success", with = "color_serde")]
    pub success: Color,
    #[serde(default = "default_error", with = "color_serde")]
    pub error: Color,
    #[serde(default = "default_warning", with = "color_serde")]
    pub warning: Color,
    #[serde(default = "default_info", with = "color_serde")]
    pub info: Color,
    #[serde(default)]
    pub reactive_theme: bool,
    #[serde(default = "default_cross_fade_ms")]
    pub reactive_cross_fade_ms: u64,
    #[serde(default)]
    pub visualizer: VisualizerConfig,
    #[serde(default)]
    pub borders: HashMap<UiWidget, BorderConfig>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border_active: Color::Rgb(0xd0, 0xd0, 0xd0),
            border_inactive: Color::Rgb(0x77, 0x77, 0x77),
            highlight_bg: Color::Rgb(0x2a, 0x2a, 0x2a),
            text_primary: Color::Rgb(0xe6, 0xe6, 0xe6),
            accent_color: Color::Rgb(0xc4, 0xc4, 0xc4),
            widget_styles: HashMap::new(),
            layout_tree: LayoutNode::default(),
            ascii_art: None,
            ascii_art_inline: None,
            ascii_art_path: None,
            show_ascii_art: false,
            compact_layout: default_compact_layout(),
            fullscreen_layout: default_fullscreen_layout(),
            background: Color::Rgb(0x14, 0x14, 0x14),
            text_secondary: Color::Rgb(0x9e, 0x9e, 0x9e),
            status_bar: Color::Rgb(0x1c, 0x1c, 0x1c),
            highlight_symbol: default_highlight_symbol(),
            options_panel_symbol: default_options_panel_symbol(),
            background_panel: default_bg_panel(),
            background_element: default_bg_element(),
            border_subtle: default_border_subtle(),
            border_dimmest: default_border_dimmest(),
            primary: default_primary(),
            success: default_success(),
            error: default_error(),
            warning: default_warning(),
            info: default_info(),
            reactive_theme: false,
            reactive_cross_fade_ms: default_cross_fade_ms(),
            visualizer: VisualizerConfig::default(),
            borders: HashMap::new(),
        }
    }
}

impl Theme {
    pub fn lerp(from: &Theme, to: &Theme, t: f32) -> Theme {
        let t = t.clamp(0.0, 1.0);
        let mut out = to.clone();
        out.background = lerp_color(from.background, to.background, t);
        out.background_panel = lerp_color(from.background_panel, to.background_panel, t);
        out.background_element = lerp_color(from.background_element, to.background_element, t);
        out.border_active = lerp_color(from.border_active, to.border_active, t);
        out.border_inactive = lerp_color(from.border_inactive, to.border_inactive, t);
        out.border_subtle = lerp_color(from.border_subtle, to.border_subtle, t);
        out.border_dimmest = lerp_color(from.border_dimmest, to.border_dimmest, t);
        out.text_primary = lerp_color(from.text_primary, to.text_primary, t);
        out.text_secondary = lerp_color(from.text_secondary, to.text_secondary, t);
        out.status_bar = lerp_color(from.status_bar, to.status_bar, t);
        out.highlight_bg = lerp_color(from.highlight_bg, to.highlight_bg, t);
        out.primary = lerp_color(from.primary, to.primary, t);
        out.accent_color = lerp_color(from.accent_color, to.accent_color, t);
        out.success = lerp_color(from.success, to.success, t);
        out.error = lerp_color(from.error, to.error, t);
        out.warning = lerp_color(from.warning, to.warning, t);
        out.info = lerp_color(from.info, to.info, t);
        out
    }

    pub fn get_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut p| {
            p.push("isi-music/theme.toml");
            p
        })
    }

    pub fn load() -> Self {
        let path = Self::get_path().unwrap_or_else(|| PathBuf::from("theme.toml"));
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let default_theme = Self::default();
            if let Ok(toml_str) = toml::to_string_pretty(&default_theme) {
                let _ = fs::write(&path, toml_str);
            }
            return default_theme;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read theme file: {}", e);
                return Self::default();
            }
        };
        match toml::from_str::<Theme>(&content) {
            Ok(theme) => {
                let has_new_fields = content.contains("background_panel")
                    || content.contains("border_subtle")
                    || content.contains("primary");
                if !has_new_fields {
                    let migrated = Self::migrate_from_legacy(&theme);
                    if let Ok(toml_str) = toml::to_string_pretty(&migrated) {
                        let _ = fs::write(&path, toml_str);
                    }
                    return migrated;
                }
                theme
            }
            Err(e) => {
                warn!("Failed to parse theme.toml: {}", e);
                Self::default()
            }
        }
    }

    fn migrate_from_legacy(legacy: &Theme) -> Theme {
        let mut migrated = Theme::default();
        migrated.layout_tree = legacy.layout_tree.clone();
        migrated.compact_layout = legacy.compact_layout.clone();
        migrated.fullscreen_layout = legacy.fullscreen_layout.clone();
        migrated.widget_styles = legacy.widget_styles.clone();
        migrated.ascii_art = legacy.ascii_art.clone();
        migrated.ascii_art_inline = legacy.ascii_art_inline.clone();
        migrated.ascii_art_path = legacy.ascii_art_path.clone();
        migrated.show_ascii_art = legacy.show_ascii_art;
        migrated.highlight_symbol = legacy.highlight_symbol.clone();
        migrated.options_panel_symbol = legacy.options_panel_symbol.clone();
        migrated
    }

    pub fn watch() -> std::io::Result<ThemeWatcher> {
        watch_theme()
    }

    pub fn load_ascii_art(&self) -> Option<Vec<String>> {
        if let Some(ref lines) = self.ascii_art_inline {
            if !lines.is_empty() {
                return Some(lines.clone());
            }
        }

        if let Some(ref path) = self.ascii_art_path {
            if let Ok(content) = fs::read_to_string(path) {
                return Some(content.lines().map(|s| s.to_string()).collect());
            }
        }
        None
    }
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let (fr, fg, fb) = match from {
        Color::Rgb(r, g, b) => (u16::from(r), u16::from(g), u16::from(b)),
        _ => return to,
    };
    let (tr, tg, tb) = match to {
        Color::Rgb(r, g, b) => (u16::from(r), u16::from(g), u16::from(b)),
        _ => return to,
    };
    let l = |a: u16, b: u16| -> u8 {
        (a as f32 + (b as f32 - a as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::Rgb(l(fr, tr), l(fg, tg), l(fb, tb))
}

#[cfg(test)]
#[path = "../../../tests/utils/theme.rs"]
mod tests;
