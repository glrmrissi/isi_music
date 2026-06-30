pub mod auth;
mod client;
mod token;

pub use client::{
    AlbumSummary, ArtistSummary, FullSearchResults, PlaylistSummary, ShowSummary, SpotifyClient,
    TrackSummary, save_track_http, unlike_track_http,
};

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum RepeatState {
    #[default]
    Off,
    Context,
    Track,
}
