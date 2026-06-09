use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use ratatui_image::picker::Picker;
use std::fs::OpenOptions;
use std::io::{self, Write};
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
use rspotify::clients::OAuthClient;

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
const GRAY: &str = "\x1b[90m";

async fn run_lastfm_setup(cfg: &mut config::AppConfig) -> Result<()> {
    println!("\n{RED}┌───────────────────────────────────────────────────────────────┐{RESET}");
    println!(
        "{RED}│{RESET}  {BOLD}Last.fm Integration Setup{RESET}                                    {RED}│{RESET}"
    );
    println!("{RED}├───────────────────────────────────────────────────────────────┤{RESET}");
    println!(
        "{RED}│{RESET}  1. Go to: {BOLD}https://www.last.fm/api/account/create{RESET}             {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  2. Create an API application                                 {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  3. Copy your {BOLD}API Key{RESET} and {BOLD}Shared Secret{RESET}                       {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}                                                               {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  4. Create/Edit: {GREEN}~/.config/isi-music/config.toml{RESET}              {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  5. Add the following content:                                {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}                                                               {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}     {GRAY}[lastfm]{RESET}                                                  {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}     api_key = {GREEN}\"YOUR_API_KEY\"{RESET}                                  {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}     api_secret = {GREEN}\"YOUR_API_SECRET\"{RESET}                            {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}     session_key = \"\"                                          {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}                                                               {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {YELLOW}{BOLD}SECURITY NOTE:{RESET} {YELLOW}Don't share your credentials!{RESET}                 {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {YELLOW}Never commit your API Secret to Git.{RESET}                         {RED}│{RESET}"
    );
    println!("{RED}└───────────────────────────────────────────────────────────────┘{RESET}\n");

    let api_key = loop {
        let v = prompt("API Key: ");
        if !v.is_empty() {
            break v;
        }
        println!("Cannot be empty.");
    };
    let api_secret = loop {
        let v = prompt("API Secret: ");
        if !v.is_empty() {
            break v;
        }
        println!("Cannot be empty.");
    };

    println!("Requesting authorization token...");
    let token = utils::lastfm::LastfmClient::get_auth_token(&api_key).await?;

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

    match utils::lastfm::LastfmClient::get_session(&api_key, &api_secret, &token).await {
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

async fn run_spotify_setup(cfg: &mut config::AppConfig) -> Result<()> {
    println!("\n{RED}┌───────────────────────────────────────────────────────────────┐{RESET}");
    println!(
        "{RED}│{RESET}  {BOLD}Spotify Setup{RESET}                                                {RED}│{RESET}"
    );
    println!("{RED}├───────────────────────────────────────────────────────────────┤{RESET}");
    println!(
        "{RED}│{RESET}  To stream from Spotify you need your own Client ID:          {RED}│{RESET}"
    );
    println!("{RED}│{RESET}                                                               {RED}│{RESET}");
    println!(
        "{RED}│{RESET}  {BOLD}1.{RESET} Go to: {GREEN}https://developer.spotify.com/dashboard{RESET}             {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {BOLD}2.{RESET} Click {BOLD}\"Create app\"{RESET}                                        {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {BOLD}3.{RESET} Give it any name & description                             {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {BOLD}4.{RESET} Add this Redirect URI:                                      {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}       {YELLOW}http://127.0.0.1:8888/callback{RESET}                          {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {BOLD}5.{RESET} Click {BOLD}\"Save\"{RESET}                                               {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {BOLD}6.{RESET} Copy the {BOLD}Client ID{RESET} and paste it below                      {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}                                                               {RED}│{RESET}"
    );
    println!(
        "{RED}│{RESET}  {YELLOW}Uses PKCE — no client_secret needed!{RESET}                         {RED}│{RESET}"
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
    println!("  {GREEN}✓{RESET}  Saved to ~/.config/isi-music/config.toml\n");

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
            match crate::spotify::auth::SpotifyAuth::build_client() {
                Ok(mut rspotify_client) => match rspotify_client.get_authorize_url(None) {
                    Ok(url) => {
                        let code = crate::spotify::auth::SpotifyAuth::run_oauth_flow(&url).await?;
                        match rspotify_client.request_token(&code).await {
                            Ok(_) => {
                                if let Ok(guard) = rspotify_client.token.lock().await {
                                    if let Some(token) = guard.as_ref() {
                                        let rt = token.refresh_token.as_deref().unwrap_or("");
                                        crate::config::save_refresh_token(rt);
                                        println!(
                                            "  {GREEN}✓{RESET}  Authenticated successfully!\n"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                println!("  {YELLOW}Authentication failed: {e}{RESET}");
                                println!(
                                    "  You can authenticate later by launching isi-music normally.\n"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        println!("  {YELLOW}Failed to generate auth URL: {e}{RESET}");
                    }
                },
                Err(e) => {
                    println!("  {YELLOW}Failed to build Spotify client: {e}{RESET}");
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
  isi-music --liked                  Load all liked songs and play
  isi-music --play <spotify:playlist:ID>   Load a playlist and play
  isi-music --ls                     List loaded tracks with their ID
  isi-music --play-id <N>            Play track by ID (from --ls)

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
  Socket   $XDG_RUNTIME_DIR/isi-music.sock
"
    );
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
            cmd @ ("--toggle" | "--next" | "--prev" | "--vol+" | "--vol-" | "--status" | "--ls"
            | "--liked" | "--quit-daemon"),
        ) => {
            let c = cmd.trim_start_matches('-');
            Some(if c == "quit-daemon" {
                "quit".into()
            } else {
                c.into()
            })
        }
        Some("--play") => {
            let uri = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("Usage: isi-music --play <spotify:playlist:ID>"))?;
            Some(format!("play {uri}"))
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
                        .add_directive("isi_music=warn".parse()?),
                )
                .init();

            let theme = utils::theme::Theme::load();
            let theme_rx = utils::theme::Theme::watch()?;
            let keybinds = keybinds::Keybinds::load();
            let keybinds_rx = keybinds::KeybindsWatcher::watch()?;
            let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

            enable_raw_mode()?;
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen)?;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;

            let mut app = App::new(picker, theme, theme_rx, keybinds, keybinds_rx).await?;
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
