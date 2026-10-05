use crate::config;
use crate::spotify::TrackSummary;
use crate::ui::PlaybackState;
use anyhow::Result;
use librespot_core::spotify_uri::SpotifyUri;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, atomic::Ordering};
use tracing::{error, info};

use super::NativePlayer;
use crate::player::{AudioPlayer, PlayerNotification, QueuedTrack, RepeatMode};

impl AudioPlayer for NativePlayer {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.set_queue(uris, start_index);
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
        self.add_to_queue(uri, name, artist, album, duration_ms, cover_path);
    }
    fn user_queue(&self) -> &[QueuedTrack] {
        self.user_queue()
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
        let track = self.user_queue.remove(index);
        match SpotifyUri::from_uri(&track.uri) {
            Ok(spotify_uri) => {
                info!("Playing from user queue: {}", track.uri);
                self.player.stop();
                self.player.load(spotify_uri, true, 0);
                self.is_playing = true;
                self.playing_queued = Some(track);
                self.preload_next();
                true
            }
            Err(e) => {
                error!("Invalid URI in user queue: {e}");
                false
            }
        }
    }

    fn play(&mut self) {
        self.play();
    }
    fn pause(&mut self) {
        self.pause();
    }
    fn toggle(&mut self) {
        self.toggle();
    }
    fn next(&mut self) -> bool {
        self.next()
    }
    fn prev(&mut self) -> bool {
        self.prev()
    }
    fn play_at(&mut self, index: usize) {
        self.play_at(index);
    }
    fn seek(&self, position_ms: u32) {
        self.seek(position_ms);
    }
    fn seek_mut(&mut self, position_ms: u32) {
        self.seek(position_ms);
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
        self.current_index()
    }
    fn current_track_summary(&self) -> Option<TrackSummary> {
        self.queue.get(self.current_index?).map(|uri| TrackSummary {
            name: String::new(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            uri: uri.clone(),
            cover_path: None,
            added_at: None,
        })
    }

    fn trim_played(&mut self, keep_behind: usize) -> bool {
        self.trim_played(keep_behind)
    }

    fn volume_up(&mut self) {
        self.volume_up();
    }
    fn volume_down(&mut self) {
        self.volume_down();
    }
    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
        self.apply_volume();
        config::save_volume(self.volume);
    }
    fn toggle_shuffle(&mut self) {
        self.toggle_shuffle();
    }
    fn cycle_repeat(&mut self) {
        self.cycle_repeat();
    }

    fn try_recv_event(&mut self) -> Option<PlayerNotification> {
        while let Ok(notif) = self.event_rx.try_recv() {
            match &notif {
                PlayerNotification::Playing
                    if self.audio_output_lost || self.audio_output_loss.is_lost() =>
                {
                    continue;
                }
                PlayerNotification::Playing => self.is_playing = true,
                PlayerNotification::Paused => self.is_playing = false,
                _ => {}
            }
            return Some(notif);
        }
        if self.audio_output_loss.take_notification() {
            self.audio_output_lost = true;
            return Some(PlayerNotification::AudioOutputLost);
        }
        if self.audio_output_loss.take_restored_notification() {
            self.audio_output_lost = false;
            self.is_playing = true;
            return Some(PlayerNotification::AudioOutputRestored);
        }
        None
    }

    fn preload_next(&mut self) {
        self.preload_next();
    }

    fn set_visualizer_enabled(&mut self, enabled: bool) {
        self.analyzer_enabled.store(enabled, Ordering::Relaxed);
    }

    fn set_mono_enabled(&mut self, enabled: bool) {
        self.mono_enabled.store(enabled, Ordering::Relaxed);
    }

    fn set_eq_gains(&mut self, gains: [i8; crate::audio::eq::EQ_BANDS]) {
        self.eq_gains
            .store(crate::audio::eq::pack_gains(&gains), Ordering::Relaxed);
    }

    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        if self.analyzer_enabled.load(Ordering::Relaxed) {
            Some(Arc::clone(&self.band_energies))
        } else {
            None
        }
    }

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        self.snapshot_queue()
    }

    fn snapshot_user_queue(&self) -> Vec<QueuedTrack> {
        self.snapshot_user_queue()
    }

    fn current_playback_state(&self) -> Option<PlaybackState> {
        let guard = self.server_position.lock().ok()?;
        let (base, recorded_at) = *guard;
        let elapsed = if self.is_playing {
            recorded_at.elapsed().as_millis() as u64
        } else {
            0
        };
        Some(PlaybackState {
            is_playing: self.is_playing,
            volume: self.volume,
            shuffle: self.shuffle,
            repeat: match self.repeat {
                RepeatMode::Off => crate::spotify::RepeatState::Off,
                RepeatMode::Queue => crate::spotify::RepeatState::Context,
                RepeatMode::Track => crate::spotify::RepeatState::Track,
            },
            is_local: false,
            progress_ms: base.saturating_add(elapsed),
            ..PlaybackState::default()
        })
    }

    fn fetch_playlist_via_mercury<'a>(
        &'a self,
        playlist_uri: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TrackSummary>>> + Send + 'a>> {
        Box::pin(self.fetch_playlist_tracks_via_mercury(playlist_uri))
    }
}
