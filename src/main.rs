use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
#[cfg(feature = "album-art")]
use ratatui_image::picker::Picker;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;

mod app;
mod audio;
mod backend;
mod cli;
mod config;
mod daemon;
mod keybinds;
mod player;
mod settings;
mod spotify;
mod ui;
mod utils;

use app::App;
use cli::{GREEN, RESET, YELLOW};

fn main() -> Result<()> {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);

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

    // Daemon mode is Spotify-only (no local playback) — refuse to start when disabled.
    if (arg1 == Some("--daemon") || arg1 == Some("--daemon-child")) && !cfg.spotify_enabled() {
        anyhow::bail!(
            "Spotify is disabled in config.toml ([spotify] enabled = false). \
             Daemon mode requires Spotify."
        );
    }

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

    if arg1 == Some("--version") || arg1 == Some("-V") {
        println!("isi-music v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if arg1 == Some("--help") || arg1 == Some("-h") {
        cli::print_help();
        return Ok(());
    }

    if arg1 == Some("--clear-logs") {
        let path = config::log_path()?;
        std::fs::write(&path, "")?;
        println!("Logs cleared: {}", path.display());
        return Ok(());
    }

    if arg1 == Some("doctor") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(utils::doctor::run());
    }

    if arg1 == Some("update") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(utils::updater::run());
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
            .block_on(cli::run_spotify_setup(&mut cfg));
    }

    if arg1 == Some("setup-lastfm") {
        return tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(cli::run_lastfm_setup(&mut cfg));
    }

    if arg1 == Some("local-only") {
        cfg.spotify = config::SpotifyConfig {
            client_id: cfg.spotify.client_id.clone(),
            enabled: Some(false),
        };
        cfg.save()?;
        println!("{GREEN}Spotify disabled.{RESET} Running in local-only mode.");
        return Ok(());
    }

    let config_missing = config::config_path().map(|p| !p.exists()).unwrap_or(true);

    let mut first_run = false;
    if config_missing {
        first_run = true;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(utils::wizard::run())?;
        // Re-load config after wizard writes it
        cfg = config::AppConfig::load()?;
    }

    if first_run {
        // Safety: set_var is unsafe in edition 2024 because it's not thread-safe.
        // We call this before any threads are spawned.
        unsafe {
            std::env::set_var("ISI_MUSIC_FIRST_RUN", "1");
        }
    }

    if cfg.spotify_enabled()
        && config::load_refresh_token().is_none()
        && config::load_streaming_refresh_token().is_none()
    {
        println!();
        println!("  {YELLOW}First time with Spotify?{RESET} Authenticate now to enable streaming.");
        println!("  (Streaming uses the built-in client; Web API needs your Client ID.)\n");
        let setup_now = loop {
            let v = cli::prompt("Authenticate with Spotify now? (Y/n): ");
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
                .block_on(cli::run_spotify_setup(&mut cfg))?;
        }
    }

    // Streaming auth before the TUI takes over: the browser-OAuth path prints
    // a URL the user must click, which must land on the normal screen buffer.
    if cfg.spotify_enabled()
        && (config::load_refresh_token().is_some()
            || config::load_streaming_refresh_token().is_some())
        && let Err(e) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(player::ensure_streaming_auth())
    {
        eprintln!("Spotify streaming authentication failed: {e:#}");
        eprintln!("Local playback remains available; run `isi-music setup-spotify` to retry.");
    }

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
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

            let mut theme = utils::theme::Theme::load();
            if let Some(ref layout_name) = cfg.ui.default_layout {
                utils::wizard::apply_layout_to_theme(&mut theme, layout_name);
            }
            let theme_rx = if cfg.hot_reload() {
                utils::theme::Theme::watch()?
            } else {
                utils::theme::ThemeWatcher::disabled()
            };
            let keybinds = keybinds::Keybinds::load();
            let keybinds_rx = if cfg.hot_reload() {
                keybinds::KeybindsWatcher::watch()?
            } else {
                keybinds::KeybindsWatcher::disabled()
            };
            #[cfg(feature = "album-art")]
            let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

            enable_raw_mode()?;
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
            let backend = backend::StableCrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;
            terminal.clear()?;

            let mut app = match App::new(
                #[cfg(feature = "album-art")]
                picker,
                theme,
                theme_rx,
                keybinds,
                keybinds_rx,
            )
            .await
            {
                Ok(app) => app,
                Err(err) => {
                    let _ = disable_raw_mode();
                    let _ = execute!(
                        terminal.backend_mut(),
                        DisableMouseCapture,
                        LeaveAlternateScreen
                    );
                    let _ = terminal.show_cursor();
                    return Err(err);
                }
            };

            let res = app.run(&mut terminal).await;

            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                DisableMouseCapture,
                LeaveAlternateScreen
            )?;
            terminal.show_cursor()?;

            if let Err(err) = res {
                eprintln!("[Error]: {err:?}");
            }
            Ok(())
        })
}
