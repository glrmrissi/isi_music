// TODO: modularize this file (~570 lines) into smaller modules
use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
#[cfg(feature = "album-art")]
use ratatui_image::picker::Picker;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;

mod app;
mod audio;
mod config;
mod daemon;
mod keybinds;
mod player;
mod spotify;
mod ui;
mod utils;

use app::App;

fn prompt(label: &str) -> String {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    buf.trim().to_string()
}

const RED: &str = "\x1b[1;31m";
const YELLOW: &str = "\x1b[1;33m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";

const BOX_W: usize = 63;

fn visible_len(s: &str) -> usize {
    let mut count = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            while let Some(c) = chars.next() {
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

async fn run_lastfm_setup(cfg: &mut config::AppConfig) -> Result<()> {
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

    match utils::lastfm::LastfmClient::authenticate_with_default().await {
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

async fn run_spotify_setup(cfg: &mut config::AppConfig) -> Result<()> {
    println!("\n{RED}┌───────────────────────────────────────────────────────────────┐{RESET}");
    println!("{}", bl!(format!("  {BOLD}Spotify Setup{RESET}")));
    println!("{RED}├───────────────────────────────────────────────────────────────┤{RESET}");
    println!(
        "{}",
        bl!("  To stream from Spotify you need your own Client ID:")
    );
    println!("{}", bl!(""));
    println!(
        "{}",
        bl!(format!(
            "  {BOLD}1.{RESET} Go to: {GREEN}https://developer.spotify.com/dashboard{RESET}"
        ))
    );
    println!(
        "{}",
        bl!(format!(
            "  {BOLD}2.{RESET} Click {BOLD}\"Create app\"{RESET}"
        ))
    );
    println!(
        "{}",
        bl!(format!("  {BOLD}3.{RESET} Give it any name & description"))
    );
    println!(
        "{}",
        bl!(format!("  {BOLD}4.{RESET} Add this Redirect URI:"))
    );
    println!(
        "{}",
        bl!(format!(
            "       {YELLOW}http://127.0.0.1:8888/callback{RESET}"
        ))
    );
    println!(
        "{}",
        bl!(format!("  {BOLD}5.{RESET} Click {BOLD}\"Save\"{RESET}"))
    );
    println!(
        "{}",
        bl!(format!(
            "  {BOLD}6.{RESET} Copy the {BOLD}Client ID{RESET} and paste it below"
        ))
    );
    println!("{}", bl!(""));
    println!(
        "{}",
        bl!(format!(
            "  {YELLOW}Uses PKCE - no client_secret needed!{RESET}"
        ))
    );
    println!("{RED}└───────────────────────────────────────────────────────────────┘{RESET}\n");

    let client_id = loop {
        let v = prompt("Client ID: ");
        if !v.is_empty() {
            if v.len() < 10 {
                println!(
                    "  {YELLOW}That doesn't look like a valid Client ID, but I'll save it anyway.{RESET}"
                );
            }
            break v;
        }
        println!("  {YELLOW}Client ID cannot be empty.{RESET}");
    };

    cfg.spotify.client_id = Some(client_id);
    cfg.save()?;
    println!("  {GREEN}[OK]{RESET}  Saved to ~/.config/isi-music/config.toml\n");

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
        let client_id = cfg.get_client_id().unwrap_or_default();
        if !client_id.is_empty() {
            match crate::spotify::auth::SpotifyAuth::authenticate().await {
                Ok((_access_token, refresh_token, _expires_in)) => {
                    crate::config::save_refresh_token(&refresh_token);
                    println!("  {GREEN}[OK]{RESET}  Authenticated successfully!\n");
                }
                Err(e) => {
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

fn print_help() {
    println!(
        "\
isi-music — terminal Spotify player

USAGE
  isi-music               Launch the TUI player
  isi-music [COMMAND]

TUI KEYBINDINGS"
    );

    let kb = keybinds::Keybinds::load();
    for (category, entries) in kb.format_help_text() {
        println!("  {category}:");
        for entry in entries {
            println!("    {entry}");
        }
    }

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
  isi-music --clear-logs             Clear the log file

SPOTIFY STREAMING
  Run `isi-music setup-spotify` to configure Spotify.
  The setup will:
    1. Guide you to create a Spotify app at developer.spotify.com
    2. Ask for your Client ID (no secret needed — uses PKCE)
    3. Authenticate with Spotify in your browser
    4. Save credentials to ~/.config/isi-music/config.toml
  Each user needs their own Client ID (5-user limit in Dev Mode).
  Set redirect URI to: http://127.0.0.1:8888/callback

LAST.FM SCROBBLING
  Run `isi-music setup-lastfm` to enable scrobbling.
  The setup will:
    1. Ask for your Last.fm API Key and API Secret
       (create an app at https://www.last.fm/api/account/create)
    2. Open the Last.fm authorization page in your browser
    3. Wait for you to authorize, then obtain a session key
    4. Save credentials to ~/.config/isi-music/config.toml
  Once configured, isi-music will:
    - Send \"now playing\" updates when a track starts
    - Scrobble tracks after 50% of the song has been played

FILES
  Config   ~/.config/isi-music/config.toml
  Log      ~/.local/share/isi-music/isi-music.log
"
    );

    #[cfg(unix)]
    println!("  Socket   $XDG_RUNTIME_DIR/isi-music.sock");
    #[cfg(windows)]
    println!("  Pipe     \\\\.\\pipe\\isi-music");
}

fn main() -> Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);

    if let Ok(env_path) = config::env_path() {
        dotenvy::from_path(&env_path).ok();
    }
    dotenvy::dotenv().ok();

    #[cfg(target_os = "linux")]
    unsafe {
        // buffers >=40KB use mmap, freed pages return to OS immediately
        libc::mallopt(libc::M_MMAP_THRESHOLD, 40960);
        // aggressive auto-trim
        libc::mallopt(libc::M_TRIM_THRESHOLD, 4096);
    }

    let mut cfg = config::AppConfig::load()?;
    let args: Vec<String> = std::env::args().collect();
    let arg1 = args.get(1).map(|s| s.as_str());

    if arg1 == Some("--daemon") {
        // Unix: classic double-step daemonize (fork + setsid + stdio to /dev/null)
        #[cfg(unix)]
        {
            let child_pid = unsafe { libc::fork() };
            if child_pid < 0 {
                anyhow::bail!("fork() failed");
            }
            if child_pid > 0 {
                println!("isi-music daemon started (PID {child_pid})");
                return Ok(());
            }
            unsafe {
                libc::setsid();
            }

            if let Ok(file) = OpenOptions::new().read(true).write(true).open("/dev/null") {
                let fd = file.as_raw_fd();
                unsafe {
                    libc::dup2(fd, libc::STDIN_FILENO);
                    libc::dup2(fd, libc::STDOUT_FILENO);
                    libc::dup2(fd, libc::STDERR_FILENO);
                }
            }

            return tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(daemon::run(cfg));
        }

        // Windows has no fork(): re-launch ourselves as a detached background process
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;

            let exe = std::env::current_exe()?;
            let child = std::process::Command::new(exe)
                .arg("--daemon-child")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
                .spawn()?;
            println!("isi-music daemon started (PID {})", child.id());
            return Ok(());
        }

        // Other platforms: run the daemon in the foreground
        #[cfg(not(any(unix, windows)))]
        {
            return tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(daemon::run(cfg));
        }
    }

    // Internal: spawned detached by `--daemon` on Windows
    if arg1 == Some("--daemon-child") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(daemon::run(cfg));
    }

    if arg1 == Some("--help") || arg1 == Some("-h") {
        print_help();
        return Ok(());
    }

    if arg1 == Some("--clear-logs") {
        let path = config::log_path()?;
        std::fs::write(&path, "")?;
        println!("Logs cleared: {}", path.display());
        return Ok(());
    }

    let ipc_cmd: Option<String> = match arg1 {
        Some("setup") => {
            return tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(utils::wizard::run());
        }

        Some(
            cmd @ ("--toggle" | "--next" | "--prev" | "--vol+" | "--vol-" | "--status"
            | "--quit-daemon" | "--playlists" | "--devices"),
        ) => {
            let c = cmd.trim_start_matches('-');
            Some(if c == "quit-daemon" {
                "quit".into()
            } else {
                c.into()
            })
        }
        Some("--device") => {
            let name = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --device <name>"))?;
            Some(format!("device {name}"))
        }
        Some("--ls") => {
            if args.get(2).is_some() && args.get(2) == Some(&"--limit".to_string()) {
                let limit = args
                    .get(3)
                    .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --ls --limit <N>"))?;
                Some(format!("ls --limit {limit}"))
            } else {
                Some("ls".into())
            }
        }
        Some("--play") => {
            let uri = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --play <ID|name>"))?;
            Some(format!("play {uri}"))
        }
        Some("--liked") => {
            if args.get(2).is_some() && args.get(2) == Some(&"--limit".to_string()) {
                let limit = args
                    .get(3)
                    .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --liked --limit <N>"))?;
                Some(format!("liked --limit {limit}"))
            } else {
                Some("liked".into())
            }
        }
        Some("--search") => {
            let query = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --search <query>"))?;
            Some(format!("search {query}"))
        }
        Some("--search-global") => {
            let query = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --search-global <query>"))?;
            Some(format!("search-global {query}"))
        }
        Some("--play-id") => {
            let id = args.get(2).ok_or_else(|| {
                anyhow::anyhow!("Usage: isi-music --play-id <N>  (see: isi-music --ls)")
            })?;
            Some(format!("play-id {id}"))
        }
        _ => None,
    };

    if let Some(cmd) = ipc_cmd {
        let response = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(utils::ipc::send_command(&cmd))?;
        println!("{response}");
        return Ok(());
    }

    if arg1 == Some("setup-spotify") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(run_spotify_setup(&mut cfg));
    }

    if arg1 == Some("setup-lastfm") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(run_lastfm_setup(&mut cfg));
    }

    let config_missing = crate::config::config_path()
        .map(|p| !p.exists())
        .unwrap_or(true);

    if config_missing {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(utils::wizard::run())?;
        // Re-load config after wizard writes it
        cfg = config::AppConfig::load()?;
    }

    if cfg.spotify.client_id.is_none() && std::env::var("SPOTIFY_CLIENT_ID").is_err() {
        println!();
        println!("  {YELLOW}Spotify not configured.{RESET} You can still use local files.");
        println!("  Run {BOLD}isi-music setup-spotify{RESET} to enable Spotify streaming.\n");
        let setup_now = loop {
            let v = prompt("Configure Spotify now? (Y/n): ");
            let v = v.trim().to_lowercase();
            if v.is_empty() || v == "y" || v == "yes" {
                break true;
            }
            if v == "n" || v == "no" {
                break false;
            }
        };
        if setup_now {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(run_spotify_setup(&mut cfg))?;
        }
    }

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            let log_path = config::log_path()?;
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)?;

            tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(log_file))
                .with_ansi(false)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive("isi_music=info".parse()?),
                )
                .init();

            let theme = utils::theme::Theme::load();
            let theme_rx = utils::theme::Theme::watch()?;
            let keybinds = keybinds::Keybinds::load();
            let keybinds_rx = keybinds::KeybindsWatcher::watch()?;
            #[cfg(feature = "album-art")]
            let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

            enable_raw_mode()?;
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;
            terminal.clear()?;

            let mut app = App::new(
                #[cfg(feature = "album-art")]
                picker,
                theme,
                theme_rx,
                keybinds,
                keybinds_rx,
            )
            .await?;
            let res = app.run(&mut terminal).await;

            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            terminal.show_cursor()?;

            if let Err(err) = res {
                eprintln!("[Error]: {err:?}");
            }
            Ok(())
        })
}
