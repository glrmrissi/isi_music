use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    PlayPause,
    NextTrack,
    PrevTrack,
    VolumeUp,
    VolumeDown,
    SeekForward,
    SeekBackward,
    ToggleShuffle,
    CycleRepeat,
    ToggleRadio,
    GetRecommendations,
    LikeTrack,
    AddToQueue,
    RemoveFromQueue,
    SortTracks,
    NavUp,
    NavDown,
    NavFirst,
    NavLast,
    TabNext,
    TabPrev,
    Enter,
    Back,
    Search,
    QuickSearch,
    Help,
    ToggleCompact,
    ToggleFullscreen,
    ToggleVisualizer,
    ToggleLyrics,
    ToggleDebug,
    ScrollUp,
    ScrollDown,
    Quit,
}

impl Action {
    fn all() -> &'static [( &'static str, &'static [&'static str], Action)] {
        use Action::*;
        &[
            ("play_pause",        &["space"],          PlayPause),
            ("next_track",        &["n"],              NextTrack),
            ("prev_track",        &["p"],              PrevTrack),
            ("volume_up",         &["+", "="],         VolumeUp),
            ("volume_down",       &["-"],              VolumeDown),
            ("seek_forward",      &["right"],          SeekForward),
            ("seek_backward",     &["left"],           SeekBackward),
            ("toggle_shuffle",    &["s"],              ToggleShuffle),
            ("cycle_repeat",      &["r"],              CycleRepeat),
            ("toggle_radio",      &["R"],              ToggleRadio),
            ("recommendations",   &["alt+r"],          GetRecommendations),
            ("like_track",        &["l"],              LikeTrack),
            ("add_to_queue",      &["a"],              AddToQueue),
            ("remove_from_queue", &["delete"],         RemoveFromQueue),
            ("sort_tracks",       &["o"],              SortTracks),
            ("nav_up",            &["up", "k"],        NavUp),
            ("nav_down",          &["down", "j"],      NavDown),
            ("nav_first",         &["ctrl+up"],        NavFirst),
            ("nav_last",          &["ctrl+down"],      NavLast),
            ("tab_next",          &["tab"],            TabNext),
            ("tab_prev",          &["backtab"],        TabPrev),
            ("enter",             &["enter"],          Enter),
            ("back",              &["esc"],            Back),
            ("search",            &["/"],              Search),
            ("quick_search",      &["ctrl+f"],         QuickSearch),
            ("help",              &["?"],              Help),
            ("toggle_compact",    &["m"],              ToggleCompact),
            ("toggle_fullscreen", &["z"],              ToggleFullscreen),
            ("toggle_visualizer", &["v"],              ToggleVisualizer),
            ("toggle_lyrics",     &["y"],              ToggleLyrics),
            ("toggle_debug",      &["d"],              ToggleDebug),
            ("scroll_up",         &["pageup"],         ScrollUp),
            ("scroll_down",       &["pagedown"],       ScrollDown),
            ("quit",              &["q", "ctrl+c"],    Quit),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyId {
    Char(char),
    Space,
    Enter,
    Tab,
    BackTab,
    Esc,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub key: KeyId,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

fn parse_key_combo(s: &str) -> Option<KeyCombo> {
    let s = s.trim().to_lowercase();
    let parts: Vec<&str> = s.split('+').collect();

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key_str = "";

    for part in &parts {
        let p = part.trim();
        match p {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => key_str = p,
        }
    }

    if key_str.is_empty() {
        return None;
    }

    let key = match key_str {
        "space" => KeyId::Space,
        "enter" | "return" => KeyId::Enter,
        "tab" => KeyId::Tab,
        "backtab" => KeyId::BackTab,
        "esc" | "escape" => KeyId::Esc,
        "backspace" | "bs" => KeyId::Backspace,
        "delete" | "del" => KeyId::Delete,
        "up" => KeyId::Up,
        "down" => KeyId::Down,
        "left" => KeyId::Left,
        "right" => KeyId::Right,
        "home" => KeyId::Home,
        "end" => KeyId::End,
        "pageup" | "pgup" => KeyId::PageUp,
        "pagedown" | "pgdn" => KeyId::PageDown,
        s if s.starts_with('f') && s.len() > 1 => {
            let n: u8 = s[1..].parse().ok()?;
            if n < 1 || n > 12 { return None; }
            KeyId::F(n)
        }
        s => {
            let chars: Vec<char> = s.chars().collect();
            if chars.len() != 1 { return None; }
            let c = chars[0];
            if c.is_ascii_uppercase() {
                shift = true;
            }
            KeyId::Char(c.to_ascii_lowercase())
        }
    };

    Some(KeyCombo { key, ctrl, alt, shift })
}

fn key_combo_to_string(kc: &KeyCombo) -> String {
    let mut parts = Vec::new();
    if kc.ctrl { parts.push("Ctrl"); }
    if kc.alt { parts.push("Alt"); }
    if kc.shift { parts.push("Shift"); }
    let key = match &kc.key {
        KeyId::Char(c) => {
            if kc.shift { c.to_ascii_uppercase().to_string() }
            else { c.to_string() }
        }
        KeyId::Space => "Space".into(),
        KeyId::Enter => "Enter".into(),
        KeyId::Tab => "Tab".into(),
        KeyId::BackTab => "Shift+Tab".into(),
        KeyId::Esc => "Esc".into(),
        KeyId::Backspace => "BS".into(),
        KeyId::Delete => "Del".into(),
        KeyId::Up => "↑".into(),
        KeyId::Down => "↓".into(),
        KeyId::Left => "←".into(),
        KeyId::Right => "→".into(),
        KeyId::Home => "Home".into(),
        KeyId::End => "End".into(),
        KeyId::PageUp => "PgUp".into(),
        KeyId::PageDown => "PgDn".into(),
        KeyId::F(n) => format!("F{n}"),
    };
    parts.push(&key);
    parts.join("+")
}

#[derive(Debug, Deserialize)]
struct KeybindsToml {
    playback: Option<HashMap<String, KeySpec>>,
    navigation: Option<HashMap<String, KeySpec>>,
    modes: Option<HashMap<String, KeySpec>>,
    actions: Option<HashMap<String, KeySpec>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KeySpec {
    Single(String),
    Multiple(Vec<String>),
}

fn name_to_action(name: &str) -> Option<Action> {
    Action::all().iter().find(|(n, _, _)| *n == name).map(|(_, _, a)| *a)
}

pub fn keybinds_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("isi-music").join("keybinds.toml")
}

pub struct Keybinds {
    action_for: HashMap<KeyCombo, Action>,
    pub keys_for: HashMap<Action, Vec<KeyCombo>>,
}

impl Keybinds {
    pub fn defaults() -> Self {
        let mut keys_for: HashMap<Action, Vec<KeyCombo>> = HashMap::new();
        let mut action_for: HashMap<KeyCombo, Action> = HashMap::new();

        for (_, key_strs, action) in Action::all() {
            let mut combos = Vec::new();
            for ks in *key_strs {
                if let Some(kc) = parse_key_combo(ks) {
                    action_for.insert(kc.clone(), *action);
                    combos.push(kc);
                }
            }
            keys_for.insert(*action, combos);
        }

        Keybinds { action_for, keys_for }
    }

    pub fn load() -> Self {
        let path = keybinds_path();
        let defaults = Self::defaults();

        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&PathBuf::from(".")));
            if let Ok(toml_str) = toml::to_string_pretty(&KeybindsTomlOutput::from_defaults()) {
                let _ = std::fs::write(&path, toml_str);
            }
            return defaults;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return defaults,
        };

        let parsed: KeybindsToml = match toml::from_str(&content) {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to parse keybinds.toml: {e}");
                return defaults;
            }
        };

        let mut result = defaults;

        for (section_name, section) in [
            ("playback", parsed.playback),
            ("navigation", parsed.navigation),
            ("modes", parsed.modes),
            ("actions", parsed.actions),
        ] {
            let Some(section) = section else { continue; };
            for (name, spec) in section {
                let Some(action) = name_to_action(&name) else {
                    warn!("Unknown action '{}' in [{}] section", name, section_name);
                    continue;
                };

                let key_strs = match spec {
                    KeySpec::Single(s) => vec![s],
                    KeySpec::Multiple(v) => v,
                };

                // Remove old key combos for this action (from defaults)
                if let Some(old_combos) = result.keys_for.get(&action) {
                    for kc in old_combos {
                        result.action_for.remove(kc);
                    }
                }

                let mut new_combos = Vec::new();
                for ks in &key_strs {
                    if let Some(kc) = parse_key_combo(ks) {
                        result.action_for.insert(kc.clone(), action);
                        new_combos.push(kc);
                    } else {
                        warn!("Invalid key combo '{}' for action '{}'", ks, name);
                    }
                }
                result.keys_for.insert(action, new_combos);
            }
        }

        result
    }

    pub fn lookup(&self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        if let KeyCode::Char(c) = code {
            let shift = modifiers.contains(KeyModifiers::SHIFT) || c.is_uppercase();
            let c_lower = c.to_ascii_lowercase();
            let combo = KeyCombo {
                key: KeyId::Char(c_lower),
                ctrl: modifiers.contains(KeyModifiers::CONTROL),
                alt: modifiers.contains(KeyModifiers::ALT),
                shift,
            };
            return self.action_for.get(&combo).copied();
        }

        let key = match code {
            KeyCode::Char(' ') => KeyId::Space,
            KeyCode::Enter => KeyId::Enter,
            KeyCode::Tab => KeyId::Tab,
            KeyCode::BackTab => KeyId::BackTab,
            KeyCode::Esc => KeyId::Esc,
            KeyCode::Backspace => KeyId::Backspace,
            KeyCode::Delete => KeyId::Delete,
            KeyCode::Up => KeyId::Up,
            KeyCode::Down => KeyId::Down,
            KeyCode::Left => KeyId::Left,
            KeyCode::Right => KeyId::Right,
            KeyCode::Home => KeyId::Home,
            KeyCode::End => KeyId::End,
            KeyCode::PageUp => KeyId::PageUp,
            KeyCode::PageDown => KeyId::PageDown,
            KeyCode::F(n) => KeyId::F(n),
            _ => return None,
        };

        let combo = KeyCombo {
            key,
            ctrl: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            shift: modifiers.contains(KeyModifiers::SHIFT),
        };

        self.action_for.get(&combo).copied()
    }

    pub fn format_help_text(&self) -> Vec<(String, Vec<String>)> {
        let categories: &[(&str, &[Action])] = &[
            ("Playback", &[
                Action::PlayPause, Action::NextTrack, Action::PrevTrack,
                Action::VolumeUp, Action::VolumeDown,
                Action::SeekForward, Action::SeekBackward,
                Action::ToggleShuffle, Action::CycleRepeat,
                Action::ToggleRadio, Action::GetRecommendations,
            ]),
            ("Navigation", &[
                Action::NavUp, Action::NavDown, Action::NavFirst, Action::NavLast,
                Action::TabNext, Action::TabPrev,
                Action::Enter, Action::Back,
            ]),
            ("Modes", &[
                Action::Search, Action::QuickSearch, Action::Help,
                Action::ToggleCompact, Action::ToggleFullscreen,
                Action::ToggleVisualizer, Action::ToggleLyrics,
                Action::ToggleDebug, Action::ScrollUp, Action::ScrollDown,
            ]),
            ("Actions", &[
                Action::LikeTrack, Action::AddToQueue, Action::RemoveFromQueue,
                Action::SortTracks, Action::Quit,
            ]),
        ];

        let mut result = Vec::new();
        for (cat_name, actions) in categories {
            let entries: Vec<String> = actions.iter().filter_map(|a| {
                let keys = self.keys_for.get(a)?;
                if keys.is_empty() { return None; }
                let key_strs: Vec<String> = keys.iter().map(key_combo_to_string).collect();
                let action_name = format!("{:?}", a);
                let spaced = action_name
                    .chars()
                    .flat_map(|c| {
                        if c.is_uppercase() { vec![' ', c] } else { vec![c] }
                    })
                    .collect::<String>()
                    .trim()
                    .to_string();
                Some(format!("{}  {}", key_strs.join("/"), spaced))
            }).collect();
            if !entries.is_empty() {
                result.push((cat_name.to_string(), entries));
            }
        }
        result
    }
}

struct KeybindsTomlOutput {
    playback: Vec<(String, Vec<String>)>,
    navigation: Vec<(String, Vec<String>)>,
    modes: Vec<(String, Vec<String>)>,
    actions: Vec<(String, Vec<String>)>,
}

impl KeybindsTomlOutput {
    fn from_defaults() -> Self {
        let mut playback = Vec::new();
        let mut navigation = Vec::new();
        let mut modes = Vec::new();
        let mut actions = Vec::new();

        for (name, keys, action) in Action::all() {
            let key_strs: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
            let entry = (name.to_string(), key_strs);
            match action {
                Action::PlayPause | Action::NextTrack | Action::PrevTrack
                | Action::VolumeUp | Action::VolumeDown
                | Action::SeekForward | Action::SeekBackward
                | Action::ToggleShuffle | Action::CycleRepeat
                | Action::ToggleRadio | Action::GetRecommendations
                | Action::LikeTrack => playback.push(entry),
                Action::NavUp | Action::NavDown | Action::NavFirst | Action::NavLast
                | Action::TabNext | Action::TabPrev
                | Action::Enter | Action::Back => navigation.push(entry),
                Action::Search | Action::QuickSearch | Action::Help
                | Action::ToggleCompact | Action::ToggleFullscreen
                | Action::ToggleVisualizer | Action::ToggleLyrics
                | Action::ToggleDebug | Action::ScrollUp | Action::ScrollDown => modes.push(entry),
                Action::AddToQueue | Action::RemoveFromQueue | Action::SortTracks
                | Action::Quit => actions.push(entry),
            }
        }

        Self { playback, navigation, modes, actions }
    }
}

impl Serialize for KeybindsTomlOutput {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(4))?;

        let serialize_section = |map: &mut <S as serde::Serializer>::SerializeMap, name: &str, entries: &[(String, Vec<String>)]| -> Result<(), S::Error> {
            if entries.is_empty() { return Ok(()); }
            let mut section = std::collections::BTreeMap::new();
            for (k, v) in entries {
                let val = if v.len() == 1 {
                    toml::Value::String(v[0].clone())
                } else {
                    toml::Value::Array(v.iter().map(|s| toml::Value::String(s.clone())).collect())
                };
                section.insert(k.clone(), val);
            }
            map.serialize_entry(name, &section)?;
            Ok(())
        };

        serialize_section(&mut map, "playback", &self.playback)?;
        serialize_section(&mut map, "navigation", &self.navigation)?;
        serialize_section(&mut map, "modes", &self.modes)?;
        serialize_section(&mut map, "actions", &self.actions)?;

        map.end()
    }
}

#[allow(dead_code)]
pub struct KeybindsWatcher {
    pub rx: mpsc::Receiver<Keybinds>,
    stop: Arc<AtomicBool>,
}

impl KeybindsWatcher {
    pub fn watch() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let path = keybinds_path();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        std::thread::spawn(move || {
            let mut last_content = std::fs::read_to_string(&path).unwrap_or_default();
            loop {
                if stop_clone.load(Ordering::Relaxed) { break; }
                std::thread::sleep(Duration::from_millis(500));
                if let Ok(current_content) = std::fs::read_to_string(&path) {
                    if current_content != last_content {
                        std::thread::sleep(Duration::from_millis(50));
                        let new = Keybinds::load();
                        if tx.send(new).is_ok() {
                            last_content = current_content;
                        }
                    }
                }
            }
        });

        Ok(KeybindsWatcher { rx, stop })
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
