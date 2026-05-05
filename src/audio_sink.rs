use std::sync::{Arc, Mutex};
use std::time::Duration;
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use rustfft::{FftPlanner, Fft, num_complex::Complex};

pub const N_BANDS: usize = 64;
const FFT_SIZE: usize = 2048;
const SAMPLE_RATE: f32 = 44100.0;
const STEP: usize = 512;
const RING_CAP: usize = FFT_SIZE * 8;

struct RingBuffer {
    buf: Vec<f32>,
    write_pos: usize,
    total_written: usize,
}

impl RingBuffer {
    fn new(cap: usize) -> Self {
        Self { buf: vec![0.0; cap], write_pos: 0, total_written: 0 }
    }

    fn push(&mut self, sample: f32) {
        self.buf[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.buf.len();
        self.total_written += 1;
    }

    fn read_latest(&self, out: &mut [f32]) -> bool {
        let n = out.len();
        if self.total_written < n { return false; }
        let cap = self.buf.len();
        let start = (self.write_pos + cap - n) % cap;
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.buf[(start + i) % cap];
        }
        true
    }
}

struct AnalyzerThread {
    ring: Arc<Mutex<RingBuffer>>,
    bands: Arc<Mutex<Vec<f32>>>,
    fft: Arc<dyn Fft<f32>>,
    fft_input: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    band_peaks: Vec<f32>,
    window: Vec<f32>,
    chunk_buf: Vec<f32>,
    magnitudes_buf: Vec<f32>,
    new_bands_buf: Vec<f32>,
    last_processed: usize,
}

impl AnalyzerThread {
    fn new(ring: Arc<Mutex<RingBuffer>>, bands: Arc<Mutex<Vec<f32>>>) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();

        // Pre-compute Hann window
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / (FFT_SIZE as f32 - 1.0)).cos()))
            .collect();

        Self {
            ring,
            bands,
            fft,
            fft_input: vec![Complex::default(); FFT_SIZE],
            scratch: vec![Complex::default(); scratch_len],
            band_peaks: vec![1e-6; N_BANDS],
            window,
            chunk_buf: vec![0.0f32; FFT_SIZE],
            magnitudes_buf: vec![0.0f32; FFT_SIZE / 2 - 1],
            new_bands_buf: vec![0.0f32; N_BANDS],
            last_processed: 0,
        }
    }

    fn run(mut self) {
        loop {
            // Check how many samples have been written since last FFT
            let (total_written, should_run) = {
                match self.ring.lock() {
                    Ok(ring) => {
                        let written = ring.total_written;
                        let should = written.saturating_sub(self.last_processed) >= STEP;
                        (written, should)
                    }
                    Err(_) => break,
                }
            };

            if should_run {
                // Read the latest FFT_SIZE samples out of the ring
                let got = {
                    match self.ring.lock() {
                        Ok(ring) => ring.read_latest(&mut self.chunk_buf),
                        Err(_) => break,
                    }
                };

                if got {
                    self.last_processed = total_written;
                    self.compute_bands();
                }
            }

            // Sleep ~8ms between polls (~120 Hz) — smooth but cheap
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    fn compute_bands(&mut self) {
        let n = FFT_SIZE as f32;

        let frame_rms = (self.chunk_buf.iter().map(|x| x * x).sum::<f32>() / n).sqrt();
        if frame_rms < 5e-4 {
            if let Ok(mut bands) = self.bands.lock() {
                for v in bands.iter_mut() { *v *= 0.85; }
            }
            return;
        }

        for (i, (&s, c)) in self.chunk_buf.iter().zip(self.fft_input.iter_mut()).enumerate() {
            *c = Complex::new(s * self.window[i], 0.0);
        }

        self.fft.process_with_scratch(&mut self.fft_input, &mut self.scratch);

        let half = FFT_SIZE / 2;
        for (out, c) in self.magnitudes_buf.iter_mut().zip(self.fft_input[1..half].iter()) {
            *out = c.norm();
        }

        let freq_per_bin = SAMPLE_RATE / FFT_SIZE as f32;
        let log_min = 20.0f32.log2();
        let log_max = (SAMPLE_RATE / 2.0).log2();

        for v in self.new_bands_buf.iter_mut() { *v = 0.0; }
        for band in 0..N_BANDS {
            let f_target = 2.0f32.powf(
                log_min + (band as f32 / N_BANDS as f32) * (log_max - log_min),
            );
            let bin_idx = f_target / freq_per_bin;
            let i = bin_idx.floor() as usize;
            let fract = bin_idx.fract();
            if i + 1 < self.magnitudes_buf.len() {
                self.new_bands_buf[band] =
                    self.magnitudes_buf[i] * (1.0 - fract) + self.magnitudes_buf[i + 1] * fract;
            } else if i < self.magnitudes_buf.len() {
                self.new_bands_buf[band] = self.magnitudes_buf[i];
            }
        }

        for i in 0..N_BANDS {
            if self.new_bands_buf[i] > self.band_peaks[i] {
                self.band_peaks[i] = self.new_bands_buf[i];
            } else {
                self.band_peaks[i] *= 0.9998;
                self.band_peaks[i] = self.band_peaks[i].max(1e-6);
            }
            self.new_bands_buf[i] = (self.new_bands_buf[i] / self.band_peaks[i]).clamp(0.0, 1.0);
        }

        if let Ok(mut bands) = self.bands.lock() {
            for (cur, &next) in bands.iter_mut().zip(self.new_bands_buf.iter()) {
                // Attack fast, decay a little slower for smoother motion
                if next > *cur {
                    *cur = next;
                } else {
                    *cur = cur.mul_add(0.80, next * 0.20);
                }
            }
        }
    }
}

fn spawn_analyzer(bands: Arc<Mutex<Vec<f32>>>) -> Arc<Mutex<RingBuffer>> {
    let ring = Arc::new(Mutex::new(RingBuffer::new(RING_CAP)));
    let ring2 = Arc::clone(&ring);
    std::thread::Builder::new()
        .name("band-analyzer".into())
        .spawn(move || AnalyzerThread::new(ring2, bands).run())
        .expect("failed to spawn band-analyzer thread");
    ring
}


pub struct AnalyzerSink {
    inner: Box<dyn Sink>,
    ring: Arc<Mutex<RingBuffer>>,
    bands: Arc<Mutex<Vec<f32>>>,
}

impl AnalyzerSink {
    pub fn new(inner: Box<dyn Sink>, bands: Arc<Mutex<Vec<f32>>>) -> Self {
        let ring = spawn_analyzer(Arc::clone(&bands));
        Self { inner, ring, bands }
    }

    fn push_stereo_f64(&self, samples: &[f64]) {
        if let Ok(mut ring) = self.ring.lock() {
            for ch in samples.chunks_exact(2) {
                ring.push(((ch[0] + ch[1]) * 0.5) as f32);
            }
            if samples.len() % 2 == 1 {
                ring.push(samples[samples.len() - 1] as f32);
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
            self.push_stereo_f64(samples);
        }
        self.inner.write(packet, converter)
    }
}


pub struct AnalyzingSource<S> {
    inner: S,
    ring: Arc<Mutex<RingBuffer>>,
    _bands: Arc<Mutex<Vec<f32>>>,
    channels: u16,
    channel_pos: u16,
    mix_acc: f32,
}

impl<S> AnalyzingSource<S>
where
    S: rodio::Source<Item = f32>,
{
    pub fn new(inner: S, _bands: Arc<Mutex<Vec<f32>>>) -> Self {
        let channels = inner.channels().max(1);
        let ring = spawn_analyzer(Arc::clone(&_bands));
        Self { inner, ring, _bands, channels, channel_pos: 0, mix_acc: 0.0 }
    }
}

impl<S> Iterator for AnalyzingSource<S>
where
    S: rodio::Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        self.mix_acc += sample;
        self.channel_pos += 1;
        if self.channel_pos >= self.channels {
            let mono = self.mix_acc / self.channels as f32;
            // try_lock: never block the audio thread if the analyzer is reading
            if let Ok(mut ring) = self.ring.try_lock() {
                ring.push(mono);
            }
            self.mix_acc = 0.0;
            self.channel_pos = 0;
        }
        Some(sample)
    }
}

impl<S> rodio::Source for AnalyzingSource<S>
where
    S: rodio::Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> { self.inner.current_span_len() }
    fn channels(&self) -> u16 { self.inner.channels() }
    fn sample_rate(&self) -> u32 { self.inner.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.inner.total_duration() }
    
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}