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

/// Generates the waveform envelope and measures the actual duration of the
/// file. Returns `(duration_ms, envelope)`. Duration is 0 when served from cache.
pub(crate) fn generate_for_file_with_cancel(
    path: &Path,
    mut is_cancelled: impl FnMut() -> bool,
) -> Option<(u64, Vec<u8>)> {
    if is_cancelled() {
        return None;
    }

    // Fast path: use cache if available.
    let uri = format!("file://{}", path.display());
    if let Some(cached) = load(&uri) {
        return Some((0, cached));
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
    let mut total_samples: u64 = 0;

    for (index, sample) in decoder.by_ref().enumerate() {
        if index % 4096 == 0 && is_cancelled() {
            return None;
        }
        total_samples += 1;
        window.push(sample);
        if window.len() >= window_samples {
            let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
            envelope.push(rms);
            window.clear();
        }
    }

    if is_cancelled() {
        return None;
    }

    if !window.is_empty() {
        let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
        envelope.push(rms);
    }

    if envelope.is_empty() {
        return None;
    }

    let duration_ms = (total_samples / (sample_rate * channels) as u64) * 1000;

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
    Some((duration_ms, quantized))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn resource_regression_waveform_generation_stops_when_cancelled_during_decode() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let sample_count = 16_384u32;
        let data_size = sample_count * 2;
        let mut wav = Vec::with_capacity(44 + data_size as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&88_200u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        wav.resize(44 + data_size as usize, 0);
        std::fs::write(file.path(), wav).unwrap();

        let checks = Cell::new(0);
        let result = generate_for_file_with_cancel(file.path(), || {
            let next = checks.get() + 1;
            checks.set(next);
            next == 3
        });

        assert!(result.is_none());
        assert_eq!(checks.get(), 3);
    }
}
