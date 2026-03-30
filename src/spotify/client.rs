use anyhow::{Context, Result};
use rspotify::{
    AuthCodeSpotify,
    clients::{BaseClient, OAuthClient},
    model::{
        Id, LibraryId, Offset, PlayContextId, PlayableItem, PlaylistId, RepeatState, TrackId,
    },
};
use tracing::{info, warn};

use crate::config;
use crate::ui::PlaybackState;
use super::auth::SpotifyAuth;

pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub uri: String,
    pub total_tracks: u32,
}

pub struct TrackSummary {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub uri: String,
}

pub struct SpotifyClient {
    client: AuthCodeSpotify,
    shuffle_state: bool,
    repeat_state: RepeatState,
}

impl SpotifyClient {
    pub async fn new() -> Result<Self> {
        let client = SpotifyAuth::build_client()?;

        let needs_auth = if client.read_token_cache(true).await.is_ok() {
            if client.current_user().await.is_err() {
                warn!("Cached token is invalid, re-authenticating...");
                let _ = std::fs::remove_file(config::cache_path()?);
                true
            } else {
                false
            }
        } else {
            true
        };

        if needs_auth {
            let url = client
                .get_authorize_url(false)
                .context("Failed to generate authorization URL")?;
            let code = SpotifyAuth::run_oauth_flow(&url).await?;
            client
                .request_token(&code)
                .await
                .context("Failed to exchange code for token")?;
        }

        info!("Authenticated with Spotify");
        Ok(Self {
            client,
            shuffle_state: false,
            repeat_state: RepeatState::Off,
        })
    }

    /// Returns the current access token for use with librespot.
    pub async fn get_access_token(&self) -> Option<String> {
        let guard = self.client.token.lock().await.ok()?;
        guard.as_ref().map(|t| t.access_token.clone())
    }

    pub async fn fetch_playlists(&self) -> Result<Vec<PlaylistSummary>> {
        let mut all = Vec::new();
        let mut offset = 0u32;
        loop {
            let page = self
                .client
                .current_user_playlists_manual(Some(50), Some(offset))
                .await?;
            let fetched = page.items.len() as u32;
            for p in page.items {
                all.push(PlaylistSummary {
                    id: p.id.id().to_owned(),
                    uri: p.id.uri(),
                    name: p.name,
                    total_tracks: p.tracks.total,
                });
            }
            if page.next.is_none() || fetched == 0 {
                break;
            }
            offset += fetched;
        }
        Ok(all)
    }

    pub async fn fetch_playlist_tracks(&self, playlist_id: &str) -> Result<Vec<TrackSummary>> {
        let id = PlaylistId::from_id(playlist_id)
            .map_err(|e| anyhow::anyhow!("Invalid playlist ID: {e}"))?;
        let mut all = Vec::new();
        let mut offset = 0u32;
        loop {
            let page = self
                .client
                .playlist_items_manual(id.clone(), None, None, Some(50), Some(offset))
                .await?;
            let fetched = page.items.len() as u32;
            for item in page.items {
                if let Some(PlayableItem::Track(track)) = item.track {
                    let artist = track
                        .artists
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let duration_ms =
                        track.duration.num_milliseconds().try_into().unwrap_or(0u64);
                    let uri = track.id.as_ref().map(|id| id.uri()).unwrap_or_default();
                    all.push(TrackSummary {
                        name: track.name,
                        artist,
                        album: track.album.name,
                        duration_ms,
                        uri,
                    });
                }
            }
            if page.next.is_none() || fetched == 0 {
                break;
            }
            offset += fetched;
        }
        Ok(all)
    }

    pub async fn play_in_context(&self, playlist_uri: &str, track_uri: &str) -> Result<()> {
        let id = PlaylistId::from_uri(playlist_uri)
            .map_err(|e| anyhow::anyhow!("Invalid playlist URI: {e}"))?;
        self.client
            .start_context_playback(
                PlayContextId::Playlist(id),
                None,
                Some(Offset::Uri(track_uri.to_owned())),
                None,
            )
            .await?;
        Ok(())
    }

    pub async fn fetch_playback(&mut self) -> Result<PlaybackState> {
        let ctx = match self.client.current_playback(None, None::<&[_]>).await {
            Ok(ctx) => ctx,
            Err(e) => {
                warn!("Failed to fetch playback: {e}");
                return Ok(PlaybackState::default());
            }
        };

        let Some(ctx) = ctx else {
            return Ok(PlaybackState::default());
        };

        self.shuffle_state = ctx.shuffle_state;
        self.repeat_state = ctx.repeat_state;

        let (title, artist, album, duration_ms) = match ctx.item {
            Some(PlayableItem::Track(track)) => {
                let artist = track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let duration = track.duration.num_milliseconds().try_into().unwrap_or(0u64);
                (track.name, artist, track.album.name, duration)
            }
            Some(PlayableItem::Episode(ep)) => {
                let duration = ep.duration.num_milliseconds().try_into().unwrap_or(0u64);
                (ep.name, ep.show.name, String::new(), duration)
            }
            Some(PlayableItem::Unknown(_)) | None => return Ok(PlaybackState::default()),
        };

        let progress_ms = ctx
            .progress
            .and_then(|p| p.num_milliseconds().try_into().ok())
            .unwrap_or(0u64);

        Ok(PlaybackState {
            title,
            artist,
            album,
            is_playing: ctx.is_playing,
            shuffle: self.shuffle_state,
            repeat: self.repeat_state,
            progress_ms,
            duration_ms,
            volume: 100,
        })
    }

    pub async fn toggle_playback(&self) -> Result<()> {
        let ctx = self.client.current_playback(None, None::<&[_]>).await?;
        match ctx {
            Some(c) if c.is_playing => self.client.pause_playback(None).await?,
            _ => self.client.resume_playback(None, None).await?,
        }
        Ok(())
    }

    pub async fn next_track(&self) -> Result<()> {
        self.client.next_track(None).await?;
        Ok(())
    }

    pub async fn prev_track(&self) -> Result<()> {
        self.client.previous_track(None).await?;
        Ok(())
    }

    pub async fn toggle_shuffle(&mut self) -> Result<()> {
        self.shuffle_state = !self.shuffle_state;
        self.client.shuffle(self.shuffle_state, None).await?;
        Ok(())
    }

    pub async fn cycle_repeat(&mut self) -> Result<()> {
        self.repeat_state = match self.repeat_state {
            RepeatState::Off => RepeatState::Context,
            RepeatState::Context => RepeatState::Track,
            RepeatState::Track => RepeatState::Off,
        };
        self.client.repeat(self.repeat_state, None).await?;
        Ok(())
    }

    pub async fn save_current_track(&self) -> Result<()> {
        let ctx = self.client.current_playback(None, None::<&[_]>).await?;
        if let Some(PlayableItem::Track(track)) = ctx.and_then(|c| c.item) {
            if let Some(id) = track.id {
                self.client
                    .library_add([LibraryId::Track(TrackId::from_id(id.id())?)])
                    .await?;
            }
        }
        Ok(())
    }
}
