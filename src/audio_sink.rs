use std::sync::{Arc, Mutex};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use rustfft::{FftPlanner, Fft, num_complex::Complex};

pub const N_BANDS: usize = 64;
const FFT_SIZE: usize = 2048;
const SAMPLE_RATE: f32 = 44100.0;

pub struct AnalyzerSink {
    inner: Box<dyn Sink>,
    bands: Arc<Mutex<Vec<f32>>>,
    buffer: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
    fft_input: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// Per-band running peak for independent normalization (decays slowly).
    band_peaks: Vec<f32>,
    // Reusable scratch buffers to avoid per-frame heap allocations
    chunk_buf: Vec<f32>,
    magnitudes_buf: Vec<f32>,
    new_bands_buf: Vec<f32>,
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
            band_peaks: vec![1e-6; N_BANDS],
            chunk_buf: vec![0.0f32; FFT_SIZE],
            magnitudes_buf: vec![0.0f32; FFT_SIZE / 2 - 1],
            new_bands_buf: vec![0.0f32; N_BANDS],
        }
    }

    fn process_packet(&mut self, samples: &[f64]) {
        let mono = samples.chunks(2).map(|ch| {
            let l = ch[0] as f32;
            let r = ch.get(1).copied().unwrap_or(0.0) as f32;
            (l + r) * 0.5
        });
        self.buffer.extend(mono);

        let step = 512;
        while self.buffer.len() >= FFT_SIZE {
            self.chunk_buf.copy_from_slice(&self.buffer[..FFT_SIZE]);
            self.buffer.drain(..step);
            self.compute_bands();
        }
    }

    fn compute_bands(&mut self) {
        let n = FFT_SIZE as f32;

        // Noise gate
        let frame_rms = (self.chunk_buf.iter().map(|x| x * x).sum::<f32>() / n).sqrt();
        if frame_rms < 5e-4 {
            if let Ok(mut bands) = self.bands.lock() {
                for v in bands.iter_mut() { *v *= 0.90; }
            }
            return;
        }

        // Hann window
        for (i, (&s, c)) in self.chunk_buf.iter().zip(self.fft_input.iter_mut()).enumerate() {
            let w = 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / (n - 1.0)).cos());
            *c = Complex::new(s * w, 0.0);
        }

        self.fft.process_with_scratch(&mut self.fft_input, &mut self.scratch);

        let half = FFT_SIZE / 2;
        for (out, c) in self.magnitudes_buf.iter_mut().zip(self.fft_input[1..half].iter()) {
            *out = c.norm();
        }

        // Map FFT bins → N_BANDS (logarithmic, 20 Hz – Nyquist)
        let freq_per_bin = SAMPLE_RATE / FFT_SIZE as f32;
        let log_min = 20.0f32.log2();
        let log_max = (SAMPLE_RATE / 2.0).log2();

        for v in self.new_bands_buf.iter_mut() { *v = 0.0; }
        for band in 0..N_BANDS {
            let f_target = 2.0f32.powf(log_min + (band as f32 / N_BANDS as f32) * (log_max - log_min));
            let bin_idx = f_target / freq_per_bin;
            
            let i = bin_idx.floor() as usize;
            let fract = bin_idx.fract();
            
            if i + 1 < self.magnitudes_buf.len() {
                self.new_bands_buf[band] = self.magnitudes_buf[i] * (1.0 - fract) + self.magnitudes_buf[i+1] * fract;
            } else {
                self.new_bands_buf[band] = self.magnitudes_buf[i];
            }
        }

        // Per-band normalization: each band is divided by its own running peak.
        // The peak decays slowly so the scale adapts to the song over time.
        // This prevents bass from dominating just because it has more raw energy.
        for i in 0..N_BANDS {
            if self.new_bands_buf[i] > self.band_peaks[i] {
                self.band_peaks[i] = self.new_bands_buf[i];
            } else {
                self.band_peaks[i] *= 0.9998; // ~30s half-life at 43 FFT frames/s
                self.band_peaks[i] = self.band_peaks[i].max(1e-6);
            }
            self.new_bands_buf[i] = (self.new_bands_buf[i] / self.band_peaks[i]).clamp(0.0, 1.0);
        }

        // Smooth: instant attack, fast decay
        if let Ok(mut bands) = self.bands.lock() {
            for (cur, &next) in bands.iter_mut().zip(self.new_bands_buf.iter()) {
                if next > *cur {
                    *cur = next;
                } else {
                    *cur *= 0.88; 
                }
            }
        }
    }
}

impl Sink for AnalyzerSink {
    fn start(&mut self) -> SinkResult<()> { self.inner.start() }
    fn stop(&mut self) -> SinkResult<()> {
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
