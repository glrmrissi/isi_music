use chrono::Utc;

use crate::App;
use crate::utils::debug_overlay::LogLevel;

impl App {
    pub(super) async fn dispatch_like_track(&mut self) {
        if !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected - run: isi-music setup-spotify".to_string());
        } else if self.current_track_uri.is_empty() {
            self.debug_overlay.log(
                LogLevel::Warn,
                "LikeTrack: no current track URI".to_string(),
            );
            self.state.status_msg = Some("No track to like".to_string());
        } else {
            self.state.status_msg = Some("Liking...".to_string());
            let Some(token) = self.spotify.get_access_token().await else {
                self.state.status_msg = Some("Like failed: no token".to_string());
                return;
            };
            let track_id = self
                .current_track_uri
                .rsplit(':')
                .next()
                .unwrap_or("")
                .to_string();
            if track_id.is_empty() {
                self.state.status_msg = Some("Like failed: empty track ID".to_string());
                return;
            }
            match crate::spotify::save_track_http(&self.spotify.http, &token, &track_id).await {
                Ok(_) => {
                    self.state.status_msg = Some("Liked".to_string());
                    let new_track = crate::spotify::TrackSummary {
                        name: self.state.playback.title.clone(),
                        artist: self.state.playback.artist.clone(),
                        album: self.state.playback.album.clone(),
                        duration_ms: self.state.playback.duration_ms,
                        uri: self.current_track_uri.clone(),
                        cover_path: self.state.playback.cover_path.clone(),
                        added_at: None,
                    };
                    if self.state.active_playlist_id.as_deref() == Some("liked_songs") {
                        self.state.tracks.insert(0, new_track.clone());
                        self.state.tracks_offset += 1;
                        self.state.tracks_total += 1;
                        self.state.rebuild_sort_indices();
                    }
                    let library_cache = self.spotify.library_cache.clone();
                    let added_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    tokio::task::spawn_blocking(move || {
                        library_cache.insert_liked_track(&added_at, &new_track);
                    })
                    .await
                    .ok();
                    tracing::info!("LikeTrack: saved successfully");
                }
                Err(e) => {
                    self.state.status_msg = Some(format!("Like failed: {e}"));
                    tracing::error!("LikeTrack failed: {e}");
                }
            }
        }
    }

    pub(super) async fn dispatch_copy_track_link(&mut self) {
        let url = self
            .current_track_uri
            .strip_prefix("spotify:track:")
            .map(|id| format!("https://open.spotify.com/track/{id}"))
            .unwrap_or_default();
        if url.is_empty() {
            self.state.status_msg = Some("No track playing".to_string());
            self.debug_overlay
                .log(LogLevel::Warn, "CopyTrackLink: no track playing");
        } else {
            // Try arboard (cross-platform) first, fall back to xclip/wl-copy on Linux
            let copied = match arboard::Clipboard::new() {
                Ok(mut cb) => cb.set_text(&url).is_ok(),
                Err(_) => false,
            };
            if !copied {
                // Fallback: xclip (X11) / wl-copy (Wayland)
                let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
                let cmd = if wayland { "wl-copy" } else { "xclip" };
                let args: &[&str] = if wayland {
                    &[]
                } else {
                    &["-selection", "clipboard"]
                };
                let mut child = match std::process::Command::new(cmd)
                    .args(args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        self.state.status_msg = Some(format!("Copy failed: {cmd} not found ({e})"));
                        self.debug_overlay.log(
                            LogLevel::Error,
                            format!("CopyTrackLink: failed to spawn {cmd}: {e}"),
                        );
                        return;
                    }
                };
                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    let _ = stdin.write_all(url.as_bytes());
                }
                match child.wait() {
                    Ok(status) if status.success() => {
                        self.state.status_msg = Some(format!("Link copied: {url}"));
                        self.debug_overlay
                            .log(LogLevel::Info, format!("CopyTrackLink: copied {url}"));
                    }
                    Ok(status) => {
                        self.state.status_msg =
                            Some(format!("Copy failed: {cmd} exited with {status}"));
                        self.debug_overlay.log(
                            LogLevel::Error,
                            format!("CopyTrackLink: {cmd} exited with {status}"),
                        );
                        return;
                    }
                    Err(e) => {
                        self.state.status_msg = Some(format!("Copy failed: {cmd} error ({e})"));
                        self.debug_overlay.log(
                            LogLevel::Error,
                            format!("CopyTrackLink: {cmd} wait error: {e}"),
                        );
                        return;
                    }
                }
            } else {
                self.state.status_msg = Some(format!("Link copied: {url}"));
                self.debug_overlay
                    .log(LogLevel::Info, format!("CopyTrackLink: copied {url}"));
            }
        }
    }

    pub(super) async fn dispatch_remove_from_playlist(&mut self) {
        if !self.spotify.authenticated {
            self.state.status_msg =
                Some("Spotify not connected - run: isi-music setup-spotify".to_string());
        } else {
            let is_playlist = self
                .state
                .active_playlist_uri
                .as_deref()
                .map(|u| u.starts_with("spotify:playlist:"))
                .unwrap_or(false);

            if is_playlist {
                // Get the selected track from the tracks view
                let track_uri = self.state.track_list.selected().and_then(|display_idx| {
                    let actual_idx = self.state.sorted_track_indices.get(display_idx)?;
                    self.state.tracks.get(*actual_idx).map(|t| t.uri.clone())
                });

                let (track_uri, playlist_id) =
                    match (track_uri, &self.state.active_playlist_id.clone()) {
                        (Some(uri), Some(pid)) => (uri, pid.clone()),
                        _ => {
                            self.state.status_msg = Some("No track selected".to_string());
                            return;
                        }
                    };

                self.state.status_msg = Some("Removing from playlist...".to_string());
                match self
                    .spotify
                    .remove_tracks_from_playlist(&playlist_id, &[track_uri])
                    .await
                {
                    Ok(_) => {
                        if let Some(idx) = self.state.track_list.selected() {
                            let actual_idx = self
                                .state
                                .sorted_track_indices
                                .get(idx)
                                .copied()
                                .unwrap_or(idx);
                            if actual_idx < self.state.tracks.len() {
                                self.state.tracks.remove(actual_idx);
                            }
                        }
                        self.state.tracks_total = self.state.tracks_total.saturating_sub(1);
                        self.state.rebuild_sort_indices();
                        for pl in &mut self.state.playlists {
                            if Some(&pl.id) == self.state.active_playlist_id.as_ref() {
                                pl.total_tracks = pl.total_tracks.saturating_sub(1);
                            }
                        }
                        self.state.status_msg = Some("Removed from playlist".to_string());
                        self.spotify
                            .library_cache
                            .delete_key_pattern(&format!("playlist:{}:%", playlist_id));
                    }
                    Err(e) => {
                        self.state.status_msg = Some(format!("Remove failed: {e}"));
                    }
                }
            } else if self.current_track_uri.is_empty() {
                self.state.status_msg = Some("No track playing".to_string());
            } else {
                // Not a playlist — check if track is liked
                let track_id = self
                    .current_track_uri
                    .rsplit(':')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if track_id.is_empty() {
                    self.state.status_msg = Some("Invalid track".to_string());
                    return;
                }

                self.state.status_msg = Some("Checking...".to_string());
                match self.spotify.check_track_saved(&track_id).await {
                    Ok(true) => {
                        let Some(token) = self.spotify.get_access_token().await else {
                            self.state.status_msg = Some("Unlike failed: no token".to_string());
                            return;
                        };
                        match crate::spotify::unlike_track_http(
                            &self.spotify.http,
                            &token,
                            &track_id,
                        )
                        .await
                        {
                            Ok(_) => {
                                self.state.status_msg = Some("Unliked".to_string());
                                let uri = self.current_track_uri.clone();
                                if self.state.active_playlist_id.as_deref() == Some("liked_songs") {
                                    if let Some(pos) =
                                        self.state.tracks.iter().position(|t| t.uri == uri)
                                    {
                                        self.state.tracks.remove(pos);
                                        self.state.tracks_offset =
                                            self.state.tracks_offset.saturating_sub(1);
                                        self.state.tracks_total =
                                            self.state.tracks_total.saturating_sub(1);
                                        self.state.rebuild_sort_indices();
                                    }
                                }
                                let library_cache = self.spotify.library_cache.clone();
                                tokio::task::spawn_blocking(move || {
                                    library_cache.delete_liked_track(&uri);
                                })
                                .await
                                .ok();
                                tracing::info!("UnlikeTrack: removed from library");
                            }
                            Err(e) => {
                                self.state.status_msg = Some(format!("Unlike failed: {e}"));
                            }
                        }
                    }
                    Ok(false) => {
                        self.state.status_msg = Some("Track is not liked".to_string());
                    }
                    Err(e) => {
                        self.state.status_msg = Some(format!("Check failed: {e}"));
                    }
                }
            }
        }
    }
}
