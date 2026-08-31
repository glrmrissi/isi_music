use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::player::{AudioPlayer, NativePlayer, PlayerNotification};
use crate::spotify::SpotifyClient;
use crate::utils::ipc::IpcListener;
use crate::utils::lastfm::LastfmClient;
#[cfg(windows)]
use crate::utils::media_keys::MediaKey;
#[cfg(all(feature = "mpris", target_os = "linux"))]
use crate::utils::mpris::{MprisCmd, MprisState};
#[cfg(windows)]
use crate::utils::smtc::{SmtcCmd, SmtcState};

struct TrackInfo {
    name: String,
    artist: String,
    album: String,
    duration_ms: u64,
    uri: String,
}

pub async fn run(cfg: AppConfig) -> Result<()> {
    // Daemon mode is Spotify-only (no local playback support).
    if !cfg.spotify_enabled() {
        anyhow::bail!(
            "Spotify is disabled in config.toml ([spotify] enabled = false). \
             Daemon mode requires Spotify."
        );
    }

    // stdout/stderr are redirected to /dev/null after fork — log to file instead
    if let Ok(log_path) = crate::config::log_path()
        && let Ok(log_file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
    {
        let _ = tracing_subscriber::fmt()
            .with_writer(std::sync::Mutex::new(log_file))
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env().add_directive(
                    "isi_music=debug".parse().unwrap_or_else(|_| {
                        "isi_music=info"
                            .parse()
                            .unwrap_or_else(|_| tracing::Level::INFO.into())
                    }),
                ),
            )
            .try_init();
    }
    info!("daemon starting");

    let lastfm = match &cfg.lastfm.session_key {
        Some(sk) => {
            use crate::utils::lastfm::{get_api_key, get_api_secret};
            Some(Arc::new(LastfmClient::new(
                get_api_key(),
                get_api_secret(),
                sk.clone(),
            )))
        }
        _ => None,
    };

    let mut spotify = crate::spotify::SpotifyClient::new().await?;
    let access_token = spotify.get_access_token().await;
    crate::player::ensure_streaming_auth().await?;

    let mut player: Box<dyn AudioPlayer> = Box::new(
        NativePlayer::new(
            access_token,
            true,
            cfg.audio.librespot_bitrate(),
            cfg.audio.gapless,
        )
        .await?,
    );

    // MPRIS D-Bus (optional — gracefully degrades if D-Bus unavailable; Linux only)
    #[cfg(all(feature = "mpris", target_os = "linux"))]
    let mut mpris = match crate::utils::mpris::spawn().await {
        Ok(h) => {
            info!("MPRIS D-Bus server started");
            Some(h)
        }
        Err(e) => {
            warn!("MPRIS unavailable: {e}");
            None
        }
    };

    // Global media hotkeys (Windows)
    #[cfg(windows)]
    let mut media_keys = match crate::utils::media_keys::spawn() {
        Ok(h) => {
            info!("Global media hotkeys registered");
            Some(h)
        }
        Err(e) => {
            warn!("Media hotkeys unavailable: {e}");
            None
        }
    };

    // SMTC (Windows)
    #[cfg(windows)]
    let smtc = match crate::utils::smtc::spawn() {
        Ok(h) => {
            info!("SMTC integration enabled");
            Some(h)
        }
        Err(e) => {
            warn!("SMTC unavailable: {e}");
            None
        }
    };

    // IPC endpoint (Unix socket on Unix, named pipe on Windows)
    let mut listener = IpcListener::bind()?;

    info!("daemon ready — {}", listener.describe());

    // Playback tracking (for scrobble + status)
    let mut track_list: Vec<TrackInfo> = Vec::new();
    let mut progress_ms: u64 = 0;
    let mut track_start_unix: u64 = 0;
    let mut scrobble_sent = false;
    let mut last_tick = Instant::now();
    let autoplay_enabled = cfg.autoplay_enabled();
    let mut recent_track_uris: std::collections::VecDeque<String> =
        std::collections::VecDeque::new();

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let Ok(stream) = accept else { continue };
                let (r, mut w) = tokio::io::split(stream);
                let mut reader = BufReader::new(r);
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_err() { continue }

                let cmd = line.trim().to_string();

                let response: String = if cmd == "playlists" {
                    match list_playlists(&mut spotify).await {
                        Ok(list) => list,
                        Err(e) => format!("error: {e}"),
                    }
                } else if cmd.starts_with("play ") {
                    let arg = cmd.trim_start_matches("play ").trim();
                    match load_playlist(&mut spotify, &mut *player, &mut track_list, arg).await {
                        Ok(n) => {
                            progress_ms = 0;
                            scrobble_sent = false;
                            track_start_unix = unix_now();
                            format!("ok — {n} tracks loaded")
                        }
                        Err(e) => format!("error: {e}"),
                    }
                } else if cmd.starts_with("liked") {
                    let limit = if cmd == "liked" {
                        None
                    } else {
                        cmd.strip_prefix("liked --limit ")
                            .and_then(|s| s.parse::<usize>().ok())
                    };
                    match load_liked(&mut spotify, &mut *player, &mut track_list, limit).await {
                        Ok(n) => {
                            progress_ms = 0;
                            scrobble_sent = false;
                            track_start_unix = unix_now();
                            format!("ok — {n} liked tracks loaded")
                        }
                        Err(e) => format!("error: {e}"),
                    }
                } else if cmd.starts_with("search ") {
                    let query = cmd.trim_start_matches("search ").trim();
                    search_in_queue(&track_list, query)
                } else if cmd.starts_with("search-global ") {
                    let query = cmd.trim_start_matches("search-global ").trim();
                    match search_global(&mut spotify, query).await {
                        Ok(results) => results,
                        Err(e) => format!("error: {e}"),
                    }
                } else if cmd.starts_with("play-id ") {
                    let arg = cmd.trim_start_matches("play-id ").trim();
                    match arg.parse::<usize>() {
                        Ok(idx) if idx < track_list.len() => {
                            player.play_at(idx);
                            progress_ms = 0;
                            scrobble_sent = false;
                            track_start_unix = unix_now();
                            let t = &track_list[idx];
                            format!("playing #{idx}  {} — {}", t.name, t.artist)
                        }
                        Ok(idx) => format!("error: id {idx} out of range (0–{})", track_list.len().saturating_sub(1)),
                        Err(_)  => "error: id must be a number".into(),
                    }
                } else {
                    match cmd.as_str() {
                        "toggle" => {
                            player.toggle();
                            if player.is_playing() { "playing".into() } else { "paused".into() }
                        }
                        "next" => {
                            if player.next() {
                                progress_ms = 0;
                                scrobble_sent = false;
                                track_start_unix = unix_now();
                            }
                            "ok".into()
                        }
                        "prev" => {
                            if player.prev() {
                                progress_ms = 0;
                                scrobble_sent = false;
                                track_start_unix = unix_now();
                            }
                            "ok".into()
                        }
                        "vol+" => { player.volume_up();   format!("vol {}", player.volume()) }
                        "vol-" => { player.volume_down(); format!("vol {}", player.volume()) }
                        "status" => status_string(&*player, &track_list, progress_ms),
                        cmd if cmd.starts_with("ls") => {
                            let limit = cmd.strip_prefix("ls --limit ")
                                .and_then(|s| s.parse::<usize>().ok());
                            ls_string(&*player, &track_list, limit)
                        }
                        "devices" => {
                            match list_devices(&mut spotify).await {
                                Ok(list) => list,
                                Err(e) => format!("error: {e}"),
                            }
                        }
                        cmd if cmd.starts_with("device") => {
                            let name = cmd.strip_prefix("device ").unwrap_or("");
                            if name.is_empty() {
                                "usage: device <name>".into()
                            } else {
                                match set_device(&mut spotify, name).await {
                                    Ok(msg) => msg,
                                    Err(e) => format!("error: {e}"),
                                }
                            }
                        }
                        "quit" => {
                            let _ = w.write_all(b"bye\n").await;
                            break;
                        }
                        _ => "unknown command".into(),
                    }
                };

                let _ = w.write_all(format!("{response}\n").as_bytes()).await;
            }

            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                let now = Instant::now();
                let delta = now.duration_since(last_tick).as_millis() as u64;
                last_tick = now;

                // MPRIS: push state + handle incoming commands (media keys, playerctl)
                #[cfg(all(feature = "mpris", target_os = "linux"))]
                if let Some(mpris) = &mut mpris {
                    let idx = player.current_index();
                    let (title, artist, duration_us) = idx
                        .and_then(|i| track_list.get(i))
                        .map(|t| (t.name.clone(), t.artist.clone(), t.duration_ms as i64 * 1000))
                        .unwrap_or_default();
                    mpris.update(MprisState {
                        title,
                        artist,
                        album: String::new(),
                        art_url: None,
                        duration_us,
                        position_us: progress_ms as i64 * 1000,
                        volume: player.volume() as f64 / 100.0,
                        is_playing: player.is_playing(),
                        shuffle: player.shuffle(),
                        repeat_track: player.repeat() == crate::player::RepeatMode::Track,
                        repeat_queue: player.repeat() == crate::player::RepeatMode::Queue,
                    });
                    while let Ok(cmd) = mpris.cmd_rx.try_recv() {
                        match cmd {
                            MprisCmd::Play  => { player.play(); }
                            MprisCmd::Pause => { player.pause(); }
                            MprisCmd::Next  => {
                                if player.next() { progress_ms = 0; scrobble_sent = false; track_start_unix = unix_now(); }
                            }
                            MprisCmd::Prev  => {
                                if player.prev() { progress_ms = 0; scrobble_sent = false; track_start_unix = unix_now(); }
                            }
                            MprisCmd::Seek(us) => {
                                progress_ms = (us / 1000) as u64;
                                player.seek(progress_ms as u32);
                            }
                            MprisCmd::SetVolume(v) => {
                                player.set_volume((v * 100.0).round() as u8);
                            }
                        }
                    }
                }

                // Global media hotkeys (Windows)
                #[cfg(windows)]
                if let Some(media_keys) = &mut media_keys {
                    while let Ok(cmd) = media_keys.cmd_rx.try_recv() {
                        match cmd {
                            MediaKey::PlayPause => {
                                player.toggle();
                            }
                            MediaKey::Next => {
                                if player.next() {
                                    progress_ms = 0;
                                    scrobble_sent = false;
                                    track_start_unix = unix_now();
                                }
                            }
                            MediaKey::Previous => {
                                if player.prev() {
                                    progress_ms = 0;
                                    scrobble_sent = false;
                                    track_start_unix = unix_now();
                                }
                            }
                        }
                    }
                }

                // SMTC (Windows)
                #[cfg(windows)]
                if let Some(smtc) = &smtc {
                    let idx = player.current_index();
                    let (title, artist, duration_ms) = idx
                        .and_then(|i| track_list.get(i))
                        .map(|t| (t.name.clone(), t.artist.clone(), t.duration_ms))
                        .unwrap_or_default();
                    smtc.update(&SmtcState {
                        title,
                        artist,
                        album: String::new(),
                        art_url: None,
                        cover_path: None,
                        duration_ms,
                        position_ms: progress_ms,
                        is_playing: player.is_playing(),
                    });

                    while let Ok(cmd) = smtc.cmd_rx.try_recv() {
                        match cmd {
                            SmtcCmd::Play => { player.play(); }
                            SmtcCmd::Pause => { player.pause(); }
                            SmtcCmd::Next => {
                                if player.next() { progress_ms = 0; scrobble_sent = false; track_start_unix = unix_now(); }
                            }
                            SmtcCmd::Previous => {
                                if player.prev() { progress_ms = 0; scrobble_sent = false; track_start_unix = unix_now(); }
                            }
                            SmtcCmd::Seek(ms) => {
                                progress_ms = ms;
                                player.seek(ms as u32);
                            }
                        }
                    }
                }

                // Player events
                while let Some(notif) = player.try_recv_event() {
                    match notif {
                        PlayerNotification::TrackEnded | PlayerNotification::TrackUnavailable => {
                            // Track recent URIs for autoplay seeding
                            if let Some(idx) = player.current_index()
                                && let Some(t) = track_list.get(idx)
                                && t.uri.starts_with("spotify:track:")
                            {
                                recent_track_uris.push_back(t.uri.clone());
                                if recent_track_uris.len() > 5 {
                                    recent_track_uris.pop_front();
                                }
                            }

                            if player.next() {
                                progress_ms = 0;
                                scrobble_sent = false;
                                track_start_unix = unix_now();
                            } else if autoplay_enabled && !recent_track_uris.is_empty() {
                                // Queue exhausted — fetch recommendations and continue
                                info!("daemon: queue exhausted, fetching autoplay recommendations");
                                let seeds: Vec<String> = recent_track_uris.iter().cloned().collect();
                                match spotify.fetch_recommendations(&seeds, 20).await {
                                    Ok(tracks) if !tracks.is_empty() => {
                                        for t in &tracks {
                                            player.add_to_queue(
                                                t.uri.clone(),
                                                t.name.clone(),
                                                t.artist.clone(),
                                                t.album.clone(),
                                                t.duration_ms,
                                                None,
                                            );
                                        }
                                        // Update track_list so status/ls reflect the new tracks
                                        for t in tracks {
                                            track_list.push(TrackInfo {
                                                name: t.name,
                                                artist: t.artist,
                                                album: t.album,
                                                duration_ms: t.duration_ms,
                                                uri: t.uri,
                                            });
                                        }
                                        info!("daemon: autoplay queued tracks, continuing");
                                        if player.next() {
                                            progress_ms = 0;
                                            scrobble_sent = false;
                                            track_start_unix = unix_now();
                                        }
                                    }
                                    Ok(_) => {
                                        warn!("daemon: autoplay returned no tracks");
                                    }
                                    Err(e) => {
                                        warn!("daemon: autoplay failed: {e}");
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if player.is_playing() {
                    if let Some(pb) = player.current_playback_state() {
                        progress_ms = pb.progress_ms;
                    } else {
                        progress_ms += delta;
                    }

                    if !scrobble_sent
                        && let Some(idx) = player.current_index()
                        && let Some(t) = track_list.get(idx)
                        && t.duration_ms >= 30_000
                    {
                        let threshold = (t.duration_ms / 2).min(4 * 60 * 1000);
                        if progress_ms >= threshold {
                            if let Some(lfm) = lastfm.clone() {
                                let artist = t.artist.clone();
                                let title  = t.name.clone();
                                let album  = t.album.clone();
                                let now = unix_now();
                                let ts = if track_start_unix > 0 {
                                    track_start_unix
                                } else {
                                    now.saturating_sub(progress_ms / 1000)
                                };
                                let dur = t.duration_ms;
                                tokio::spawn(async move {
                                    lfm.scrobble(&artist, &title, &album, ts, dur).await;
                                });
                            }
                            scrobble_sent = true;
                        }
                    }
                }
            }
        }
    }

    IpcListener::cleanup();
    Ok(())
}

async fn load_playlist(
    spotify: &mut SpotifyClient,
    player: &mut dyn AudioPlayer,
    track_list: &mut Vec<TrackInfo>,
    uri_or_id_or_name: &str,
) -> Result<usize> {
    let id = if uri_or_id_or_name.contains(':') {
        // Full URI: spotify:playlist:ID
        uri_or_id_or_name
            .trim_start_matches("spotify:playlist:")
            .trim_start_matches("spotify:album:")
            .to_string()
    } else if uri_or_id_or_name
        .chars()
        .all(|c| c.is_ascii_digit() || c == '_')
    {
        // Likely an ID (alphanumeric with underscores)
        uri_or_id_or_name.to_string()
    } else {
        // Treat as name: search playlists for match
        let playlists = spotify.fetch_playlists().await?;
        let query_lower = uri_or_id_or_name.to_lowercase();
        let match_id = playlists
            .iter()
            .find(|p| p.name.to_lowercase().contains(&query_lower))
            .map(|p| p.id.clone());
        match match_id {
            Some(id) => id,
            None => anyhow::bail!("playlist not found: {uri_or_id_or_name}"),
        }
    };

    track_list.clear();
    let mut uris: Vec<String> = Vec::new();
    let mut offset = 0u32;

    loop {
        let (batch, total, page_items) = spotify.fetch_playlist_tracks(&id, offset).await?;
        let n = batch.len();
        if n == 0 {
            break;
        }
        for t in batch {
            uris.push(t.uri.clone());
            track_list.push(TrackInfo {
                name: t.name,
                artist: t.artist,
                album: t.album,
                duration_ms: t.duration_ms,
                uri: t.uri,
            });
        }
        offset += page_items;
        if offset >= total {
            break;
        }
    }

    let total = uris.len();
    if total > 0 {
        player.set_queue(uris, 0);
    }
    Ok(total)
}

async fn load_liked(
    spotify: &mut SpotifyClient,
    player: &mut dyn AudioPlayer,
    track_list: &mut Vec<TrackInfo>,
    limit: Option<usize>,
) -> Result<usize> {
    track_list.clear();
    let mut uris: Vec<String> = Vec::new();
    let mut offset = 0u32;
    let max = limit.unwrap_or(usize::MAX);

    loop {
        let (batch, total) = spotify.fetch_liked_tracks(offset, false).await?;
        let n = batch.len();
        if n == 0 {
            break;
        }
        for t in batch {
            if uris.len() >= max {
                break;
            }
            uris.push(t.uri.clone());
            track_list.push(TrackInfo {
                name: t.name,
                artist: t.artist,
                album: t.album,
                duration_ms: t.duration_ms,
                uri: t.uri,
            });
        }
        offset += n as u32;
        if offset >= total || uris.len() >= max {
            break;
        }
    }

    let total = uris.len();
    if total > 0 {
        player.set_queue(uris, 0);
    }
    Ok(total)
}

/// List all tracks with their index (ID), marking the currently playing one.
/// If limit is Some(N), only show the first N tracks.
fn ls_string(player: &dyn AudioPlayer, tracks: &[TrackInfo], limit: Option<usize>) -> String {
    if tracks.is_empty() {
        return "no playlist loaded — use: isi-music --play <ID|name>".into();
    }
    let current = player.current_index();
    let tracks_to_show: Vec<_> = if let Some(n) = limit {
        tracks.iter().take(n).collect()
    } else {
        tracks.iter().collect()
    };
    let total = tracks.len();
    let shown = tracks_to_show.len();
    let mut lines = tracks_to_show
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let marker = if current == Some(i) {
                if player.is_playing() { ">" } else { "||" }
            } else {
                " "
            };
            format!("{marker} {:>4}  {} — {}", i, t.name, t.artist)
        })
        .collect::<Vec<_>>();
    if shown < total {
        lines.push(format!(
            "... ({} more tracks, use --ls --limit N to see more)",
            total - shown
        ));
    }
    lines.join("\n")
}

/// Build a human-readable status line.
fn status_string(player: &dyn AudioPlayer, tracks: &[TrackInfo], progress_ms: u64) -> String {
    let Some(idx) = player.current_index() else {
        return "stopped".into();
    };
    let state = if player.is_playing() { ">" } else { "||" };
    match tracks.get(idx) {
        Some(t) => format!(
            "{state}  {} — {}  |  {} / {}  |  vol {}%",
            t.name,
            t.artist,
            fmt_duration(progress_ms),
            fmt_duration(t.duration_ms),
            player.volume(),
        ),
        None => format!("{state}  track #{idx}  |  vol {}%", player.volume()),
    }
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// List all user playlists with name and ID.
async fn list_playlists(spotify: &mut SpotifyClient) -> Result<String> {
    let playlists = spotify.fetch_playlists().await?;
    if playlists.is_empty() {
        return Ok("no playlists found".into());
    }
    let lines = playlists
        .iter()
        .map(|p| format!("{}  {}", p.id, p.name))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(lines)
}

/// Search within the loaded queue by name/artist (case-insensitive, substring).
fn search_in_queue(tracks: &[TrackInfo], query: &str) -> String {
    if tracks.is_empty() {
        return "no playlist loaded — use: isi-music --play <spotify:playlist:ID>".into();
    }
    let query_lower = query.to_lowercase();
    let matches: Vec<_> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.name.to_lowercase().contains(&query_lower)
                || t.artist.to_lowercase().contains(&query_lower)
        })
        .collect();
    if matches.is_empty() {
        return format!("no matches for: {query}");
    }
    matches
        .iter()
        .map(|(i, t)| format!("{:>4}  {} — {}", i, t.name, t.artist))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Search globally on Spotify (tracks, albums, artists, playlists).
async fn search_global(spotify: &mut SpotifyClient, query: &str) -> Result<String> {
    let results = spotify.search_all(query).await?;
    let mut lines = Vec::new();

    if !results.tracks.is_empty() {
        lines.push("TRACKS:".into());
        for t in results.tracks.iter().take(10) {
            lines.push(format!("  {} — {}", t.name, t.artist));
        }
    }

    if !results.albums.is_empty() {
        lines.push("ALBUMS:".into());
        for a in results.albums.iter().take(5) {
            lines.push(format!("  {} — {}", a.name, a.artist));
        }
    }

    if !results.artists.is_empty() {
        lines.push("ARTISTS:".into());
        for a in results.artists.iter().take(5) {
            lines.push(format!("  {}", a.name));
        }
    }

    if !results.playlists.is_empty() {
        lines.push("PLAYLISTS:".into());
        for p in results.playlists.iter().take(5) {
            lines.push(format!("  {}  {}", p.id, p.name));
        }
    }

    if lines.is_empty() {
        Ok(format!("no results for: {query}"))
    } else {
        Ok(lines.join("\n"))
    }
}

/// List available Spotify Connect devices.
async fn list_devices(spotify: &mut SpotifyClient) -> Result<String> {
    let devices = spotify.fetch_devices().await?;
    if devices.is_empty() {
        return Ok("no devices found".into());
    }
    let lines = devices
        .iter()
        .map(|d: &crate::spotify::Device| {
            let marker = if d.is_active { "*" } else { " " };
            format!("{} {}  ({})", marker, d.name, d.device_type)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(lines)
}

/// Transfer playback to a device by name (fuzzy match).
async fn set_device(spotify: &mut SpotifyClient, name: &str) -> Result<String> {
    let devices = spotify.fetch_devices().await?;
    let name_lower = name.to_lowercase();
    let match_id = devices
        .iter()
        .find(|d| d.name.to_lowercase().contains(&name_lower))
        .map(|d| d.id.clone());
    match match_id {
        Some(id) => {
            spotify.transfer_playback(&id).await?;
            Ok(format!("transferred to: {}", name))
        }
        None => anyhow::bail!("device not found: {}", name),
    }
}
