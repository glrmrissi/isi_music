use crossterm::event::KeyCode;

use crate::App;

impl App {
    pub async fn handle_add_to_playlist_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                let len = self.state.playlists.len() + 1;
                let i = self.state.add_to_playlist_list.selected().unwrap_or(0);
                let next = if i == 0 { len - 1 } else { i - 1 };
                self.state.add_to_playlist_list.select(Some(next));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.state.playlists.len() + 1;
                let i = self.state.add_to_playlist_list.selected().unwrap_or(0);
                let next = if i >= len - 1 { 0 } else { i + 1 };
                self.state.add_to_playlist_list.select(Some(next));
            }
            KeyCode::Enter => {
                self.add_to_playlist_confirm().await;
            }
            KeyCode::Esc => {
                self.state.add_to_playlist_mode = false;
                self.state.status_msg = Some("Cancelled".to_string());
            }
            _ => {}
        }
    }

    pub async fn handle_command_mode_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter => {
                let cmd = self.state.command_buffer.trim().to_string();
                self.state.command_mode = false;
                self.state.command_buffer.clear();
                self.handle_command(&cmd).await;
            }
            KeyCode::Esc => {
                self.state.command_mode = false;
                self.state.command_buffer.clear();
                self.state.status_msg = Some("Cancelled".to_string());
            }
            KeyCode::Backspace => {
                self.state.command_buffer.pop();
            }
            KeyCode::Char(c) if c.is_ascii() => {
                self.state.command_buffer.push(c);
            }
            _ => {}
        }
    }

    pub async fn handle_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }
        match parts[0] {
            "play" | "open" => {
                let input = parts[1..].join(" ");
                if let Some((content_type, id)) = self.extract_spotify_content(&input) {
                    self.play_spotify_content(&content_type, &id).await;
                } else {
                    self.state.status_msg =
                        Some("Could not extract valid Spotify ID/URI from input".to_string());
                }
            }
            "ap" | "addtoplaylist" => {
                let name = parts[1..].join(" ");
                self.add_current_track_to_playlist_by_name(&name).await;
            }
            "newplaylist" | "createplaylist" => {
                let name = parts[1..].join(" ");
                if name.is_empty() {
                    self.state.status_msg = Some("Usage: newplaylist <name>".to_string());
                    return;
                }
                if !self.spotify.authenticated {
                    self.state.status_msg =
                        Some("Spotify not connected - run: isi-music setup-spotify".to_string());
                    return;
                }
                match self.spotify.create_playlist(&name, false, None).await {
                    Ok(p) => {
                        self.state.status_msg = Some(format!("Created playlist '{}'", p.name));
                        if let Ok(playlists) = self.spotify.fetch_playlists().await {
                            self.state.playlists = playlists;
                        }
                    }
                    Err(e) => {
                        self.state.status_msg = Some(format!("Failed: {e}"));
                    }
                }
            }
            _ => {
                if let Some((content_type, id)) = self.extract_spotify_content(cmd) {
                    self.play_spotify_content(&content_type, &id).await;
                } else {
                    self.state.status_msg = Some(format!("Unknown command: {}", parts[0]));
                }
            }
        }
    }

    async fn add_current_track_to_playlist_by_name(&mut self, name: &str) {
        let lower = name.to_lowercase();
        let playlist = self
            .state
            .playlists
            .iter()
            .find(|p| p.name.to_lowercase().contains(&lower));
        let Some(playlist) = playlist else {
            self.state.status_msg = Some(format!("No playlist found matching '{name}'"));
            return;
        };
        let uri = &self.current_track_uri;
        if uri.is_empty() {
            self.state.status_msg = Some("No track playing".to_string());
            return;
        }
        match self
            .spotify
            .add_tracks_to_playlist(&playlist.id, std::slice::from_ref(uri), None)
            .await
        {
            Ok(_) => {
                self.state.status_msg = Some(format!("Added to '{}'", playlist.name));
            }
            Err(e) => {
                self.state.status_msg = Some(format!("Failed: {e}"));
            }
        }
    }

    async fn add_to_playlist_confirm(&mut self) {
        let idx = match self.state.add_to_playlist_list.selected() {
            Some(i) => i,
            None => return,
        };
        self.state.add_to_playlist_mode = false;

        if idx == self.state.playlists.len() {
            // "Create new playlist" option selected
            self.state.command_mode = true;
            self.state.command_buffer = "newplaylist ".to_string();
            self.state.status_msg = Some("Enter playlist name:".to_string());
            return;
        }

        let uri = &self.current_track_uri;
        if uri.is_empty() {
            self.state.status_msg = Some("No track playing".to_string());
            return;
        }

        if let Some(playlist) = self.state.playlists.get(idx) {
            match self
                .spotify
                .add_tracks_to_playlist(&playlist.id, std::slice::from_ref(uri), None)
                .await
            {
                Ok(_) => {
                    self.state.status_msg = Some(format!("Added to '{}'", playlist.name));
                    self.spotify
                        .library_cache
                        .delete_key_pattern(&format!("playlist:{}:%", playlist.id));
                }
                Err(e) => {
                    self.state.status_msg = Some(format!("Failed: {e}"));
                }
            }
        }
    }
}
