use std::sync::{Arc, Mutex};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use rustfft::{FftPlanner, Fft, num_complex::Complex};

pub const N_BANDS: usize = 64;
const FFT_SIZE: usize = 1024;
const SAMPLE_RATE: f32 = 44100.0;

pub struct AnalyzerSink {
    inner: Box<dyn Sink>,
    bands: Arc<Mutex<Vec<f32>>>,
    buffer: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
    fft_input: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl AnalyzerSink {
    pub fn new(inner: Box<dyn Sink>, bands: Arc<Mutex<Vec<f32>>>) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();
        Self {
            inner,
            bands,
            buffer: Vec::with_capacity(FFT_SIZE * 2),
            fft,
            fft_input: vec![Complex::default(); FFT_SIZE],
            scratch: vec![Complex::default(); scratch_len],
        }
    }

    fn process_packet(&mut self, samples: &[f64]) {
        // Stereo interleaved → mono f32
        let mono = samples.chunks(2).map(|ch| {
            let l = ch[0] as f32;
            let r = ch.get(1).copied().unwrap_or(0.0) as f32;
            (l + r) * 0.5
        });
        self.buffer.extend(mono);

        // Process all complete FFT_SIZE chunks
        while self.buffer.len() >= FFT_SIZE {
            let chunk: Vec<f32> = self.buffer.drain(..FFT_SIZE).collect();
            self.compute_bands(&chunk);
        }
    }

    fn compute_bands(&mut self, samples: &[f32]) {
        let n = FFT_SIZE as f32;

        // Noise gate: skip silent frames instead of amplifying noise
        let frame_rms = (samples.iter().map(|x| x * x).sum::<f32>() / n).sqrt();
        if frame_rms < 5e-4 {
            if let Ok(mut bands) = self.bands.lock() {
                for v in bands.iter_mut() { *v *= 0.90; }
            }
            return;
        }

        // Apply Hann window
        for (i, (&s, c)) in samples.iter().zip(self.fft_input.iter_mut()).enumerate() {
            let w = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / (n - 1.0)).cos());
            *c = Complex::new(s * w, 0.0);
        }

        self.fft.process_with_scratch(&mut self.fft_input, &mut self.scratch);

        // Raw magnitudes (positive frequencies only)
        let half = FFT_SIZE / 2;
        let magnitudes: Vec<f32> = self.fft_input[1..half].iter().map(|c| c.norm()).collect();

        // Map FFT bins → N_BANDS with logarithmic frequency spacing (20 Hz – Nyquist)
        let freq_per_bin = SAMPLE_RATE / FFT_SIZE as f32;
        let log_min = 20.0f32.log2();
        let log_max = (SAMPLE_RATE / 2.0).log2();

        let mut new_bands = vec![0.0f32; N_BANDS];
        for band in 0..N_BANDS {
            let f_low  = 2.0f32.powf(log_min + (band       as f32 / N_BANDS as f32) * (log_max - log_min));
            let f_high = 2.0f32.powf(log_min + ((band + 1) as f32 / N_BANDS as f32) * (log_max - log_min));
            let bin_low  = ((f_low  / freq_per_bin) as usize).max(0);
            let bin_high = ((f_high / freq_per_bin) as usize).min(magnitudes.len().saturating_sub(1));

            // Use peak (not RMS) per band — more visually responsive
            new_bands[band] = if bin_low >= bin_high {
                magnitudes.get(bin_low).copied().unwrap_or(0.0)
            } else {
                magnitudes[bin_low..=bin_high].iter().cloned().fold(0.0f32, f32::max)
            };
        }

        // Normalize by the frame peak so bars always fill the space,
        // independent of system volume. Apply sqrt for a more natural curve.
        let peak = new_bands.iter().cloned().fold(0.0f32, f32::max);
        if peak > 0.0 {
            for v in &mut new_bands {
                *v = (*v / peak).sqrt();
            }
        }

        // Smooth: fast attack, slow decay
        if let Ok(mut bands) = self.bands.lock() {
            for (cur, &next) in bands.iter_mut().zip(new_bands.iter()) {
                if next > *cur {
                    *cur = *cur * 0.2 + next * 0.8; // fast attack
                } else {
                    *cur = *cur * 0.88 + next * 0.12; // slow decay
                }
            }
        }
    }
}

impl Sink for AnalyzerSink {
    fn start(&mut self) -> SinkResult<()> { self.inner.start() }
    fn stop(&mut self) -> SinkResult<()> {
        // Zero out bands when playback stops
        if let Ok(mut bands) = self.bands.lock() {
            for v in bands.iter_mut() { *v = 0.0; }
        }
        self.inner.stop()
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(ref samples) = packet {
            self.process_packet(samples);
        }
        self.inner.write(packet, converter)
    }
}
