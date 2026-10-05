mod actions;
mod combo;
mod map;
mod schema;
pub mod watcher;

pub use actions::Action;
pub use map::{Keybinds, keybinds_path};
pub use watcher::KeybindsWatcher;

#[cfg(test)]
#[path = "../../tests/keybinds/mod.rs"]
mod tests;
