use librespot_core::spotify_uri::SpotifyUri;
use rand::seq::SliceRandom;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

use super::NativePlayer;
use crate::player::types::{QueuedTrack, RepeatMode, remember_play_history};

impl NativePlayer {
    pub fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        (self.queue.clone(), self.current_index)
    }

    pub fn snapshot_user_queue(&self) -> Vec<QueuedTrack> {
        self.user_queue.clone()
    }

    pub fn set_queue(&mut self, uris: Vec<String>, start_index: usize) {
        self.queue = uris;
        self.play_at(start_index);
    }

    pub fn add_to_queue(
        &mut self,
        uri: String,
        name: String,
        artist: String,
        album: String,
        duration_ms: u64,
        cover_path: Option<PathBuf>,
    ) {
        self.user_queue.push(QueuedTrack {
            uri,
            name,
            artist,
            album,
            duration_ms,
            cover_path,
        });
    }

    pub fn user_queue(&self) -> &[QueuedTrack] {
        &self.user_queue
    }

    pub fn play_at(&mut self, index: usize) {
        let Some(uri) = self.queue.get(index) else {
            warn!("Index {index} out of queue bounds");
            return;
        };
        match SpotifyUri::from_uri(uri) {
            Ok(spotify_uri) => {
                info!("Loading URI: {uri}");
                self.player.stop();
                self.player.load(spotify_uri, true, 0);
                if self.shuffle
                    && let Some(prev) = self.current_index
                    && prev != index
                {
                    remember_play_history(&mut self.play_history, prev);
                }
                self.current_index = Some(index);
                self.is_playing = true;
                self.playing_queued = None;
                self.preload_next();
            }
            Err(e) => error!("Invalid URI '{uri}': {e}"),
        }
    }

    fn load_index(&mut self, index: usize) {
        let Some(uri) = self.queue.get(index) else {
            warn!("Index {index} out of queue bounds");
            return;
        };
        match SpotifyUri::from_uri(uri) {
            Ok(spotify_uri) => {
                self.player.stop();
                self.player.load(spotify_uri, true, 0);
                self.current_index = Some(index);
                self.is_playing = true;
                self.playing_queued = None;
                self.preload_next();
            }
            Err(e) => error!("Invalid URI '{uri}': {e}"),
        }
    }

    pub fn preload_next(&mut self) {
        if self.repeat == RepeatMode::Track || self.shuffle {
            return;
        }

        if self.playing_queued.is_some() {
            if let Some(track) = self.user_queue.first()
                && let Ok(uri) = SpotifyUri::from_uri(&track.uri)
            {
                debug!("Preloading next user-queue track");
                self.player.preload(uri);
            }
            return;
        }

        let next_idx = self.next_index();

        if let Some(idx) = next_idx
            && let Some(uri) = self.queue.get(idx)
            && let Ok(spotify_uri) = SpotifyUri::from_uri(uri)
        {
            debug!("Preloading next track at index {idx}: {uri}");
            self.player.preload(spotify_uri);
        }
    }

    fn next_index(&self) -> Option<usize> {
        let current = self.current_index?;
        let len = self.queue.len();
        if len == 0 {
            return None;
        }
        if self.repeat == RepeatMode::Queue {
            return Some((current + 1) % len);
        }
        if current + 1 < len {
            Some(current + 1)
        } else {
            None
        }
    }

    pub fn next(&mut self) -> bool {
        self.playing_queued = None;
        if self.repeat == RepeatMode::Track
            && let Some(idx) = self.current_index
        {
            self.play_at(idx);
            return true;
        }
        if !self.user_queue.is_empty() {
            let track = self.user_queue.remove(0);
            match SpotifyUri::from_uri(&track.uri) {
                Ok(spotify_uri) => {
                    info!("Playing from user queue: {}", track.uri);
                    self.player.stop();
                    self.player.load(spotify_uri, true, 0);
                    self.is_playing = true;
                    self.playing_queued = Some(track);
                    self.preload_next();
                    return true;
                }
                Err(e) => error!("Invalid URI in user queue: {e}"),
            }
        }
        if let Some(idx) = self.current_index {
            let len = self.queue.len();
            let next = if self.shuffle && len > 1 {
                let mut rng = rand::thread_rng();
                let candidates: Vec<usize> = (0..len).filter(|&i| i != idx).collect();
                *candidates.choose(&mut rng).unwrap_or(&((idx + 1) % len))
            } else {
                idx + 1
            };
            if next < len {
                self.play_at(next);
                return true;
            }
            if self.repeat == RepeatMode::Queue && len > 0 {
                self.play_at(0);
                return true;
            }
        }
        false
    }

    pub fn prev(&mut self) -> bool {
        if self.shuffle {
            if let Some(prev_idx) = self.play_history.pop_back() {
                self.load_index(prev_idx);
                return true;
            }
            return false;
        }
        if let Some(idx) = self.current_index
            && idx > 0
        {
            self.play_at(idx - 1);
            return true;
        }
        false
    }

    pub fn trim_played(&mut self, keep_behind: usize) -> bool {
        let Some(idx) = self.current_index else {
            return false;
        };
        if idx <= keep_behind {
            return false;
        }
        let remove = idx - keep_behind;
        self.queue.drain(0..remove);
        if let Some(i) = &mut self.current_index {
            *i -= remove;
        }
        self.play_history.retain(|&i| i >= remove);
        for i in &mut self.play_history {
            *i -= remove;
        }
        true
    }
}
