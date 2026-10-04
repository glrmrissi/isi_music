use anyhow::{Context, Result};
use librespot_playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use rodio::{OutputStream, OutputStreamBuilder, Sink as RodioSink, buffer::SamplesBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const MAX_QUEUED_CHUNKS: usize = 26;
const QUEUE_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const QUEUE_DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Default)]
pub(crate) struct AudioOutputLoss {
    lost: AtomicBool,
    notification_pending: AtomicBool,
    restoration_pending: AtomicBool,
}

impl AudioOutputLoss {
    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    pub(crate) fn take_notification(&self) -> bool {
        self.notification_pending.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn take_restored_notification(&self) -> bool {
        self.restoration_pending.swap(false, Ordering::AcqRel) && !self.lost.load(Ordering::Acquire)
    }

    fn mark_lost(&self) {
        if self
            .lost
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.notification_pending.store(true, Ordering::Release);
        }
    }

    fn mark_restored(&self) {
        if self.lost.swap(false, Ordering::AcqRel) {
            self.restoration_pending.store(true, Ordering::Release);
        }
    }

    fn cancel_restoration(&self) {
        self.restoration_pending.store(false, Ordering::Release);
        self.mark_lost();
    }
}

pub(crate) struct SpotifyAudioSink {
    sink: Option<RodioSink>,
    _stream: Option<OutputStream>,
    output_loss: Arc<AudioOutputLoss>,
    generation_lost: Arc<AtomicBool>,
}

impl SpotifyAudioSink {
    pub(crate) fn open(output_loss: Arc<AudioOutputLoss>) -> Result<Self> {
        let generation_lost = Arc::new(AtomicBool::new(false));
        let loss_from_callback = Arc::clone(&output_loss);
        let lost_from_callback = Arc::clone(&generation_lost);
        let mut stream = OutputStreamBuilder::from_default_device()
            .context("no default audio output device")?
            .with_error_callback(move |error| {
                if !lost_from_callback.swap(true, Ordering::AcqRel) {
                    loss_from_callback.mark_lost();
                    tracing::error!("Spotify audio output stream failed: {error}");
                }
            })
            .open_stream()
            .context("failed to open default audio output stream")?;
        stream.log_on_drop(false);
        let sink = RodioSink::connect_new(stream.mixer());

        Ok(Self {
            sink: Some(sink),
            _stream: Some(stream),
            output_loss,
            generation_lost,
        })
    }

    pub(crate) fn open_or_unavailable(output_loss: Arc<AudioOutputLoss>) -> Self {
        match Self::open(Arc::clone(&output_loss)) {
            Ok(sink) => sink,
            Err(error) => {
                tracing::warn!("Spotify audio output is unavailable: {error:#}");
                output_loss.mark_lost();
                Self {
                    sink: None,
                    _stream: None,
                    output_loss,
                    generation_lost: Arc::new(AtomicBool::new(true)),
                }
            }
        }
    }

    fn reopen(&mut self) -> SinkResult<()> {
        match Self::open(Arc::clone(&self.output_loss)) {
            Ok(sink) => {
                *self = sink;
                Ok(())
            }
            Err(error) => {
                self.generation_lost.store(true, Ordering::Release);
                self.output_loss.mark_lost();
                Err(SinkError::ConnectionRefused(format!(
                    "audio output could not be reopened: {error:#}"
                )))
            }
        }
    }
}

impl Sink for SpotifyAudioSink {
    fn start(&mut self) -> SinkResult<()> {
        if self.sink.is_none() || self.generation_lost.load(Ordering::Acquire) {
            self.reopen()?;
        }
        let Some(sink) = &self.sink else {
            return Err(SinkError::ConnectionRefused(
                "audio output is unavailable".to_string(),
            ));
        };
        sink.play();
        if self.generation_lost.load(Ordering::Acquire) {
            self.output_loss.cancel_restoration();
            return Err(SinkError::OnWrite(
                "audio output device was lost".to_string(),
            ));
        }
        self.output_loss.mark_restored();
        if self.generation_lost.load(Ordering::Acquire) {
            self.output_loss.cancel_restoration();
            return Err(SinkError::OnWrite(
                "audio output device was lost".to_string(),
            ));
        }
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        if self.generation_lost.load(Ordering::Acquire) {
            self.output_loss.mark_lost();
            return Err(SinkError::OnWrite(
                "audio output device was lost".to_string(),
            ));
        }
        let Some(sink) = &self.sink else {
            self.output_loss.mark_lost();
            return Err(SinkError::ConnectionRefused(
                "audio output is unavailable".to_string(),
            ));
        };
        let samples = packet
            .samples()
            .map_err(|error| SinkError::OnWrite(error.to_string()))?;
        let samples_f32: &[f32] = &converter.f64_to_f32(samples);
        sink.append(SamplesBuffer::new(
            NUM_CHANNELS as u16,
            SAMPLE_RATE,
            samples_f32,
        ));

        let result = wait_for_queue_drain(
            || sink.len(),
            || self.generation_lost.load(Ordering::Acquire),
            QUEUE_DRAIN_TIMEOUT,
            QUEUE_DRAIN_POLL_INTERVAL,
        );
        if result.is_err() {
            self.generation_lost.store(true, Ordering::Release);
            self.output_loss.mark_lost();
        }
        result
    }
}

fn wait_for_queue_drain(
    mut queued_chunks: impl FnMut() -> usize,
    mut output_lost: impl FnMut() -> bool,
    timeout: Duration,
    poll_interval: Duration,
) -> SinkResult<()> {
    let started = Instant::now();
    loop {
        if output_lost() {
            return Err(SinkError::OnWrite(
                "audio output device was lost".to_string(),
            ));
        }
        if queued_chunks() <= MAX_QUEUED_CHUNKS {
            return if output_lost() {
                Err(SinkError::OnWrite(
                    "audio output device was lost".to_string(),
                ))
            } else {
                Ok(())
            };
        }
        if started.elapsed() >= timeout {
            return Err(SinkError::OnWrite(
                "audio output stopped draining queued samples".to_string(),
            ));
        }
        thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn resource_regression_queue_wait_returns_when_the_output_device_is_lost() {
        let checks = Cell::new(0);

        let result = wait_for_queue_drain(
            || 27,
            || {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 3
            },
            Duration::from_secs(1),
            Duration::from_millis(1),
        );

        assert!(result.is_err());
        assert!(checks.get() >= 3);
    }

    #[test]
    fn resource_regression_queue_wait_times_out_when_samples_never_drain() {
        let result = wait_for_queue_drain(
            || 27,
            || false,
            Duration::from_millis(5),
            Duration::from_millis(1),
        );

        assert!(result.is_err());
    }

    #[test]
    fn resource_regression_audio_output_loss_notification_is_one_shot_until_recovery() {
        let loss = AudioOutputLoss::default();
        loss.mark_lost();

        assert!(loss.is_lost());
        assert!(loss.take_notification());
        assert!(!loss.take_notification());

        loss.mark_restored();
        assert!(!loss.is_lost());
        assert!(!loss.take_notification());
        assert!(loss.take_restored_notification());
        assert!(!loss.take_restored_notification());
    }

    #[test]
    fn resource_regression_new_loss_suppresses_a_stale_restoration_event() {
        let loss = AudioOutputLoss::default();
        loss.mark_lost();
        assert!(loss.take_notification());

        loss.mark_restored();
        loss.mark_lost();

        assert!(loss.is_lost());
        assert!(loss.take_notification());
        assert!(!loss.take_restored_notification());
    }
}
