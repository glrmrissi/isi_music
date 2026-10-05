use crate::spotify::RepeatState;

#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub title: String,
    pub artist: String,
    pub album: String,
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
    pub waveform: Option<Vec<u8>>,
    pub context_uri: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
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
            waveform: None,
            context_uri: None,
        }
    }
}

impl PlaybackState {
    pub fn merge_from_api(&mut self, from_api: PlaybackState) {
        let lyrics = self.lyrics.take();
        let lyrics_loading = self.lyrics_loading;
        let lyrics_scroll = self.lyrics_scroll;
        let radio_mode = self.radio_mode;
        let art_url = self.art_url.clone();
        let waveform = self.waveform.take();

        *self = from_api;

        self.lyrics = lyrics;
        self.lyrics_loading = lyrics_loading;
        self.lyrics_scroll = lyrics_scroll;
        self.radio_mode = radio_mode;
        if self.art_url.is_none() && !self.is_local {
            self.art_url = art_url;
        }
        self.waveform = waveform;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_local_drops_stale_remote_art() {
        let mut state = PlaybackState {
            art_url: Some("https://i.scdn.co/image/abc".into()),
            ..PlaybackState::default()
        };
        state.merge_from_api(PlaybackState {
            is_local: true,
            art_url: None,
            ..PlaybackState::default()
        });
        assert_eq!(state.art_url, None);
    }

    #[test]
    fn merge_remote_keeps_art_when_incoming_is_none() {
        let mut state = PlaybackState {
            art_url: Some("https://i.scdn.co/image/abc".into()),
            ..PlaybackState::default()
        };
        state.merge_from_api(PlaybackState::default());
        assert_eq!(
            state.art_url.as_deref(),
            Some("https://i.scdn.co/image/abc")
        );
    }

    #[test]
    fn merge_remote_prefers_fresh_art() {
        let mut state = PlaybackState {
            art_url: Some("https://i.scdn.co/image/old".into()),
            ..PlaybackState::default()
        };
        state.merge_from_api(PlaybackState {
            art_url: Some("https://i.scdn.co/image/new".into()),
            ..PlaybackState::default()
        });
        assert_eq!(
            state.art_url.as_deref(),
            Some("https://i.scdn.co/image/new")
        );
    }
}
