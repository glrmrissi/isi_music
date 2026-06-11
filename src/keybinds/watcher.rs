use super::{Keybinds, keybinds_path};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

#[allow(dead_code)]
pub struct KeybindsWatcher {
    pub rx: mpsc::Receiver<Keybinds>,
    stop: Arc<AtomicBool>,
}

impl KeybindsWatcher {
    pub fn watch() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let path = keybinds_path();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        std::thread::spawn(move || {
            let mut last_content = std::fs::read_to_string(&path).unwrap_or_default();
            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
                if let Ok(current_content) = std::fs::read_to_string(&path) {
                    if current_content != last_content {
                        std::thread::sleep(Duration::from_millis(50));
                        let new = Keybinds::load();
                        if tx.send(new).is_ok() {
                            last_content = current_content;
                        }
                    }
                }
            }
        });

        Ok(KeybindsWatcher { rx, stop })
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
        Self {
            rx,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}
