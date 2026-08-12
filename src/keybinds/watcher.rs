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
    _watcher: RecommendedWatcher,
    #[allow(dead_code)]
    stop: Arc<AtomicBool>,
}

impl KeybindsWatcher {
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
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        if let Some(parent) = path.parent() {
            watcher
                .watch(parent.as_ref(), RecursiveMode::NonRecursive)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        }

        Ok(KeybindsWatcher {
            rx,
            _watcher: watcher,
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
        let watcher = notify::recommended_watcher(|_| {}).unwrap();
        KeybindsWatcher {
            rx,
            _watcher: watcher,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}
