use rodio::{Decoder, decoder::DecoderBuilder};
use std::{fs::File, io::BufReader, path::Path};

use crate::audio::opus::OpusSource;

pub enum LocalDecoder {
    Symphonia(Decoder<BufReader<File>>),
    Opus(OpusSource),
}

impl LocalDecoder {
    pub fn open(path: &Path) -> Option<Self> {
        let is_opus = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("opus"))
            .unwrap_or(false);

        if is_opus {
            OpusSource::open(path).ok().map(LocalDecoder::Opus)
        } else {
            let file = File::open(path).ok()?;
            let len = file.metadata().ok()?.len();
            DecoderBuilder::new()
                .with_data(BufReader::new(file))
                .with_byte_len(len)
                .with_seekable(true)
                .with_coarse_seek(true)
                .build()
                .ok()
                .map(LocalDecoder::Symphonia)
        }
    }
}

impl Iterator for LocalDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        match self {
            LocalDecoder::Symphonia(d) => d.next(),
            LocalDecoder::Opus(d) => d.next(),
        }
    }
}

impl rodio::Source for LocalDecoder {
    fn current_span_len(&self) -> Option<usize> {
        match self {
            LocalDecoder::Symphonia(d) => d.current_span_len(),
            LocalDecoder::Opus(d) => d.current_span_len(),
        }
    }

    fn channels(&self) -> u16 {
        match self {
            LocalDecoder::Symphonia(d) => d.channels(),
            LocalDecoder::Opus(d) => d.channels(),
        }
    }

    fn sample_rate(&self) -> u32 {
        match self {
            LocalDecoder::Symphonia(d) => d.sample_rate(),
            LocalDecoder::Opus(d) => d.sample_rate(),
        }
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        match self {
            LocalDecoder::Symphonia(d) => d.total_duration(),
            LocalDecoder::Opus(d) => d.total_duration(),
        }
    }

    fn try_seek(&mut self, pos: std::time::Duration) -> Result<(), rodio::source::SeekError> {
        match self {
            LocalDecoder::Symphonia(d) => d.try_seek(pos),
            LocalDecoder::Opus(d) => d.try_seek(pos),
        }
    }
}
