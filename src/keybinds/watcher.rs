use super::{Keybinds, keybinds_path};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

pub struct KeybindsWatcher {
    pub rx: mpsc::Receiver<Keybinds>,
    #[allow(dead_code)]
    _watcher: Option<RecommendedWatcher>,
    #[allow(dead_code)]
    stop: Arc<AtomicBool>,
}

impl KeybindsWatcher {
    pub fn disabled() -> Self {
        let (_, rx) = mpsc::channel();
        Self {
            rx,
            _watcher: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn watch() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let path = keybinds_path();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

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
                .any(|p| p.to_string_lossy().contains("keybinds.toml"));
            if !dominated {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
            let new = Keybinds::load();
            let _ = tx.send(new);
        })
        .map_err(std::io::Error::other)?;

        if let Some(parent) = path.parent() {
            watcher
                .watch(parent.as_ref(), RecursiveMode::NonRecursive)
                .map_err(std::io::Error::other)?;
        }

        Ok(KeybindsWatcher {
            rx,
            _watcher: Some(watcher),
            stop,
        })
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
impl KeybindsWatcher {
    pub fn noop() -> Self {
        let (_, rx) = std::sync::mpsc::channel();
        KeybindsWatcher {
            rx,
            _watcher: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}
