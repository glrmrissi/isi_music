use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};

use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

use rustfft::{Fft, FftPlanner, num_complex::Complex};

pub const N_BANDS: usize = 64;

const FFT_SIZE: usize = 1024;
const SAMPLE_RATE: f32 = 44100.0;
const RING_CAP: usize = FFT_SIZE * 16;

struct AnalyzerThread {
    bands: Arc<Mutex<Vec<f32>>>,
    fft: Arc<dyn Fft<f32>>,
    fft_input: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    band_peaks: Vec<f32>,
    window: Vec<f32>,
    chunk_buf: Vec<f32>,
    magnitudes_buf: Vec<f32>,
    new_bands_buf: Vec<f32>,
    write_pos: usize,
    sample_counter: usize,
    last_fft_counter: usize,
}

impl AnalyzerThread {
    fn new(bands: Arc<Mutex<Vec<f32>>>) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / (FFT_SIZE as f32 - 1.0)).cos())
            })
            .collect();
        Self {
            bands,
            fft,
            fft_input: vec![Complex::default(); FFT_SIZE],
            scratch: vec![Complex::default(); scratch_len],
            band_peaks: vec![1e-6; N_BANDS],
            window,
            chunk_buf: vec![0.0; FFT_SIZE],
            magnitudes_buf: vec![0.0; FFT_SIZE / 2],
            new_bands_buf: vec![0.0; N_BANDS],
            write_pos: 0,
            sample_counter: 0,
            last_fft_counter: 0,
        }
    }

    fn push_sample(&mut self, sample: f32) {
        self.chunk_buf[self.write_pos] = sample;
        self.write_pos += 1;
        if self.write_pos >= FFT_SIZE {
            self.write_pos = 0;
        }
        self.sample_counter += 1;
    }

    fn tick(&mut self) {
        const STEP: usize = 512;
        if self.sample_counter - self.last_fft_counter < STEP {
            return;
        }
        self.last_fft_counter = self.sample_counter;
        self.compute_bands();
    }

    fn compute_bands(&mut self) {
        let n = FFT_SIZE as f32;
        let frame_rms = (self.chunk_buf.iter().map(|x| x * x).sum::<f32>() / n).sqrt();
        if frame_rms < 5e-4 {
            if let Ok(mut bands) = self.bands.lock() {
                for v in bands.iter_mut() {
                    *v *= 0.85;
                }
            }
            return;
        }
        for i in 0..FFT_SIZE {
            let idx = (self.write_pos + i) % FFT_SIZE;
            self.fft_input[i] = Complex::new(self.chunk_buf[idx] * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.fft_input, &mut self.scratch);
        let half = FFT_SIZE / 2;
        for (out, c) in self
            .magnitudes_buf
            .iter_mut()
            .zip(self.fft_input[1..half].iter())
        {
            *out = c.norm_sqr();
        }
        let freq_per_bin = SAMPLE_RATE / FFT_SIZE as f32;
        let log_min = 30.0f32.log2();
        let log_max = (SAMPLE_RATE / 2.0).log2();
        for v in self.new_bands_buf.iter_mut() {
            *v = 0.0;
        }
        for band in 0..N_BANDS {
            let f_target =
                2.0f32.powf(log_min + (band as f32 / N_BANDS as f32) * (log_max - log_min));
            let bin_idx = f_target / freq_per_bin;
            let i = bin_idx.floor() as usize;
            let fract = bin_idx.fract();
            if i + 1 < self.magnitudes_buf.len() {
                let val =
                    self.magnitudes_buf[i] * (1.0 - fract) + self.magnitudes_buf[i + 1] * fract;
                self.new_bands_buf[band] = val.sqrt();
            }
        }
        for i in 0..N_BANDS {
            let peak_decay = 0.99 - (i as f32 / N_BANDS as f32) * 0.02;
            if self.new_bands_buf[i] > self.band_peaks[i] {
                self.band_peaks[i] = self.new_bands_buf[i];
            } else {
                self.band_peaks[i] *= peak_decay;
                self.band_peaks[i] = self.band_peaks[i].max(1e-6);
            }
            self.new_bands_buf[i] = (self.new_bands_buf[i] / self.band_peaks[i]).clamp(0.0, 1.0);
        }
        if let Ok(mut bands) = self.bands.lock() {
            for (cur, &next) in bands.iter_mut().zip(self.new_bands_buf.iter()) {
                let attack = 0.15;
                let decay = 0.88;
                if next > *cur {
                    *cur = *cur * (1.0 - attack) + next * attack;
                } else {
                    *cur = *cur * decay + next * (1.0 - decay);
                }
            }
        }
    }
}

type AnalyzerProducer = ringbuf::wrap::caching::CachingProd<Arc<HeapRb<f32>>>;

#[derive(Clone)]
pub struct AnalyzerHandle {
    bands: Arc<Mutex<Vec<f32>>>,
    producer: Arc<Mutex<AnalyzerProducer>>,
    enabled: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}

impl AnalyzerHandle {
    pub fn spawn_with_enabled(bands: Arc<Mutex<Vec<f32>>>, enabled: Arc<AtomicBool>) -> Self {
        let rb = HeapRb::<f32>::new(RING_CAP);
        let (prod, mut cons) = rb.split();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        let bands_clone = Arc::clone(&bands);
        let enabled_clone = Arc::clone(&enabled);

        std::thread::Builder::new()
            .name("band-analyzer".into())
            .spawn(move || {
                let mut analyzer = AnalyzerThread::new(bands_clone);
                loop {
                    if shutdown_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    if enabled_clone.load(Ordering::Relaxed) {
                        while let Some(sample) = cons.try_pop() {
                            analyzer.push_sample(sample);
                        }
                        analyzer.tick();
                        std::thread::sleep(Duration::from_millis(30));
                    } else {
                        while let Some(_) = cons.try_pop() {}
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .expect("failed to spawn analyzer thread");

        Self {
            bands,
            producer: Arc::new(Mutex::new(prod)),
            enabled,
            shutdown,
        }
    }

    pub(crate) fn push_mono(&self, sample: f32) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut prod) = self.producer.lock() {
            let _ = prod.try_push(sample);
        }
    }

    pub fn bands(&self) -> Arc<Mutex<Vec<f32>>> {
        Arc::clone(&self.bands)
    }

    #[allow(dead_code)]
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for AnalyzerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

pub struct SharedAnalyzerState {
    pub band_energies: Arc<Mutex<Vec<f32>>>,
    handle: Mutex<Option<AnalyzerHandle>>,
    enabled: Arc<AtomicBool>,
}

impl SharedAnalyzerState {
    pub fn new() -> Self {
        Self {
            band_energies: Arc::new(Mutex::new(vec![0.0f32; N_BANDS])),
            handle: Mutex::new(None),
            enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        let mut handle = self.handle.lock().unwrap();
        if on && handle.is_none() {
            *handle = Some(AnalyzerHandle::spawn_with_enabled(
                Arc::clone(&self.band_energies),
                Arc::clone(&self.enabled),
            ));
        } else if !on {
            *handle = None;
        }
    }

    pub fn handle(&self) -> Option<AnalyzerHandle> {
        self.handle.lock().unwrap().clone()
    }

    pub fn band_energies(&self) -> Option<Arc<Mutex<Vec<f32>>>> {
        if self.enabled.load(Ordering::Relaxed) {
            Some(Arc::clone(&self.band_energies))
        } else {
            None
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

pub struct AnalyzerSink {
    inner: Box<dyn Sink>,
    pub handle: AnalyzerHandle,
    sink_factory: Option<Box<dyn Fn() -> Box<dyn Sink> + Send>>,
}

impl AnalyzerSink {
    pub fn with_factory(
        inner: Box<dyn Sink>,
        bands: Arc<Mutex<Vec<f32>>>,
        enabled: Arc<AtomicBool>,
        sink_factory: Box<dyn Fn() -> Box<dyn Sink> + Send>,
    ) -> Self {
        Self {
            inner,
            handle: AnalyzerHandle::spawn_with_enabled(bands, enabled),
            sink_factory: Some(sink_factory),
        }
    }

    fn push_stereo_f64(&mut self, samples: &[f64]) {
        for ch in samples.chunks_exact(2) {
            let mono = ((ch[0] + ch[1]) * 0.5) as f32;
            self.handle.push_mono(mono);
        }
    }
}

impl Sink for AnalyzerSink {
    fn start(&mut self) -> SinkResult<()> {
        self.inner.start()
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Ok(mut bands) = self.handle.bands().lock() {
            for v in bands.iter_mut() {
                *v = 0.0;
            }
        }
        // fresh sink drops all queued audio buffers
        if let Some(ref factory) = self.sink_factory {
            self.inner = factory();
        }
        self.inner.stop()
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = if let AudioPacket::Samples(ref s) = packet {
            Some(s.clone())
        } else {
            None
        };
        let result = self.inner.write(packet, converter);
        if let Some(samples) = samples {
            self.push_stereo_f64(&samples);
        }
        result
    }
}

pub struct AnalyzingSource<S> {
    inner: S,
    handle: AnalyzerHandle,
    channels: u16,
    channel_pos: u16,
    mix_acc: f32,
}

impl<S> AnalyzingSource<S>
where
    S: rodio::Source<Item = f32>,
{
    pub fn with_handle(inner: S, handle: AnalyzerHandle) -> Self {
        let channels = inner.channels().max(1);
        Self {
            inner,
            handle,
            channels,
            channel_pos: 0,
            mix_acc: 0.0,
        }
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
            self.handle.push_mono(mono);
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
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}
