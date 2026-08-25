use ratatui::layout::{Constraint, Direction};
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        deserialize_with = "super::color_serde::deserialize_opt",
        serialize_with = "super::color_serde::serialize_opt"
    )]
    pub fg: Option<Color>,
    #[serde(
        default,
        deserialize_with = "super::color_serde::deserialize_opt",
        serialize_with = "super::color_serde::serialize_opt"
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

#[derive(Serialize, Deserialize, Clone, Debug, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisualizerStyle {
    #[default]
    BrailleBars,
    BlockBars,
    Plasma,
    AnimeArt,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VisualizerConfig {
    #[serde(default)]
    pub style: VisualizerStyle,
    #[serde(
        default,
        deserialize_with = "super::color_serde::deserialize_opt",
        serialize_with = "super::color_serde::serialize_opt"
    )]
    pub color: Option<Color>,
    #[serde(default)]
    pub bar_count: Option<usize>,
    #[serde(default)]
    pub height: Option<u16>,
    #[serde(default)]
    pub art_path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    Rounded,
    Thick,
    #[default]
    LeftBar,
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct BorderConfig {
    #[serde(default)]
    pub style: BorderStyle,
    #[serde(
        default,
        deserialize_with = "super::color_serde::deserialize_opt",
        serialize_with = "super::color_serde::serialize_opt"
    )]
    pub color_focused: Option<Color>,
    #[serde(
        default,
        deserialize_with = "super::color_serde::deserialize_opt",
        serialize_with = "super::color_serde::serialize_opt"
    )]
    pub color_unfocused: Option<Color>,
}
