#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum KeyId {
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
pub(crate) struct KeyCombo {
    pub key: KeyId,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

pub(crate) fn parse_key_combo(s: &str) -> Option<KeyCombo> {
    let parts: Vec<&str> = s.trim().split('+').collect();

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key_str = "";

    for part in &parts {
        let p = part.trim();
        match &*p.to_lowercase() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => key_str = p,
        }
    }

    if key_str.is_empty() {
        return None;
    }

    let key = match &*key_str.to_lowercase() {
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
            if !(1..=12).contains(&n) {
                return None;
            }
            KeyId::F(n)
        }
        _ => {
            let chars: Vec<char> = key_str.chars().collect();
            if chars.len() != 1 {
                return None;
            }
            let c = chars[0];
            if c.is_ascii_uppercase() {
                shift = true;
            }
            const SHIFTED_PUNCT: &str = "!@#$%^&*()_+{}|:\"<>?~";
            if SHIFTED_PUNCT.contains(c) {
                shift = true;
            }
            KeyId::Char(c.to_ascii_lowercase())
        }
    };

    Some(KeyCombo {
        key,
        ctrl,
        alt,
        shift,
    })
}

pub(crate) fn key_combo_to_string(kc: &KeyCombo) -> String {
    let mut parts = Vec::new();
    if kc.ctrl {
        parts.push("Ctrl");
    }
    if kc.alt {
        parts.push("Alt");
    }
    if kc.shift && !matches!(&kc.key, KeyId::Char(_)) {
        parts.push("Shift");
    }
    let key = match &kc.key {
        KeyId::Char(c) => {
            if kc.shift {
                c.to_ascii_uppercase().to_string()
            } else {
                c.to_string()
            }
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
