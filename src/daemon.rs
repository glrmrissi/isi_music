use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::ipc::socket_path;
use crate::lastfm::LastfmClient;
use crate::player::{AudioPlayer, NativePlayer, PlayerNotification};
use crate::spotify::SpotifyClient;

struct TrackInfo {
    name: String,
    artist: String,
    duration_ms: u64,
}

pub async fn run(cfg: AppConfig) -> Result<()> {
    // stdout/stderr are redirected to /dev/null after fork — log to file instead
    if let Ok(log_path) = crate::config::log_path() {
        if let Ok(log_file) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(log_file))
                .with_ansi(false)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive("isi_music=debug".parse().unwrap()),
                )
                .try_init();
        }
    }
    info!("daemon starting");

    let lastfm = match (&cfg.lastfm.api_key, &cfg.lastfm.api_secret, &cfg.lastfm.session_key) {
        (Some(k), Some(s), Some(sk)) => {
            Some(Arc::new(LastfmClient::new(k.clone(), s.clone(), sk.clone())))
        }
        _ => None,
    };

    let mut spotify = SpotifyClient::new().await?;
    let token = spotify
        .get_access_token()
        .await
        .ok_or_else(|| anyhow::anyhow!("No Spotify access token"))?;
    let mut player: Box<dyn AudioPlayer> = Box::new(NativePlayer::new(token, true).await?);

    // IPC socket
    let sock = socket_path();
    if sock.exists() {
        std::fs::remove_file(&sock).ok();
    }
    let listener = UnixListener::bind(&sock)?;

    info!("daemon ready — {}", sock.display());

    // Playback tracking (for scrobble + status)
    let mut track_list: Vec<TrackInfo> = Vec::new();
    let mut progress_ms: u64 = 0;
    let mut track_start_unix: u64 = 0;
    let mut scrobble_sent = false;
    let mut last_tick = Instant::now();

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let Ok((stream, _)) = accept else { continue };
                let (r, mut w) = stream.into_split();
                let mut reader = BufReader::new(r);
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_err() { continue }

                let cmd = line.trim().to_string();

                let response: String = if cmd.starts_with("play ") {
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
                } else if cmd == "liked" {
                    match load_liked(&mut spotify, &mut *player, &mut track_list).await {
                        Ok(n) => {
                            progress_ms = 0;
                            scrobble_sent = false;
                            track_start_unix = unix_now();
                            format!("ok — {n} liked tracks loaded")
                        }
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
                        "ls" => ls_string(&*player, &track_list),
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

                // Player events
                while let Some(notif) = player.try_recv_event() {
                    match notif {
                        PlayerNotification::TrackEnded | PlayerNotification::TrackUnavailable => {
                            if player.next() {
                                progress_ms = 0;
                                scrobble_sent = false;
                                track_start_unix = unix_now();
                            }
                        }
                        _ => {}
                    }
                }

                if player.is_playing() {
                    progress_ms += delta;

                    if !scrobble_sent {
                        if let Some(idx) = player.current_index() {
                            if let Some(t) = track_list.get(idx) {
                                let threshold = (t.duration_ms / 2).min(4 * 60 * 1000);
                                if progress_ms >= 30_000 && progress_ms >= threshold {
                                    if let Some(lfm) = lastfm.clone() {
                                        let artist = t.artist.clone();
                                        let title  = t.name.clone();
                                        let ts  = track_start_unix;
                                        let dur = t.duration_ms;
                                        tokio::spawn(async move {
                                            lfm.scrobble(&artist, &title, ts, dur).await;
                                        });
                                    }
                                    scrobble_sent = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    std::fs::remove_file(&sock).ok();
    Ok(())
}

/// Load all tracks from a Spotify playlist URI/ID into the player.
async fn load_playlist(
    spotify: &mut SpotifyClient,
    player: &mut dyn AudioPlayer,
    track_list: &mut Vec<TrackInfo>,
    uri_or_id: &str,
) -> Result<usize> {
    // Accept both "spotify:playlist:ID" and bare "ID"
    let id = uri_or_id
        .trim_start_matches("spotify:playlist:")
        .trim_start_matches("spotify:album:");

    track_list.clear();
    let mut uris: Vec<String> = Vec::new();
    let mut offset = 0u32;

    loop {
        let (batch, total) = spotify.fetch_playlist_tracks(id, offset).await?;
        let n = batch.len();
        if n == 0 { break; }
        for t in batch {
            uris.push(t.uri.clone());
            track_list.push(TrackInfo {
                name: t.name,
                artist: t.artist,
                duration_ms: t.duration_ms,
            });
        }
        offset += n as u32;
        if offset >= total { break; }
    }

    let total = uris.len();
    if total > 0 {
        player.set_queue(uris, 0);
    }
    Ok(total)
}

/// Load all liked/saved tracks into the player queue.
async fn load_liked(
    spotify: &mut SpotifyClient,
    player: &mut dyn AudioPlayer,
    track_list: &mut Vec<TrackInfo>,
) -> Result<usize> {
    track_list.clear();
    let mut uris: Vec<String> = Vec::new();
    let mut offset = 0u32;

    loop {
        let (batch, total) = spotify.fetch_liked_tracks(offset).await?;
        let n = batch.len();
        if n == 0 { break; }
        for t in batch {
            uris.push(t.uri.clone());
            track_list.push(TrackInfo { name: t.name, artist: t.artist, duration_ms: t.duration_ms });
        }
        offset += n as u32;
        if offset >= total { break; }
    }

    let total = uris.len();
    if total > 0 {
        player.set_queue(uris, 0);
    }
    Ok(total)
}

/// List all tracks with their index (ID), marking the currently playing one.
fn ls_string(player: &dyn AudioPlayer, tracks: &[TrackInfo]) -> String {
    if tracks.is_empty() {
        return "no playlist loaded — use: isi-music --play <spotify:playlist:ID>".into();
    }
    let current = player.current_index();
    tracks.iter().enumerate().map(|(i, t)| {
        let marker = if current == Some(i) {
            if player.is_playing() { "▶" } else { "⏸" }
        } else { " " };
        format!("{marker} {:>4}  {} — {}", i, t.name, t.artist)
    }).collect::<Vec<_>>().join("\n")
}

/// Build a human-readable status line.
fn status_string(player: &dyn AudioPlayer, tracks: &[TrackInfo], progress_ms: u64) -> String {
    let Some(idx) = player.current_index() else {
        return "stopped".into();
    };
    let state = if player.is_playing() { "▶" } else { "⏸" };
    match tracks.get(idx) {
        Some(t) => format!(
            "{state}  {} — {}  |  {} / {}  |  vol {}%",
            t.name, t.artist,
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
