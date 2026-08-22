#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub is_active: bool,
}

#[derive(Clone, Debug)]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub uri: String,
    pub total_tracks: u32,
    pub art_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TrackSummary {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub uri: String,
    pub cover_path: Option<String>,
    pub added_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ArtistSummary {
    pub id: String,
    pub name: String,
    pub uri: String,
    pub genres: String,
}

#[derive(Clone, Debug)]
pub struct AlbumSummary {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub uri: String,
    pub total_tracks: u32,
}

#[derive(Clone, Debug)]
pub struct ShowSummary {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub total_episodes: u32,
}

#[derive(Clone, Debug)]
pub struct FullSearchResults {
    pub tracks: Vec<TrackSummary>,
    pub artists: Vec<ArtistSummary>,
    pub albums: Vec<AlbumSummary>,
    pub playlists: Vec<PlaylistSummary>,
    pub tracks_total: u32,
    pub artists_total: u32,
    pub albums_total: u32,
    pub playlists_total: u32,
}
