pub mod local;
mod native;
mod token;
mod types;

pub use local::LocalPlayer;
pub use native::NativePlayer;
pub use token::ensure_streaming_auth;
pub(crate) use types::remember_play_history;
pub use types::{PlayerNotification, QueuedTrack, RepeatMode};
// Re-exported for tests/app/player.rs.
#[allow(unused_imports)]
pub(crate) use types::MAX_PLAY_HISTORY;

use crate::spotify::TrackSummary;
use crate::ui::PlaybackState;
use anyhow::Result;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub trait AudioPlayer: Send {
    fn set_queue(&mut self, uris: Vec<String>, start_index: usize);
    fn add_to_queue(
        &mut self,
        uri: String,
        name: String,
        artist: String,
        album: String,
        duration_ms: u64,
        cover_path: Option<PathBuf>,
    );
    fn user_queue(&self) -> &[QueuedTrack];
    fn remove_from_user_queue(&mut self, index: usize);
    fn take_playing_queued(&mut self) -> Option<QueuedTrack>;
    fn play_from_user_queue(&mut self, index: usize) -> bool;

    fn set_queue_tracks(&mut self, tracks: &[TrackSummary], start_index: usize) {
        let uris = tracks.iter().map(|t| t.uri.clone()).collect();
        self.set_queue(uris, start_index);
    }

    fn play(&mut self);
    fn pause(&mut self);
    fn toggle(&mut self);
    fn next(&mut self) -> bool;
    fn prev(&mut self) -> bool;
    fn play_at(&mut self, index: usize);
    fn seek(&self, position_ms: u32);
    fn seek_mut(&mut self, position_ms: u32) {
        self.seek(position_ms);
    }

    fn is_playing(&self) -> bool;
    fn volume(&self) -> u8;
    fn shuffle(&self) -> bool;
    fn repeat(&self) -> RepeatMode;
    fn current_index(&self) -> Option<usize>;
    fn current_track_summary(&self) -> Option<TrackSummary>;

    /// Remove already-played tracks from the front of the queue.
    /// Returns true if the queue was actually trimmed.
    fn trim_played(&mut self, _keep_behind: usize) -> bool {
        false
    }

    fn volume_up(&mut self);
    fn volume_down(&mut self);
    fn set_volume(&mut self, volume: u8);
    fn toggle_shuffle(&mut self);
    fn cycle_repeat(&mut self);

    fn try_recv_event(&mut self) -> Option<PlayerNotification>;
    fn preload_next(&mut self) {}

    fn snapshot_queue(&self) -> (Vec<String>, Option<usize>) {
        (vec![], None)
    }
    fn snapshot_user_queue(&self) -> Vec<QueuedTrack> {
        vec![]
    }
    fn set_visualizer_enabled(&mut self, _enabled: bool) {}
    fn set_mono_enabled(&mut self, _enabled: bool) {}
    fn set_eq_gains(&mut self, _gains: [i8; crate::audio::eq::EQ_BANDS]) {}
    fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        None
    }
    fn current_playback_state(&self) -> Option<PlaybackState> {
        None
    }

    fn fetch_playlist_via_mercury<'a>(
        &'a self,
        _playlist_uri: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TrackSummary>>> + Send + 'a>> {
        Box::pin(async {
            anyhow::bail!("Mercury playlist fetch not supported by this player backend")
        })
    }
}
