use std::time::Instant;
use tracing::{info, warn};

use crate::App;

impl App {
    pub(super) fn extract_spotify_content(&self, input: &str) -> Option<(String, String)> {
        let input = input.trim();

        if let Some(start) = input.find("spotify:") {
            let rest = &input[start + 8..];
            if let Some(colon_pos) = rest.find(':') {
                let content_type = &rest[..colon_pos];
                let id_part = &rest[colon_pos + 1..];
                let id = id_part.split(&['?', '/'][..]).next().unwrap_or(id_part);
                if id.len() == 22 {
                    return Some((content_type.to_string(), id.to_string()));
                }
            }
        }

        for content_type in &["track", "playlist", "album", "artist", "episode", "show"] {
            let pattern = format!("{content_type}/");
            if let Some(start) = input.find(&pattern) {
                let id_part = &input[start + pattern.len()..];
                let id = id_part.split(&['?', '/'][..]).next().unwrap_or(id_part);
                if id.len() == 22 {
                    return Some((content_type.to_string(), id.to_string()));
                }
            }
        }

        if input.len() == 22 && input.chars().all(|c| c.is_alphanumeric()) {
            return Some(("track".to_string(), input.to_string()));
        }

        None
    }

    pub async fn play_spotify_content(&mut self, content_type: &str, id: &str) {
        match content_type {
            "track" => self.play_track_by_id(id).await,
            "playlist" => self.play_playlist_by_id(id).await,
            "album" => self.play_album_by_id(id).await,
            "artist" => self.play_artist_by_id(id).await,
            _ => self.state.status_msg = Some(format!("Unsupported content type: {content_type}")),
        }
    }

    async fn play_track_by_id(&mut self, track_id: &str) {
        if !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected - run: isi-music setup-spotify".to_string());
            return;
        }

        self.state.status_msg = Some(format!("Loading track {track_id}..."));
        let track_uri = format!("spotify:track:{track_id}");

        info!("Attempting to play track: {track_uri}");

        self.activate_spotify_player();
        if !self.ensure_spotify_player().await {
            warn!("Failed to create Spotify player");
            self.state.status_msg = Some("Failed to create Spotify player".to_string());
            return;
        }

        let track_summary = self.spotify.fetch_track_summary(track_id).await.ok();

        if let Some(player) = &mut self.player_mgr.player {
            self.current_track_uri = track_uri.clone();
            player.set_queue(vec![track_uri], 0);

            if let Some(t) = track_summary {
                self.state.playback.title = t.name.clone();
                self.state.playback.artist = t.artist.clone();
                self.state.playback.album = t.album.clone();
                self.state.playback.duration_ms = t.duration_ms;
                self.state.playback.art_url = t.cover_path;
                self.state.status_msg = Some(format!("Playing '{}' by {}", t.name, t.artist));
            } else {
                self.state.status_msg = Some(format!("Playing track {track_id}"));
            }

            self.state.playback.progress_ms = 0;
            self.state.playback.is_playing = true;
            self.state.playback.is_local = false;
            self.player_mgr.playing_started_at = Some(Instant::now());
            self.on_track_started();
        } else {
            warn!("No player available");
            self.state.status_msg = Some("No player available".to_string());
        }
    }

    async fn play_playlist_by_id(&mut self, playlist_id: &str) {
        if !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected - run: isi-music setup-spotify".to_string());
            return;
        }

        self.state.status_msg = Some(format!("Loading playlist {playlist_id}..."));

        let mut tracks = Vec::new();
        let mut uris = Vec::new();
        let mut offset = 0u32;

        loop {
            match self
                .spotify
                .fetch_playlist_tracks(playlist_id, offset)
                .await
            {
                Ok((batch, total, page_items)) => {
                    if batch.is_empty() {
                        break;
                    }
                    for t in batch {
                        uris.push(t.uri.clone());
                        tracks.push(t);
                    }
                    offset += page_items;
                    if offset >= total || tracks.len() >= 500 {
                        break;
                    }
                }
                Err(e) => {
                    if tracks.is_empty() {
                        self.state.status_msg = Some(format!("Failed to load playlist: {e}"));
                        return;
                    }
                    break;
                }
            }
        }

        if tracks.is_empty() {
            self.state.status_msg = Some("No tracks found in playlist".to_string());
            return;
        }

        self.activate_spotify_player();
        if !self.ensure_spotify_player().await {
            warn!("Failed to create Spotify player");
            self.state.status_msg = Some("Failed to create Spotify player".to_string());
            return;
        }

        if let Some(player) = &mut self.player_mgr.player {
            let tracks_len = tracks.len();
            if let Some(first) = tracks.first() {
                self.state.playback.title = first.name.clone();
                self.state.playback.artist = first.artist.clone();
                self.state.playback.album = first.album.clone();
                self.state.playback.duration_ms = first.duration_ms;
                self.state.playback.art_url = first.cover_path.clone();
            }
            self.state.tracks = tracks;
            self.state.rebuild_sort_indices();
            self.current_track_uri = uris[0].clone();
            player.set_queue(uris, 0);

            self.state.playback.progress_ms = 0;
            self.state.playback.is_playing = true;
            self.state.playback.is_local = false;
            self.player_mgr.playing_started_at = Some(Instant::now());
            self.state.status_msg = Some(format!("Playing playlist ({tracks_len} tracks)"));
            self.on_track_started();
        } else {
            warn!("No player available");
            self.state.status_msg = Some("No player available".to_string());
        }
    }

    async fn play_album_by_id(&mut self, album_id: &str) {
        if !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected - run: isi-music setup-spotify".to_string());
            return;
        }

        self.state.status_msg = Some(format!("Loading album {album_id}..."));

        let mut tracks = Vec::new();
        let mut uris = Vec::new();
        let mut offset = 0u32;

        loop {
            match self.spotify.fetch_album_tracks(album_id, offset).await {
                Ok((batch, total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    let n = batch.len() as u32;
                    for t in batch {
                        uris.push(t.uri.clone());
                        tracks.push(t);
                    }
                    offset += n;
                    if offset >= total || tracks.len() >= 500 {
                        break;
                    }
                }
                Err(e) => {
                    if tracks.is_empty() {
                        self.state.status_msg = Some(format!("Failed to load album: {e}"));
                        return;
                    }
                    break;
                }
            }
        }

        if tracks.is_empty() {
            self.state.status_msg = Some("No tracks found in album".to_string());
            return;
        }

        self.activate_spotify_player();
        if !self.ensure_spotify_player().await {
            warn!("Failed to create Spotify player");
            self.state.status_msg = Some("Failed to create Spotify player".to_string());
            return;
        }

        if let Some(player) = &mut self.player_mgr.player {
            let tracks_len = tracks.len();
            if let Some(first) = tracks.first() {
                self.state.playback.title = first.name.clone();
                self.state.playback.artist = first.artist.clone();
                self.state.playback.album = first.album.clone();
                self.state.playback.duration_ms = first.duration_ms;
                self.state.playback.art_url = first.cover_path.clone();
            }
            self.state.tracks = tracks;
            self.state.rebuild_sort_indices();
            self.current_track_uri = uris[0].clone();
            player.set_queue(uris, 0);

            self.state.playback.progress_ms = 0;
            self.state.playback.is_playing = true;
            self.state.playback.is_local = false;
            self.player_mgr.playing_started_at = Some(Instant::now());
            self.state.status_msg = Some(format!("Playing album ({tracks_len} tracks)"));
            self.on_track_started();
        } else {
            warn!("No player available");
            self.state.status_msg = Some("No player available".to_string());
        }
    }

    async fn play_artist_by_id(&mut self, artist_id: &str) {
        if !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected - run: isi-music setup-spotify".to_string());
            return;
        }

        self.state.status_msg = Some(format!("Loading artist {artist_id}..."));

        let mut tracks = Vec::new();
        let mut uris = Vec::new();
        let mut offset = 0u32;

        loop {
            match self.spotify.fetch_artist_tracks(artist_id, offset).await {
                Ok((batch, total)) => {
                    if batch.is_empty() {
                        break;
                    }
                    let n = batch.len() as u32;
                    for t in batch {
                        uris.push(t.uri.clone());
                        tracks.push(t);
                    }
                    offset += n;
                    if offset >= total || tracks.len() >= 100 {
                        break;
                    }
                }
                Err(e) => {
                    if tracks.is_empty() {
                        self.state.status_msg = Some(format!("Failed to load artist tracks: {e}"));
                        return;
                    }
                    break;
                }
            }
        }

        if tracks.is_empty() {
            self.state.status_msg = Some("No tracks found for artist".to_string());
            return;
        }

        self.activate_spotify_player();
        if !self.ensure_spotify_player().await {
            warn!("Failed to create Spotify player");
            self.state.status_msg = Some("Failed to create Spotify player".to_string());
            return;
        }

        if let Some(player) = &mut self.player_mgr.player {
            let tracks_len = tracks.len();
            if let Some(first) = tracks.first() {
                self.state.playback.title = first.name.clone();
                self.state.playback.artist = first.artist.clone();
                self.state.playback.album = first.album.clone();
                self.state.playback.duration_ms = first.duration_ms;
                self.state.playback.art_url = first.cover_path.clone();
            }
            self.state.tracks = tracks;
            self.state.rebuild_sort_indices();
            self.current_track_uri = uris[0].clone();
            player.set_queue(uris, 0);

            self.state.playback.progress_ms = 0;
            self.state.playback.is_playing = true;
            self.state.playback.is_local = false;
            self.player_mgr.playing_started_at = Some(Instant::now());
            self.state.status_msg = Some(format!("Playing artist ({tracks_len} tracks)"));
            self.on_track_started();
        } else {
            warn!("No player available");
            self.state.status_msg = Some("No player available".to_string());
        }
    }
}
