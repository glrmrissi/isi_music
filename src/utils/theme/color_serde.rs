use ratatui::style::Color;
use serde::{Deserialize, Deserializer, Serializer};

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

pub fn parse_color_from_str(s: &str) -> Result<Color, String> {
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

pub fn color_to_string(color: &Color) -> String {
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
