mod helpers;
mod layouts;
mod presets;
mod spotify;

use anyhow::{Context, Result};
use console::{Term, style};
use dialoguer::{Confirm, Select};

use crate::config::{AppConfig, LastfmConfig};
use crate::utils::theme::Theme;

use helpers::{confirm_overwrite, detect_music_dir, header, optional_input, theme as dialog_theme};
use layouts::{apply_layout_to_theme, pick_layout};
use presets::PRESETS;
use spotify::configure_spotify;

fn pick_preset(_term: &Term) -> Result<Theme> {
    let items: Vec<String> = PRESETS
        .iter()
        .map(|p| format!("{:<25} {}", p.name, p.preview))
        .collect();

    let idx = Select::with_theme(&dialog_theme())
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

    let configure_now = Confirm::with_theme(&dialog_theme())
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

    let raw: String = dialoguer::Input::with_theme(&dialog_theme())
        .with_prompt("Music directory")
        .default(default_dir.clone())
        .allow_empty(true)
        .interact_text()?;

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

    let configure_now = Confirm::with_theme(&dialog_theme())
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

    let discord_enabled = Confirm::with_theme(&dialog_theme())
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

    let lastfm_enabled = Confirm::with_theme(&dialog_theme())
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

    let theme_choice = Confirm::with_theme(&dialog_theme())
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

pub(super) fn parse_hex(hex: &str) -> ratatui::style::Color {
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

    let path_idx = Select::with_theme(&dialog_theme())
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
