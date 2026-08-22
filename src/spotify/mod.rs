pub mod auth;
mod client;
mod library_cache;
mod search_cache;
mod token;
mod types;

pub use client::{SpotifyClient, save_track_http, unlike_track_http};
pub use types::{
    AlbumSummary, ArtistSummary, Device, FullSearchResults, PlaylistSummary, ShowSummary,
    TrackSummary,
};

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum RepeatState {
    #[default]
    Off,
    Context,
    Track,
}
