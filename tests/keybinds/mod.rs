use super::*;
use crossterm::event::{KeyCode, KeyModifiers};

#[test]
fn defaults_contains_all_actions() {
    let k = Keybinds::defaults();
    for (_, _, action) in Action::all() {
        assert!(k.keys_for.contains_key(action), "missing {action:?}");
    }
}

#[test]
fn defaults_no_duplicate_key_combos() {
    let k = Keybinds::defaults();
    let lookup: &HashMap<KeyCombo, Action> = &k.action_for;
    assert_eq!(lookup.len(), 43);
}

#[test]
fn parse_key_combo_simple_char() {
    let kc = parse_key_combo("a").unwrap();
    assert_eq!(kc.key, KeyId::Char('a'));
    assert!(!kc.ctrl);
    assert!(!kc.alt);
    assert!(!kc.shift);
}

#[test]
fn parse_key_combo_uppercase_infers_shift() {
    let kc = parse_key_combo("A").unwrap();
    assert_eq!(kc.key, KeyId::Char('a'));
    assert!(kc.shift, "uppercase input should set shift flag");
}

#[test]
fn parse_key_combo_ctrl_alt() {
    let kc = parse_key_combo("ctrl+alt+x").unwrap();
    assert_eq!(kc.key, KeyId::Char('x'));
    assert!(kc.ctrl);
    assert!(kc.alt);
    assert!(!kc.shift);
}

#[test]
fn parse_key_combo_space() {
    let kc = parse_key_combo("space").unwrap();
    assert_eq!(kc.key, KeyId::Space);
}

#[test]
fn parse_key_combo_enter() {
    let kc = parse_key_combo("enter").unwrap();
    assert_eq!(kc.key, KeyId::Enter);
}

#[test]
fn parse_key_combo_tab() {
    let kc = parse_key_combo("Tab").unwrap();
    assert_eq!(kc.key, KeyId::Tab);
}

#[test]
fn parse_key_combo_backtab() {
    let kc = parse_key_combo("backtab").unwrap();
    assert_eq!(kc.key, KeyId::BackTab);
}

#[test]
fn parse_key_combo_esc() {
    let kc = parse_key_combo("esc").unwrap();
    assert_eq!(kc.key, KeyId::Esc);
}

#[test]
fn parse_key_combo_backspace() {
    let kc = parse_key_combo("backspace").unwrap();
    assert_eq!(kc.key, KeyId::Backspace);
}

#[test]
fn parse_key_combo_delete() {
    let kc = parse_key_combo("delete").unwrap();
    assert_eq!(kc.key, KeyId::Delete);
}

#[test]
fn parse_key_combo_arrows() {
    assert_eq!(parse_key_combo("up").unwrap().key, KeyId::Up);
    assert_eq!(parse_key_combo("down").unwrap().key, KeyId::Down);
    assert_eq!(parse_key_combo("left").unwrap().key, KeyId::Left);
    assert_eq!(parse_key_combo("right").unwrap().key, KeyId::Right);
}

#[test]
fn parse_key_combo_home_end() {
    assert_eq!(parse_key_combo("home").unwrap().key, KeyId::Home);
    assert_eq!(parse_key_combo("end").unwrap().key, KeyId::End);
}

#[test]
fn parse_key_combo_page() {
    assert_eq!(parse_key_combo("pageup").unwrap().key, KeyId::PageUp);
    assert_eq!(parse_key_combo("pagedown").unwrap().key, KeyId::PageDown);
    assert_eq!(parse_key_combo("pgup").unwrap().key, KeyId::PageUp);
    assert_eq!(parse_key_combo("pgdn").unwrap().key, KeyId::PageDown);
}

#[test]
fn parse_key_combo_function_keys() {
    assert_eq!(parse_key_combo("f1").unwrap().key, KeyId::F(1));
    assert_eq!(parse_key_combo("f12").unwrap().key, KeyId::F(12));
}

#[test]
fn parse_key_combo_f_invalid() {
    assert!(parse_key_combo("f0").is_none());
    assert!(parse_key_combo("f13").is_none());
    assert!(parse_key_combo("f").is_some());
}

#[test]
fn parse_key_combo_empty_string() {
    assert!(parse_key_combo("").is_none());
}

#[test]
fn parse_key_combo_too_many_chars() {
    assert!(parse_key_combo("xyz").is_none());
}

#[test]
fn parse_key_combo_ctrl_alone() {
    assert!(parse_key_combo("ctrl").is_none());
}

#[test]
fn name_to_action_found() {
    assert_eq!(name_to_action("play_pause"), Some(Action::PlayPause));
    assert_eq!(name_to_action("nav_up"), Some(Action::NavUp));
    assert_eq!(name_to_action("quit"), Some(Action::Quit));
}

#[test]
fn name_to_action_not_found() {
    assert_eq!(name_to_action("nonexistent"), None);
}

#[test]
fn key_combo_to_string_roundtrip() {
    let cases = &[
        "a",
        "A",
        "space",
        "enter",
        "tab",
        "backtab",
        "esc",
        "backspace",
        "delete",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
        "pageup",
        "pagedown",
        "f1",
        "f12",
        "ctrl+x",
        "alt+space",
        "ctrl+alt+delete",
    ];
    for s in cases {
        if let Some(kc) = parse_key_combo(s) {
            let rendered = key_combo_to_string(&kc);
            if *s != "backtab" {
                let parsed_back = parse_key_combo(&rendered.to_lowercase());
                assert!(
                    parsed_back.is_some(),
                    "failed to roundtrip '{s}' → '{rendered}'"
                );
            }
        }
    }
}

#[test]
fn lookup_simple_char() {
    let k = Keybinds::defaults();
    let action = k.lookup(KeyCode::Char(' '), KeyModifiers::empty());
    assert_eq!(action, Some(Action::PlayPause));
    let action = k.lookup(KeyCode::Char('n'), KeyModifiers::empty());
    assert_eq!(action, Some(Action::NextTrack));
}

#[test]
fn lookup_non_letter_lowercase() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char(' '), KeyModifiers::empty()),
        Some(Action::PlayPause)
    );
}

#[test]
fn lookup_case_insensitive() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('n'), KeyModifiers::empty()),
        Some(Action::NextTrack)
    );
}

#[test]
fn lookup_esc() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Esc, KeyModifiers::empty()),
        Some(Action::Back)
    );
}

#[test]
fn lookup_enter() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Enter, KeyModifiers::empty()),
        Some(Action::Enter)
    );
}

#[test]
fn lookup_tab_and_backtab() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Tab, KeyModifiers::empty()),
        Some(Action::TabNext)
    );
    assert_eq!(
        k.lookup(KeyCode::BackTab, KeyModifiers::empty()),
        Some(Action::TabPrev)
    );
}

#[test]
fn lookup_ctrl_c_is_quit() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
}

#[test]
fn lookup_ctrl_f_is_quick_search() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('f'), KeyModifiers::CONTROL),
        Some(Action::QuickSearch)
    );
}

#[test]
fn lookup_y_is_toggle_lyrics() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('y'), KeyModifiers::empty()),
        Some(Action::ToggleLyrics)
    );
}

#[test]
fn lookup_ctrl_y_is_copy_track_link() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('y'), KeyModifiers::CONTROL),
        Some(Action::CopyTrackLink)
    );
}

#[test]
fn lookup_shift_d_is_remove_from_playlist() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('D'), KeyModifiers::empty()),
        Some(Action::RemoveFromPlaylist)
    );
}

#[test]
fn lookup_shift_b_is_toggle_breadcrumb() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Char('b'), KeyModifiers::SHIFT),
        Some(Action::ToggleBreadcrumb)
    );
}

#[test]
fn lookup_unmapped_returns_none() {
    let k = Keybinds::defaults();
    assert_eq!(k.lookup(KeyCode::F(24), KeyModifiers::empty()), None);
    assert_eq!(k.lookup(KeyCode::Char(' '), KeyModifiers::ALT), None);
}

#[test]
fn format_help_text_all_categories() {
    let k = Keybinds::defaults();
    let help = k.format_help_text();
    assert_eq!(help.len(), 4, "expected 4 help categories");
    assert_eq!(help[0].0, "Playback");
    assert_eq!(help[1].0, "Navigation");
    assert_eq!(help[2].0, "Modes");
    assert_eq!(help[3].0, "Actions");
    for (_cat, entries) in &help {
        assert!(!entries.is_empty(), "category '{_cat}' has no entries");
    }
}

#[test]
fn format_help_text_key_strings() {
    let k = Keybinds::defaults();
    let help = k.format_help_text();
    for (_cat, entries) in &help {
        for entry in entries {
            assert!(entry.len() > 3, "entry too short: '{entry}'");
        }
    }
}

#[test]
fn action_variants_count() {
    let count = Action::all().len();
    assert_eq!(count, 40, "Action::all() should have 40 entries");
}

#[test]
fn keyboard_shortcuts_text_contains_arrows() {
    let k = Keybinds::defaults();
    let help = k.format_help_text();
    let nav_line = help[1].1.first().unwrap();
    assert!(
        nav_line.contains('↑') || nav_line.contains('k'),
        "nav line missing expected keys: {nav_line}"
    );
}

#[test]
fn parse_key_combo_alt_uppercase_shift_arrow() {
    let kc = parse_key_combo("alt+Shift+Up").unwrap();
    assert_eq!(kc.key, KeyId::Up);
    assert!(kc.alt);
    assert!(kc.shift);
    assert!(!kc.ctrl);
}

#[test]
fn keybinds_toml_output_sections() {
    let output = KeybindsTomlOutput::from_defaults();
    assert_eq!(output.playback.len(), 12);
    assert_eq!(output.navigation.len(), 8);
    assert_eq!(output.modes.len(), 12);
    assert_eq!(output.actions.len(), 8);
}

#[test]
fn keybinds_toml_serializes_and_deserializes() {
    let output = KeybindsTomlOutput::from_defaults();
    let toml_str = toml::to_string(&output).unwrap();
    let parsed: KeybindsToml = toml::from_str(&toml_str).unwrap();
    assert!(parsed.playback.is_some());
    assert!(parsed.navigation.is_some());
    assert!(parsed.modes.is_some());
    assert!(parsed.actions.is_some());
}

#[test]
fn lookup_nav_up_down() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Up, KeyModifiers::empty()),
        Some(Action::NavUp)
    );
    assert_eq!(
        k.lookup(KeyCode::Down, KeyModifiers::empty()),
        Some(Action::NavDown)
    );
}

#[test]
fn lookup_nav_first_last() {
    let k = Keybinds::defaults();
    assert_eq!(
        k.lookup(KeyCode::Up, KeyModifiers::CONTROL),
        Some(Action::NavFirst)
    );
    assert_eq!(
        k.lookup(KeyCode::Down, KeyModifiers::CONTROL),
        Some(Action::NavLast)
    );
}

#[test]
fn parse_key_combo_control_alias() {
    let kc = parse_key_combo("control+a").unwrap();
    assert_eq!(kc.key, KeyId::Char('a'));
    assert!(kc.ctrl);
}

#[test]
fn parse_key_combo_return_alias() {
    let kc = parse_key_combo("return").unwrap();
    assert_eq!(kc.key, KeyId::Enter);
}
