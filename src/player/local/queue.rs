use rand::seq::SliceRandom;

use super::LocalPlayer;
use super::track::LocalTrack;
use crate::player::RepeatMode;

impl LocalPlayer {
    pub(super) fn next_inner(&mut self) -> bool {
        if self.repeat == RepeatMode::Track {
            if let Some(idx) = self.current_idx {
                self.playing_queued = None;
                return self.load_track_inner(idx, false);
            }
            if let Some(temp_idx) = self.temp_playing {
                return self.load_temp_track(temp_idx);
            }
        }
        self.playing_queued = None;

        if !self.user_queue.is_empty() {
            let track = self.user_queue.remove(0);
            let path = LocalTrack::uri_to_path(&track.uri);
            let lt = LocalTrack {
                path,
                uri: track.uri.clone(),
                name: track.name.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                duration_ms: track.duration_ms,
                cover_path: track.cover_path.clone(),
            };
            self.temp_queue.push(lt);
            let temp_idx = self.temp_queue.len() - 1;
            if self.load_temp_track(temp_idx) {
                self.playing_queued = Some(track);
                return true;
            }
            self.temp_queue.pop();
            return false;
        }

        self.temp_playing = None;

        // Check context queue first
        if let Some(ref context) = self.playing_context
            && let Some(pos) = self.playing_context_pos
        {
            let next_pos = if self.shuffle && context.len() > 1 {
                let mut candidates: Vec<usize> = (0..context.len()).filter(|&i| i != pos).collect();
                candidates.shuffle(&mut rand::thread_rng());
                candidates[0]
            } else {
                pos + 1
            };

            if next_pos < context.len() {
                let target_uri = &context[next_pos];
                if let Some(idx) = self.queue.iter().position(|t| &t.uri == target_uri) {
                    self.playing_context_pos = Some(next_pos);
                    return self.try_load_track(idx);
                }
            } else if self.repeat == RepeatMode::Queue && !context.is_empty() {
                let target_uri = &context[0];
                if let Some(idx) = self.queue.iter().position(|t| &t.uri == target_uri) {
                    self.playing_context_pos = Some(0);
                    return self.try_load_track(idx);
                }
            }
        }

        // Fallback to library queue
        if let Some(idx) = self.current_idx {
            let len = self.queue.len();
            let next = if self.shuffle && len > 1 {
                let mut candidates: Vec<usize> = (0..len).filter(|&i| i != idx).collect();
                candidates.shuffle(&mut rand::thread_rng());
                candidates[0]
            } else {
                idx + 1
            };

            if next < len {
                self.playing_context = None;
                self.playing_context_pos = None;
                self.load_and_play(next);
                return true;
            }
            if self.repeat == RepeatMode::Queue && len > 0 {
                self.playing_context = None;
                self.playing_context_pos = None;
                self.load_and_play(0);
                return true;
            }
        }
        false
    }

    pub(super) fn prev_inner(&mut self) -> bool {
        if self.repeat == RepeatMode::Track {
            if let Some(idx) = self.current_idx {
                self.playing_queued = None;
                return self.load_track_inner(idx, false);
            }
            if let Some(temp_idx) = self.temp_playing {
                return self.load_temp_track(temp_idx);
            }
        }
        self.playing_queued = None;

        if self.shuffle {
            if let Some(prev_idx) = self.play_history.pop() {
                return self.load_index_priv(prev_idx);
            }
            return false;
        }

        if let Some(ref context) = self.playing_context
            && let Some(pos) = self.playing_context_pos
            && pos > 0
        {
            let prev_pos = pos - 1;
            let target_uri = &context[prev_pos];
            if let Some(idx) = self.queue.iter().position(|t| &t.uri == target_uri) {
                self.playing_context_pos = Some(prev_pos);
                return self.try_load_track(idx);
            }
        }

        if let Some(idx) = self.current_idx {
            let target = if idx > 0 { idx - 1 } else { 0 };
            self.playing_context = None;
            self.playing_context_pos = None;
            return self.try_load_track(target);
        }
        false
    }
}
