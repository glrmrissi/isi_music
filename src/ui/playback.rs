use rspotify::model::RepeatState;

#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub title: String,
    pub artist: String,
    pub album: String,
    #[allow(dead_code)]
    pub path: Option<String>,
    pub is_playing: bool,
    pub shuffle: bool,
    pub repeat: RepeatState,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    pub art_url: Option<String>,
    pub cover_path: Option<String>,
    pub is_local: bool,
    pub radio_mode: bool,
    pub lyrics: Option<crate::utils::lyrics::LyricsData>,
    pub lyrics_scroll: usize,
    pub lyrics_loading: bool,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            path: None,
            is_playing: false,
            shuffle: false,
            repeat: RepeatState::Off,
            progress_ms: 0,
            duration_ms: 0,
            volume: 100,
            art_url: None,
            cover_path: None,
            is_local: false,
            radio_mode: false,
            lyrics: None,
            lyrics_scroll: 0,
            lyrics_loading: false,
        }
    }
}
