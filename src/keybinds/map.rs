use crossterm::event::{KeyCode, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;

use super::actions::Action;
use super::combo::{KeyCombo, KeyId, key_combo_to_string, parse_key_combo};
use super::schema::{KeySpec, KeybindsToml, KeybindsTomlOutput, name_to_action};

pub fn keybinds_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("isi-music").join("keybinds.toml")
}

pub struct Keybinds {
    pub(crate) action_for: HashMap<KeyCombo, Action>,
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

        Keybinds {
            action_for,
            keys_for,
        }
    }

    pub fn load() -> Self {
        let path = keybinds_path();
        let defaults = Self::defaults();

        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&PathBuf::from(".")));
            if let Ok(toml_str) = toml::to_string_pretty(&KeybindsTomlOutput::from_defaults()) {
                let _ = crate::config::write_atomic(&path, &toml_str);
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
                tracing::warn!("Failed to parse keybinds.toml: {e}");
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
            let Some(section) = section else {
                continue;
            };
            for (name, spec) in section {
                let Some(action) = name_to_action(&name) else {
                    tracing::warn!("Unknown action '{}' in [{}] section", name, section_name);
                    continue;
                };

                let key_strs = match spec {
                    KeySpec::Single(s) => vec![s],
                    KeySpec::Multiple(v) => v,
                };

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
                        tracing::warn!("Invalid key combo '{}' for action '{}'", ks, name);
                    }
                }
                result.keys_for.insert(action, new_combos);
            }
        }

        result
    }

    pub fn lookup(&self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        if matches!(code, KeyCode::Char(' ')) {
            let combo = KeyCombo {
                key: KeyId::Space,
                ctrl: modifiers.contains(KeyModifiers::CONTROL),
                alt: modifiers.contains(KeyModifiers::ALT),
                shift: modifiers.contains(KeyModifiers::SHIFT),
            };
            return self.action_for.get(&combo).copied();
        }
        if let KeyCode::Char(c) = code {
            let c_lower = c.to_ascii_lowercase();
            let is_alpha = c_lower.is_ascii_alphabetic();
            let shift = if is_alpha {
                modifiers.contains(KeyModifiers::SHIFT) || c.is_uppercase()
            } else {
                modifiers.contains(KeyModifiers::SHIFT)
            };
            let combo = KeyCombo {
                key: KeyId::Char(c_lower),
                ctrl: modifiers.contains(KeyModifiers::CONTROL),
                alt: modifiers.contains(KeyModifiers::ALT),
                shift,
            };
            if let Some(a) = self.action_for.get(&combo).copied() {
                return Some(a);
            }
            if !is_alpha {
                let combo_no_shift = KeyCombo {
                    shift: false,
                    ..combo
                };
                if let Some(a) = self.action_for.get(&combo_no_shift).copied() {
                    return Some(a);
                }
            }
            return None;
        }

        let key = match code {
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

        let shift = match key {
            KeyId::BackTab => false,
            _ => modifiers.contains(KeyModifiers::SHIFT),
        };
        let combo = KeyCombo {
            key,
            ctrl: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            shift,
        };

        self.action_for.get(&combo).copied()
    }

    pub fn format_help_text(&self) -> Vec<(String, Vec<String>)> {
        let categories: &[(&str, &[Action])] = &[
            (
                "Playback",
                &[
                    Action::PlayPause,
                    Action::NextTrack,
                    Action::PrevTrack,
                    Action::VolumeUp,
                    Action::VolumeDown,
                    Action::SeekForward,
                    Action::SeekBackward,
                    Action::SeekMiddle,
                    Action::ToggleShuffle,
                    Action::CycleRepeat,
                    Action::ToggleRadio,
                    Action::GetRecommendations,
                ],
            ),
            (
                "Navigation",
                &[
                    Action::NavUp,
                    Action::NavDown,
                    Action::NavFirst,
                    Action::NavLast,
                    Action::NavMiddle,
                    Action::TabNext,
                    Action::TabPrev,
                    Action::Enter,
                    Action::Back,
                    Action::FocusLibrary,
                    Action::FocusPlaylists,
                    Action::FocusTracks,
                    Action::FocusQueue,
                    Action::JumpToPlaying,
                ],
            ),
            (
                "Modes",
                &[
                    Action::Search,
                    Action::QuickSearch,
                    Action::Help,
                    Action::ToggleCompact,
                    Action::ToggleFullscreen,
                    Action::ToggleVisualizer,
                    Action::ToggleLyrics,
                    Action::ToggleDebug,
                    Action::ToggleBreadcrumb,
                    Action::ScrollUp,
                    Action::ScrollDown,
                    Action::OptionsPanel,
                ],
            ),
            (
                "Actions",
                &[
                    Action::LikeTrack,
                    Action::AddToQueue,
                    Action::RemoveFromQueue,
                    Action::SortTracks,
                    Action::CopyTrackLink,
                    Action::AddToPlaylist,
                    Action::RemoveFromPlaylist,
                    Action::DeletePlaylist,
                    Action::CommandPrompt,
                    Action::Quit,
                ],
            ),
        ];

        let mut result = Vec::new();
        for (cat_name, actions) in categories {
            let entries: Vec<String> = actions
                .iter()
                .filter_map(|a| {
                    let keys = self.keys_for.get(a)?;
                    if keys.is_empty() {
                        return None;
                    }
                    let key_strs: Vec<String> = keys.iter().map(key_combo_to_string).collect();
                    let action_name = format!("{:?}", a);
                    let spaced = action_name
                        .chars()
                        .flat_map(|c| {
                            if c.is_uppercase() {
                                vec![' ', c]
                            } else {
                                vec![c]
                            }
                        })
                        .collect::<String>()
                        .trim()
                        .to_string();
                    Some(format!("{}  {}", key_strs.join("/"), spaced))
                })
                .collect();
            if !entries.is_empty() {
                result.push((cat_name.to_string(), entries));
            }
        }
        result
    }
}
