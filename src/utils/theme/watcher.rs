use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, channel},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::warn;

use super::Theme;

pub struct ThemeWatcher {
    rx: Receiver<Theme>,
    #[allow(dead_code)]
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
}

impl ThemeWatcher {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for ThemeWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

impl std::ops::Deref for ThemeWatcher {
    type Target = std::sync::mpsc::Receiver<Theme>;
    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

#[cfg(test)]
impl ThemeWatcher {
    pub fn noop() -> Self {
        let (_, rx) = std::sync::mpsc::channel();
        let watcher = notify::recommended_watcher(|_| {}).unwrap();
        Self {
            rx,
            _watcher: watcher,
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn with_sender() -> (Self, std::sync::mpsc::Sender<Theme>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = notify::recommended_watcher(|_| {}).unwrap();
        let w = Self {
            rx,
            _watcher: watcher,
            stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        (w, tx)
    }
}

pub fn watch_theme() -> std::io::Result<ThemeWatcher> {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    let (tx, rx) = channel();
    let path = Theme::get_path().unwrap_or_else(|| PathBuf::from("theme.toml"));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);

    let watch_path = path.clone();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if stop_clone.load(Ordering::Relaxed) {
            return;
        }
        let Ok(event) = res else { return };
        let relevant = matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
        );
        if !relevant {
            return;
        }
        let dominated = event
            .paths
            .iter()
            .any(|p| p.to_string_lossy().contains("theme.toml"));
        if !dominated {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(current_content) = fs::read_to_string(&watch_path) {
            if let Ok(new_theme) = toml::from_str::<Theme>(&current_content) {
                let _ = tx.send(new_theme);
            } else {
                warn!("Error on theme.toml");
            }
        }
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    if let Some(parent) = path.parent() {
        watcher
            .watch(parent.as_ref(), RecursiveMode::NonRecursive)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }

    Ok(ThemeWatcher {
        rx,
        _watcher: watcher,
        stop,
    })
}
