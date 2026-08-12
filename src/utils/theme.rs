// TODO: modularize this file (~590 lines) into smaller modules
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UiWidget {
    Header,
    Search,
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
    AlbumArtWithInfo,
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

#[derive(Serialize, Deserialize, Clone, Debug, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualizerStyle {
    BrailleBars,
    BlockBars,
    Plasma,
    AnimeArt,
}

impl Default for VisualizerStyle {
    fn default() -> Self {
        Self::BrailleBars
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VisualizerConfig {
    #[serde(default)]
    pub style: VisualizerStyle,
    #[serde(
        default,
        deserialize_with = "color_serde::deserialize_opt",
        serialize_with = "color_serde::serialize_opt"
    )]
    pub color: Option<Color>,
    #[serde(default)]
    pub bar_count: Option<usize>,
    #[serde(default)]
    pub height: Option<u16>,
    #[serde(default)]
    pub art_path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    Rounded,
    Thick,
    LeftBar,
    None,
}

impl Default for BorderStyle {
    fn default() -> Self {
        Self::LeftBar
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BorderConfig {
    #[serde(default)]
    pub style: BorderStyle,
    #[serde(
        default,
        deserialize_with = "color_serde::deserialize_opt",
        serialize_with = "color_serde::serialize_opt"
    )]
    pub color_focused: Option<Color>,
    #[serde(
        default,
        deserialize_with = "color_serde::deserialize_opt",
        serialize_with = "color_serde::serialize_opt"
    )]
    pub color_unfocused: Option<Color>,
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

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Neutral dark palette — adapts to most terminal profiles
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

pub struct ThemeWatcher {
    rx: Receiver<Theme>,
    #[allow(dead_code)]
    _watcher: RecommendedWatcher,
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
        let watcher = notify::recommended_watcher(|_| {}).unwrap();
        Self {
            rx,
            _watcher: watcher,
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
        let (tx, rx) = channel();
        let path = Self::get_path().unwrap_or_else(|| PathBuf::from("theme.toml"));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        let watch_path = path.clone();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if stop_clone.load(Ordering::Relaxed) {
                return;
            }
            let Ok(event) = res else { return };
            let relevant = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            );
            if !relevant {
                return;
            }
            let dominated = event
                .paths
                .iter()
                .any(|p| p.to_string_lossy().contains("theme.toml"));
            if !dominated {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(current_content) = fs::read_to_string(&watch_path) {
                if let Ok(new_theme) = toml::from_str::<Theme>(&current_content) {
                    let _ = tx.send(new_theme);
                } else {
                    warn!("Error on theme.toml");
                }
            }
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        if let Some(parent) = path.parent() {
            watcher
                .watch(parent.as_ref(), RecursiveMode::NonRecursive)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        }

        Ok(ThemeWatcher {
            rx,
            _watcher: watcher,
            stop,
        })
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
