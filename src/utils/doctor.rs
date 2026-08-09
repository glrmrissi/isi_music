use std::path::Path;

use crate::config;

const RED: &str = "\x1b[1;31m";
const GREEN: &str = "\x1b[1;32m";
const YELLOW: &str = "\x1b[1;33m";
const CYAN: &str = "\x1b[1;36m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

enum Status {
    Ok,
    Warn,
    Fail,
}

struct CheckResult {
    name: String,
    status: Status,
    detail: String,
    hint: Option<String>,
}

impl CheckResult {
    fn ok(name: &str, detail: &str) -> Self {
        Self {
            name: name.to_string(),
            status: Status::Ok,
            detail: detail.to_string(),
            hint: None,
        }
    }

    fn warn(name: &str, detail: &str, hint: &str) -> Self {
        Self {
            name: name.to_string(),
            status: Status::Warn,
            detail: detail.to_string(),
            hint: Some(hint.to_string()),
        }
    }

    fn fail(name: &str, detail: &str, hint: &str) -> Self {
        Self {
            name: name.to_string(),
            status: Status::Fail,
            detail: detail.to_string(),
            hint: Some(hint.to_string()),
        }
    }
}

fn print_result(r: &CheckResult) {
    let (icon, color) = match r.status {
        Status::Ok => ("[OK]", GREEN),
        Status::Warn => ("[WARN]", YELLOW),
        Status::Fail => ("[FAIL]", RED),
    };
    println!("  {color}{icon}{RESET}  {BOLD}{}{RESET}  {DIM}{}{RESET}", r.name, r.detail);
    if let Some(hint) = &r.hint {
        println!("        {CYAN}→ {hint}{RESET}");
    }
}

/// Run all diagnostic checks and print a report.
pub async fn run() -> anyhow::Result<()> {
    println!();
    println!("  {BOLD}{GREEN}isi-music{RESET} {BOLD}- Diagnostics{RESET}");
    println!("  {}", "-".repeat(50));
    println!();

    let mut results = Vec::new();

    results.push(check_config());

    results.push(check_spotify_client_id());
    results.push(check_spotify_refresh_token());

    results.push(check_local_music_dir());

    results.push(check_lastfm());

    results.push(check_nerd_font());

    #[cfg(target_os = "linux")]
    {
        results.push(check_audio_deps_linux());
        results.push(check_terminal_color_linux());
    }

    #[cfg(windows)]
    {
        results.push(check_windows_terminal());
    }

    results.push(check_network().await);

    let mut ok_count = 0;
    let mut warn_count = 0;
    let mut fail_count = 0;

    for r in &results {
        print_result(r);
        match r.status {
            Status::Ok => ok_count += 1,
            Status::Warn => warn_count += 1,
            Status::Fail => fail_count += 1,
        }
    }

    println!();
    println!("  {}", "-".repeat(50));
    print!("  ");
    if ok_count > 0 {
        print!("{GREEN}{ok_count} OK{RESET}  ");
    }
    if warn_count > 0 {
        print!("{YELLOW}{warn_count} WARN{RESET}  ");
    }
    if fail_count > 0 {
        print!("{RED}{fail_count} FAIL{RESET}");
    }
    println!();
    println!();

    if fail_count > 0 {
        println!("  {YELLOW}Some checks failed. See the hints above to fix them.{RESET}");
    } else if warn_count > 0 {
        println!("  {GREEN}All critical checks passed.{RESET} {DIM}Warnings are optional.{RESET}");
    } else {
        println!("  {GREEN}All checks passed! You're good to go.{RESET}");
    }
    println!();

    Ok(())
}

// Individual checks

fn check_config() -> CheckResult {
    match config::config_path() {
        Ok(path) => {
            if !path.exists() {
                return CheckResult::warn(
                    "Config file",
                    &format!("not found at {}", path.display()),
                    "Run: isi-music setup",
                );
            }
            match config::AppConfig::load() {
                Ok(cfg) => {
                    let sections: Vec<&str> = [
                        cfg.spotify.client_id.is_some().then_some("spotify"),
                        cfg.local.music_dir.is_some().then_some("local"),
                        cfg.lastfm.session_key.is_some().then_some("lastfm"),
                        cfg.discord.enabled.unwrap_or(false).then_some("discord"),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    CheckResult::ok(
                        "Config file",
                        &format!("{} ({})", path.display(), sections.join(", ")),
                    )
                }
                Err(e) => CheckResult::fail(
                    "Config file",
                    &format!("parse error: {e}"),
                    &format!("Fix the TOML syntax in {}", path.display()),
                ),
            }
        }
        Err(_) => CheckResult::fail(
            "Config file",
            "could not determine config path",
            "Set XDG_CONFIG_HOME or HOME environment variable",
        ),
    }
}

fn check_spotify_client_id() -> CheckResult {
    match config::AppConfig::load() {
        Ok(cfg) => {
            let cid = cfg.get_client_id();
            match cid {
                Some(id) if !id.is_empty() && id != "your_client_id_here" => {
                    CheckResult::ok("Spotify Client ID", &format!("set ({}…)", &id[..8.min(id.len())]))
                }
                _ => CheckResult::warn(
                    "Spotify Client ID",
                    "not configured",
                    "Run: isi-music setup-spotify",
                ),
            }
        }
        Err(_) => CheckResult::warn(
            "Spotify Client ID",
            "could not load config",
            "Run: isi-music setup",
        ),
    }
}

fn check_spotify_refresh_token() -> CheckResult {
    match config::load_refresh_token() {
        Some(_) => CheckResult::ok("Spotify token", "refresh token present"),
        None => CheckResult::warn(
            "Spotify token",
            "no refresh token (not authenticated)",
            "Run: isi-music setup-spotify to authenticate",
        ),
    }
}

fn check_local_music_dir() -> CheckResult {
    match config::AppConfig::load() {
        Ok(cfg) => {
            match &cfg.local.music_dir {
                Some(dir) if !dir.is_empty() => {
                    let expanded = if dir.starts_with("~/") {
                        dirs::home_dir()
                            .map(|h| h.join(&dir[2..]))
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| dir.clone())
                    } else {
                        dir.clone()
                    };
                    if Path::new(&expanded).exists() {
                        CheckResult::ok("Local music dir", &expanded)
                    } else {
                        CheckResult::warn(
                            "Local music dir",
                            &format!("directory does not exist: {expanded}"),
                            "Set [local] music_dir in config.toml to an existing folder",
                        )
                    }
                }
                _ => CheckResult::warn(
                    "Local music dir",
                    "not set",
                    "Add [local] music_dir = \"~/Music\" to config.toml",
                ),
            }
        }
        Err(_) => CheckResult::warn(
            "Local music dir",
            "could not load config",
            "Run: isi-music setup",
        ),
    }
}

fn check_lastfm() -> CheckResult {
    match config::AppConfig::load() {
        Ok(cfg) => {
            if cfg.lastfm.session_key.is_some() {
                CheckResult::ok("Last.fm", "scrobbling enabled")
            } else {
                CheckResult::warn(
                    "Last.fm",
                    "not configured (optional)",
                    "Run: isi-music setup-lastfm to enable scrobbling",
                )
            }
        }
        Err(_) => CheckResult::warn("Last.fm", "could not load config", "Run: isi-music setup"),
    }
}

fn check_nerd_font() -> CheckResult {
    // Heuristic: check if common Nerd Font env vars are set, or if the
    // terminal reports a Nerd Font. This is unreliable — many terminals
    // don't expose this info.
    let env_hints = ["TERM_FONT", "TERMINAL_FONT", "WT_FONT"];
    for var in env_hints {
        if let Ok(val) = std::env::var(var) {
            if val.to_lowercase().contains("nerd") || val.to_lowercase().contains("nf") {
                return CheckResult::ok("Nerd Font", &format!("detected via {var}: {val}"));
            }
        }
    }

    // On Linux, check fc-list
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("fc-list").output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.to_lowercase().contains("nerd") {
                let count = stdout.lines().filter(|l| l.to_lowercase().contains("nerd")).count();
                return CheckResult::ok("Nerd Font", &format!("{count} Nerd Font family(ies) found via fc-list"));
            }
        }
        CheckResult::warn(
            "Nerd Font",
            "no Nerd Font detected (heuristic check)",
            "Install a Nerd Font from https://www.nerdfonts.com/ and set it in your terminal",
        )
    }

    #[cfg(not(target_os = "linux"))]
    {
        CheckResult::warn(
            "Nerd Font",
            "cannot auto-detect on this platform",
            "Ensure your terminal uses a Nerd Font (e.g. 'FiraCode Nerd Font')",
        )
    }
}

#[cfg(target_os = "linux")]
fn check_audio_deps_linux() -> CheckResult {
    let mut missing = Vec::new();

    // Check for ALSA
    let alsa_ok = std::path::Path::new("/usr/lib/x86_64-linux-gnu/libasound.so.2").exists()
        || std::path::Path::new("/usr/lib/libasound.so.2").exists()
        || std::process::Command::new("ldconfig")
            .arg("-p")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("libasound.so"))
            .unwrap_or(false);
    if !alsa_ok {
        missing.push("libasound2 (ALSA)");
    }

    // Check for PulseAudio
    let pulse_ok = std::process::Command::new("ldconfig")
        .arg("-p")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("libpulse.so"))
        .unwrap_or(false);
    if !pulse_ok {
        missing.push("libpulse0 (PulseAudio)");
    }

    if missing.is_empty() {
        CheckResult::ok("Audio deps", "ALSA + PulseAudio found")
    } else {
        CheckResult::fail(
            "Audio deps",
            &format!("missing: {}", missing.join(", ")),
            "Install: sudo apt install libasound2 libpulse0 (or your distro equivalent)",
        )
    }
}

#[cfg(target_os = "linux")]
fn check_terminal_color_linux() -> CheckResult {
    let term = std::env::var("TERM").unwrap_or_default();
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();

    if colorterm == "truecolor" || colorterm == "24bit" {
        CheckResult::ok("Terminal color", &format!("24-bit color (TERM={term}, COLORTERM={colorterm})"))
    } else if term.contains("256") {
        CheckResult::warn(
            "Terminal color",
            &format!("256-color only (TERM={term})"),
            "Set COLORTERM=truecolor in your shell profile for 24-bit color",
        )
    } else {
        CheckResult::warn(
            "Terminal color",
            &format!("unknown color support (TERM={term})"),
            "Use a modern terminal like Kitty, Alacritty, or Windows Terminal",
        )
    }
}

#[cfg(windows)]
fn check_windows_terminal() -> CheckResult {
    let wt_session = std::env::var("WT_SESSION").is_ok();
    if wt_session {
        CheckResult::ok("Terminal", "Windows Terminal detected")
    } else {
        CheckResult::warn(
            "Terminal",
            "not Windows Terminal (WT_SESSION not set)",
            "For best experience, use Windows Terminal with a Nerd Font",
        )
    }
}

async fn check_network() -> CheckResult {
    let timeout = std::time::Duration::from_secs(5);
    let result = tokio::time::timeout(
        timeout,
        tokio::net::TcpStream::connect("api.spotify.com:443"),
    )
    .await;

    match result {
        Ok(Ok(_)) => CheckResult::ok("Network", "can reach api.spotify.com:443"),
        Ok(Err(e)) => CheckResult::fail(
            "Network",
            &format!("cannot reach Spotify API: {e}"),
            "Check your internet connection or firewall",
        ),
        Err(_) => CheckResult::fail(
            "Network",
            "connection to api.spotify.com timed out (5s)",
            "Check your internet connection or proxy settings",
        ),
    }
}
