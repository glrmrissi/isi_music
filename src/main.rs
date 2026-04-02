use anyhow::Result;
use ratatui_image::picker::Picker;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{EnableMouseCapture, DisableMouseCapture},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Write};

mod app;
mod config;
mod lastfm;
mod player;
mod spotify;
mod ui;

use app::App;

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
    println!();

    let client_id = loop {
        let v = prompt("Client ID: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };
    let client_secret = loop {
        let v = prompt("Client Secret: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };

    cfg.spotify.client_id = Some(client_id);
    cfg.spotify.client_secret = Some(client_secret);
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
    let username = loop {
        let v = prompt("Last.fm username: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };
    let password = loop {
        let v = prompt("Last.fm password: ");
        if !v.is_empty() { break v; }
        println!("Cannot be empty.");
    };

    print!("Authenticating with Last.fm...");
    io::stdout().flush().ok();

    match lastfm::LastfmClient::authenticate(&api_key, &api_secret, &username, &password).await {
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

#[tokio::main]
async fn main() -> Result<()> {
    // Reset any leftover terminal state from a previous crash
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);

    if let Ok(env_path) = config::env_path() {
        dotenvy::from_path(&env_path).ok();
    }
    dotenvy::dotenv().ok();

    let mut cfg = config::AppConfig::load()?;

    // Handle subcommands before entering TUI
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("setup-lastfm") {
        run_lastfm_setup(&mut cfg).await?;
        return Ok(());
    }

    if cfg.needs_setup() {
        run_setup(&mut cfg)?;
    }

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
                .add_directive("isi_music=debug".parse()?),
        )
        .init();

    // Query terminal for image protocol support BEFORE entering raw mode
    let picker = Picker::from_query_stdio()
        .unwrap_or_else(|_| Picker::halfblocks());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(picker).await?;
    let res = app.run(&mut terminal).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("[Error]: {err:?}");
    }

    Ok(())
}