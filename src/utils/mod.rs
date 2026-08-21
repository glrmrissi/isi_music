pub mod cache;
pub mod debug_overlay;
pub mod discord;
pub mod doctor;
pub mod ipc;
pub mod lastfm;
pub mod lock;
pub mod lyrics;
#[cfg(windows)]
pub mod media_keys;
#[cfg(all(feature = "mpris", target_os = "linux"))]
pub mod mpris;
#[cfg(all(feature = "palette", feature = "album-art"))]
pub mod palette;
#[cfg(windows)]
pub mod smtc;
pub mod theme;
pub mod updater;
pub mod waveform;
pub mod wizard;
