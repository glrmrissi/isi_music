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

pub struct ArtistSummary {
    pub name: String,
    pub uri: String,
    pub genres: String,
}

pub struct AlbumSummary {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub uri: String,
    pub total_tracks: u32,
}

pub struct ShowSummary {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub uri: String,
    pub total_episodes: u32,
}

pub struct FullSearchResults {
    pub tracks: Vec<TrackSummary>,
    pub artists: Vec<ArtistSummary>,
    pub albums: Vec<AlbumSummary>,
    pub playlists: Vec<PlaylistSummary>,
}

pub struct SpotifyClient {
    client: AuthCodeSpotify,
    http: reqwest::Client,
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
            http: reqwest::Client::new(),
            shuffle_state: false,
            repeat_state: RepeatState::Off,
        })
    }

    /// Returns the current access token for use with librespot.
    pub async fn get_access_token(&self) -> Option<String> {
        let guard = self.client.token.lock().await.ok()?;
        guard.as_ref().map(|t| t.access_token.clone())
    }

    pub async fn fetch_liked_tracks(&self, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        let page = self
            .client
            .current_user_saved_tracks_manual(None, Some(50), Some(offset))
            .await?;
        let total = page.total;
        let mut tracks = Vec::new();
        for saved in page.items {
            let track = saved.track;
            let artist = track
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let duration_ms = track.duration.num_milliseconds().try_into().unwrap_or(0u64);
            let uri = track.id.as_ref().map(|id| id.uri()).unwrap_or_default();
            tracks.push(TrackSummary {
                name: track.name,
                artist,
                album: track.album.name,
                duration_ms,
                uri,
            });
        }
        Ok((tracks, total))
    }

    pub async fn play_track_uri(&self, track_uri: &str) -> Result<()> {
        use rspotify::model::PlayableId;
        let id = TrackId::from_uri(track_uri)
            .map_err(|e| anyhow::anyhow!("Invalid track URI: {e}"))?;
        self.client
            .start_uris_playback([PlayableId::Track(id)], None, None, None)
            .await?;
        Ok(())
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

    pub async fn fetch_playlist_tracks(&self, playlist_id: &str, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        let id = PlaylistId::from_id(playlist_id)
            .map_err(|e| anyhow::anyhow!("Invalid playlist ID: {e}"))?;
        let page = self
            .client
            .playlist_items_manual(id, None, None, Some(50), Some(offset))
            .await?;
        let total = page.total;
        let mut tracks = Vec::new();
        for item in page.items {
            if let Some(PlayableItem::Track(track)) = item.track {
                let artist = track
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let duration_ms = track.duration.num_milliseconds().try_into().unwrap_or(0u64);
                let uri = track.id.as_ref().map(|id| id.uri()).unwrap_or_default();
                tracks.push(TrackSummary {
                    name: track.name,
                    artist,
                    album: track.album.name,
                    duration_ms,
                    uri,
                });
            }
        }
        Ok((tracks, total))
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

    pub async fn search_all(&self, query: &str) -> Result<FullSearchResults> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let client = &self.http;
        let request = client
            .get("https://api.spotify.com/v1/search")
            .bearer_auth(&token)
            .query(&[
                ("q", query),
                ("type", "track,artist,album,playlist"),
                ("limit", "8"),
            ])
            .build()?;

        let response = client.execute(request).await?;
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;

        let mut tracks = Vec::new();
        if let Some(items) = json["tracks"]["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let album = item["album"]["name"].as_str().unwrap_or("").to_string();
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary { name, artist, album, duration_ms, uri });
            }
        }

        let mut artists = Vec::new();
        if let Some(items) = json["artists"]["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let genres = item["genres"]
                    .as_array()
                    .map(|g| g.iter().filter_map(|x| x.as_str()).take(2).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                artists.push(ArtistSummary { name, uri, genres });
            }
        }

        let mut albums = Vec::new();
        if let Some(items) = json["albums"]["items"].as_array() {
            for item in items {
                let id = item["id"].as_str().unwrap_or("").to_string();
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let total_tracks = item["total_tracks"].as_u64().unwrap_or(0) as u32;
                albums.push(AlbumSummary { id, name, artist, uri, total_tracks });
            }
        }

        let mut playlists = Vec::new();
        if let Some(items) = json["playlists"]["items"].as_array() {
            for item in items {
                let id = item["id"].as_str().unwrap_or("").to_string();
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                let total_tracks = item["tracks"]["total"].as_u64().unwrap_or(0) as u32;
                playlists.push(PlaylistSummary { id, name, uri, total_tracks });
            }
        }

        Ok(FullSearchResults { tracks, artists, albums, playlists })
    }

    pub async fn fetch_album_tracks(&self, album_id: &str, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let client = &self.http;
        let response = client
            .get(format!("https://api.spotify.com/v1/albums/{album_id}/tracks"))
            .bearer_auth(&token)
            .query(&[("limit", "50"), ("offset", &offset_str)])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let mut tracks = Vec::new();

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary { name, artist, album: String::new(), duration_ms, uri });
            }
        }

        Ok((tracks, total))
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

    pub async fn fetch_saved_albums(&self, offset: u32) -> Result<(Vec<AlbumSummary>, u32)> {
        let page = self
            .client
            .current_user_saved_albums_manual(None, Some(20), Some(offset))
            .await?;
        let total = page.total;
        let mut albums = Vec::new();
        for saved in page.items {
            let album = saved.album;
            let artist = album
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let id = album.id.id().to_owned();
            let uri = album.id.uri();
            let total_tracks = album.tracks.total;
            albums.push(AlbumSummary {
                id,
                name: album.name,
                artist,
                uri,
                total_tracks,
            });
        }
        Ok((albums, total))
    }

    pub async fn fetch_followed_artists(&self) -> Result<Vec<ArtistSummary>> {
        let page = self
            .client
            .current_user_followed_artists(None, Some(50))
            .await?;
        let mut artists = Vec::new();
        for artist in page.items {
            let name = artist.name;
            let uri = artist.id.uri();
            let genres = artist.genres.iter().take(2).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            artists.push(ArtistSummary { name, uri, genres });
        }
        Ok(artists)
    }

    pub async fn fetch_artist_top_tracks(&self, artist_id: &str) -> Result<Vec<TrackSummary>> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let client = &self.http;
        let response = client
            .get(format!("https://api.spotify.com/v1/artists/{artist_id}/top-tracks"))
            .bearer_auth(&token)
            .query(&[("market", "from_token")])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;
        let mut tracks = Vec::new();

        if let Some(items) = json["tracks"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let album = item["album"]["name"].as_str().unwrap_or("").to_string();
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary { name, artist, album, duration_ms, uri });
            }
        }

        Ok(tracks)
    }

    pub async fn fetch_saved_shows(&self, offset: u32) -> Result<(Vec<ShowSummary>, u32)> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let client = &self.http;
        let response = client
            .get("https://api.spotify.com/v1/me/shows")
            .bearer_auth(&token)
            .query(&[("limit", "20"), ("offset", &offset_str)])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let mut shows = Vec::new();

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let show = &item["show"];
                let id = show["id"].as_str().unwrap_or("").to_string();
                let name = show["name"].as_str().unwrap_or("Unknown").to_string();
                let publisher = show["publisher"].as_str().unwrap_or("").to_string();
                let uri = show["uri"].as_str().unwrap_or("").to_string();
                let total_episodes = show["total_episodes"].as_u64().unwrap_or(0) as u32;
                shows.push(ShowSummary { id, name, publisher, uri, total_episodes });
            }
        }

        Ok((shows, total))
    }

    pub async fn fetch_show_episodes(&self, show_id: &str, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let client = &self.http;
        let response = client
            .get(format!("https://api.spotify.com/v1/shows/{show_id}/episodes"))
            .bearer_auth(&token)
            .query(&[("limit", "20"), ("offset", &offset_str)])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let mut tracks = Vec::new();

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let description = item["description"].as_str().unwrap_or("").to_string();
                let artist = if description.len() > 60 {
                    format!("{}…", &description[..60])
                } else {
                    description
                };
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary { name, artist, album: String::new(), duration_ms, uri });
            }
        }

        Ok((tracks, total))
    }

    /// Return a cheap clone of the inner HTTP client (reqwest::Client is Arc-backed).
    pub fn http_client(&self) -> reqwest::Client {
        self.http.clone()
    }

    /// Fetch the smallest album art URL for a track URI (spotify:track:<id>).
    pub async fn fetch_track_art_url(&self, track_uri: &str) -> Option<String> {
        let track_id = track_uri.strip_prefix("spotify:track:")?;
        let token = self.get_access_token().await?;
        let json: serde_json::Value = self.http
            .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
            .bearer_auth(&token)
            .send().await.ok()?
            .json().await.ok()?;
        // images are sorted largest→smallest; last() is smallest (64×64)
        json["album"]["images"].as_array()?
            .last()
            .and_then(|img| img["url"].as_str())
            .map(|s| s.to_string())
    }
}
