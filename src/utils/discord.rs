pub const DEFAULT_APP_ID: &str = "1489692487541850324";

/// Discord Rich Presence — shows current track in Discord activity.
///
/// Runs in a dedicated std::thread (discord-rich-presence is blocking).
/// The app sends updates via an mpsc channel; the thread applies them.
#[cfg(feature = "discord")]
use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
use std::sync::mpsc;

#[cfg(feature = "discord")]
fn http_image(url: &str) -> Option<&str> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url)
    } else {
        None
    }
}

#[cfg(feature = "discord")]
fn build_playing_activity<'a>(
    title: &'a str,
    artist: &'a str,
    album: &'a str,
    image: Option<&'a str>,
    start_unix: i64,
    end_unix: Option<i64>,
) -> activity::Activity<'a> {
    let large_text = if album.is_empty() { title } else { album };
    let mut assets = activity::Assets::new().large_text(large_text);
    if let Some(img) = image {
        assets = assets.large_image(img);
    }
    let mut timestamps = activity::Timestamps::new().start(start_unix);
    if let Some(end) = end_unix {
        timestamps = timestamps.end(end);
    }
    activity::Activity::new()
        .activity_type(activity::ActivityType::Listening)
        .details(title)
        .state(artist)
        .timestamps(timestamps)
        .assets(assets)
}

pub struct DiscordRpc {
    tx: mpsc::SyncSender<RpcUpdate>,
}

enum RpcUpdate {
    Playing {
        title: String,
        artist: String,
        album: String,
        art_url: Option<String>,
        start_unix: i64,
        end_unix: Option<i64>,
    },
    Paused {
        title: String,
        artist: String,
        art_url: Option<String>,
        start_unix: i64,
        end_unix: i64,
    },
    Clear,
}

#[cfg(feature = "discord")]
fn send_update(
    client: &mut discord_rich_presence::DiscordIpcClient,
    update: &RpcUpdate,
) -> Result<(), Box<dyn std::error::Error>> {
    match update {
        RpcUpdate::Playing {
            title,
            artist,
            album,
            art_url,
            start_unix,
            end_unix,
        } => {
            let image = art_url.as_deref().and_then(http_image);
            client.set_activity(build_playing_activity(
                title,
                artist,
                album,
                image,
                *start_unix,
                *end_unix,
            ))
        }
        RpcUpdate::Paused {
            title,
            artist,
            art_url,
            start_unix,
            end_unix,
        } => {
            let image = art_url.as_deref().and_then(http_image);
            let state = format!("{artist} · Paused");
            let mut assets = activity::Assets::new().large_text(title.as_str());
            if let Some(img) = image {
                assets = assets.large_image(img);
            }
            client.set_activity(
                activity::Activity::new()
                    .activity_type(activity::ActivityType::Listening)
                    .details(title.as_str())
                    .state(&state)
                    .timestamps(
                        activity::Timestamps::new()
                            .start(*start_unix)
                            .end(*end_unix),
                    )
                    .assets(assets),
            )
        }
        RpcUpdate::Clear => client.clear_activity(),
    }
}

impl DiscordRpc {
    /// Spawns the background thread and connects to Discord IPC.
    /// Returns `None` if the `discord` feature is disabled or connection fails.
    pub fn spawn(app_id: &str) -> Option<Self> {
        #[cfg(not(feature = "discord"))]
        {
            let _ = app_id;
            return None;
        }

        #[cfg(feature = "discord")]
        {
            let app_id = app_id.to_string();
            let (tx, rx) = mpsc::sync_channel::<RpcUpdate>(8);

            std::thread::Builder::new()
                .name("discord-rpc".into())
                .spawn(move || {
                    let Ok(mut client) = DiscordIpcClient::new(&app_id) else {
                        tracing::warn!("Discord RPC: failed to create IPC client");
                        return;
                    };
                    if let Err(e) = client.connect() {
                        tracing::warn!("Discord RPC: connect failed: {e}");
                        return;
                    }
                    tracing::info!("Discord RPC: connected");

                    let mut backoff_secs = 1u64;
                    const MAX_BACKOFF_SECS: u64 = 60;

                    while let Ok(mut update) = rx.recv() {
                        if let RpcUpdate::Playing { title, art_url, .. } = &update {
                            tracing::info!(
                                "Discord RPC: sending playing for '{title}' art={art_url:?}"
                            );
                        }
                        let result = send_update(&mut client, &update);

                        if result.is_ok() {
                            match client.recv() {
                                Ok((_, v)) => {
                                    let s = v.to_string();
                                    tracing::info!("Discord RPC: resp {}", &s[..s.len().min(500)]);
                                }
                                Err(e) => {
                                    tracing::warn!("Discord RPC: response read failed: {e}")
                                }
                            }
                        }

                        if let Err(e) = result {
                            tracing::warn!(
                                "Discord RPC: activity update failed: {e}; reconnecting"
                            );
                            'reconnect: loop {
                                std::thread::sleep(std::time::Duration::from_secs(backoff_secs));
                                if client.reconnect().is_ok() {
                                    backoff_secs = 1;
                                    tracing::info!("Discord RPC: reconnected");
                                    loop {
                                        match rx.try_recv() {
                                            Ok(newer) => update = newer,
                                            Err(mpsc::TryRecvError::Empty) => break,
                                            Err(mpsc::TryRecvError::Disconnected) => return,
                                        }
                                    }
                                    let _ = send_update(&mut client, &update);
                                    break 'reconnect;
                                }
                                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                            }
                        }
                    }
                })
                .ok()?;

            Some(Self { tx })
        }
    }

    pub fn update_playing(
        &self,
        title: &str,
        artist: &str,
        album: &str,
        art_url: Option<&str>,
        start_unix: i64,
        end_unix: Option<i64>,
    ) {
        if self
            .tx
            .try_send(RpcUpdate::Playing {
                title: title.to_string(),
                artist: artist.to_string(),
                album: album.to_string(),
                art_url: art_url.map(|s| s.to_string()),
                start_unix,
                end_unix,
            })
            .is_err()
        {
            tracing::trace!("Discord RPC channel full, dropping update_playing");
        }
    }

    pub fn update_paused(
        &self,
        title: &str,
        artist: &str,
        art_url: Option<&str>,
        start_unix: i64,
        end_unix: i64,
    ) {
        if self
            .tx
            .try_send(RpcUpdate::Paused {
                title: title.to_string(),
                artist: artist.to_string(),
                art_url: art_url.map(|s| s.to_string()),
                start_unix,
                end_unix,
            })
            .is_err()
        {
            tracing::trace!("Discord RPC channel full, dropping update_paused");
        }
    }

    pub fn clear(&self) {
        if self.tx.try_send(RpcUpdate::Clear).is_err() {
            tracing::trace!("Discord RPC channel full, dropping clear");
        }
    }
}

#[cfg(all(test, feature = "discord"))]
mod tests {
    use super::*;

    #[test]
    fn http_image_passes_http_urls() {
        assert_eq!(
            http_image("https://i.scdn.co/image/abc123"),
            Some("https://i.scdn.co/image/abc123")
        );
        assert_eq!(
            http_image("http://img.example.com/a.png"),
            Some("http://img.example.com/a.png")
        );
    }

    #[test]
    fn http_image_rejects_local_and_empty() {
        assert_eq!(http_image("C:\\covers\\art.jpg"), None);
        assert_eq!(http_image("/home/user/cover.jpg"), None);
        assert_eq!(http_image("file:///tmp/art.jpg"), None);
        assert_eq!(http_image("spotify:image:abc"), None);
        assert_eq!(http_image(""), None);
    }
}
