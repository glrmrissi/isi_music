use std::path::{Path, PathBuf};

use anyhow::Result;
use console::{Term, style};
use dialoguer::{Confirm, Input, theme::ColorfulTheme};

pub(super) fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

pub(super) fn header(term: &Term, title: &str) {
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

pub(super) fn optional_input(prompt: &str) -> Result<Option<String>> {
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

pub(super) fn confirm_overwrite(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let overwrite = Confirm::with_theme(&theme())
        .with_prompt(format!("{} already exists. Overwrite?", path.display()))
        .default(false)
        .interact()?;
    Ok(overwrite)
}

pub(super) fn detect_music_dir() -> Option<String> {
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
        if let Some(p) = expanded
            && p.exists()
        {
            return p.to_str().map(|s| s.to_string());
        }
    }

    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Music")))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
}
