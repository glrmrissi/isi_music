mod cache;
mod files;
mod handle;
mod parse;
mod providers;
mod types;

pub use handle::LyricsHandle;
pub use types::LyricsData;
// Re-exported for integration tests that construct LyricLine literals.
#[allow(unused_imports)]
pub use types::LyricLine;
