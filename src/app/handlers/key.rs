use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use std::time::{Duration, Instant};

use crate::App;

fn smtc_toggle_index() -> usize {
    let base = {
        #[cfg(all(feature = "album-art", feature = "palette"))]
        {
            7
        }
        #[cfg(all(feature = "album-art", not(feature = "palette")))]
        {
            6
        }
        #[cfg(not(feature = "album-art"))]
        {
            5
        }
    };
    #[cfg(windows)]
    {
        base
    }
    #[cfg(not(windows))]
    {
        usize::MAX
    }
}

fn media_keys_toggle_index() -> usize {
    #[cfg(windows)]
    {
        smtc_toggle_index() + 1
    }
    #[cfg(not(windows))]
    {
        usize::MAX
    }
}

impl App {
    pub async fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
        self.state.status_msg = None;

        if self.state.delete_playlist_confirm {
            self.handle_delete_playlist_confirm_key(code).await;
            return Ok(());
        }

        if self.state.add_to_playlist_mode {
            self.handle_add_to_playlist_key(code).await;
            return Ok(());
        }

        if self.state.command_mode {
            self.handle_command_mode_key(code).await;
            return Ok(());
        }

        if self.state.quick_search_active {
            self.handle_quick_search_key(code).await;
            return Ok(());
        }

        if self.state.search_active {
            return self.handle_search_key(code).await;
        }

        if let Some(ref mut panel) = self.settings_panel
            && panel.visible
        {
            use crate::ui::options::{SettingsAction, SettingsSection};
            match panel.handle_key(code) {
                SettingsAction::Close => {
                    panel.visible = false;
                    panel.save_config();
                    self.state.status_msg = Some("Settings panel closed".to_string());
                }
                SettingsAction::ToggleItem => match panel.focused_section {
                    SettingsSection::General => {
                        let idx = panel.selected_item;
                        #[cfg(feature = "album-art")]
                        if idx == 0 {
                            self.state.show_album_art = !self.state.show_album_art;
                            panel.config.ui.show_cover_images = Some(self.state.show_album_art);
                            panel.save_config();
                            self.state.status_msg = Some(if self.state.show_album_art {
                                "Cover images enabled".to_string()
                            } else {
                                "Cover images disabled".to_string()
                            });
                            return Ok(());
                        }
                        #[cfg(feature = "album-art")]
                        let idx = idx - 1;
                        match idx {
                            0 => {
                                let v = !panel.config.enable_lyrics();
                                panel.config.ui.enable_lyrics = Some(v);
                                panel.save_config();
                                self.state.status_msg = Some(if v {
                                    "Lyrics fetching enabled".to_string()
                                } else {
                                    "Lyrics fetching disabled".to_string()
                                });
                            }
                            1 => {
                                self.state.show_visualizer = !self.state.show_visualizer;
                                panel.config.ui.show_visualizer = Some(self.state.show_visualizer);
                                panel.save_config();
                                if let Some(player) = &mut self.player_mgr.player {
                                    player.set_visualizer_enabled(self.state.show_visualizer);
                                }
                                self.state.status_msg = Some(if self.state.show_visualizer {
                                    "Visualizer enabled".to_string()
                                } else {
                                    "Visualizer disabled".to_string()
                                });
                            }
                            2 => {
                                self.state.compact_mode = !self.state.compact_mode;
                                panel.config.ui.compact_mode_default =
                                    Some(self.state.compact_mode);
                                panel.save_config();
                                self.state.status_msg = Some(if self.state.compact_mode {
                                    "Compact mode on".to_string()
                                } else {
                                    "Compact mode off".to_string()
                                });
                            }
                            3 => {
                                self.state.show_breadcrumb = !self.state.show_breadcrumb;
                                panel.config.ui.show_breadcrumb = Some(self.state.show_breadcrumb);
                                panel.save_config();
                                self.state.status_msg = Some(if self.state.show_breadcrumb {
                                    "Breadcrumb on".to_string()
                                } else {
                                    "Breadcrumb off".to_string()
                                });
                            }
                            4 => {
                                self.toggle_lastfm_scrobbling().await;
                            }
                            5 => {
                                let v = !panel.config.autoplay_enabled();
                                panel.config.ui.autoplay = Some(v);
                                self.player_mgr.autoplay_enabled = v;
                                panel.save_config();
                                self.state.status_msg = Some(if v {
                                    "Autoplay enabled".to_string()
                                } else {
                                    "Autoplay disabled".to_string()
                                });
                            }
                            #[cfg(all(feature = "album-art", feature = "palette"))]
                            6 => {
                                let enabled = !self.theme_mgr.reactive_theme_enabled();
                                if let Err(e) = self.theme_mgr.toggle_reactive(enabled) {
                                    self.state.status_msg =
                                        Some(format!("Failed to toggle reactive theme: {e}"));
                                } else {
                                    self.state.reactive_theme_enabled = enabled;
                                    #[cfg(all(feature = "palette", feature = "album-art"))]
                                    if enabled {
                                        self.theme_mgr.set_reactive_toggle_pending(true);
                                        if let Some(swatches) = self.theme_mgr.swatches_clone() {
                                            self.theme_mgr.start_reactive(&swatches, &self.ui);
                                        } else {
                                            self.fetcher.last_art_uri.clear();
                                        }
                                    } else {
                                        let restored = self.theme_mgr.disable_reactive();
                                        self.ui = crate::ui::Ui::new(
                                            restored,
                                            self.debug_overlay.clone(),
                                        );
                                    }
                                    self.state.status_msg = Some(if enabled {
                                        "Reactive theme enabled: colors will adapt to album art"
                                            .to_string()
                                    } else {
                                        "Reactive theme disabled".to_string()
                                    });
                                }
                            }
                            n if n == smtc_toggle_index() => {
                                #[cfg(windows)]
                                {
                                    let v = !panel.config.smtc_enabled();
                                    panel.config.options.smtc_enabled = Some(v);
                                    panel.save_config();
                                    self.state.status_msg = Some(if v {
                                        "SMTC enabled (restart to apply)".to_string()
                                    } else {
                                        "SMTC disabled (restart to apply)".to_string()
                                    });
                                }
                                #[cfg(not(windows))]
                                {
                                    let _ = n;
                                }
                            }
                            n if n == media_keys_toggle_index() => {
                                #[cfg(windows)]
                                {
                                    let v = !panel.config.media_keys_enabled();
                                    panel.config.options.media_keys_enabled = Some(v);
                                    panel.save_config();
                                    self.state.status_msg = Some(if v {
                                        "Media hotkeys enabled (restart to apply)".to_string()
                                    } else {
                                        "Media hotkeys disabled (restart to apply)".to_string()
                                    });
                                }
                                #[cfg(not(windows))]
                                {
                                    let _ = n;
                                }
                            }
                            _ => {}
                        }
                    }
                    SettingsSection::Account => {
                        let idx = panel.selected_item;
                        if idx == 3 {
                            let v = !panel.config.discord.enabled.unwrap_or(false);
                            panel.config.discord.enabled = Some(v);
                            panel.save_config();
                            self.state.status_msg = Some(if v {
                                "Discord Rich Presence enabled".to_string()
                            } else {
                                "Discord Rich Presence disabled".to_string()
                            });
                        }
                    }
                    _ => {}
                },
                SettingsAction::ClearAllCache => {
                    let _ = panel.cache_manager.clear_all().await;
                    self.spotify.library_cache.clear_all_library_cache();
                    panel.cache_stats = Some(panel.cache_manager.get_stats().await);
                    self.state.status_msg = Some("All caches cleared".to_string());
                }
                SettingsAction::CleanupExpired => {
                    let _ = panel.cache_manager.cleanup_expired().await;
                    panel.cache_stats = Some(panel.cache_manager.get_stats().await);
                    self.state.status_msg = Some("Expired cache entries cleaned up".to_string());
                }
                SettingsAction::RefreshStats => {
                    panel.load_cache_stats().await;
                    self.state.status_msg = Some("Cache stats refreshed".to_string());
                }
                SettingsAction::RefreshPlaylists => {
                    if self.spotify.authenticated {
                        match self.spotify.fetch_playlists().await {
                            Ok(playlists) => {
                                self.state.playlists = playlists;
                                if !self.state.playlists.is_empty() {
                                    self.state.playlist_list.select(Some(0));
                                }
                                self.state.status_msg = Some("Playlists refreshed".to_string());
                            }
                            Err(e) => {
                                self.state.status_msg =
                                    Some(format!("Failed to refresh playlists: {e}"));
                            }
                        }
                    } else {
                        self.state.status_msg = Some(
                            "Spotify not connected - run: isi-music setup-spotify".to_string(),
                        );
                    }
                }
                SettingsAction::SetupSpotify => {
                    self.state.status_msg =
                        Some("Exit isi-music and run: isi-music setup-spotify".to_string());
                }
                SettingsAction::SetupLastfm => {
                    self.state.status_msg =
                        Some("Exit isi-music and run: isi-music setup-lastfm".to_string());
                }
                SettingsAction::EditMusicDir => {
                    self.state.status_msg =
                        Some("Type the music dir path, Enter to save".to_string());
                }
                SettingsAction::SaveMusicDir => {
                    self.state.status_msg = Some(
                        "Music dir saved. Select Local Files and press Enter to scan".to_string(),
                    );
                    self.needs_redraw = true;
                }
                SettingsAction::None => {}
            }
            return Ok(());
        }

        match code {
            KeyCode::Left | KeyCode::Right => {
                let now = Instant::now();
                let is_held = self
                    .last_seek_time
                    .map(|t| t.elapsed() < Duration::from_millis(300))
                    .unwrap_or(false);

                if is_held {
                    self.seek_hold_count += 1;
                } else {
                    self.seek_hold_count = 0;
                }
                self.last_seek_time = Some(now);

                let step_ms = if self.seek_hold_count > 4 {
                    10_000
                } else {
                    5_000
                };

                let new_pos = match code {
                    KeyCode::Right => {
                        let d = self.state.playback.duration_ms;
                        let target = self.state.playback.progress_ms + step_ms;
                        if d > 0 { target.min(d) } else { target }
                    }
                    _ => self.state.playback.progress_ms.saturating_sub(step_ms),
                };

                self.state.playback.progress_ms = new_pos;
                self.player_mgr.progress_at_play_start = new_pos;
                if self.state.playback.is_playing {
                    self.player_mgr.playing_started_at = Some(Instant::now());
                }
                let _ = self.seek_tx.send(new_pos as u32);
                return Ok(());
            }
            _ => {}
        }

        if let Some(action) = self.keybinds.lookup(code, modifiers) {
            self.dispatch(action).await;
        }

        Ok(())
    }
}
