use rodio::Source;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tracing::{info, warn};

use super::LocalPlayer;
use super::decoder::LocalDecoder;
use crate::audio::audio_sink::AnalyzingSource;
use crate::audio::mono::MonoSource;
use crate::player::PlayerNotification;
use crate::utils::lock::lock_or_recover;

impl LocalPlayer {
    fn spawn_waveform_loader(&mut self, path: PathBuf) {
        *lock_or_recover(&self.waveform) = None;
        *lock_or_recover(&self.duration_measured) = None;
        if let Some(task) = self.waveform_task.take() {
            task.abort();
        }
        let waveform = Arc::clone(&self.waveform);
        let duration_measured = Arc::clone(&self.duration_measured);
        let token = self.track_token.fetch_add(1, Ordering::SeqCst) + 1;
        let token_arc = Arc::clone(&self.track_token);
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            warn!("waveform worker unavailable outside a Tokio runtime");
            return;
        };
        let task = runtime.spawn_blocking(move || {
            if let Some((dur, data)) =
                crate::utils::waveform::generate_for_file_with_cancel(&path, || {
                    token_arc.load(Ordering::SeqCst) != token
                })
                && token_arc.load(Ordering::SeqCst) == token
            {
                if let Ok(mut w) = waveform.lock() {
                    *w = Some(data);
                }
                if dur > 0
                    && let Ok(mut d) = duration_measured.lock()
                {
                    *d = Some(dur);
                }
            }
        });
        self.waveform_task = Some(task.abort_handle());
    }

    pub(super) fn load_track_inner(&mut self, idx: usize, record_history: bool) -> bool {
        let Some(track) = self.queue.get(idx) else {
            return false;
        };
        let path = track.path.clone();

        if !path.exists() {
            return false;
        }

        self.sink.clear();
        self.sink.stop();

        let decoder = match LocalDecoder::open(&path) {
            Some(d) => MonoSource::new(d, Arc::clone(&self.mono_enabled)),
            None => return false,
        };

        if let Some(total) = decoder.total_duration()
            && let Some(track) = self.queue.get_mut(idx)
        {
            track.duration_ms = total.as_millis() as u64;
        }

        if self.analyzer.enabled() {
            if let Some(handle) = self.analyzer.handle() {
                let analyzing = AnalyzingSource::with_handle(decoder, handle);
                self.sink.append(analyzing);
            } else {
                self.sink.append(decoder);
            }
        } else {
            self.sink.append(decoder);
        }
        self.sink.play();

        if record_history
            && self.shuffle
            && let Some(prev) = self.current_idx
            && prev != idx
        {
            crate::player::remember_play_history(&mut self.play_history, prev);
        }
        self.current_idx = Some(idx);
        self.temp_playing = None;
        self.temp_queue.clear();
        self.playing_queued = None;
        self.is_playing = true;
        self.load_guard = Some(Instant::now());

        self.spawn_waveform_loader(path);

        let _ = self.event_tx.send(PlayerNotification::Playing);
        true
    }

    pub(super) fn load_temp_track(&mut self, temp_idx: usize) -> bool {
        let Some(track) = self.temp_queue.get(temp_idx) else {
            return false;
        };
        let path = track.path.clone();

        if !path.exists() {
            return false;
        }

        let decoder = match LocalDecoder::open(&path) {
            Some(d) => MonoSource::new(d, Arc::clone(&self.mono_enabled)),
            None => return false,
        };

        self.sink.clear();
        self.sink.stop();

        if self.analyzer.enabled() {
            if let Some(handle) = self.analyzer.handle() {
                let analyzing = AnalyzingSource::with_handle(decoder, handle);
                self.sink.append(analyzing);
            } else {
                self.sink.append(decoder);
            }
        } else {
            self.sink.append(decoder);
        }
        self.sink.play();

        self.current_idx = None;
        self.temp_playing = Some(temp_idx);
        self.playing_context = None;
        self.playing_context_pos = None;
        self.is_playing = true;
        self.load_guard = Some(Instant::now());

        self.spawn_waveform_loader(path);

        let _ = self.event_tx.send(PlayerNotification::Playing);
        true
    }

    pub(super) fn try_load_track(&mut self, idx: usize) -> bool {
        self.load_track_inner(idx, true)
    }

    pub(super) fn load_index_priv(&mut self, idx: usize) -> bool {
        self.load_track_inner(idx, false)
    }

    pub(super) fn load_and_play(&mut self, start_idx: usize) {
        let len = self.queue.len();

        for attempt in 0..len {
            let idx = (start_idx + attempt) % len;
            if self.try_load_track(idx) {
                return;
            }
        }

        self.is_playing = false;
        warn!("LocalPlayer: no playable tracks found");
        let _ = self.event_tx.send(PlayerNotification::TrackUnavailable);
    }

    pub(super) fn seek_by_reload(&mut self, position_ms: u32) {
        let pos = std::time::Duration::from_millis(position_ms as u64);
        self.is_seeking.store(true, Ordering::SeqCst);
        self.load_guard = Some(Instant::now());
        if let Ok(mut d) = self.seek_deadline.lock() {
            *d = Some(Instant::now() + std::time::Duration::from_millis(500));
        }
        match self.sink.try_seek(pos) {
            Ok(()) => info!("Seeked to {}ms", position_ms),
            Err(e) => {
                warn!("Seek failed: {:?}", e);
                self.is_seeking.store(false, Ordering::SeqCst);
                self.load_guard = None;
                if let Ok(mut d) = self.seek_deadline.lock() {
                    *d = None;
                }
            }
        }
    }

    pub(super) fn poll_sink(&mut self) {
        if self.is_seeking.load(Ordering::SeqCst) {
            let ready = if let Ok(d) = self.seek_deadline.lock() {
                d.map(|deadline| Instant::now() >= deadline).unwrap_or(true)
            } else {
                true
            };
            if !ready {
                return;
            }
            self.is_seeking.store(false, Ordering::SeqCst);
            return;
        }

        if let Ok(mut d) = self.duration_measured.lock()
            && let Some(dur) = d.take()
        {
            if let Some(idx) = self.current_idx
                && let Some(t) = self.queue.get_mut(idx)
            {
                t.duration_ms = dur;
            } else if let Some(temp_idx) = self.temp_playing
                && let Some(t) = self.temp_queue.get_mut(temp_idx)
            {
                t.duration_ms = dur;
            }
        }

        if let Some(t) = self.load_guard {
            if t.elapsed().as_millis() < 500 {
                return;
            }
            self.load_guard = None;
        }

        if self.is_playing && self.sink.empty() {
            self.is_playing = false;
            let _ = self.event_tx.send(PlayerNotification::TrackEnded);
        }
    }

    pub fn waveform(&self) -> Option<Vec<u8>> {
        self.waveform.lock().ok().and_then(|w| w.clone())
    }
}
