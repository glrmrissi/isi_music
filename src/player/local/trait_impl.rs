use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use super::LocalPlayer;
use super::track::LocalTrack;
use crate::player::{AudioPlayer, PlayerNotification, QueuedTrack, RepeatMode};
use crate::spotify::TrackSummary;

impl AudioPlayer for LocalPlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.playing_queued = None;
        if uris.is_empty() {
            self.playing_context = None;
            self.playing_context_pos = None;
            self.temp_playing = None;
            self.temp_queue.clear();
            self.sink.stop();
            self.is_playing = false;
            self.current_idx = None;
            return;
        }
        let target_uri = match uris.get(start_index) {
            Some(u) => u.clone(),
            None => return,
        };
        if let Some(idx) = self.queue.iter().position(|t| t.uri == target_uri) {
            self.playing_context = Some(uris);
            self.playing_context_pos = Some(start_index);
            self.load_and_play(idx);
        }
    }

    fn set_queue_tracks(&mut self, tracks: &[TrackSummary], start_index: usize) {
        self.playing_queued = None;
        let uris: Vec<String> = tracks.iter().map(|t| t.uri.clone()).collect();
        let target_uri = match uris.get(start_index) {
            Some(u) => u.clone(),
            None => return,
        };
        if let Some(idx) = self.queue.iter().position(|t| t.uri == target_uri) {
            self.playing_context = Some(uris);
            self.playing_context_pos = Some(start_index);
            self.load_and_play(idx);
        }
    }

    fn add_to_queue(
        &mut self,
        uri: String,
        name: String,
        artist: String,
        album: String,
        duration_ms: u64,
        cover_path: Option<PathBuf>,
    ) {
        if !uri.starts_with("file://") {
            tracing::debug!("LocalPlayer: ignoring non-local queue URI: {}", uri);
            return;
        }
        self.user_queue.push(QueuedTrack {
            uri,
            name,
            artist,
            album,
            duration_ms,
            cover_path,
        });
    }

    fn user_queue(&self) -> &[QueuedTrack] {
        &self.user_queue
    }

    fn remove_from_user_queue(&mut self, index: usize) {
        if index < self.user_queue.len() {
            self.user_queue.remove(index);
        }
    }

    fn take_playing_queued(&mut self) -> Option<QueuedTrack> {
        self.playing_queued.take()
    }

    fn play_from_user_queue(&mut self, index: usize) -> bool {
        if index >= self.user_queue.len() {
            return false;
        }
        let track = &self.user_queue[index];
        if !track.uri.starts_with("file://") {
            tracing::warn!("Skipping non-local track from user queue: {}", track.uri);
            self.user_queue.remove(index);
            return false;
        }
        let path = LocalTrack::uri_to_path(&track.uri);
        if !path.exists() {
            tracing::warn!("File not found for queued track: {}", path.display());
            self.user_queue.remove(index);
            return false;
        }
        let track = self.user_queue.remove(index);
        let lt = LocalTrack {
            path,
            uri: track.uri.clone(),
            name: track.name.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            duration_ms: track.duration_ms,
            cover_path: track.cover_path.clone(),
        };
        let previous_temp_queue = super::queue::replace_temp_track(&mut self.temp_queue, lt);
        let previous_temp_playing = self.temp_playing;
        if self.load_temp_track(0) {
            self.playing_queued = Some(track);
            return true;
        }
        self.temp_queue = previous_temp_queue;
        self.temp_playing = previous_temp_playing;
        false
    }

    fn play(&mut self) {
        self.sink.play();
        self.is_playing = true;
    }

    fn pause(&mut self) {
        self.sink.pause();
        self.is_playing = false;
    }

    fn toggle(&mut self) {
        if self.sink.is_paused() {
            self.play();
        } else {
            self.pause();
        }
    }

    fn next(&mut self) -> bool {
        self.next_inner()
    }

    fn prev(&mut self) -> bool {
        self.prev_inner()
    }

    fn play_at(&mut self, index: usize) {
        self.playing_context = None;
        self.playing_context_pos = None;
        self.playing_queued = None;
        self.load_and_play(index);
    }

    fn seek(&self, position_ms: u32) {
        let pos = std::time::Duration::from_millis(position_ms as u64);
        self.is_seeking.store(true, Ordering::SeqCst);
        if let Err(e) = self.sink.try_seek(pos) {
            tracing::warn!("Seek failed: {:?}", e);
            self.is_seeking.store(false, Ordering::SeqCst);
        } else {
            let _ = self.seek_deadline.lock().map(|mut d| {
                *d = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
            });
        }
    }

    fn seek_mut(&mut self, position_ms: u32) {
        self.seek_by_reload(position_ms);
    }

    fn is_playing(&self) -> bool {
        self.is_playing
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn shuffle(&self) -> bool {
        self.shuffle
    }

    fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    fn current_index(&self) -> Option<usize> {
        self.current_idx
    }

    fn current_track_summary(&self) -> Option<TrackSummary> {
        self.current_track_meta().map(|t| TrackSummary {
            name: t.name.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
            uri: t.uri.clone(),
            cover_path: t
                .cover_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            added_at: None,
        })
    }

    fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.apply_volume();
    }

    fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.apply_volume();
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.apply_volume();
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.play_history.clear();
    }

    fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };
    }

    fn try_recv_event(&mut self) -> Option<PlayerNotification> {
        self.poll_sink();
        self.event_rx.try_recv().ok()
    }

    fn set_visualizer_enabled(&mut self, enabled: bool) {
        self.analyzer.set_enabled(enabled);
    }

    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        self.analyzer.band_energies()
    }

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        let uris = self.queue.iter().map(|t| t.uri.clone()).collect();
        (uris, self.current_idx)
    }

    fn snapshot_user_queue(&self) -> Vec<QueuedTrack> {
        self.user_queue.clone()
    }

    fn current_playback_state(&self) -> Option<crate::ui::PlaybackState> {
        let t = self.current_track_meta()?;
        Some(crate::ui::PlaybackState {
            title: if t.name.is_empty() {
                t.path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown Track")
                    .to_string()
            } else {
                t.name.clone()
            },
            artist: t.artist.clone(),
            album: if t.album.is_empty() {
                "Local Archive".to_string()
            } else {
                t.album.clone()
            },
            cover_path: t
                .cover_path
                .as_ref()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string()),
            duration_ms: t.duration_ms,
            is_playing: self.is_playing,
            volume: self.volume,
            shuffle: self.shuffle,
            repeat: match self.repeat {
                super::RepeatMode::Off => crate::spotify::RepeatState::Off,
                super::RepeatMode::Queue => crate::spotify::RepeatState::Context,
                super::RepeatMode::Track => crate::spotify::RepeatState::Track,
            },
            is_local: true,
            art_url: None,
            waveform: self.waveform(),
            ..crate::ui::PlaybackState::default()
        })
    }
}
