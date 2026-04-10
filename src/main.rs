use anyhow::Result;
use ratatui_image::picker::Picker;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Write};

mod app;
mod audio_sink;
mod config;
mod daemon;
mod discord;
mod ipc;
mod lastfm;
#[cfg(feature = "mpris")]
mod mpris;
mod player;
mod spotify;
mod ui;
mod theme;

use app::App;
use theme::Theme;

fn prompt(label: &str) -> String {
    print!("{}", label);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    buf.trim().to_string()
}

fn run_setup(cfg: &mut config::AppConfig) -> Result<()> {
    println!("isi-music — First-time setup");
    println!("────────────────────────────────────────");
    println!("Create a Spotify app at: https://developer.spotify.com/dashboard");
    println!("Set the redirect URI to: http://127.0.0.1:8888/callback");
    println!("No client secret needed — isi-music uses PKCE authentication.");
    println!();

    let client_id = loop {
        let v = prompt("Client ID: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };

    cfg.spotify.client_id = Some(client_id);
    cfg.save()?;

    println!();
    println!("Saved to ~/.config/isi-music/config.toml");
    println!();
    Ok(())
}

async fn run_lastfm_setup(cfg: &mut config::AppConfig) -> Result<()> {
    println!();
    println!("Last.fm setup");
    println!("────────────────────────────────────────");
    println!("Create an API account at: https://www.last.fm/api/account/create");
    println!();

    let api_key = loop {
        let v = prompt("API Key: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };
    let api_secret = loop {
        let v = prompt("API Secret: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };

    println!("Requesting authorization token...");
    let token = lastfm::LastfmClient::get_auth_token(&api_key).await?;

    let auth_url = format!(
        "https://www.last.fm/api/auth/?api_key={}&token={}",
        api_key, token
    );

    println!("\nOpening Last.fm authorization in your browser...");
    if open::that(&auth_url).is_err() {
        println!("Could not open browser automatically. Please visit:");
        println!("URL: {}", auth_url);
    } else {
        println!("URL: {}", auth_url);
    }
    println!("\nAfter authorizing, return here and press ENTER.");
    
    let mut _unused = String::new();
    std::io::stdin().read_line(&mut _unused)?;

    print!("Finalizing Last.fm authentication...");
    std::io::stdout().flush().ok();

    match lastfm::LastfmClient::get_session(&api_key, &api_secret, &token).await {
        Ok(session_key) => {
            cfg.lastfm.api_key = Some(api_key);
            cfg.lastfm.api_secret = Some(api_secret);
            cfg.lastfm.session_key = Some(session_key);
            cfg.save()?;
            println!(" OK");
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

fn run_discord_setup(cfg: &mut config::AppConfig) -> Result<()> {
    println!();
    println!("Discord Rich Presence (optional)");
    println!("────────────────────────────────────────");
    println!("Show the current track in your Discord activity status.");
    println!();

    let answer = prompt("Enable Discord Rich Presence? (y/n): ");
    if !matches!(answer.to_lowercase().as_str(), "y" | "yes") {
        cfg.discord.enabled = Some(false);
        cfg.save()?;
        println!("Discord Rich Presence disabled. You can enable it later by editing");
        println!("~/.config/isi-music/config.toml  (set discord.enabled = true)");
        println!();
        return Ok(());
    }

    cfg.discord.enabled = Some(true);
    cfg.save()?;

    println!();
    println!("Discord Rich Presence enabled!");
    println!();
    Ok(())
}

fn print_help() {
    println!("\
isi-music — terminal Spotify player

USAGE
  isi-music               Launch the TUI player
  isi-music [COMMAND]

TUI KEYBINDINGS
  Tab / hjkl / ↑↓     Navigate panels
  Enter                Play selected track / open album or artist
  Space                Play / pause
  n / p                Next / previous track
  s                    Toggle shuffle
  r                    Cycle repeat  (off → queue → track)
  + / -                Volume up / down
  ←→                   Seek ±5 s  (hold for ±10 s)
  /                    Search Spotify
  a                    Add track to queue
  c                    Toggle album art panel
  z                    Toggle fullscreen player
  l                    Like current track
  Backspace            Back to previous search results
  Esc                  Close search / exit fullscreen
  q / Ctrl-C           Quit

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
  isi-music --liked                  Load all liked songs and play
  isi-music --play <spotify:playlist:ID>   Load a playlist and play
  isi-music --ls                     List loaded tracks with their ID
  isi-music --play-id <N>            Play track by ID (from --ls)

SETUP
  isi-music setup-lastfm             Configure Last.fm scrobbling
  isi-music --clear-logs             Clear the log file

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

AUTH
  Uses PKCE — only client_id is needed (no client_secret)
  Register at https://developer.spotify.com/dashboard
  Set redirect URI to http://127.0.0.1:8888/callback

FILES
  Config   ~/.config/isi-music/config.toml
  Log      ~/.local/share/isi-music/isi-music.log
  Socket   $XDG_RUNTIME_DIR/isi-music.sock
");
}

fn main() -> Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);

    if let Ok(env_path) = config::env_path() {
        dotenvy::from_path(&env_path).ok();
    }
    dotenvy::dotenv().ok();

    let mut cfg = config::AppConfig::load()?;
    let args: Vec<String> = std::env::args().collect();
    let arg1 = args.get(1).map(|s| s.as_str());

    if arg1 == Some("--daemon") {
        let child_pid = unsafe { libc::fork() };
        if child_pid < 0 { anyhow::bail!("fork() failed"); }
        if child_pid > 0 {
            println!("isi-music daemon started (PID {child_pid})");
            return Ok(());
        }
        unsafe {
            libc::setsid();
            let null = libc::open(b"/dev/null\0".as_ptr() as *const libc::c_char, libc::O_RDWR);
            if null >= 0 {
                libc::dup2(null, 0);
                libc::dup2(null, 1);
                libc::dup2(null, 2);
                libc::close(null);
            }
        }
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
        Some(cmd @ ("--toggle" | "--next" | "--prev" | "--vol+" | "--vol-"
                    | "--status" | "--ls" | "--liked" | "--quit-daemon")) => {
            let c = cmd.trim_start_matches('-');
            Some(if c == "quit-daemon" { "quit".into() } else { c.into() })
        }
        Some("--play") => {
            let uri = args.get(2).ok_or_else(|| anyhow::anyhow!(
                "Usage: isi-music --play <spotify:playlist:ID>"
            ))?;
            Some(format!("play {uri}"))
        }
        Some("--play-id") => {
            let id = args.get(2).ok_or_else(|| anyhow::anyhow!(
                "Usage: isi-music --play-id <N>  (see: isi-music --ls)"
            ))?;
            Some(format!("play-id {id}"))
        }
        _ => None,
    };

    if let Some(cmd) = ipc_cmd {
        let response = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(ipc::send_command(&cmd))?;
        println!("{response}");
        return Ok(());
    }

    if arg1 == Some("setup-lastfm") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(run_lastfm_setup(&mut cfg));
    }

    if cfg.needs_setup() { run_setup(&mut cfg)?; }
    if cfg.discord.enabled.is_none() { run_discord_setup(&mut cfg)?; }

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            let log_path = config::log_path()?;
            let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;

            tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(log_file))
                .with_ansi(false)
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("isi_music=warn".parse()?),)
                .init();

            let theme = Theme::load();
            let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

            enable_raw_mode()?;
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;

            let mut app = App::new(picker, theme).await?;
            let res = app.run(&mut terminal).await;

            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            terminal.show_cursor()?;

            if let Err(err) = res { eprintln!("[Error]: {err:?}"); }
            Ok(())
        })
}