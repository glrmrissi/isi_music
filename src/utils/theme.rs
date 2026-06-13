use ratatui::layout::{Constraint, Direction};
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, channel},
};
use std::thread;
use std::time::Duration;
use tracing::warn;

#[derive(Serialize, Deserialize, Clone, Debug, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SerializableDirection {
    Horizontal,
    Vertical,
}

impl From<SerializableDirection> for Direction {
    fn from(d: SerializableDirection) -> Self {
        match d {
            SerializableDirection::Horizontal => Direction::Horizontal,
            SerializableDirection::Vertical => Direction::Vertical,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UiWidget {
    Header,
    Library,
    Playlists,
    AlbumArt,
    MainContent,
    Queue,
    Progress,
    Marquee,
    Visualizer,
    Help,
    AsciiArt,
    Spacer,
    Lyrics,
    NowPlaying,
    FullscreenLyrics,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SerializableConstraint {
    Length(u16),
    Percentage(u16),
    Ratio(u32, u32),
    Min(u16),
    Max(u16),
    Fill(u16),
}

impl From<SerializableConstraint> for Constraint {
    fn from(c: SerializableConstraint) -> Self {
        match c {
            SerializableConstraint::Length(v) => Constraint::Length(v),
            SerializableConstraint::Percentage(v) => Constraint::Percentage(v),
            SerializableConstraint::Ratio(n, d) => Constraint::Ratio(n, d),
            SerializableConstraint::Min(v) => Constraint::Min(v),
            SerializableConstraint::Max(v) => Constraint::Max(v),
            SerializableConstraint::Fill(v) => Constraint::Fill(v),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WidgetStyle {
    #[serde(
        default,
        deserialize_with = "color_serde::deserialize_opt",
        serialize_with = "color_serde::serialize_opt"
    )]
    pub fg: Option<Color>,
    #[serde(
        default,
        deserialize_with = "color_serde::deserialize_opt",
        serialize_with = "color_serde::serialize_opt"
    )]
    pub bg: Option<Color>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl From<WidgetStyle> for Style {
    fn from(w: WidgetStyle) -> Self {
        let mut s = Style::default();
        if let Some(c) = w.fg {
            s = s.fg(c);
        }
        if let Some(c) = w.bg {
            s = s.bg(c);
        }
        if w.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if w.italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        s
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LayoutNode {
    pub direction: Option<SerializableDirection>,
    pub constraints: Option<Vec<SerializableConstraint>>,
    pub children: Option<Vec<LayoutNode>>,
    pub widget: Option<UiWidget>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        use SerializableConstraint::*;
        Self {
            direction: Some(SerializableDirection::Vertical),
            constraints: Some(vec![Length(3), Fill(1), Length(1), Length(1)]),
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
                    constraints: Some(vec![Percentage(25), Fill(1)]),
                    widget: None,
                    children: Some(vec![
                        LayoutNode {
                            direction: Some(SerializableDirection::Vertical),
                            constraints: Some(vec![Length(7), Fill(1)]),
                            widget: None,
                            children: Some(vec![
                                LayoutNode {
                                    widget: Some(UiWidget::Library),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                                LayoutNode {
                                    widget: Some(UiWidget::Playlists),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                            ]),
                        },
                        LayoutNode {
                            direction: Some(SerializableDirection::Vertical),
                            constraints: Some(vec![Fill(1), Length(8)]),
                            widget: None,
                            children: Some(vec![
                                LayoutNode {
                                    widget: Some(UiWidget::MainContent),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                                LayoutNode {
                                    widget: Some(UiWidget::Queue),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                            ]),
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
                LayoutNode {
                    widget: Some(UiWidget::Help),
                    direction: None,
                    constraints: None,
                    children: None,
                },
            ]),
        }
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
}

fn default_true() -> bool {
    true
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

impl Default for Theme {
    fn default() -> Self {
        Self {
            border_active: Color::Green,
            border_inactive: Color::DarkGray,
            highlight_bg: Color::Rgb(40, 40, 40),
            text_primary: Color::White,
            accent_color: Color::Green,
            widget_styles: HashMap::new(),
            layout_tree: LayoutNode::default(),
            ascii_art: None,
            ascii_art_inline: None,
            ascii_art_path: None,
            show_ascii_art: false,
            compact_layout: default_compact_layout(),
            fullscreen_layout: default_fullscreen_layout(),
            background: Color::Rgb(20, 20, 20),
            text_secondary: Color::Gray,
            status_bar: Color::Rgb(30, 30, 30),
        }
    }
}

pub struct ThemeWatcher {
    rx: Receiver<Theme>,
    stop: Arc<AtomicBool>,
}

impl ThemeWatcher {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for ThemeWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

impl std::ops::Deref for ThemeWatcher {
    type Target = std::sync::mpsc::Receiver<Theme>;
    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

#[cfg(test)]
impl ThemeWatcher {
    pub fn noop() -> Self {
        let (_, rx) = std::sync::mpsc::channel();
        Self {
            rx,
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

impl Theme {
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
        fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn watch() -> std::io::Result<ThemeWatcher> {
        let (tx, rx) = channel();
        let path = Self::get_path().unwrap_or_else(|| PathBuf::from("theme.toml"));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        thread::spawn(move || {
            let mut last_content = fs::read_to_string(&path).unwrap_or_default();

            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                if let Ok(current_content) = fs::read_to_string(&path) {
                    if current_content != last_content {
                        thread::sleep(Duration::from_millis(50));

                        if let Ok(new_theme) = toml::from_str::<Theme>(&current_content) {
                            if tx.send(new_theme).is_ok() {
                                last_content = current_content;
                            }
                        } else {
                            warn!("Error on theme.toml");
                        }
                    }
                }

                thread::sleep(Duration::from_millis(500));
            }
        });

        Ok(ThemeWatcher { rx, stop })
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_path().unwrap_or_else(|| PathBuf::from("theme.toml"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create theme dir: {e}"))?;
        }
        let toml_str =
            toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize theme: {e}"))?;
        fs::write(&path, toml_str).map_err(|e| format!("Failed to write theme: {e}"))?;
        Ok(())
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

mod color_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn deserialize<'de, D>(d: D) -> Result<Color, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        parse_color_from_str(&s).map_err(serde::de::Error::custom)
    }

    pub fn serialize<S>(c: &Color, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&color_to_string(c))
    }

    pub fn deserialize_opt<'de, D>(d: D) -> Result<Option<Color>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Option::<String>::deserialize(d)?;
        match s {
            Some(s) => parse_color_from_str(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }

    pub fn serialize_opt<S>(c: &Option<Color>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match c {
            Some(c) => s.serialize_str(&color_to_string(c)),
            None => s.serialize_none(),
        }
    }
}

fn parse_color_from_str(s: &str) -> Result<Color, String> {
    let s = s.trim().to_lowercase();

    if s.starts_with('#') && s.len() == 7 {
        let r = u8::from_str_radix(&s[1..3], 16).map_err(|_| "Invalid R")?;
        let g = u8::from_str_radix(&s[3..5], 16).map_err(|_| "Invalid G")?;
        let b = u8::from_str_radix(&s[5..7], 16).map_err(|_| "Invalid B")?;
        return Ok(Color::Rgb(r, g, b));
    }

    match s.as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "white" => Ok(Color::White),
        "gray" => Ok(Color::Gray),
        "dark_gray" => Ok(Color::DarkGray),
        "light_red" => Ok(Color::LightRed),
        "light_green" => Ok(Color::LightGreen),
        "light_yellow" => Ok(Color::LightYellow),
        "light_blue" => Ok(Color::LightBlue),
        "light_magenta" => Ok(Color::LightMagenta),
        "light_cyan" => Ok(Color::LightCyan),
        "transparent" | "none" | "reset" => Ok(Color::Reset),
        s if s.starts_with("rgb") && s.ends_with(')') => {
            let is_rgba = s.starts_with("rgba(");
            let start_idx = if is_rgba { 5 } else { 4 };
            let inner = &s[start_idx..s.len() - 1];
            let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
            if parts.len() < 3 {
                return Err(format!("Invalid RGB format: {}", s));
            }
            let r: u8 = parts[0].parse().map_err(|_| "Invalid R")?;
            let g: u8 = parts[1].parse().map_err(|_| "Invalid G")?;
            let b: u8 = parts[2].parse().map_err(|_| "Invalid B")?;
            Ok(Color::Rgb(r, g, b))
        }
        _ => Err(format!("Unknown color: {}", s)),
    }
}

#[cfg(test)]
#[path = "../../tests/utils/theme.rs"]
mod tests;

fn color_to_string(color: &Color) -> String {
    match color {
        Color::Black => "black".into(),
        Color::Red => "red".into(),
        Color::Green => "green".into(),
        Color::Yellow => "yellow".into(),
        Color::Blue => "blue".into(),
        Color::Magenta => "magenta".into(),
        Color::Cyan => "cyan".into(),
        Color::White => "white".into(),
        Color::Gray => "gray".into(),
        Color::DarkGray => "dark_gray".into(),
        Color::LightRed => "light_red".into(),
        Color::LightGreen => "light_green".into(),
        Color::LightYellow => "light_yellow".into(),
        Color::LightBlue => "light_blue".into(),
        Color::LightMagenta => "light_magenta".into(),
        Color::LightCyan => "light_cyan".into(),
        Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        _ => "white".into(),
    }
}
