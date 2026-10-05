use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum RepeatMode {
    #[default]
    Off,
    Track,
    Queue,
}

pub(crate) const MAX_PLAY_HISTORY: usize = 512;

pub(crate) fn remember_play_history(history: &mut VecDeque<usize>, index: usize) {
    if history.len() >= MAX_PLAY_HISTORY {
        history.pop_front();
    }
    history.push_back(index);
}

pub enum PlayerNotification {
    TrackEnded,
    TrackUnavailable,
    Playing,
    Paused,
    SessionLost,
    AudioOutputLost,
    AudioOutputRestored,
    FreeAccountDetected,
    PreloadNextTrack,
}

#[derive(Clone)]
pub struct QueuedTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub cover_path: Option<PathBuf>,
}
