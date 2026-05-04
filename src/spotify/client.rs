use anyhow::{Context, Result};
use rspotify::{
    AuthCodePkceSpotify, Token,
    clients::{OAuthClient},
    model::{
        Id, LibraryId, Offset, PlayContextId, PlayableItem, PlaylistId,
        RepeatState, TrackId,
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
    #[allow(dead_code)]
    pub art_url: Option<String>
}

#[derive(Clone)]
pub struct TrackSummary {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub uri: String,
}

pub struct ArtistSummary {
    pub id: String,
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
    #[allow(dead_code)]
    pub uri: String,
    pub total_episodes: u32,
}

pub struct FullSearchResults {
    pub tracks: Vec<TrackSummary>,
    pub artists: Vec<ArtistSummary>,
    pub albums: Vec<AlbumSummary>,
    pub playlists: Vec<PlaylistSummary>,
    pub tracks_total: u32,
    pub artists_total: u32,
    pub albums_total: u32,
    pub playlists_total: u32,
}

pub struct SpotifyClient {
    client: AuthCodePkceSpotify,
    http: reqwest::Client,
    shuffle_state: bool,
    repeat_state: RepeatState,
    user_market: Option<String>,
    pub authenticated: bool,
}

impl SpotifyClient {
    pub fn new_unauthenticated() -> Self {
        let client = SpotifyAuth::build_client().unwrap_or_else(|_| {
            let creds = rspotify::Credentials::new_pkce("dummy");
            let oauth = rspotify::OAuth {
                redirect_uri: "http://127.0.0.1:8888/callback".to_string(),
                scopes: rspotify::scopes!("streaming"),
                ..Default::default()
            };
            rspotify::AuthCodePkceSpotify::new(creds, oauth)
        });
        Self {
            client,
            http: reqwest::Client::new(),
            shuffle_state: false,
            repeat_state: RepeatState::Off,
            user_market: None,
            authenticated: false,
        }
    }

    pub async fn new() -> Result<Self> {
        let cfg = config::AppConfig::load()?;
        let client_id = cfg.get_client_id().unwrap_or_default();

        if client_id.is_empty() || client_id == "your_client_id_here" {
            warn!("Spotify client_id is empty or default. Starting in unauthenticated mode.");
            return Ok(Self::new_unauthenticated());
        }
        
        let mut client = SpotifyAuth::build_client()?;

        let saved_rt = config::load_refresh_token();

        let needs_auth = if let Some(ref rt) = saved_rt {
            match Self::exchange_refresh_token(rt).await {
                Ok((access_token, expires_in_secs, new_rt)) => {
                    let effective_rt = new_rt.as_deref().unwrap_or(rt.as_str());
                    config::save_refresh_token(effective_rt);

                    use chrono::{Duration, Utc};
                    use std::collections::HashSet;
                    let expires_at = Utc::now() + Duration::try_seconds(expires_in_secs as i64)
                        .unwrap_or_else(|| Duration::try_seconds(3600).unwrap());
                    let scopes: HashSet<String> = [
                        "streaming", "user-read-private", "user-library-read",
                        "user-modify-playback-state", "user-read-playback-state",
                        "user-read-currently-playing", "user-library-modify",
                        "playlist-read-private", "playlist-modify-private",
                        "playlist-modify-public",
                    ].iter().map(|s| s.to_string()).collect();
                    let token = Token {
                        access_token,
                        expires_in: Duration::try_seconds(expires_in_secs as i64)
                            .unwrap_or_else(|| Duration::try_seconds(3600).unwrap()),
                        expires_at: Some(expires_at),
                        refresh_token: Some(effective_rt.to_string()),
                        scopes,
                    };
                    if let Ok(mut guard) = client.token.lock().await {
                        *guard = Some(token);
                    }
                    false
                }
                Err(e) => {
                    warn!("Refresh token exchange failed ({e}), re-authenticating...");
                    true
                }
            }
        } else {
            true
        };

        if needs_auth {
            let url = client
                .get_authorize_url(None)
                .context("Failed to generate authorization URL")?;
            let code = SpotifyAuth::run_oauth_flow(&url).await?;
            client
                .request_token(&code)
                .await
                .context("Failed to exchange code for token")?;
        }

        {
            if let Ok(guard) = client.token.lock().await {
                if let Some(token) = guard.as_ref() {
                    if let Some(rt) = &token.refresh_token {
                        config::save_refresh_token(rt);
                    }
                }
            }
        }

        info!("Authenticated with Spotify");
        let mut spotify = Self {
            client,
            http: reqwest::Client::new(),
            shuffle_state: false,
            repeat_state: RepeatState::Off,
            user_market: None,
            authenticated: true,
        };
        spotify.user_market = spotify.fetch_user_market().await.ok();
        Ok(spotify)
    }

    pub async fn get_access_token(&self) -> Option<String> {
        if !self.authenticated { return None; }
        let guard = self.client.token.lock().await.ok()?;
        guard.as_ref().map(|t| t.access_token.clone())
    }

    async fn fetch_user_market(&self) -> Result<String> {
        let token = self.get_access_token().await
            .ok_or_else(|| anyhow::anyhow!("No access token"))?;
        let json: serde_json::Value = self.http
            .get("https://api.spotify.com/v1/me")
            .bearer_auth(&token)
            .send()
            .await?
            .json()
            .await?;
        json["country"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No country in profile"))
    }

    pub async fn fetch_liked_tracks(&self, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated { return Ok((Vec::new(), 0)); }
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
        if !self.authenticated { return Ok(()); }
        use rspotify::model::PlayableId;
        let id = TrackId::from_uri(track_uri)
            .map_err(|e| anyhow::anyhow!("Invalid track URI: {e}"))?;
        self.client
            .start_uris_playback([PlayableId::Track(id)], None, None, None)
            .await?;
        Ok(())
    }

    pub async fn fetch_playlists(&self) -> Result<Vec<PlaylistSummary>> {
        if !self.authenticated { return Ok(Vec::new()); }
        let mut all = Vec::new();
        let mut offset = 0u32;
        loop {
            let page = self
                .client
                .current_user_playlists_manual(Some(50), Some(offset))
                .await?;
            let fetched = page.items.len() as u32;
            for p in page.items {
                let art_url = p.images.first().map(|img| img.url.clone());
                all.push(PlaylistSummary {
                    id: p.id.id().to_owned(),
                    uri: p.id.uri(),
                    name: p.name,
                    total_tracks: p.items.total,
                    art_url,
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
        if !self.authenticated { return Ok((Vec::new(), 0)); }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let url = format!("https://api.spotify.com/v1/playlists/{playlist_id}/items");
        let response = self.http
            .get(&url)
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
            for item_wrapper in items {
                let track = if !item_wrapper["track"].is_null() {
                    &item_wrapper["track"]
                } else if !item_wrapper["item"].is_null() {
                    &item_wrapper["item"]
                } else {
                    continue;
                };

                if track.is_null() || track["type"].as_str() == Some("episode") {
                    continue;
                }

                let name = track["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = track["artists"].as_array()
                    .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let album = track["album"]["name"].as_str().unwrap_or("").to_string();
                let duration_ms = track["duration_ms"].as_u64().unwrap_or(0);
                let uri = track["uri"].as_str().unwrap_or("").to_string();

                if !uri.is_empty() {
                    tracks.push(TrackSummary { name, artist, album, duration_ms, uri});
                }
            }
        }

        Ok((tracks, total))
    }

    pub async fn play_in_context(&self, playlist_uri: &str, track_uri: &str) -> Result<()> {
        if !self.authenticated { return Ok(()); }
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
        if !self.authenticated { return Ok(PlaybackState::default()); }
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

        let (title, artist, album, path, duration_ms, art_url) = match ctx.item {
            Some(PlayableItem::Track(track)) => {
                let artist = track.artists.iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let duration = track.duration.num_milliseconds().try_into().unwrap_or(0u64);

                let url = if let Some(img) = track.album.images.first() {
                    Some(img.url.clone())
                } else if let Some(id) = &track.id {
                    self.fetch_track_art_url(&id.uri()).await
                } else {
                    None
                };

                (track.name, artist, track.album.name, None, duration, url)
            }
            Some(PlayableItem::Episode(ep)) => {
                let duration = ep.duration.num_milliseconds().try_into().unwrap_or(0u64);
                let url = ep.images.first().map(|img| img.url.clone());
                (ep.name, ep.show.name, String::new(), None, duration, url)
            }
            Some(PlayableItem::Unknown(_)) | None => return Ok(PlaybackState::default()),
        };

        let progress_ms = ctx.progress.and_then(|p| p.num_milliseconds().try_into().ok()).unwrap_or(0u64);

        Ok(PlaybackState {
            title,
            artist,
            album,
            path,
            is_playing: ctx.is_playing,
            shuffle: self.shuffle_state,
            repeat: self.repeat_state,
            progress_ms,
            duration_ms,
            volume: 100,
            art_url,
            is_local: false,
            radio_mode: false,
        })
    }

    pub async fn toggle_playback(&self) -> Result<()> {
        if !self.authenticated { return Ok(()); }
        let ctx = self.client.current_playback(None, None::<&[_]>).await?;
        match ctx {
            Some(c) if c.is_playing => self.client.pause_playback(None).await?,
            _ => self.client.resume_playback(None, None).await?,
        }
        Ok(())
    }

    pub async fn next_track(&self) -> Result<()> {
        if !self.authenticated { return Ok(()); }
        self.client.next_track(None).await?;
        Ok(())
    }

    pub async fn prev_track(&self) -> Result<()> {
        if !self.authenticated { return Ok(()); }
        self.client.previous_track(None).await?;
        Ok(())
    }

    pub async fn search_all(&self, query: &str) -> Result<FullSearchResults> {
        if !self.authenticated {
            return Ok(FullSearchResults {
                tracks: vec![], artists: vec![], albums: vec![], playlists: vec![],
                tracks_total: 0, artists_total: 0, albums_total: 0, playlists_total: 0,
            });
        }
        self.search_internal(query, "track,artist,album,playlist", 0, 10).await
    }

    pub async fn search_more(&self, query: &str, search_type: &str, offset: u32) -> Result<FullSearchResults> {
        if !self.authenticated {
            return Ok(FullSearchResults {
                tracks: vec![], artists: vec![], albums: vec![], playlists: vec![],
                tracks_total: 0, artists_total: 0, albums_total: 0, playlists_total: 0,
            });
        }
        self.search_internal(query, search_type, offset, 10).await
    }

    async fn search_internal(&self, query: &str, search_type: &str, offset: u32, limit: u32) -> Result<FullSearchResults> {
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let limit_str = limit.to_string();
        let response = self.http
            .get("https://api.spotify.com/v1/search")
            .bearer_auth(&token)
            .query(&[
                ("q", query),
                ("type", search_type),
                ("limit", &limit_str),
                ("offset", &offset_str),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;

        let mut tracks = Vec::new();
        let mut tracks_total = 0u32;
        if let Some(obj) = json["tracks"].as_object() {
            tracks_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                    let artist = item["artists"].as_array()
                        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default();
                    let album = item["album"]["name"].as_str().unwrap_or("").to_string();
                    let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    tracks.push(TrackSummary { name, artist, album, duration_ms, uri});
                }
            }
        }

        let mut artists = Vec::new();
        let mut artists_total = 0u32;
        if let Some(obj) = json["artists"].as_object() {
            artists_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    let genres = item["genres"].as_array()
                        .map(|g| g.iter().filter_map(|x| x.as_str()).take(2).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default();
                    artists.push(ArtistSummary { id, name, uri, genres });
                }
            }
        }

        let mut albums = Vec::new();
        let mut albums_total = 0u32;
        if let Some(obj) = json["albums"].as_object() {
            albums_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                    let artist = item["artists"].as_array()
                        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default();
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    let total_tracks = item["total_tracks"].as_u64().unwrap_or(0) as u32;
                    albums.push(AlbumSummary { id, name, artist, uri, total_tracks });
                }
            }
        }

        let mut playlists = Vec::new();
        let mut playlists_total = 0u32;
        if let Some(obj) = json["playlists"].as_object() {
            playlists_total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                    let uri = item["uri"].as_str().unwrap_or("").to_string();
                    let total_tracks = item["tracks"]["total"].as_u64().unwrap_or(0) as u32;
                    let art_url = item["images"].as_array()
                        .and_then(|imgs| imgs.first())
                        .and_then(|img| img["url"].as_str())
                        .map(|s| s.to_string());
                    playlists.push(PlaylistSummary { id, name, uri, total_tracks, art_url });
                }
            }
        }

        Ok(FullSearchResults { tracks, artists, albums, playlists, tracks_total, artists_total, albums_total, playlists_total })
    }

    pub async fn fetch_album_tracks(&self, album_id: &str, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated { return Ok((Vec::new(), 0)); }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let response = self.http
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
                tracks.push(TrackSummary { name, artist, album: String::new(), duration_ms, uri});
            }
        }

        Ok((tracks, total))
    }

    pub async fn save_current_track(&self) -> Result<()> {
        if !self.authenticated { return Ok(()); }
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
        if !self.authenticated { return Ok((Vec::new(), 0)); }
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
            albums.push(AlbumSummary { id, name: album.name, artist, uri, total_tracks });
        }
        Ok((albums, total))
    }

    pub async fn fetch_followed_artists(&self) -> Result<Vec<ArtistSummary>> {
        if !self.authenticated { return Ok(Vec::new()); }
        let page = self
            .client
            .current_user_followed_artists(None, Some(50))
            .await?;
        let mut artists = Vec::new();
        for artist in page.items {
            let id = artist.id.id().to_string();
            let name = artist.name;
            let uri = artist.id.uri();
            let genres = artist.genres.iter().take(2).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            artists.push(ArtistSummary { id, name, uri, genres });
        }
        Ok(artists)
    }

    pub async fn fetch_artist_tracks(&self, artist_name: &str, offset: u32) -> Result<(Vec<TrackSummary>, u32)> {
        if !self.authenticated { return Ok((Vec::new(), 0)); }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let market = self.user_market.as_deref().unwrap_or("BR");
        let query = format!("artist:\"{}\"", artist_name);
        let offset_str = offset.to_string();
        let response = self.http
            .get("https://api.spotify.com/v1/search")
            .bearer_auth(&token)
            .query(&[
                ("q", query.as_str()),
                ("type", "track"),
                ("limit", "10"),
                ("offset", &offset_str),
                ("market", market),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["tracks"]["total"].as_u64().unwrap_or(0) as u32;
        let mut tracks = Vec::new();

        if let Some(items) = json["tracks"]["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let artist = item["artists"].as_array()
                    .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                let album = item["album"]["name"].as_str().unwrap_or("").to_string();
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary { name, artist, album, duration_ms, uri});
            }
        }

        Ok((tracks, total))
    }

    pub async fn fetch_saved_shows(&self, offset: u32) -> Result<(Vec<ShowSummary>, u32)> {
        if !self.authenticated { return Ok((Vec::new(), 0)); }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let response = self.http
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
        if !self.authenticated { return Ok((Vec::new(), 0)); }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let offset_str = offset.to_string();
        let mut query = vec![("limit", "50"), ("offset", &offset_str)];
        let market_owned;
        if let Some(m) = &self.user_market {
            market_owned = m.clone();
            query.push(("market", &market_owned));
        }
        let response = self.http
            .get(format!("https://api.spotify.com/v1/shows/{show_id}/episodes"))
            .bearer_auth(&token)
            .query(&query)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::error!("fetch_show_episodes {status}: {body}");
            return Err(anyhow::anyhow!("Spotify {status}: {body}"));
        }

        let json: serde_json::Value = response.json().await?;
        let total = json["total"].as_u64().unwrap_or(0) as u32;
        let mut tracks = Vec::new();

        if let Some(items) = json["items"].as_array() {
            for item in items {
                let name = item["name"].as_str().unwrap_or("Unknown").to_string();
                let description = item["description"].as_str().unwrap_or("").to_string();
                let artist = {
                    let chars: Vec<char> = description.chars().collect();
                    if chars.len() > 60 {
                        format!("{}…", chars[..60].iter().collect::<String>())
                    } else {
                        description
                    }
                };
                let duration_ms = item["duration_ms"].as_u64().unwrap_or(0);
                let uri = item["uri"].as_str().unwrap_or("").to_string();
                tracks.push(TrackSummary { name, artist, album: String::new(), duration_ms, uri});
            }
        }

        Ok((tracks, total))
    }

    pub fn http_client(&self) -> reqwest::Client {
        self.http.clone()
    }

    async fn exchange_refresh_token(refresh_token: &str) -> Result<(String, u64, Option<String>)> {
        let cfg = config::AppConfig::load()?;
        let client_id = cfg.get_client_id()
            .ok_or_else(|| anyhow::anyhow!("No client_id configured"))?;

        let http = reqwest::Client::new();
        let resp = http
            .post("https://accounts.spotify.com/api/token")
            .form(&[
                ("grant_type",    "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id",     &client_id),
            ])
            .send()
            .await?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            anyhow::bail!("token endpoint {status}: {}", json);
        }

        let access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no access_token in response"))?
            .to_string();
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
        let new_rt = json["refresh_token"].as_str().map(|s| s.to_string());

        Ok((access_token, expires_in, new_rt))
    }

    pub async fn fetch_recommendations(
        &self,
        seed_uris: &[String],
        limit: u8,
    ) -> Result<Vec<TrackSummary>> {
        if !self.authenticated { return Ok(Vec::new()); }
        let token = self
            .get_access_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No access token available"))?;

        let mut seed_artists: Vec<(String, String)> = Vec::new();

        for uri in seed_uris {
            if let Some(id) = uri.strip_prefix("spotify:artist:") {
                if !seed_artists.iter().any(|(i, _)| i == id) {
                    seed_artists.push((id.to_string(), String::new()));
                }
            } else if let Some(track_id) = uri.strip_prefix("spotify:track:") {
                if let Ok(resp) = self.http
                    .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
                    .bearer_auth(&token)
                    .send()
                    .await
                {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        if let (Some(a_id), Some(a_name)) = (
                            json["artists"].as_array().and_then(|a| a.first()).and_then(|a| a["id"].as_str()),
                            json["artists"].as_array().and_then(|a| a.first()).and_then(|a| a["name"].as_str()),
                        ) {
                            if !seed_artists.iter().any(|(i, _)| i == a_id) {
                                seed_artists.push((a_id.to_string(), a_name.to_string()));
                            }
                        }
                    }
                }
            }
        }

        if seed_artists.is_empty() { return Ok(vec![]); }

        let seed_artist_names: Vec<String> = seed_artists.iter().map(|(_, n)| n.clone()).collect();
        let market = self.user_market.as_deref().unwrap_or("BR");

        let mut featured_artists: Vec<String> = Vec::new();

        for (artist_id, _) in seed_artists.iter().take(2) {
            if let Ok(resp) = self.http
                .get(format!("https://api.spotify.com/v1/artists/{artist_id}/albums"))
                .bearer_auth(&token)
                .query(&[("limit", "5"), ("include_groups", "album,single"), ("market", market)])
                .send()
                .await
            {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let album_ids: Vec<String> = json["items"].as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .filter_map(|a| a["id"].as_str())
                        .take(4)
                        .map(|s| s.to_string())
                        .collect();

                    for album_id in &album_ids {
                        if let Ok(resp2) = self.http
                            .get(format!("https://api.spotify.com/v1/albums/{album_id}/tracks"))
                            .bearer_auth(&token)
                            .query(&[("limit", "10"), ("market", market)])
                            .send()
                            .await
                        {
                            if let Ok(json2) = resp2.json::<serde_json::Value>().await {
                                if let Some(items) = json2["items"].as_array() {
                                    for track in items {
                                        if let Some(artists) = track["artists"].as_array() {
                                            for a in artists {
                                                if let Some(name) = a["name"].as_str() {
                                                    let is_seed = seed_artist_names.iter()
                                                        .any(|n| n.eq_ignore_ascii_case(name));
                                                    if !is_seed && !featured_artists.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                                                        featured_artists.push(name.to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if featured_artists.len() >= 10 { break; }
                    }
                }
            }
            if featured_artists.len() >= 10 { break; }
        }

        let mut pool: Vec<TrackSummary> = Vec::new();
        use rand::Rng;
        let mut rng = rand::thread_rng();

        for artist_name in featured_artists.iter().take(8) {
            let offset: u32 = rng.gen_range(0..20);
            let query = format!("artist:\"{}\"", artist_name);
            let offset_str = offset.to_string();
            if let Ok(resp) = self.http
                .get("https://api.spotify.com/v1/search")
                .bearer_auth(&token)
                .query(&[("q", query.as_str()), ("type", "track"), ("limit", "3"), ("offset", &offset_str), ("market", market)])
                .send()
                .await
            {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(tracks) = json["tracks"]["items"].as_array() {
                        for t in tracks {
                            let t_artist = t["artists"].as_array()
                                .and_then(|a| a.first())
                                .and_then(|a| a["name"].as_str())
                                .unwrap_or("")
                                .to_string();
                            if seed_artist_names.iter().any(|n| n.eq_ignore_ascii_case(&t_artist)) {
                                continue;
                            }
                            let name = t["name"].as_str().unwrap_or("Unknown").to_string();
                            let artist = t["artists"].as_array()
                                .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
                                .unwrap_or_default();
                            let uri = t["uri"].as_str().unwrap_or_default().to_string();
                            if !uri.is_empty() {
                                pool.push(TrackSummary {
                                    name, artist,
                                    album: t["album"]["name"].as_str().unwrap_or_default().to_string(),
                                    duration_ms: t["duration_ms"].as_u64().unwrap_or(0),
                                    uri,
                                });
                            }
                        }
                    }
                }
            }
            if pool.len() >= (limit as usize * 2) { break; }
        }

        use rand::seq::SliceRandom;
        pool.shuffle(&mut rand::thread_rng());
        pool.truncate(limit as usize);

        info!("Generated {} manual recommendations", pool.len());
        Ok(pool)
    }

    pub async fn fetch_track_art_url(&self, track_uri: &str) -> Option<String> {
        if !self.authenticated { return None; }
        let track_id = track_uri.strip_prefix("spotify:track:")?;
        let token = self.get_access_token().await?;
        let json: serde_json::Value = self.http
            .get(format!("https://api.spotify.com/v1/tracks/{track_id}"))
            .bearer_auth(&token)
            .send().await.ok()?
            .json().await.ok()?;
        json["album"]["images"].as_array()?
            .last()
            .and_then(|img| img["url"].as_str())
            .map(|s| s.to_string())
    }
}