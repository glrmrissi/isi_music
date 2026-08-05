pub mod cache;
pub mod debug_overlay;
pub mod discord;
pub mod ipc;
pub mod lastfm;
pub mod lyrics;
#[cfg(windows)]
pub mod media_keys;
#[cfg(all(feature = "mpris", target_os = "linux"))]
pub mod mpris;
#[cfg(windows)]
pub mod smtc;
pub mod theme;
pub mod waveform;
pub mod wizard;
