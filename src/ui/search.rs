use crate::spotify::{
    AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, ShowSummary, TrackSummary,
};
use crate::ui::SearchPanel;
use ratatui::widgets::ListState;

pub enum PodcastSelection<'a> {
    Show(&'a ShowSummary),
    Episode(&'a TrackSummary),
}

#[derive(Clone)]
pub struct SearchResults {
    pub tracks: Vec<TrackSummary>,
    pub artists: Vec<ArtistSummary>,
    pub albums: Vec<AlbumSummary>,
    pub playlists: Vec<PlaylistSummary>,
    pub shows: Vec<ShowSummary>,
    pub episodes: Vec<TrackSummary>,
    pub track_list: ListState,
    pub artist_list: ListState,
    pub album_list: ListState,
    pub playlist_list: ListState,
    pub podcast_list: ListState,
    pub panel: SearchPanel,
    pub query: String,
    pub tracks_total: u32,
    pub artists_total: u32,
    pub albums_total: u32,
    pub playlists_total: u32,
    pub shows_total: u32,
    pub episodes_total: u32,
    pub tracks_api_offset: u32,
    pub artists_api_offset: u32,
    pub albums_api_offset: u32,
    pub playlists_api_offset: u32,
    pub podcasts_api_offset: u32,
    pub loading: bool,
    pub local_only: bool,
}

impl SearchResults {
    pub fn new(query: String, r: FullSearchResults) -> Self {
        let mut tl = ListState::default();
        if !r.tracks.is_empty() {
            tl.select(Some(0));
        }
        let podcasts_api_offset = r.shows.len().max(r.episodes.len()) as u32;
        Self {
            tracks_api_offset: r.tracks.len() as u32,
            artists_api_offset: r.artists.len() as u32,
            albums_api_offset: r.albums.len() as u32,
            playlists_api_offset: r.playlists.len() as u32,
            tracks: r.tracks,
            artists: r.artists,
            albums: r.albums,
            playlists: r.playlists,
            shows: r.shows,
            episodes: r.episodes,
            track_list: tl,
            artist_list: ListState::default(),
            album_list: ListState::default(),
            playlist_list: ListState::default(),
            podcast_list: ListState::default(),
            panel: SearchPanel::Tracks,
            query,
            tracks_total: r.tracks_total,
            artists_total: r.artists_total,
            albums_total: r.albums_total,
            playlists_total: r.playlists_total,
            shows_total: r.shows_total,
            episodes_total: r.episodes_total,
            podcasts_api_offset,
            loading: false,
            local_only: false,
        }
    }

    pub fn current_len(&self) -> usize {
        match self.panel {
            SearchPanel::Tracks => self.tracks.len(),
            SearchPanel::Artists => self.artists.len(),
            SearchPanel::Albums => self.albums.len(),
            SearchPanel::Playlists => self.playlists.len(),
            SearchPanel::Podcasts => self.shows.len() + self.episodes.len(),
        }
    }

    pub fn current_list_mut(&mut self) -> &mut ListState {
        match self.panel {
            SearchPanel::Tracks => &mut self.track_list,
            SearchPanel::Artists => &mut self.artist_list,
            SearchPanel::Albums => &mut self.album_list,
            SearchPanel::Playlists => &mut self.playlist_list,
            SearchPanel::Podcasts => &mut self.podcast_list,
        }
    }

    pub fn nav_up(&mut self) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let list = self.current_list_mut();
        let i = list
            .selected()
            .map(|i| if i == 0 { len - 1 } else { i - 1 })
            .unwrap_or(0);
        list.select(Some(i));
    }

    pub fn nav_down(&mut self) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        let list = self.current_list_mut();
        let i = list
            .selected()
            .map(|i| if i >= len - 1 { 0 } else { i + 1 })
            .unwrap_or(0);
        list.select(Some(i));
    }

    pub fn next_panel(&mut self) {
        if self.local_only {
            return;
        }
        self.panel = self.panel.next();
    }

    pub fn prev_panel(&mut self) {
        if self.local_only {
            return;
        }
        self.panel = self.panel.prev();
    }

    pub fn selected_track_uri(&self) -> Option<&str> {
        self.track_list
            .selected()
            .and_then(|i| self.tracks.get(i))
            .map(|t| t.uri.as_str())
    }

    pub fn selected_album(&self) -> Option<&AlbumSummary> {
        self.album_list.selected().and_then(|i| self.albums.get(i))
    }

    pub fn selected_artist(&self) -> Option<&ArtistSummary> {
        self.artist_list
            .selected()
            .and_then(|i| self.artists.get(i))
    }

    pub fn selected_playlist(&self) -> Option<&PlaylistSummary> {
        self.playlist_list
            .selected()
            .and_then(|i| self.playlists.get(i))
    }

    pub fn selected_podcast(&self) -> Option<PodcastSelection<'_>> {
        let i = self.podcast_list.selected()?;
        if i < self.shows.len() {
            self.shows.get(i).map(PodcastSelection::Show)
        } else {
            self.episodes
                .get(i - self.shows.len())
                .map(PodcastSelection::Episode)
        }
    }
}
