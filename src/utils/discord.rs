pub const DEFAULT_APP_ID: &str = "1489692487541850324";

/// Discord Rich Presence — shows current track in Discord activity.
///
/// Runs in a dedicated std::thread (discord-rich-presence is blocking).
/// The app sends updates via an mpsc channel; the thread applies them.
#[cfg(feature = "discord")]
use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
use std::sync::mpsc;

pub struct DiscordRpc {
    tx: mpsc::SyncSender<RpcUpdate>,
}

enum RpcUpdate {
    Playing {
        title: String,
        artist: String,
        album: String,
        art_url: Option<String>,
    },
    Paused {
        title: String,
        artist: String,
    },
    Clear,
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
                        return;
                    };
                    if client.connect().is_err() {
                        return;
                    }

                    let mut backoff_secs = 1u64;
                    const MAX_BACKOFF_SECS: u64 = 60;

                    for update in rx {
                        let result = match &update {
                            RpcUpdate::Playing {
                                title,
                                artist,
                                album,
                                art_url,
                            } => {
                                let act = activity::Activity::new()
                                    .activity_type(activity::ActivityType::Listening)
                                    .details(title)
                                    .state(artist)
                                    .timestamps(
                                        activity::Timestamps::new().start(
                                            std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs()
                                                as i64,
                                        ),
                                    );

                                let large_text = if album.is_empty() { title } else { album };
                                let mut assets = activity::Assets::new().large_text(large_text);

                                if let Some(url) = art_url.as_deref() {
                                    if url.starts_with("http") {
                                        assets = assets.large_image(url);
                                    } else {
                                        assets = assets.large_image("default_music_icon");
                                    }
                                } else {
                                    assets = assets.large_image("default_music_icon");
                                }

                                client.set_activity(act.assets(assets))
                            }
                            RpcUpdate::Paused { title, artist } => client.set_activity(
                                activity::Activity::new()
                                    .activity_type(activity::ActivityType::Playing)
                                    .details(title)
                                    .state(&format!("{artist} · Paused")),
                            ),
                            RpcUpdate::Clear => client.clear_activity(),
                        };

                        if result.is_err() {
                            let mut reconnected = false;
                            for _ in 0..3 {
                                std::thread::sleep(std::time::Duration::from_secs(backoff_secs));
                                if client.reconnect().is_ok() {
                                    backoff_secs = 1;
                                    reconnected = true;
                                    break;
                                }
                                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
                            }
                            if !reconnected {
                                break;
                            }
                            let _ = match &update {
                                RpcUpdate::Playing {
                                    title,
                                    artist,
                                    album,
                                    art_url,
                                } => {
                                    let act = activity::Activity::new()
                                        .activity_type(activity::ActivityType::Listening)
                                        .details(title)
                                        .state(artist)
                                        .timestamps(
                                            activity::Timestamps::new().start(
                                                std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_secs()
                                                    as i64,
                                            ),
                                        );
                                    let large_text = if album.is_empty() { title } else { album };
                                    let mut assets = activity::Assets::new().large_text(large_text);
                                    if let Some(url) = art_url.as_deref() {
                                        if url.starts_with("http") {
                                            assets = assets.large_image(url);
                                        } else {
                                            assets = assets.large_image("default_music_icon");
                                        }
                                    } else {
                                        assets = assets.large_image("default_music_icon");
                                    }
                                    client.set_activity(act.assets(assets))
                                }
                                RpcUpdate::Paused { title, artist } => client.set_activity(
                                    activity::Activity::new()
                                        .activity_type(activity::ActivityType::Playing)
                                        .details(title)
                                        .state(&format!("{artist} · Paused")),
                                ),
                                RpcUpdate::Clear => client.clear_activity(),
                            };
                        }
                    }
                })
                .ok()?;

            Some(Self { tx })
        }
    }

    pub fn update_playing(&self, title: &str, artist: &str, album: &str, art_url: Option<&str>) {
        if self
            .tx
            .try_send(RpcUpdate::Playing {
                title: title.to_string(),
                artist: artist.to_string(),
                album: album.to_string(),
                art_url: art_url.map(|s| s.to_string()),
            })
            .is_err()
        {
            tracing::trace!("Discord RPC channel full, dropping update_playing");
        }
    }

    pub fn update_paused(&self, title: &str, artist: &str) {
        if self
            .tx
            .try_send(RpcUpdate::Paused {
                title: title.to_string(),
                artist: artist.to_string(),
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
