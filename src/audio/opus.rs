//! Opus (.opus / RFC 7845) playback support.
//!
//! symphonia has no native Opus codec, so local Opus files are demuxed with the
//! pure-Rust `ogg` crate and decoded by libopus (via the safe `opus2` bindings).
//! The result is exposed as a rodio `Source`, so it plugs into the same
//! Sink / AnalyzingSource pipeline as MP3 and FLAC.

use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

const OPUS_SAMPLE_RATE: u32 = 48_000;
/// Maximum decoded frames per Opus packet (120 ms at 48 kHz).
const MAX_FRAMES: usize = 5760;

pub struct OpusSource {
    path: PathBuf,
    reader: ogg::PacketReader<BufReader<File>>,
    decoder: opus2::Decoder,
    channels: u16,
    /// Pre-skip samples still to discard (interleaved count).
    preskip_remaining: u64,
    /// Decoded interleaved samples not yet consumed.
    buf: Vec<f32>,
    pos: usize,
    finished: bool,
    duration: Option<Duration>,
}

impl OpusSource {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = ogg::PacketReader::new(BufReader::new(file));

        let head = reader.read_packet_expected().context("reading OpusHead")?;
        let (channels, preskip) = parse_opus_head(&head.data)?;

        // OpusTags packet: metadata is read separately, skip it here
        let _ = reader.read_packet();

        let decoder = opus2::Decoder::new(
            OPUS_SAMPLE_RATE,
            if channels == 1 {
                opus2::Channels::Mono
            } else {
                opus2::Channels::Stereo
            },
        )
        .context("creating Opus decoder")?;

        let duration = probe_duration(path, preskip);

        Ok(Self {
            path: path.to_path_buf(),
            reader,
            decoder,
            channels: channels as u16,
            preskip_remaining: preskip as u64 * channels as u64,
            buf: Vec::new(),
            pos: 0,
            finished: false,
            duration,
        })
    }

    /// Decode the next packet into `buf`. Returns false at end of stream.
    fn fill_buffer(&mut self) -> bool {
        if self.finished {
            return false;
        }
        loop {
            match self.reader.read_packet() {
                Ok(Some(packet)) => {
                    let cap = MAX_FRAMES * self.channels as usize;
                    self.buf.clear();
                    self.buf.resize(cap, 0.0);
                    match self
                        .decoder
                        .decode_float(&packet.data, &mut self.buf, false)
                    {
                        Ok(frames) => {
                            let len = frames * self.channels as usize;
                            self.buf.truncate(len);
                            if self.preskip_remaining > 0 {
                                let skip = (self.preskip_remaining as usize).min(len);
                                self.buf.drain(..skip);
                                self.preskip_remaining -= skip as u64;
                            }
                            self.pos = 0;
                            if !self.buf.is_empty() {
                                return true;
                            }
                        }
                        // Undecodable packet: skip it and keep going
                        Err(_) => continue,
                    }
                }
                Ok(None) | Err(_) => {
                    self.finished = true;
                    return false;
                }
            }
        }
    }
}

fn parse_opus_head(data: &[u8]) -> Result<(u8, u16)> {
    if data.len() < 19 || &data[..8] != b"OpusHead" {
        bail!("not an Opus stream (missing OpusHead)");
    }
    let channels = data[9];
    let preskip = u16::from_le_bytes([data[10], data[11]]);
    let mapping = data[18];
    if mapping != 0 {
        bail!("multistream Opus (channel mapping {mapping}) not supported");
    }
    if channels != 1 && channels != 2 {
        bail!("unsupported Opus channel count: {channels}");
    }
    Ok((channels, preskip))
}

/// Duration from the last Ogg page's granule position (granulepos counts
/// 48 kHz samples including pre-skip).
fn probe_duration(path: &Path, preskip: u16) -> Option<Duration> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(65536))).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;

    let idx = buf.windows(4).rposition(|w| w == b"OggS")?;
    if buf.len() < idx + 27 || buf[idx + 4] != 0 {
        return None;
    }
    let granulepos = u64::from_le_bytes(buf[idx + 6..idx + 14].try_into().ok()?);
    let samples = granulepos.saturating_sub(preskip as u64);
    Some(Duration::from_secs_f64(
        samples as f64 / OPUS_SAMPLE_RATE as f64,
    ))
}

impl Iterator for OpusSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.buf.len() && !self.fill_buffer() {
            return None;
        }
        let sample = self.buf[self.pos];
        self.pos += 1;
        Some(sample)
    }
}

impl rodio::Source for OpusSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        OPUS_SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        // Ogg streams aren't cheap to seek: re-open from the start and
        // decode-discard up to the target. Opus decodes far faster than
        // realtime, so this is acceptable for interactive seeks.
        let fresh = Self::open(&self.path).map_err(|_| rodio::source::SeekError::NotSupported {
            underlying_source: "OpusSource",
        })?;
        *self = fresh;

        let mut remaining =
            (pos.as_secs_f64() * OPUS_SAMPLE_RATE as f64) as u64 * self.channels as u64;
        while remaining > 0 {
            if self.pos < self.buf.len() {
                let take = ((self.buf.len() - self.pos) as u64).min(remaining);
                self.pos += take as usize;
                remaining -= take;
            } else if !self.fill_buffer() {
                break; // target beyond end of stream: land at EOF
            }
        }
        Ok(())
    }
}
