use anyhow::Result;
use std::io::{self, Write};

pub(crate) fn prompt(label: &str) -> String {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    buf.trim().to_string()
}

pub(crate) const RED: &str = "\x1b[1;31m";
pub(crate) const YELLOW: &str = "\x1b[1;33m";
pub(crate) const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
pub(crate) const GREEN: &str = "\x1b[32m";

const BOX_W: usize = 63;

fn visible_len(s: &str) -> usize {
    let mut count = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            count += 1;
        }
    }
    count
}

fn box_line(content: &str) -> String {
    let padding_len = BOX_W.saturating_sub(visible_len(content));
    let padding: String = " ".repeat(padding_len);
    format!("{RED}│{RESET}{content}{padding}{RED}│{RESET}")
}

macro_rules! bl {
    ($str:expr) => {
        box_line(&$str)
    };
}

pub(crate) async fn run_lastfm_setup(cfg: &mut crate::config::AppConfig) -> Result<()> {
    println!("\n{RED}┌───────────────────────────────────────────────────────────────┐{RESET}");
    println!(
        "{}",
        bl!(format!("  {BOLD}Last.fm Integration Setup{RESET}"))
    );
    println!("{RED}├───────────────────────────────────────────────────────────────┤{RESET}");
    println!(
        "{}",
        bl!(format!(
            "  isi-music will open Last.fm authorization in your browser"
        ))
    );
    println!(
        "{}",
        bl!(format!(
            "  Just log in to your Last.fm account and authorize the app"
        ))
    );
    println!("{}", bl!(""));
    println!(
        "{}",
        bl!(format!(
            "  {YELLOW}{BOLD}No API credentials needed!{RESET} {YELLOW}isi-music handles everything.{RESET}"
        ))
    );
    println!("{RED}└───────────────────────────────────────────────────────────────┘{RESET}\n");

    println!("Opening Last.fm authorization in your browser...");

    match crate::utils::lastfm::LastfmClient::authenticate_with_default().await {
        Ok(session_key) => {
            cfg.lastfm.session_key = Some(session_key);
            cfg.save()?;
            println!("Last.fm authentication successful!");
            println!("Last.fm scrobbling enabled!");
            println!();
        }
        Err(e) => {
            println!(" FAILED");
            println!("Error: {e:#}");
            println!("Skipping Last.fm setup.");
            println!();
        }
    }

    Ok(())
}

pub(crate) async fn run_spotify_setup(cfg: &mut crate::config::AppConfig) -> Result<()> {
    use crate::config;

    cfg.spotify.enabled = Some(true);
    println!("\n{RED}┌───────────────────────────────────────────────────────────────┐{RESET}");
    println!("{}", bl!(format!("  {BOLD}Spotify Setup{RESET}")));
    println!("{RED}├───────────────────────────────────────────────────────────────┤{RESET}");
    println!(
        "{}",
        bl!("  A custom Client ID is used for Spotify Web API requests.")
    );
    println!(
        "{}",
        bl!("  The built-in client is reserved for librespot streaming.")
    );
    println!("{}", bl!(""));
    println!(
        "{}",
        bl!(format!(
            "  {YELLOW}If you hit the 5-user limit on the shared client ID,{RESET}"
        ))
    );
    println!(
        "{}",
        bl!(format!(
            "  {YELLOW}leave it blank for streaming-only mode.{RESET}"
        ))
    );
    println!("{RED}└───────────────────────────────────────────────────────────────┘{RESET}\n");

    let existing_id = cfg
        .get_client_id()
        .filter(|s| !s.is_empty() && s != "your_client_id_here");

    let prompt_text = if let Some(ref id) = existing_id {
        let masked = if id.len() > 8 {
            format!("{}…{}", &id[..4], &id[id.len().saturating_sub(4)..])
        } else {
            id.clone()
        };
        format!("Web API Client ID (Enter to keep {masked}, blank for streaming-only): ")
    } else {
        "Web API Client ID (press Enter for streaming-only): ".to_string()
    };

    let client_id = prompt(&prompt_text);
    let trimmed = client_id.trim().to_string();

    if !trimmed.is_empty() {
        if trimmed.len() < 10 {
            println!(
                "  {YELLOW}That doesn't look like a valid Client ID, but I'll save it anyway.{RESET}"
            );
        }
        cfg.spotify.client_id = Some(trimmed);
        cfg.save()?;
        let saved_path = config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "config.toml".to_string());
        println!("  {GREEN}[OK]{RESET}  Saved to {saved_path}\n");
    } else if existing_id.is_some() {
        cfg.save()?;
        println!("  {GREEN}[OK]{RESET}  Keeping existing Client ID.\n");
    } else {
        cfg.spotify.client_id = None;
        cfg.save()?;
        config::clear_refresh_token();
        config::clear_streaming_refresh_token();
        println!("  {GREEN}[OK]{RESET}  Streaming-only mode selected.\n");
    }

    let authenticate = loop {
        let v = prompt("Authenticate with Spotify now? (Y/n): ");
        let v = v.trim().to_lowercase();
        if v.is_empty() || v == "y" || v == "yes" {
            break true;
        }
        if v == "n" || v == "no" {
            break false;
        }
    };

    if authenticate {
        let has_web_api_client = cfg.get_client_id().is_some();

        if has_web_api_client {
            if let Some(cid) = cfg.get_client_id() {
                println!("  Starting authorization (2 steps: Web API + streaming)\n");
                match crate::spotify::auth::SpotifyAuth::authenticate_both(&cid).await {
                    Ok((web_api_refresh, streaming_refresh)) => {
                        config::save_refresh_token(&web_api_refresh);
                        config::save_streaming_refresh_token(&streaming_refresh);
                        println!("\n  {GREEN}[OK]{RESET}  Web API + Streaming authenticated.\n");
                    }
                    Err(e) => {
                        if e.to_string().contains("Authentication cancelled") {
                            return Err(e);
                        }
                        println!("  {YELLOW}Authentication failed: {e}{RESET}");
                        println!("  You can authenticate later by launching isi-music normally.\n");
                    }
                }
            }
        } else {
            let result = crate::spotify::auth::SpotifyAuth::authenticate_with_client_id(
                config::OFFICIAL_CLIENT_ID,
            )
            .await;

            match result {
                Ok((_access_token, refresh_token, _expires_in)) => {
                    config::save_streaming_refresh_token(&refresh_token);
                    println!("  {GREEN}[OK]{RESET}  Streaming authenticated.\n");
                }
                Err(e) => {
                    if e.to_string().contains("Authentication cancelled") {
                        return Err(e);
                    }
                    println!("  {YELLOW}Authentication failed: {e}{RESET}");
                    println!("  You can authenticate later by launching isi-music normally.\n");
                }
            }
        }
    } else {
        println!("  You can authenticate later by launching isi-music normally.\n");
    }

    Ok(())
}

pub(crate) fn print_help() {
    println!(
        "\
isi-music: terminal music player for Spotify and local files

USAGE
  isi-music               Launch the TUI player
  isi-music [COMMAND]

TUI KEYBINDINGS"
    );

    let kb = crate::keybinds::Keybinds::load();
    for (category, entries) in kb.format_help_text() {
        println!("  {category}:");
        for entry in entries {
            println!("    {entry}");
        }
    }

    let config_path_str = crate::config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "config.toml".to_string());
    let log_path_str = crate::config::log_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "isi-music.log".to_string());

    println!(
        "\
DAEMON MODE
  isi-music --daemon                 Start daemon in background
  isi-music --quit-daemon            Stop the daemon

PLAYBACK CONTROL
  isi-music --toggle                 Play / pause
  isi-music --next                   Next track
  isi-music --prev                   Previous track
  isi-music --vol+                   Volume +5 %
  isi-music --vol-                   Volume -5 %
  isi-music --status                 Show current track and progress

QUEUE MANAGEMENT
  isi-music --playlists               List your playlists (ID + name)
  isi-music --play <ID|name>         Load playlist by ID or name (fuzzy match)
  isi-music --liked [--limit N]      Load liked songs (limit to N, default 100)
  isi-music --search <query>         Search within loaded queue
  isi-music --search-global <query>  Search globally on Spotify
  isi-music --ls [--limit N]         List loaded tracks (paginate with --limit)
  isi-music --play-id <N>            Play track by ID (from --ls)

DEVICE CONTROL
  isi-music --devices                List available Spotify Connect devices
  isi-music --device <name>          Transfer playback to device (fuzzy match)

NOTE: Audio quality and crossfade changes require daemon restart (not yet supported via CLI).

SETUP
  isi-music setup                    First config (wizard)
  isi-music setup-spotify            Configure Spotify streaming
  isi-music setup-lastfm             Configure Last.fm scrobbling
  isi-music local-only               Disable Spotify, use local files only
  isi-music doctor                   Diagnose common issues
  isi-music update                   Update to the latest release
  isi-music --clear-logs             Clear the log file

SPOTIFY STREAMING
  Run `isi-music setup-spotify` to configure Spotify.
  Your Client ID is used for the Web API.
  The built-in librespot Client ID is used only for streaming.
  Custom apps use: http://127.0.0.1:8888/callback
  Streaming uses: http://127.0.0.1:8898/login
  Both OAuth flows run during setup/startup.
  Local-only mode: set [spotify] enabled = false in config.toml
  (no auth prompt, no Spotify sections, no streaming).

LAST.FM SCROBBLING
  Run `isi-music setup-lastfm` to enable scrobbling.
  No API credentials needed: isi-music handles everything.
  The setup will open the Last.fm authorization page in your browser.
  Once configured, isi-music will:
    - Send \"now playing\" updates when a track starts
    - Scrobble tracks after 50% of the song has been played

FILES
  Config   {config_path_str}
  Log      {log_path_str}"
    );

    #[cfg(unix)]
    println!("  Socket   $XDG_RUNTIME_DIR/isi-music.sock");
    #[cfg(windows)]
    println!("  Pipe     \\\\.\\pipe\\isi-music");
}
