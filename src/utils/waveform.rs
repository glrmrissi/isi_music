//! Offline waveform cache for local audio files.
//!
//! Generates a low-resolution amplitude envelope (~200 points) and renders it
//! as Unicode block characters in the progress bar. For Spotify streams this
//! module returns `None`, because the decoded audio is not available as a file.

use rodio::{Decoder, Source};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

const N_POINTS: usize = 200;

fn uri_hash(uri: &str) -> String {
    format!("{:x}", md5::compute(uri.as_bytes()))
}

pub fn cache_path(uri: &str) -> Option<PathBuf> {
    let dir = crate::config::waveform_cache_dir().ok()?;
    Some(dir.join(format!("{}.bin", uri_hash(uri))))
}

pub fn load(uri: &str) -> Option<Vec<u8>> {
    let path = cache_path(uri)?;
    let mut file = File::open(path).ok()?;
    let mut buf = Vec::with_capacity(N_POINTS);
    file.read_to_end(&mut buf).ok()?;
    if buf.len() == N_POINTS {
        Some(buf)
    } else {
        None
    }
}

pub fn save(uri: &str, data: &[u8]) {
    if let Some(path) = cache_path(uri) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = File::create(path) {
            let _ = f.write_all(data);
        }
    }
}

pub fn generate_for_file(path: &Path) -> Option<Vec<u8>> {
    // Fast path: use cache if available.
    let uri = format!("file://{}", path.display());
    if let Some(cached) = load(&uri) {
        return Some(cached);
    }

    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut decoder = Decoder::new(reader).ok()?;

    let channels = decoder.channels() as usize;
    let sample_rate = decoder.sample_rate() as usize;
    if channels == 0 || sample_rate == 0 {
        return None;
    }

    // Window of ~50ms per amplitude sample -> enough resolution for 200 points.
    let window_samples = (sample_rate * channels / 20).max(1);

    let mut envelope: Vec<f32> = Vec::new();
    let mut window: Vec<f32> = Vec::with_capacity(window_samples);

    for sample in decoder.by_ref() {
        window.push(sample);
        if window.len() >= window_samples {
            let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
            envelope.push(rms);
            window.clear();
        }
    }

    if !window.is_empty() {
        let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
        envelope.push(rms);
    }

    if envelope.is_empty() {
        return None;
    }

    let bin_size = (envelope.len() as f32 / N_POINTS as f32).max(1.0);
    let mut points = Vec::with_capacity(N_POINTS);
    for i in 0..N_POINTS {
        let start = (i as f32 * bin_size) as usize;
        let end = ((i + 1) as f32 * bin_size).min(envelope.len() as f32) as usize;
        let max = if start < end {
            envelope[start..end].iter().copied().fold(0.0, f32::max)
        } else {
            0.0
        };
        points.push(max);
    }

    // Normalize to 0-7.
    let global_max = points.iter().copied().fold(0.0, f32::max);
    let scale = if global_max > 0.0 {
        7.0 / global_max
    } else {
        0.0
    };
    let quantized: Vec<u8> = points
        .iter()
        .map(|v| (v * scale).clamp(0.0, 7.0).round() as u8)
        .collect();

    save(&uri, &quantized);
    Some(quantized)
}
