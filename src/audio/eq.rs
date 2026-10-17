use std::f32::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

pub const EQ_BANDS: usize = 6;
pub const EQ_FREQS: [u32; EQ_BANDS] = [60, 150, 400, 1000, 2400, 15000];
pub const EQ_BAND_LABELS: [&str; EQ_BANDS] = ["60Hz", "150Hz", "400Hz", "1kHz", "2.4kHz", "15kHz"];
pub const EQ_MIN_DB: i8 = -12;
pub const EQ_MAX_DB: i8 = 12;
const PEAKING_Q: f32 = 1.0;

pub const EQ_PRESETS: &[(&str, [i8; EQ_BANDS])] = &[
    ("Flat", [0, 0, 0, 0, 0, 0]),
    ("Bass Booster", [6, 5, 2, 0, 0, 0]),
    ("Bass Reducer", [-4, -4, -2, 0, 0, 0]),
    ("Treble Booster", [0, 0, 0, 0, 3, 5]),
    ("Treble Reducer", [0, 0, 0, -1, -3, -5]),
    ("Loudness", [5, 3, -2, -2, 3, 5]),
    ("Rock", [3, 2, -2, -1, 2, 4]),
    ("Pop", [-1, 0, 2, 3, 2, 1]),
    ("Electronic", [4, 3, -1, -2, 2, 4]),
    ("Hip-Hop", [5, 4, 1, 0, -1, 1]),
    ("Jazz", [0, 2, 1, 2, 1, 2]),
    ("Classical", [0, 0, -2, -1, 2, 3]),
    ("Spoken Word", [-3, -1, 2, 4, 3, 1]),
    ("Vocal Booster", [-2, 0, 3, 4, 2, 0]),
];

pub fn preset_name(gains: &[i8; EQ_BANDS]) -> &'static str {
    EQ_PRESETS
        .iter()
        .find(|(_, g)| g == gains)
        .map(|(name, _)| *name)
        .unwrap_or("Custom")
}

pub fn next_preset_gains(gains: &[i8; EQ_BANDS]) -> [i8; EQ_BANDS] {
    let idx = EQ_PRESETS
        .iter()
        .position(|(_, g)| g == gains)
        .map(|i| i + 1)
        .unwrap_or(0);
    EQ_PRESETS[idx % EQ_PRESETS.len()].1
}

pub fn prev_preset_gains(gains: &[i8; EQ_BANDS]) -> [i8; EQ_BANDS] {
    let idx = EQ_PRESETS.iter().position(|(_, g)| g == gains).unwrap_or(0);
    EQ_PRESETS[(idx + EQ_PRESETS.len() - 1) % EQ_PRESETS.len()].1
}

pub fn pack_gains(gains: &[i8; EQ_BANDS]) -> u64 {
    gains
        .iter()
        .enumerate()
        .fold(0u64, |acc, (i, &g)| acc | ((g as u8 as u64) << (i * 8)))
}

pub fn unpack_gains(packed: u64) -> [i8; EQ_BANDS] {
    std::array::from_fn(|i| ((packed >> (i * 8)) & 0xFF) as u8 as i8)
}

fn has_boost(gains: &[i8; EQ_BANDS]) -> bool {
    gains.iter().any(|g| *g > 0)
}

#[inline]
fn flush_tiny(v: f32) -> f32 {
    if v.abs() < 1e-20 { 0.0 } else { v }
}

#[inline]
fn knee_clip(y: f32) -> f32 {
    const T: f32 = 0.8;
    if y.abs() <= T {
        return y;
    }
    let t = T.copysign(y);
    t + (1.0 - T) * ((y - t) / (1.0 - T)).tanh()
}

fn build_band(band: usize, gain: i8, sample_rate: u32) -> Option<Biquad> {
    let gain = gain.clamp(EQ_MIN_DB, EQ_MAX_DB);
    if gain == 0 || EQ_FREQS[band] as f32 * 2.0 >= sample_rate as f32 {
        return None;
    }
    Some(Biquad::peaking(
        EQ_FREQS[band] as f32,
        gain as f32,
        PEAKING_Q,
        sample_rate,
    ))
}

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn peaking(center_hz: f32, gain_db: f32, q: f32, sample_rate: u32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * center_hz / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: -2.0 * cos_w0 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = flush_tiny(x);
        self.y2 = self.y1;
        self.y1 = flush_tiny(y);
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

struct ChannelEq {
    bands: [Option<Biquad>; EQ_BANDS],
}

impl ChannelEq {
    fn build(gains: &[i8; EQ_BANDS], sample_rate: u32) -> Self {
        let bands = std::array::from_fn(|i| build_band(i, gains[i], sample_rate));
        Self { bands }
    }

    #[inline]
    fn process(&mut self, mut x: f32) -> f32 {
        for b in self.bands.iter_mut().flatten() {
            x = b.process(x);
        }
        x
    }

    fn reset(&mut self) {
        for b in self.bands.iter_mut().flatten() {
            b.reset();
        }
    }
}

pub struct InterleavedEq {
    shared: Arc<AtomicU64>,
    channels: Vec<ChannelEq>,
    channel_pos: usize,
    last_packed: u64,
    last_gains: [i8; EQ_BANDS],
    sample_rate: u32,
    flat: bool,
    soft_clip: bool,
}

impl InterleavedEq {
    pub fn new(shared: Arc<AtomicU64>, channels: usize, sample_rate: u32) -> Self {
        let packed = shared.load(Ordering::Relaxed);
        let gains = unpack_gains(packed);
        Self {
            shared,
            channels: (0..channels.max(1))
                .map(|_| ChannelEq::build(&gains, sample_rate))
                .collect(),
            channel_pos: 0,
            last_packed: packed,
            last_gains: gains,
            sample_rate,
            flat: gains.iter().all(|g| *g == 0),
            soft_clip: has_boost(&gains),
        }
    }

    fn sync(&mut self) {
        let packed = self.shared.load(Ordering::Relaxed);
        if packed == self.last_packed {
            return;
        }
        self.last_packed = packed;
        let gains = unpack_gains(packed);
        for ch in &mut self.channels {
            for (i, band) in ch.bands.iter_mut().enumerate() {
                if gains[i] != self.last_gains[i] {
                    *band = build_band(i, gains[i], self.sample_rate);
                }
            }
        }
        self.last_gains = gains;
        self.flat = gains.iter().all(|g| *g == 0);
        self.soft_clip = has_boost(&gains);
    }

    #[inline]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        self.sync();
        let pos = self.channel_pos;
        self.channel_pos = (self.channel_pos + 1) % self.channels.len();
        if self.flat {
            return x;
        }
        let x = if x.is_finite() { x } else { 0.0 };
        let y = self.channels[pos].process(x);
        if self.soft_clip { knee_clip(y) } else { y }
    }

    pub fn process_interleaved(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    pub fn reset(&mut self) {
        self.channel_pos = 0;
        for ch in &mut self.channels {
            ch.reset();
        }
    }

    pub fn shared_gains(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.shared)
    }
}

pub struct EqSource<S> {
    inner: S,
    eq: InterleavedEq,
}

impl<S: Source<Item = f32>> EqSource<S> {
    pub fn new(inner: S, shared: Arc<AtomicU64>) -> Self {
        let eq = InterleavedEq::new(
            shared,
            inner.channels().max(1) as usize,
            inner.sample_rate(),
        );
        Self { inner, eq }
    }
}

impl<S: Source<Item = f32>> Iterator for EqSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.inner.next().map(|s| self.eq.process_sample(s))
    }
}

impl<S: Source<Item = f32>> Source for EqSource<S> {
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
        self.inner.try_seek(pos)?;
        self.eq.reset();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn stereo_source(samples: Vec<f32>) -> SamplesBuffer {
        SamplesBuffer::new(2, 44100, samples)
    }

    fn sine_buf(freq: f32, n: usize, sample_rate: u32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / sample_rate as f32).sin() * 0.3)
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let gains = [-12, -1, 0, 3, 7, 12];
        assert_eq!(unpack_gains(pack_gains(&gains)), gains);
    }

    #[test]
    fn preset_name_matches_table_and_custom() {
        assert_eq!(preset_name(&[0; EQ_BANDS]), "Flat");
        assert_eq!(preset_name(&[6, 5, 2, 0, 0, 0]), "Bass Booster");
        assert_eq!(preset_name(&[1, 2, 3, 4, 5, 6]), "Custom");
    }

    #[test]
    fn next_preset_cycles_from_flat_and_wraps() {
        assert_eq!(next_preset_gains(&[0; EQ_BANDS]), EQ_PRESETS[1].1);
        assert_eq!(
            next_preset_gains(&EQ_PRESETS[EQ_PRESETS.len() - 1].1),
            EQ_PRESETS[0].1
        );
        assert_eq!(next_preset_gains(&[9, 9, 9, 9, 9, 9]), EQ_PRESETS[0].1);
    }

    #[test]
    fn flat_eq_is_transparent() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[0; EQ_BANDS])));
        let input = vec![0.5, -0.5, 0.25, -0.25];
        let src = EqSource::new(stereo_source(input.clone()), shared);
        let out: Vec<f32> = src.collect();
        assert_eq!(out, input);
    }

    #[test]
    fn boosted_band_amplifies_signal_at_center_frequency() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let input = sine_buf(60.0, 8192, 44100);
        let mut eq = InterleavedEq::new(shared, 1, 44100);
        let out: Vec<f32> = input
            .iter()
            .map(|&s| eq.process_sample(s))
            .skip(2048)
            .collect();
        assert!(rms(&out) > rms(&input[2048..]) * 1.5);
    }

    #[test]
    fn boost_at_60hz_leaves_high_frequencies_near_unity() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let input = sine_buf(8000.0, 8192, 44100);
        let mut eq = InterleavedEq::new(shared, 1, 44100);
        let out: Vec<f32> = input
            .iter()
            .map(|&s| eq.process_sample(s))
            .skip(2048)
            .collect();
        let ratio = rms(&out) / rms(&input[2048..]);
        assert!((0.8..1.4).contains(&ratio));
    }

    #[test]
    fn cut_band_attenuates_at_center_frequency() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[-12, 0, 0, 0, 0, 0])));
        let input = sine_buf(60.0, 8192, 44100);
        let mut eq = InterleavedEq::new(shared, 1, 44100);
        let out: Vec<f32> = input
            .iter()
            .map(|&s| eq.process_sample(s))
            .skip(2048)
            .collect();
        assert!(rms(&out) < rms(&input[2048..]) * 0.6);
    }

    #[test]
    fn stereo_channels_have_independent_state() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let mut interleaved = Vec::new();
        for i in 0..2048usize {
            interleaved.push((2.0 * PI * 60.0 * i as f32 / 44100.0).sin());
            interleaved.push(0.0);
        }
        let mut eq = InterleavedEq::new(shared, 2, 44100);
        eq.process_interleaved(&mut interleaved);
        let right: Vec<f32> = interleaved.iter().skip(1).step_by(2).copied().collect();
        assert!(right.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn soft_clip_bounds_boosted_output() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let input: Vec<f32> = sine_buf(60.0, 8192, 44100)
            .iter()
            .map(|s| s * 3.0)
            .collect();
        let mut eq = InterleavedEq::new(shared, 1, 44100);
        let out: Vec<f32> = input.iter().map(|&s| eq.process_sample(s)).collect();
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn live_gain_change_applies_mid_stream() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[0; EQ_BANDS])));
        let input = sine_buf(60.0, 8192, 44100);
        let mut eq = InterleavedEq::new(Arc::clone(&shared), 1, 44100);
        let head: Vec<f32> = input[..2048]
            .iter()
            .map(|&s| eq.process_sample(s))
            .collect();
        assert!((rms(&head) - rms(&input[..2048])).abs() < 1e-4);
        shared.store(pack_gains(&[12, 0, 0, 0, 0, 0]), Ordering::Relaxed);
        let tail: Vec<f32> = input[2048..]
            .iter()
            .map(|&s| eq.process_sample(s))
            .skip(1024)
            .collect();
        assert!(rms(&tail) > rms(&input[2048 + 1024..]) * 1.5);
    }

    #[test]
    fn eq_source_delegates_source_properties() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[0; EQ_BANDS])));
        let src = EqSource::new(stereo_source(vec![0.1, -0.1]), shared);
        assert_eq!(src.channels(), 2);
        assert_eq!(src.sample_rate(), 44100);
    }

    struct NoSeek<S>(S);

    impl<S: Source<Item = f32>> Iterator for NoSeek<S> {
        type Item = f32;

        fn next(&mut self) -> Option<f32> {
            self.0.next()
        }
    }

    impl<S: Source<Item = f32>> Source for NoSeek<S> {
        fn current_span_len(&self) -> Option<usize> {
            self.0.current_span_len()
        }

        fn channels(&self) -> u16 {
            self.0.channels()
        }

        fn sample_rate(&self) -> u32 {
            self.0.sample_rate()
        }

        fn total_duration(&self) -> Option<Duration> {
            self.0.total_duration()
        }

        fn try_seek(&mut self, _pos: Duration) -> Result<(), SeekError> {
            Err(SeekError::NotSupported {
                underlying_source: "NoSeek",
            })
        }
    }

    #[test]
    fn prev_preset_cycles_backwards_and_wraps() {
        assert_eq!(
            prev_preset_gains(&[0; EQ_BANDS]),
            EQ_PRESETS[EQ_PRESETS.len() - 1].1
        );
        assert_eq!(prev_preset_gains(&EQ_PRESETS[1].1), EQ_PRESETS[0].1);
        assert_eq!(
            prev_preset_gains(&[9, 9, 9, 9, 9, 9]),
            EQ_PRESETS[EQ_PRESETS.len() - 1].1
        );
    }

    #[test]
    fn unaligned_writes_track_channels_consistently() {
        let gains = [12, 0, 0, 0, 0, 0];
        let mut full: Vec<f32> = (0..4096)
            .flat_map(|i| {
                let l = (2.0 * PI * 60.0 * i as f32 / 44100.0).sin();
                [l, 0.0]
            })
            .collect();
        let split = full.clone();
        let mut a = InterleavedEq::new(Arc::new(AtomicU64::new(pack_gains(&gains))), 2, 44100);
        a.process_interleaved(&mut full);

        let mut b = InterleavedEq::new(Arc::new(AtomicU64::new(pack_gains(&gains))), 2, 44100);
        let mut head = split[..333].to_vec();
        let mut tail = split[333..].to_vec();
        b.process_interleaved(&mut head);
        b.process_interleaved(&mut tail);
        let split_out: Vec<f32> = head.into_iter().chain(tail).collect();
        assert_eq!(full, split_out);
    }

    #[test]
    fn out_of_range_gains_are_clamped_to_max() {
        let input = sine_buf(60.0, 8192, 44100);
        let mut eq_max = InterleavedEq::new(
            Arc::new(AtomicU64::new(pack_gains(&[127, -128, 0, 0, 0, 0]))),
            1,
            44100,
        );
        let mut eq_clamped = InterleavedEq::new(
            Arc::new(AtomicU64::new(pack_gains(&[12, -12, 0, 0, 0, 0]))),
            1,
            44100,
        );
        let out_max: Vec<f32> = input.iter().map(|&s| eq_max.process_sample(s)).collect();
        let out_clamped: Vec<f32> = input
            .iter()
            .map(|&s| eq_clamped.process_sample(s))
            .collect();
        assert_eq!(out_max, out_clamped);
    }

    #[test]
    fn band_at_or_above_nyquist_is_skipped() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[0, 0, 0, 0, 5, 5])));
        let eq = InterleavedEq::new(shared, 1, 8000);
        assert!(eq.channels[0].bands[4].is_some());
        assert!(eq.channels[0].bands[5].is_none());
    }

    #[test]
    fn non_finite_input_does_not_poison_state() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let mut eq = InterleavedEq::new(shared, 1, 44100);
        assert_eq!(eq.process_sample(f32::NAN), 0.0);
        assert_eq!(eq.process_sample(f32::INFINITY), 0.0);
        assert_eq!(eq.process_sample(f32::NEG_INFINITY), 0.0);
        let out: Vec<f32> = sine_buf(60.0, 1024, 44100)
            .iter()
            .map(|&s| eq.process_sample(s))
            .collect();
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(
            eq.channels[0].bands[0]
                .as_ref()
                .is_some_and(|b| b.y1.is_finite() && b.x1.is_finite())
        );
    }

    #[test]
    fn gain_change_rebuilds_only_the_changed_band() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let mut eq = InterleavedEq::new(Arc::clone(&shared), 1, 44100);
        for s in sine_buf(60.0, 512, 44100) {
            eq.process_sample(s);
        }
        let band0_x1 = eq.channels[0].bands[0].as_ref().unwrap().x1;
        assert_ne!(band0_x1, 0.0);

        shared.store(pack_gains(&[12, 0, 0, 0, 0, 6]), Ordering::Relaxed);
        eq.process_sample(0.5);
        let band0 = eq.channels[0].bands[0].as_ref().unwrap();
        assert_eq!(band0.x1, 0.5);
        assert_eq!(band0.x2, band0_x1);
        let band5 = eq.channels[0].bands[5].as_ref().unwrap();
        assert_eq!(band5.x2, 0.0);
    }

    #[test]
    fn knee_clip_is_identity_below_threshold_and_bounded() {
        assert_eq!(knee_clip(0.0), 0.0);
        assert_eq!(knee_clip(0.5), 0.5);
        assert_eq!(knee_clip(-0.79), -0.79);
        assert_eq!(knee_clip(0.8), 0.8);
        assert!((knee_clip(1.0) - 0.9523).abs() < 1e-3);
        assert!(knee_clip(50.0) <= 1.0 && knee_clip(50.0) > 0.99);
        assert!(knee_clip(-50.0) >= -1.0);
    }

    #[test]
    fn denormal_states_flush_to_zero() {
        let mut b = Biquad::peaking(60.0, 6.0, PEAKING_Q, 44100);
        b.x1 = 1e-25;
        b.y1 = 1e-25;
        b.process(0.0);
        assert_eq!(b.x1, 0.0);
        assert!(b.y1.abs() < 1e-20 || b.y1 == 0.0);
    }

    #[test]
    fn failed_seek_preserves_eq_state() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let mut src = EqSource::new(NoSeek(stereo_source(sine_buf(60.0, 512, 44100))), shared);
        for _ in 0..256 {
            src.next();
        }
        let y1_before = src.eq.channels[0].bands[0].as_ref().unwrap().y1;
        assert_ne!(y1_before, 0.0);
        assert!(src.try_seek(Duration::ZERO).is_err());
        assert_eq!(src.eq.channels[0].bands[0].as_ref().unwrap().y1, y1_before);
    }

    #[test]
    fn successful_seek_resets_eq_state() {
        let shared = Arc::new(AtomicU64::new(pack_gains(&[12, 0, 0, 0, 0, 0])));
        let mut src = EqSource::new(stereo_source(sine_buf(60.0, 4096, 44100)), shared);
        for _ in 0..256 {
            src.next();
        }
        assert_ne!(src.eq.channels[0].bands[0].as_ref().unwrap().y1, 0.0);
        src.try_seek(Duration::ZERO).expect("seek");
        assert_eq!(src.eq.channels[0].bands[0].as_ref().unwrap().y1, 0.0);
        assert_eq!(src.eq.channel_pos, 0);
    }
}
