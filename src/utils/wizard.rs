// TODO: modularize this file (~660 lines) into smaller modules
use std::path::PathBuf;

use anyhow::{Context, Result};
use console::{Term, style};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};

use crate::config::{AppConfig, LastfmConfig};
use crate::utils::theme::{
    BorderConfig, BorderStyle, LayoutNode, SerializableConstraint, SerializableDirection, Theme,
    UiWidget, VisualizerStyle,
};

struct Preset {
    name: &'static str,
    border_active: &'static str,
    border_inactive: &'static str,
    highlight_bg: &'static str,
    text_primary: &'static str,
    text_secondary: &'static str,
    accent: &'static str,
    background: &'static str,
    status_bar: &'static str,
    preview: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "Neutral Dark",
        border_active: "#d0d0d0",
        border_inactive: "#777777",
        highlight_bg: "#2a2a2a",
        text_primary: "#e6e6e6",
        text_secondary: "#9e9e9e",
        accent: "#c4c4c4",
        background: "#141414",
        status_bar: "#1c1c1c",
        preview: "  ▐\x1b[37m████\x1b[0m▌ neutral dark     ",
    },
    Preset {
        name: "Catppuccin Mocha",
        border_active: "#cba6f7",
        border_inactive: "#585b70",
        highlight_bg: "#313244",
        text_primary: "#cdd6f4",
        text_secondary: "#a6adc8",
        accent: "#89b4fa",
        background: "#1e1e2e",
        status_bar: "#181825",
        preview: "  ▐\x1b[35m████\x1b[0m▌ lavender        ",
    },
    Preset {
        name: "Gruvbox Dark",
        border_active: "#d79921",
        border_inactive: "#504945",
        highlight_bg: "#3c3836",
        text_primary: "#ebdbb2",
        text_secondary: "#a89984",
        accent: "#fe8019",
        background: "#282828",
        status_bar: "#1d2021",
        preview: "  ▐\x1b[33m████\x1b[0m▌ warm amber      ",
    },
    Preset {
        name: "Nord",
        border_active: "#88c0d0",
        border_inactive: "#4c566a",
        highlight_bg: "#3b4252",
        text_primary: "#e5e9f0",
        text_secondary: "#81a1c1",
        accent: "#5e81ac",
        background: "#2e3440",
        status_bar: "#242933",
        preview: "  ▐\x1b[36m████\x1b[0m▌ arctic blue     ",
    },
    Preset {
        name: "Rose Pine",
        border_active: "#eb6f92",
        border_inactive: "#524f67",
        highlight_bg: "#26233a",
        text_primary: "#e0def4",
        text_secondary: "#908caa",
        accent: "#f6c177",
        background: "#191724",
        status_bar: "#1f1d2e",
        preview: "  ▐\x1b[31m████\x1b[0m▌ muted rose      ",
    },
    Preset {
        name: "Tokyo Night",
        border_active: "#7aa2f7",
        border_inactive: "#3b4261",
        highlight_bg: "#1f2335",
        text_primary: "#c0caf5",
        text_secondary: "#9aa5ce",
        accent: "#9ece6a",
        background: "#1a1b26",
        status_bar: "#16161e",
        preview: "  ▐\x1b[34m████\x1b[0m▌ blue / neon     ",
    },
    Preset {
        name: "Dracula",
        border_active: "#bd93f9",
        border_inactive: "#44475a",
        highlight_bg: "#282a36",
        text_primary: "#f8f8f2",
        text_secondary: "#6272a4",
        accent: "#ff79c6",
        background: "#1e1f29",
        status_bar: "#191a21",
        preview: "  ▐\x1b[35m████\x1b[0m▌ purple / pink   ",
    },
    Preset {
        name: "Monochrome",
        border_active: "#ffffff",
        border_inactive: "#666666",
        highlight_bg: "#1a1a1a",
        text_primary: "#cccccc",
        text_secondary: "#888888",
        accent: "#999999",
        background: "#111111",
        status_bar: "#222222",
        preview: "  ▐\x1b[37m████\x1b[0m▌ greyscale       ",
    },
    Preset {
        name: "Clean",
        border_active: "#5c5f77",
        border_inactive: "#3b3d4f",
        highlight_bg: "#1e1e2e",
        text_primary: "#cdd6f4",
        text_secondary: "#6c7086",
        accent: "#89b4fa",
        background: "#11111b",
        status_bar: "#181825",
        preview: "  ▐\x1b[34m░░░░\x1b[0m▌ minimal / clean ",
    },
];

fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

fn header(term: &Term, title: &str) {
    let _ = term.clear_screen();
    println!();
    println!(
        "  {} {}",
        style("isi-music").bold().green(),
        style(title).bold()
    );
    println!("  {}", style("─".repeat(50)).dim());
    println!();
}

fn optional_input(prompt: &str) -> Result<Option<String>> {
    let v: String = Input::with_theme(&theme())
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()?;
    Ok(if v.trim().is_empty() {
        None
    } else {
        Some(v.trim().to_string())
    })
}

fn confirm_overwrite(path: &PathBuf) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let overwrite = Confirm::with_theme(&theme())
        .with_prompt(format!("{} already exists. Overwrite?", path.display()))
        .default(false)
        .interact()?;
    Ok(overwrite)
}

fn detect_music_dir() -> Option<String> {
    let candidates: &[&str] = if cfg!(windows) {
        &["~/Music", "~/Downloads/Music", "~/Documents/Music"]
    } else {
        &[
            "~/Music",
            "~/music",
            "~/Downloads/Music",
            "/mnt/music",
            "/media/music",
        ]
    };

    for candidate in candidates {
        let expanded = if candidate.starts_with("~/") {
            dirs::home_dir().map(|h| h.join(&candidate[2..]))
        } else {
            Some(PathBuf::from(candidate))
        };
        if let Some(p) = expanded {
            if p.exists() {
                return p.to_str().map(|s| s.to_string());
            }
        }
    }

    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
}

async fn configure_spotify(cfg: &mut AppConfig) -> Result<()> {
    println!();
    println!("  {}", style("Configure Spotify").bold());
    println!();
    println!(
        "  {}",
        style("A custom Client ID is used for Spotify Web API requests.").dim()
    );
    println!(
        "  {}",
        style("The built-in client is reserved for librespot streaming.").dim()
    );
    println!();
    println!(
        "  {}",
        style("Leave the Client ID blank for streaming-only mode, or provide").dim()
    );
    println!(
        "  {}",
        style("your own Spotify Developer app Client ID instead.").dim()
    );
    println!();

    let use_custom = Confirm::with_theme(&theme())
        .with_prompt("Configure a Spotify Web API Client ID?")
        .default(false)
        .interact()?;

    if !use_custom {
        cfg.spotify.client_id = None;
        crate::config::clear_refresh_token();
        crate::config::clear_streaming_refresh_token();
    }

    if use_custom {
        let redirect_uri = "http://127.0.0.1:8888/callback";
        println!(
            "  {}  {}",
            style("[..]").cyan(),
            style("Opening Spotify Developer Dashboard in your browser...").dim()
        );
        let _ = open::that("https://developer.spotify.com/dashboard");

        let clipboard_msg = match arboard::Clipboard::new() {
            Ok(mut cb) => {
                if cb.set_text(redirect_uri).is_ok() {
                    "(already copied to your clipboard — just paste it)"
                } else {
                    "(copy it from below)"
                }
            }
            Err(_) => "(copy it from below)",
        };

        println!();
        println!(
            "  {}",
            style("Create a Spotify App to get your own Client ID:").dim()
        );
        println!(
            "  {}  {}",
            style("1.").cyan(),
            style("Click \"Create app\" (dashboard should be open in your browser)").dim()
        );
        println!(
            "  {}  {}",
            style("2.").cyan(),
            style("Give it any name & description, accept the terms").dim()
        );
        println!(
            "  {}  {}  {}",
            style("3.").cyan(),
            style("Add this Redirect URI:").dim(),
            style(clipboard_msg).yellow()
        );
        println!("  {}       {}", "", style(redirect_uri).yellow().bold());
        println!(
            "  {}  {}",
            style("4.").cyan(),
            style("Click \"Save\", then copy the Client ID and paste it below").dim()
        );
        println!();

        let client_id: String = Input::with_theme(&theme())
            .with_prompt("Spotify Client ID")
            .allow_empty(true)
            .interact_text()?;

        let trimmed = client_id.trim().to_string();
        if !trimmed.is_empty() {
            if trimmed.len() < 10 {
                println!(
                    "  {}  {}",
                    style("!").yellow(),
                    style(
                        "That doesn't look like a valid Client ID. It will be saved but may not work."
                    )
                    .dim()
                );
            }
            cfg.spotify.client_id = Some(trimmed);
        }
    }

    cfg.save()?;

    println!();
    let do_auth = Confirm::with_theme(&theme())
        .with_prompt("Authenticate with Spotify now? (opens browser)")
        .default(true)
        .interact()?;
    if do_auth {
        println!();
        println!(
            "  {}  {}",
            style("[..]").cyan(),
            style("Opening Spotify authorization in your browser...").dim()
        );
        let has_web_api_client = cfg.get_client_id().is_some();
        let result = if has_web_api_client {
            crate::spotify::auth::SpotifyAuth::authenticate().await
        } else {
            crate::spotify::auth::SpotifyAuth::authenticate_with_client_id(
                crate::config::OFFICIAL_CLIENT_ID,
            )
            .await
        };

        match result {
            Ok((_access_token, refresh_token, _expires_in)) => {
                if has_web_api_client {
                    crate::config::save_refresh_token(&refresh_token);
                    println!(
                        "  {}  {}",
                        style("[OK]").green(),
                        style("Web API authenticated.").bold()
                    );
                    println!(
                        "  {}",
                        style("Authenticating streaming with librespot...").dim()
                    );
                    crate::player::ensure_streaming_auth().await?;
                    println!(
                        "  {}  {}",
                        style("[OK]").green(),
                        style("Streaming authenticated.").bold()
                    );
                } else {
                    crate::config::save_streaming_refresh_token(&refresh_token);
                    println!(
                        "  {}  {}",
                        style("[OK]").green(),
                        style("Streaming authenticated.").bold()
                    );
                }
            }
            Err(e) => {
                if e.to_string().contains("Authentication cancelled") {
                    return Err(e);
                }
                println!("  {}  Authentication failed: {e}", style("[ERROR]").red());
                println!(
                    "  {}",
                    style("You can authenticate later by launching isi-music normally.").dim()
                );
            }
        }
    }

    Ok(())
}

async fn quick_start(term: &Term) -> Result<(AppConfig, Option<Theme>)> {
    header(term, "— Quick Start");

    println!("  {}", style("Generating a default configuration…").dim());
    println!();

    let mut cfg = AppConfig::default();

    let music_dir = detect_music_dir();
    if let Some(ref dir) = music_dir {
        println!(
            "  {}  {}",
            style("[OK]").green(),
            style(format!("Music directory detected: {dir}")).dim()
        );
        cfg.local.music_dir = Some(dir.clone());
    } else {
        println!(
            "  {}  {}",
            style("!").yellow(),
            style("Could not auto-detect music directory.").dim()
        );
        println!(
            "      {}",
            style("Set [local] music_dir in ~/.config/isi-music/config.toml later.").dim()
        );
    }

    let configure_now = Confirm::with_theme(&theme())
        .with_prompt("Authenticate with Spotify now? (opens browser, no Developer app needed)")
        .default(true)
        .interact()?;

    if configure_now {
        configure_spotify(&mut cfg).await?;
    } else {
        println!(
            "  {}",
            style("Skipping Spotify — you can run `isi-music setup-spotify` later.").dim()
        );
    }

    cfg.discord.enabled = Some(false);

    println!();
    println!(
        "  {}",
        style("Skipping Discord / Last.fm — run the interactive setup to configure them.").dim()
    );

    Ok((cfg, None))
}

async fn interactive_setup(term: &Term) -> Result<(AppConfig, Option<Theme>)> {
    let mut cfg = AppConfig::default();

    header(term, "— Step 1 / 4 · Local Music");

    println!("  Where is your local music library?\n");
    println!(
        "  {}",
        style("Supported formats: mp3, flac, opus, ogg, wav, aiff").dim()
    );
    println!();

    let auto = detect_music_dir();
    let default_dir = auto.clone().unwrap_or_else(|| "~/Music".to_string());

    let raw: String = Input::with_theme(&theme())
        .with_prompt("Music directory")
        .default(default_dir.clone())
        .allow_empty(true)
        .interact_text()?;

    // Windows paths with backslashes are invalid inside TOML basic strings
    // ("\U" starts a unicode escape) — forward slashes work everywhere
    let music_dir = if cfg!(windows) {
        raw.trim().replace('\\', "/")
    } else {
        raw.trim().to_string()
    };
    cfg.local.music_dir = if music_dir.is_empty() {
        None
    } else {
        Some(music_dir)
    };

    header(term, "— Step 2 / 5 · Spotify");

    println!("  Configure Spotify to stream music from your account.\n");

    let configure_now = Confirm::with_theme(&theme())
        .with_prompt("Configure Spotify now?")
        .default(true)
        .interact()?;

    if configure_now {
        configure_spotify(&mut cfg).await?;
    } else {
        println!(
            "  {}",
            style("Skipping — run `isi-music setup-spotify` later.").dim()
        );
    }

    header(term, "— Step 3 / 5 · Discord Rich Presence");

    println!("  Show the currently playing track in your Discord status.\n");

    let discord_enabled = Confirm::with_theme(&theme())
        .with_prompt("Enable Discord Rich Presence?")
        .default(false)
        .interact()?;

    cfg.discord.enabled = Some(discord_enabled);

    if discord_enabled {
        println!();
        println!(
            "  {}",
            style("Leave blank to use the default isi-music app ID.").dim()
        );
        cfg.discord.app_id = optional_input("Custom Discord App ID (optional)")?;
    }

    header(term, "— Step 4 / 5 · Last.fm Scrobbling");

    println!("  Scrobble tracks you listen to on Last.fm.\n");

    let lastfm_enabled = Confirm::with_theme(&theme())
        .with_prompt("Configure Last.fm scrobbling?")
        .default(false)
        .interact()?;

    if lastfm_enabled {
        println!();
        println!(
            "  {}",
            style("Running Last.fm auth flow — a browser window will open.").dim()
        );

        match crate::utils::lastfm::LastfmClient::authenticate_with_default().await {
            Ok(session_key) => {
                cfg.lastfm.session_key = Some(session_key);
                println!(
                    "  {}  Last.fm authentication successful!",
                    style("[OK]").green()
                );
            }
            Err(e) => {
                println!(
                    "  {}  Last.fm authentication failed: {}",
                    style("[ERROR]").red(),
                    style(format!("{e:#}")).dim()
                );
                println!(
                    "  {}",
                    style("You can run `isi-music setup-lastfm` later.").dim()
                );
                cfg.lastfm = LastfmConfig::default();
            }
        }
    }

    header(term, "— Step 5 / 5 · Colour Theme");

    let theme_choice = Confirm::with_theme(&theme())
        .with_prompt("Choose a colour preset now?")
        .default(true)
        .interact()?;

    let chosen_theme = if theme_choice {
        let mut t = pick_preset(term)?;
        let layout_name = pick_layout(term)?;
        apply_layout_to_theme(&mut t, layout_name);
        Some(t)
    } else {
        None
    };

    Ok((cfg, chosen_theme))
}

fn template_gallery(term: &Term) -> Result<(AppConfig, Option<Theme>)> {
    header(term, "— Template Gallery");

    println!("  Choose a colour preset for your theme:\n");

    let mut chosen_theme = pick_preset(term)?;
    let layout_name = pick_layout(term)?;
    apply_layout_to_theme(&mut chosen_theme, layout_name);

    let cfg = AppConfig::load().unwrap_or_else(|_| {
        let mut c = AppConfig::default();
        c.discord.enabled = Some(false);
        c.local.music_dir = detect_music_dir();
        c
    });

    Ok((cfg, Some(chosen_theme)))
}

fn pick_preset(_term: &Term) -> Result<Theme> {
    let items: Vec<String> = PRESETS
        .iter()
        .map(|p| format!("{:<25} {}", p.name, p.preview))
        .collect();

    let idx = Select::with_theme(&theme())
        .with_prompt("Colour preset")
        .items(&items)
        .default(0)
        .interact()?;

    let preset = &PRESETS[idx];

    println!(
        "\n  {} {}",
        style("[OK]").green(),
        style(format!("Colour: {}", preset.name)).bold()
    );

    let existing = Theme::load();
    let mut t = Theme {
        border_active: parse_hex(preset.border_active),
        border_inactive: parse_hex(preset.border_inactive),
        highlight_bg: parse_hex(preset.highlight_bg),
        text_primary: parse_hex(preset.text_primary),
        text_secondary: parse_hex(preset.text_secondary),
        accent_color: parse_hex(preset.accent),
        background: parse_hex(preset.background),
        status_bar: parse_hex(preset.status_bar),
        ..existing
    };

    if preset.name == "Clean" {
        t.background_panel = parse_hex("#181825");
        t.background_element = parse_hex("#1e1e2e");
        t.border_subtle = parse_hex("#3b3d4f");
        t.border_dimmest = parse_hex("#2a2b3d");
        t.primary = parse_hex("#89b4fa");
        t.success = parse_hex("#a6e3a1");
        t.error = parse_hex("#f38ba8");
        t.warning = parse_hex("#fab387");
        t.info = parse_hex("#89b4fa");
    }

    Ok(t)
}

struct LayoutPreset {
    name: &'static str,
    diagram: &'static str,
    build: fn(&mut Theme),
}

const LAYOUTS: &[LayoutPreset] = &[
    LayoutPreset {
        name: "Default",
        diagram: "  ┌─Header──┬─Help──┐\n  │ Lib │ Main │ Q │\n  │  PL │      │   │\n  │ Marq│ Prog │   │\n  └─────┴──────┴───┘",
        build: apply_default_layout,
    },
    LayoutPreset {
        name: "Clean",
        diagram: "  ┌──────── Search ────────┐\n  ├──────┬───────────┤\n  │ Lib  │           │\n  │      │  Main     │\n  │  PL  │           │\n  ├──────┴───────────┤\n  │ Marq │  Progress │\n  └──────┴───────────┘",
        build: apply_clean_layout,
    },
    LayoutPreset {
        name: "Focus",
        diagram: "  ┌──────── Search ────────┐\n  ├───────┬───────────┤\n  │ Album │           │\n  │  Art  │   Main    │\n  ├───────┤           │\n  │ Lib   │           │\n  ├───────┤           │\n  │  PL   │           │\n  ├───────┴───────────┤\n  │ Marq │  Progress │\n  └──────┴───────────┘",
        build: apply_focus_layout,
    },
    LayoutPreset {
        name: "Sidebar Right",
        diagram: "  ┌─Header────────────┐\n  │           │ Lib   │\n  │   Main    │  PL   │\n  │           │ Queue │\n  ├───────────┴───────┤\n  │ Marq │  Progress  │\n  └──────┴────────────┘",
        build: apply_sidebar_right_layout,
    },
    LayoutPreset {
        name: "Full Player",
        diagram: "  ┌──────── Search ────────┐\n  ├───────┬───────────┤\n  │ Album │           │\n  │  Art  │   Main    │\n  ├───────┤           │\n  │ Marq  │           │\n  ├───────┼───────────┤\n  │ Lib   │           │\n  ├───────┤  Progress │\n  │  PL   │           │\n  └───────┴───────────┘",
        build: apply_full_player_layout,
    },
    LayoutPreset {
        name: "Showcase",
        diagram: "  ┌──────── Search ────────┐\n  ├──────┬───────┬──────┤\n  │ Lib  │ Album │ Main │\n  │      │  Art  │      │\n  │  PL  │ Artist│ Queue│\n  │      ├───────┤      │\n  │      │ Lyrics│      │\n  │      ├───────┤      │\n  │      │  Viz  │      │\n  ├──────┴───────┴──────┤\n  │ Marq │   Progress   │\n  └──────┴──────────────┘",
        build: apply_showcase_layout,
    },
];

fn pick_layout(term: &Term) -> Result<&'static str> {
    header(term, "— Layout");

    println!("  Choose how widgets are arranged on screen.\n");
    println!(
        "  {}\n",
        style("Every widget can go anywhere — these are just starting points.").dim()
    );
    println!("  You can fine-tune the layout in theme.toml afterwards.\n");

    let items: Vec<String> = LAYOUTS
        .iter()
        .map(|l| format!("{:<16} {}", l.name, l.diagram.lines().next().unwrap_or("")))
        .collect();

    let idx = Select::with_theme(&theme())
        .with_prompt("Layout preset")
        .items(&items)
        .default(0)
        .interact()?;

    let layout = &LAYOUTS[idx];

    println!();
    for line in layout.diagram.lines() {
        println!("  {}", style(line).cyan());
    }
    println!();

    println!(
        "  {} {}",
        style("[OK]").green(),
        style(format!("Layout: {}", layout.name)).bold()
    );

    Ok(layout.name)
}

fn apply_layout_to_theme(t: &mut Theme, chosen: &str) {
    for l in LAYOUTS {
        if l.name == chosen {
            (l.build)(t);
            return;
        }
    }
}

fn apply_default_layout(_t: &mut Theme) {
    let default = Theme::default();
    _t.layout_tree = default.layout_tree;
    _t.compact_layout = default.compact_layout;
    _t.borders = default.borders;
    _t.show_ascii_art = true;
}

fn apply_clean_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = false;

    let mut borders = HashMap::new();
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };

    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Queue, no_border());
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let sidebar = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(7),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::Library), leaf(UiWidget::Playlists)]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(22),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![sidebar, leaf(UiWidget::MainContent)]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_focus_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = true;

    let mut borders = HashMap::new();
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };
    let rounded = |focused: &str| BorderConfig {
        style: BorderStyle::Rounded,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };

    borders.insert(UiWidget::AlbumArt, rounded("#89b4fa"));
    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Queue, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let left_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(10),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::AlbumArt),
            leaf(UiWidget::Library),
            leaf(UiWidget::Playlists),
        ]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(28),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![left_col, leaf(UiWidget::MainContent)]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_sidebar_right_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = true;

    let mut borders = HashMap::new();
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };

    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Queue, left_bar("#89b4fa"));
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let right_sidebar = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(7),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(8),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Library),
            leaf(UiWidget::Playlists),
            leaf(UiWidget::Queue),
        ]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(3),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Header),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Fill(1),
                    SerializableConstraint::Percentage(25),
                ]),
                widget: None,
                children: Some(vec![leaf(UiWidget::MainContent), right_sidebar]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_full_player_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = true;

    let mut borders = HashMap::new();
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };
    let rounded = |focused: &str| BorderConfig {
        style: BorderStyle::Rounded,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };

    borders.insert(UiWidget::AlbumArt, rounded("#89b4fa"));
    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Queue, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let left_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(10),
            SerializableConstraint::Length(3),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::AlbumArt),
            leaf(UiWidget::Marquee),
            leaf(UiWidget::Library),
            leaf(UiWidget::Playlists),
        ]),
    };

    let right_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::MainContent), leaf(UiWidget::Progress)]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(28),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![left_col, right_col]),
            },
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_showcase_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = false;
    t.visualizer.style = VisualizerStyle::BlockBars;

    let mut borders = HashMap::new();
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };

    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Queue, left_bar("#89b4fa"));
    borders.insert(UiWidget::AlbumArt, no_border());
    borders.insert(UiWidget::AlbumArtWithInfo, no_border());
    borders.insert(UiWidget::Visualizer, no_border());
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Search, left_bar("#89b4fa"));
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let left_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(7),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::Library), leaf(UiWidget::Playlists)]),
    };

    let center_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Percentage(65),
            SerializableConstraint::Length(3),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::AlbumArtWithInfo),
            leaf(UiWidget::Lyrics),
            leaf(UiWidget::Visualizer),
        ]),
    };

    let right_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(8),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::MainContent), leaf(UiWidget::Queue)]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(20),
                    SerializableConstraint::Fill(1),
                    SerializableConstraint::Percentage(28),
                ]),
                widget: None,
                children: Some(vec![left_col, center_col, right_col]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn leaf(widget: UiWidget) -> LayoutNode {
    LayoutNode {
        widget: Some(widget),
        direction: None,
        constraints: None,
        children: None,
    }
}

fn bottom_bar() -> LayoutNode {
    LayoutNode {
        direction: Some(SerializableDirection::Horizontal),
        constraints: Some(vec![
            SerializableConstraint::Percentage(30),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::Marquee), leaf(UiWidget::Progress)]),
    }
}

fn parse_hex(hex: &str) -> ratatui::style::Color {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return ratatui::style::Color::Rgb(r, g, b);
        }
    }
    ratatui::style::Color::White
}

fn save_config(cfg: &AppConfig) -> Result<()> {
    let path = crate::config::config_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let toml = toml::to_string_pretty(cfg).context("Failed to serialise config")?;
    std::fs::write(&path, toml).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn save_theme(theme: &Theme) -> Result<()> {
    let path = Theme::get_path().context("Could not determine theme path")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let toml = toml::to_string_pretty(theme).context("Failed to serialise theme")?;
    std::fs::write(&path, toml).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub async fn run() -> Result<()> {
    let term = Term::stdout();

    let _ = term.clear_screen();
    println!();
    println!("  {}", style("isi-music  /  Setup Wizard").bold().green());
    println!("  {}", style("─".repeat(50)).dim());
    println!();
    println!("  {}", style("Choose how you want to get started:").dim());
    println!();

    let options = &[
        "Quick Start    — sensible defaults, auto-detect music dir",
        "Interactive    — step-by-step: music dir, Discord, Last.fm, theme",
        "Template       — pick a colour preset, skip everything else",
    ];

    let path_idx = Select::with_theme(&theme())
        .with_prompt("Setup mode")
        .items(options)
        .default(0)
        .interact()?;

    println!();

    let (cfg, chosen_theme) = match path_idx {
        0 => quick_start(&term).await?,
        1 => interactive_setup(&term).await?,
        2 => template_gallery(&term)?,
        _ => unreachable!(),
    };

    let config_path = crate::config::config_path()?;
    let theme_path = Theme::get_path().context("Could not determine theme path")?;

    println!();
    if path_idx == 2 {
        println!("  {} Will write:", style("→").cyan());
        println!("      {}", style(theme_path.display()).cyan());
        println!();

        let write_theme = if theme_path.exists() {
            confirm_overwrite(&theme_path)?
        } else {
            true
        };

        if write_theme {
            if let Some(ref t) = chosen_theme {
                save_theme(t)?;
                println!(
                    "  {}  theme  saved → {}",
                    style("[OK]").green(),
                    style(theme_path.display()).dim()
                );
            }
        } else {
            println!("  {}  theme  skipped.", style("–").dim());
        }
    } else {
        println!("  {} Will write:", style("→").cyan());
        println!("      {}", style(config_path.display()).cyan());
        if chosen_theme.is_some() {
            println!("      {}", style(theme_path.display()).cyan());
        }
        println!();

        let write_config = if config_path.exists() {
            confirm_overwrite(&config_path)?
        } else {
            true
        };

        let write_theme = chosen_theme.is_some()
            && if theme_path.exists() {
                confirm_overwrite(&theme_path)?
            } else {
                true
            };

        if write_config {
            save_config(&cfg)?;
            println!(
                "  {}  config saved → {}",
                style("[OK]").green(),
                style(config_path.display()).dim()
            );
        } else {
            println!("  {}  config skipped.", style("–").dim());
        }

        if write_theme {
            if let Some(ref t) = chosen_theme {
                save_theme(t)?;
                println!(
                    "  {}  theme  saved → {}",
                    style("[OK]").green(),
                    style(theme_path.display()).dim()
                );
            }
        } else if chosen_theme.is_some() {
            println!("  {}  theme  skipped.", style("–").dim());
        }
    }

    println!();
    println!(
        "  {}  All done! Run {} to start playing.",
        style("[OK]").bold().green(),
        style("isi-music").bold()
    );
    println!();

    Ok(())
}
