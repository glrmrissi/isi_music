use serde::{Deserialize, Serialize};
use ratatui::style::Color;
use ratatui::layout::{Constraint, Direction};
use std::path::PathBuf;
use std::fs;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug, Copy)]
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

#[derive(Serialize, Deserialize, Clone, Debug)]
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
    Spacer,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LayoutNode {
    pub direction: Option<SerializableDirection>,
    pub constraints: Option<Vec<String>>,
    pub children: Option<Vec<LayoutNode>>,
    pub widget: Option<UiWidget>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self {
            direction: Some(SerializableDirection::Vertical),
            constraints: Some(vec!["4".into(), "min(0)".into(), "2".into(), "1".into()]),
            widget: None,
            children: Some(vec![
                LayoutNode { widget: Some(UiWidget::Header), direction: None, constraints: None, children: None },
                LayoutNode { 
                    direction: Some(SerializableDirection::Horizontal),
                    constraints: Some(vec!["25%".into(), "min(0)".into()]),
                    widget: None,
                    children: Some(vec![
                        LayoutNode { 
                            direction: Some(SerializableDirection::Vertical),
                            constraints: Some(vec!["7".into(), "min(0)".into()]),
                            widget: None,
                            children: Some(vec![
                                LayoutNode { widget: Some(UiWidget::Library), direction: None, constraints: None, children: None },
                                LayoutNode { widget: Some(UiWidget::Playlists), direction: None, constraints: None, children: None },
                            ]),
                        },
                        LayoutNode { widget: Some(UiWidget::MainContent), direction: None, constraints: None, children: None },
                    ]),
                },
                LayoutNode { widget: Some(UiWidget::Progress), direction: None, constraints: None, children: None },
                LayoutNode { widget: Some(UiWidget::Help), direction: None, constraints: None, children: None },
            ]),
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Theme {
    #[serde(deserialize_with = "deserialize_color", serialize_with = "serialize_color")]
    pub border_active: Color,
    #[serde(deserialize_with = "deserialize_color", serialize_with = "serialize_color")]
    pub border_inactive: Color,
    #[serde(deserialize_with = "deserialize_color", serialize_with = "serialize_color")]
    pub highlight_bg: Color,
    #[serde(deserialize_with = "deserialize_color", serialize_with = "serialize_color")]
    pub text_primary: Color,
    #[serde(deserialize_with = "deserialize_color", serialize_with = "serialize_color")]
    pub accent_color: Color,
    #[serde(default)]
    pub layout_tree: LayoutNode
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border_active: Color::Green,
            border_inactive: Color::DarkGray,
            highlight_bg: Color::Rgb(40, 40, 40),
            text_primary: Color::White,
            accent_color: Color::Green,
            layout_tree: LayoutNode::default(),
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

    pub fn watch() -> std::io::Result<Receiver<Theme>> {
        let (tx, rx) = channel();
        let path = Self::get_path().unwrap_or_else(|| PathBuf::from("theme.toml"));
        
        thread::spawn(move || {
            let mut last_modified = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok());
            
            loop {
                thread::sleep(Duration::from_millis(500));
                
                if let Ok(metadata) = std::fs::metadata(&path) {
                    if let Ok(current_modified) = metadata.modified() {
                        if Some(current_modified) != last_modified {
                            last_modified = Some(current_modified);
                            let new_theme = Theme::load();
                            let _ = tx.send(new_theme);
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
}

/// Parse color from string representation
fn parse_color_from_str(s: &str) -> Result<Color, String> {
    let s = s.trim().to_lowercase();
    
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
        s if s.starts_with("rgb") && s.ends_with(")") => {
            let is_rgba = s.starts_with("rgba(");
            let start_idx = if is_rgba { 5 } else { 4 };
            let inner = &s[start_idx..s.len() - 1];
            let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
            
            if parts.len() < 3 {
                return Err(format!("Invalid RGB/RGBA format: {}", s));
            }
            
            let r: u8 = parts[0].parse().map_err(|_| format!("Invalid R value: {}", parts[0]))?;
            let g: u8 = parts[1].parse().map_err(|_| format!("Invalid G value: {}", parts[1]))?;
            let b: u8 = parts[2].parse().map_err(|_| format!("Invalid B value: {}", parts[2]))?;
            
            Ok(Color::Rgb(r, g, b))
        }
        _ => Err(format!("Unknown color: {}", s))
    }
}

/// Convert color to string representation
fn color_to_string(color: &Color) -> String {
    match color {
        Color::Black => "black".to_string(),
        Color::Red => "red".to_string(),
        Color::Green => "green".to_string(),
        Color::Yellow => "yellow".to_string(),
        Color::Blue => "blue".to_string(),
        Color::Magenta => "magenta".to_string(),
        Color::Cyan => "cyan".to_string(),
        Color::White => "white".to_string(),
        Color::Gray => "gray".to_string(),
        Color::DarkGray => "dark_gray".to_string(),
        Color::LightRed => "light_red".to_string(),
        Color::LightGreen => "light_green".to_string(),
        Color::LightYellow => "light_yellow".to_string(),
        Color::LightBlue => "light_blue".to_string(),
        Color::LightMagenta => "light_magenta".to_string(),
        Color::LightCyan => "light_cyan".to_string(),
        Color::Rgb(r, g, b) => format!("rgb({},{},{})", r, g, b),
        Color::Indexed(idx) => format!("indexed({})", idx),
        _ => "white".to_string(),
    }
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<Color, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse_color_from_str(&s).map_err(serde::de::Error::custom)
}

fn serialize_color<S>(color: &Color, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&color_to_string(color))
}