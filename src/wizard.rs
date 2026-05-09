use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use console::{Term, style};
use dialoguer::{Confirm, Input, Password, Select, theme::ColorfulTheme};

use crate::config::{AppConfig, LastfmConfig};
use crate::theme::Theme;

struct Preset {
    name: &'static str,
    border_active: &'static str,
    border_inactive: &'static str,
    highlight_bg: &'static str,
    text_primary: &'static str,
    accent: &'static str,
    preview: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "Default (green)",
        border_active: "#00ff00",
        border_inactive: "#555555",
        highlight_bg: "#282828",
        text_primary: "#ffffff",
        accent: "#00ff00",
        preview: "  ▐\x1b[32m████\x1b[0m▌ green on dark  ",
    },
    Preset {
        name: "Catppuccin Mocha",
        border_active: "#cba6f7",
        border_inactive: "#585b70",
        highlight_bg: "#313244",
        text_primary: "#cdd6f4",
        accent: "#89b4fa",
        preview: "  ▐\x1b[35m████\x1b[0m▌ lavender        ",
    },
    Preset {
        name: "Gruvbox Dark",
        border_active: "#d79921",
        border_inactive: "#504945",
        highlight_bg: "#3c3836",
        text_primary: "#ebdbb2",
        accent: "#fe8019",
        preview: "  ▐\x1b[33m████\x1b[0m▌ warm amber      ",
    },
    Preset {
        name: "Nord",
        border_active: "#88c0d0",
        border_inactive: "#4c566a",
        highlight_bg: "#3b4252",
        text_primary: "#e5e9f0",
        accent: "#5e81ac",
        preview: "  ▐\x1b[36m████\x1b[0m▌ arctic blue     ",
    },
    Preset {
        name: "Rose Pine",
        border_active: "#eb6f92",
        border_inactive: "#524f67",
        highlight_bg: "#26233a",
        text_primary: "#e0def4",
        accent: "#f6c177",
        preview: "  ▐\x1b[31m████\x1b[0m▌ muted rose      ",
    },
    Preset {
        name: "Tokyo Night",
        border_active: "#7aa2f7",
        border_inactive: "#3b4261",
        highlight_bg: "#1f2335",
        text_primary: "#c0caf5",
        accent: "#9ece6a",
        preview: "  ▐\x1b[34m████\x1b[0m▌ blue / neon     ",
    },
    Preset {
        name: "Dracula",
        border_active: "#bd93f9",
        border_inactive: "#44475a",
        highlight_bg: "#282a36",
        text_primary: "#f8f8f2",
        accent: "#ff79c6",
        preview: "  ▐\x1b[35m████\x1b[0m▌ purple / pink   ",
    },
    Preset {
        name: "Monochrome",
        border_active: "#ffffff",
        border_inactive: "#666666",
        highlight_bg: "#1a1a1a",
        text_primary: "#cccccc",
        accent: "#999999",
        preview: "  ▐\x1b[37m████\x1b[0m▌ greyscale       ",
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
    let candidates: &[&str] = &[
        "~/Music",
        "~/music",
        "~/Downloads/Music",
        "/mnt/music",
        "/media/music",
    ];

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

fn quick_start(term: &Term) -> Result<(AppConfig, Option<Theme>)> {
    header(term, "— Quick Start");

    println!("  {}", style("Generating a default configuration…").dim());
    println!();

    let mut cfg = AppConfig::default();

    let music_dir = detect_music_dir();
    if let Some(ref dir) = music_dir {
        println!(
            "  {}  {}",
            style("✓").green(),
            style(format!("Music directory detected: {dir}")).dim()
        );
        cfg.local.music_dir = Some(dir.clone());
    } else {
        println!(
            "  {}  {}",
            style("⚠").yellow(),
            style("Could not auto-detect music directory.").dim()
        );
        println!(
            "      {}",
            style("Set [local] music_dir in ~/.config/isi-music/config.toml later.").dim()
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
        style("Supported formats: mp3, flac, ogg, wav, aiff, m4a, opus").dim()
    );
    println!();

    let auto = detect_music_dir();
    let default_dir = auto.clone().unwrap_or_else(|| "~/Music".to_string());

    let raw: String = Input::with_theme(&theme())
        .with_prompt("Music directory")
        .default(default_dir.clone())
        .allow_empty(true)
        .interact_text()?;

    let music_dir = raw.trim().to_string();
    cfg.local.music_dir = if music_dir.is_empty() {
        None
    } else {
        Some(music_dir)
    };

    header(term, "— Step 2 / 4 · Discord Rich Presence");

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

    header(term, "— Step 3 / 4 · Last.fm Scrobbling");

    println!("  Scrobble tracks you listen to on Last.fm.\n");
    println!(
        "  {}",
        style("Create an API app at https://www.last.fm/api/account/create").dim()
    );
    println!();

    let lastfm_enabled = Confirm::with_theme(&theme())
        .with_prompt("Configure Last.fm scrobbling?")
        .default(false)
        .interact()?;

    if lastfm_enabled {
        let api_key: String = Input::with_theme(&theme())
            .with_prompt("API Key")
            .validate_with(|s: &String| {
                if s.len() == 32 {
                    Ok(())
                } else {
                    Err("API Key must be 32 characters")
                }
            })
            .interact_text()?;

        let api_secret: String = Password::with_theme(&theme())
            .with_prompt("API Secret (hidden)")
            .interact()?;

        cfg.lastfm.api_key = Some(api_key.clone());
        cfg.lastfm.api_secret = Some(api_secret.clone());

        println!();
        println!(
            "  {}",
            style("Running Last.fm auth flow — a browser window will open.").dim()
        );

        let token = crate::lastfm::LastfmClient::get_auth_token(&api_key).await?;
        let auth_url = format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            api_key, token
        );

        if open::that(&auth_url).is_err() {
            println!("\n  Visit: {}", style(&auth_url).cyan().underlined());
        }

        println!("\n  Press ENTER after authorising on Last.fm…");
        let mut _buf = String::new();
        io::stdin().read_line(&mut _buf)?;

        print!("  Finalising… ");
        io::stdout().flush().ok();

        match crate::lastfm::LastfmClient::get_session(&api_key, &api_secret, &token).await {
            Ok(session_key) => {
                cfg.lastfm.session_key = Some(session_key);
                println!("{}", style("✓").green());
            }
            Err(e) => {
                println!("{}", style("✗ failed").red());
                println!("  {}", style(format!("{e:#}")).dim());
                println!("  You can run `isi-music setup-lastfm` later.");
                cfg.lastfm = LastfmConfig::default();
            }
        }
    }

    header(term, "— Step 4 / 4 · Colour Theme");

    let theme_choice = Confirm::with_theme(&theme())
        .with_prompt("Choose a colour preset now?")
        .default(true)
        .interact()?;

    let chosen_theme = if theme_choice {
        Some(pick_preset(term)?)
    } else {
        None
    };

    Ok((cfg, chosen_theme))
}

fn template_gallery(term: &Term) -> Result<(AppConfig, Option<Theme>)> {
    header(term, "— Template Gallery");

    println!("  Choose a colour preset for your theme:\n");

    let chosen_theme = pick_preset(term)?;
    let cfg = {
        let mut c = AppConfig::default();
        c.discord.enabled = Some(false);
        c.local.music_dir = detect_music_dir();
        c
    };

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
        style("✓").green(),
        style(format!("Preset selected: {}", preset.name)).bold()
    );

    let t = Theme {
        border_active: parse_hex(preset.border_active),
        border_inactive: parse_hex(preset.border_inactive),
        highlight_bg: parse_hex(preset.highlight_bg),
        text_primary: parse_hex(preset.text_primary),
        accent_color: parse_hex(preset.accent),
        ..Theme::default()
    };

    Ok(t)
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
        0 => quick_start(&term)?,
        1 => interactive_setup(&term).await?,
        2 => template_gallery(&term)?,
        _ => unreachable!(),
    };

    let config_path = crate::config::config_path()?;
    let theme_path = Theme::get_path().context("Could not determine theme path")?;

    println!();
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
            style("✓").green(),
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
                style("✓").green(),
                style(theme_path.display()).dim()
            );
        }
    } else if chosen_theme.is_some() {
        println!("  {}  theme  skipped.", style("–").dim());
    }

    println!();
    println!(
        "  {}  All done! Run {} to start playing.",
        style("✓").bold().green(),
        style("isi-music").bold()
    );
    println!();

    Ok(())
}
