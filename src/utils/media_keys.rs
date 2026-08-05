//! Global media hotkeys for Windows (Play/Pause, Next, Previous).
//!
//! `global-hotkey` creates a hidden window and relies on a Win32 message loop
//! on the same thread to dispatch `WM_HOTKEY` events. This module spawns a
//! dedicated thread that owns the `GlobalHotKeyManager` and pumps messages.

#[cfg(windows)]
use anyhow::Result;
#[cfg(windows)]
use global_hotkey::hotkey::{Code, HotKey};
#[cfg(windows)]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
#[cfg(windows)]
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
}

pub struct MediaKeysHandle {
    pub cmd_rx: mpsc::Receiver<MediaKey>,
}

#[cfg(windows)]
pub fn spawn() -> Result<MediaKeysHandle> {
    let (cmd_tx, cmd_rx) = mpsc::channel();

    std::thread::spawn(move || {
        if let Err(e) = run_media_keys_thread(cmd_tx) {
            tracing::warn!("Global media hotkeys thread failed: {e:#}");
        }
    });

    Ok(MediaKeysHandle { cmd_rx })
}

#[cfg(not(windows))]
pub fn spawn() -> Result<MediaKeysHandle> {
    let (_cmd_tx, cmd_rx) = mpsc::channel();
    Ok(MediaKeysHandle { cmd_rx })
}

#[cfg(windows)]
fn run_media_keys_thread(cmd_tx: mpsc::Sender<MediaKey>) -> Result<()> {
    let manager = GlobalHotKeyManager::new()?;

    let play_pause = HotKey::new(None, Code::MediaPlayPause);
    let next = HotKey::new(None, Code::MediaTrackNext);
    let previous = HotKey::new(None, Code::MediaTrackPrevious);

    manager.register(play_pause)?;
    manager.register(next)?;
    manager.register(previous)?;

    // Forward events from the global static channel into our typed channel.
    // We run the Win32 message pump on this same thread so WM_HOTKEY
    // messages are delivered to the hidden window created by global-hotkey.
    let receiver = GlobalHotKeyEvent::receiver();

    std::thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            let cmd = if event.id == play_pause.id() {
                Some(MediaKey::PlayPause)
            } else if event.id == next.id() {
                Some(MediaKey::Next)
            } else if event.id == previous.id() {
                Some(MediaKey::Previous)
            } else {
                None
            };
            if let Some(cmd) = cmd {
                let _ = cmd_tx.send(cmd);
            }
        }
    });

    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, MSG, MsgWaitForMultipleObjects, QS_ALLINPUT,
            TranslateMessage,
        };

        // Pump messages until the thread is asked to stop. We use
        // `MsgWaitForMultipleObjects` with no handles and `QS_ALLINPUT` so the
        // thread blocks efficiently while still being interruptible.
        loop {
            // Wait for a message to arrive.
            let _ = MsgWaitForMultipleObjects(0, std::ptr::null(), 0, u32::MAX, QS_ALLINPUT);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
