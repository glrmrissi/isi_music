use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

pub fn mixdown_interleaved(input: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    if channels < 2 {
        out.extend_from_slice(input);
        return;
    }
    let mut frames = input.chunks_exact(channels);
    for frame in &mut frames {
        let avg = frame.iter().copied().sum::<f32>() / channels as f32;
        out.extend(std::iter::repeat_n(avg, channels));
    }
    out.extend_from_slice(frames.remainder());
}

pub struct MonoSource<S> {
    inner: S,
    enabled: Arc<AtomicBool>,
    channels: usize,
    emit_remaining: usize,
    emit_value: f32,
}

impl<S: Source<Item = f32>> MonoSource<S> {
    pub fn new(inner: S, enabled: Arc<AtomicBool>) -> Self {
        let channels = inner.channels().max(1) as usize;
        Self {
            inner,
            enabled,
            channels,
            emit_remaining: 0,
            emit_value: 0.0,
        }
    }
}

impl<S: Source<Item = f32>> Iterator for MonoSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.emit_remaining > 0 {
            self.emit_remaining -= 1;
            return Some(self.emit_value);
        }
        if !self.enabled.load(Ordering::Relaxed) || self.channels < 2 {
            return self.inner.next();
        }
        let mut sum = 0.0f32;
        let mut n = 0usize;
        for _ in 0..self.channels {
            match self.inner.next() {
                Some(s) => {
                    sum += s;
                    n += 1;
                }
                None => break,
            }
        }
        if n == 0 {
            return None;
        }
        self.emit_value = sum / n as f32;
        self.emit_remaining = n - 1;
        Some(self.emit_value)
    }
}

impl<S: Source<Item = f32>> Source for MonoSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.emit_remaining = 0;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn stereo_source(samples: Vec<f32>) -> SamplesBuffer {
        SamplesBuffer::new(2, 44100, samples)
    }

    #[test]
    fn mixdown_averages_stereo_frames_onto_both_channels() {
        let mut out = Vec::new();
        mixdown_interleaved(&[1.0, -1.0, 0.6, 0.2], 2, &mut out);
        assert_eq!(out, vec![0.0, 0.0, 0.4, 0.4]);
    }

    #[test]
    fn mixdown_preserves_partial_tail_frames() {
        let mut out = Vec::new();
        mixdown_interleaved(&[1.0, -1.0, 0.9], 2, &mut out);
        assert_eq!(out, vec![0.0, 0.0, 0.9]);
    }

    #[test]
    fn mixdown_passthrough_for_single_channel() {
        let mut out = Vec::new();
        mixdown_interleaved(&[0.5, -0.5], 1, &mut out);
        assert_eq!(out, vec![0.5, -0.5]);
    }

    #[test]
    fn mono_source_passthrough_when_disabled() {
        let enabled = Arc::new(AtomicBool::new(false));
        let src = MonoSource::new(stereo_source(vec![1.0, -1.0]), Arc::clone(&enabled));
        let out: Vec<f32> = src.collect();
        assert_eq!(out, vec![1.0, -1.0]);
    }

    #[test]
    fn mono_source_downmixes_when_enabled() {
        let enabled = Arc::new(AtomicBool::new(true));
        let src = MonoSource::new(stereo_source(vec![1.0, -1.0, 0.6, 0.2]), enabled);
        let out: Vec<f32> = src.collect();
        assert_eq!(out, vec![0.0, 0.0, 0.4, 0.4]);
    }

    #[test]
    fn mono_source_toggle_applies_live_mid_stream() {
        let enabled = Arc::new(AtomicBool::new(false));
        let mut src = MonoSource::new(
            stereo_source(vec![1.0, -1.0, 1.0, -1.0]),
            Arc::clone(&enabled),
        );
        assert_eq!(src.next(), Some(1.0));
        assert_eq!(src.next(), Some(-1.0));
        enabled.store(true, Ordering::Relaxed);
        assert_eq!(src.next(), Some(0.0));
        assert_eq!(src.next(), Some(0.0));
        assert_eq!(src.next(), None);
    }

    #[test]
    fn mono_source_handles_partial_frame_at_eof() {
        let enabled = Arc::new(AtomicBool::new(true));
        let src = MonoSource::new(stereo_source(vec![0.4, 0.8, 0.5]), enabled);
        let out: Vec<f32> = src.collect();
        assert_eq!(out, vec![0.6, 0.6, 0.5]);
    }
}
