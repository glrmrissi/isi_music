use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{EnableMouseCapture, DisableMouseCapture},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Write};

mod app;
mod config;
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

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new().await?;
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