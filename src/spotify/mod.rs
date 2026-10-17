pub mod auth;
mod client;
mod library_cache;
mod search_cache;
mod token;
mod types;

pub(crate) use library_cache::LIBRARY_CACHE_TTL_SECS;

#[cfg(test)]
pub(crate) use client::playlist_item_to_track;
pub use client::{SpotifyClient, remove_uri_http, save_uri_http};
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
