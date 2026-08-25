use id3::TagLike;
use std::path::Path;

type AudioMetadata = (String, String, String, u64, Option<Vec<u8>>);

/// Try to read Opus metadata (OpusTags) using opusmeta.
/// Returns (title, artist, album, duration_ms, cover_art) if successful.
fn read_opus_metadata(path: &Path) -> Option<AudioMetadata> {
    let tag = opusmeta::Tag::read_from_path(path).ok()?;

    let title = tag
        .get_one(&opusmeta::LowercaseString::new_from_str("TITLE"))
        .or_else(|| tag.get_one(&opusmeta::LowercaseString::new_from_str("title")))
        .cloned()
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        });

    let artist = tag
        .get_one(&opusmeta::LowercaseString::new_from_str("ARTIST"))
        .or_else(|| tag.get_one(&opusmeta::LowercaseString::new_from_str("artist")))
        .cloned()
        .unwrap_or_default();

    let album = tag
        .get_one(&opusmeta::LowercaseString::new_from_str("ALBUM"))
        .or_else(|| tag.get_one(&opusmeta::LowercaseString::new_from_str("album")))
        .cloned()
        .unwrap_or_default();

    // Get first picture (usually cover front)
    let cover_art = tag
        .iter_pictures()
        .and_then(|mut iter| iter.next().and_then(|res| res.ok()).map(|pic| pic.data));

    // Duration from the last Ogg page's granule position
    let duration_ms = read_opus_duration(path).unwrap_or(0);

    Some((title, artist, album, duration_ms, cover_art))
}

/// Duration from the last Ogg page's granule position (granulepos counts
/// 48 kHz samples including pre-skip).
fn read_opus_duration(path: &Path) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    const OPUS_SAMPLE_RATE: u32 = 48_000;

    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(65536))).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;

    let idx = buf.windows(4).rposition(|w| w == b"OggS")?;
    if buf.len() < idx + 27 || buf[idx + 4] != 0 {
        return None;
    }
    let granulepos = u64::from_le_bytes(buf[idx + 6..idx + 14].try_into().ok()?);
    // Skip preskip from OpusHead (we assume 0 for simplicity; for precision we'd parse the header)
    let samples = granulepos;
    Some((samples * 1000) / OPUS_SAMPLE_RATE as u64)
}

/// Strip control characters from a string, keeping only printable chars, tab, and newline.
/// Removes C0 controls (0x00-0x08, 0x0B-0x1F), DEL (0x7F), and C1 controls (0x80-0x9F).
pub fn sanitize_control_chars(s: &str) -> std::borrow::Cow<'_, str> {
    let needs_sanitize = s.chars().any(|c| {
        let code = c as u32;
        code != 0x09
            && code != 0x0A
            && (code < 0x20 || code == 0x7F || (0x80..0xA0).contains(&code))
    });
    if !needs_sanitize {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(
        s.chars()
            .filter(|c| {
                let code = *c as u32;
                code == 0x09        // TAB
                    || code == 0x0A // LF (newline)
                    || (code >= 0x20 && code != 0x7F && !(0x80..0xA0).contains(&code))
            })
            .collect(),
    )
}

pub fn read_audio_metadata(path: &Path) -> AudioMetadata {
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    // Try Opus first (symphonia doesn't support Opus codec)
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("opus"))
        .unwrap_or(false)
        && let Some((title, artist, album, duration_ms, cover_art)) = read_opus_metadata(path)
    {
        return (
            sanitize_control_chars(&title).into_owned(),
            sanitize_control_chars(&artist).into_owned(),
            sanitize_control_chars(&album).into_owned(),
            duration_ms,
            cover_art,
        );
    }

    use symphonia::core::{
        formats::FormatOptions,
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTagKey},
        probe::Hint,
    };

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (fallback_name, String::new(), String::new(), 0, None),
    };

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = match symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    ) {
        Ok(p) => p,
        Err(_) => return (fallback_name, String::new(), String::new(), 0, None),
    };

    let mut format = probed.format;

    let duration_ms = format
        .default_track()
        .and_then(|t| {
            let tb = t.codec_params.time_base?;
            let n_frames = t.codec_params.n_frames?;
            let secs = tb.calc_time(n_frames).seconds;
            Some(secs * 1000)
        })
        .unwrap_or(0);

    let mut title = fallback_name.clone();
    let mut artist = String::new();
    let mut album = String::new();
    let mut cover_art: Option<Vec<u8>> = None;

    let meta_ref = format.metadata();
    if let Some(rev) = meta_ref.current() {
        for tag in rev.tags() {
            match tag.std_key {
                Some(StandardTagKey::TrackTitle) => {
                    if title == fallback_name {
                        title = tag.value.to_string();
                    }
                }
                Some(StandardTagKey::Artist) => {
                    if artist.is_empty() {
                        artist = tag.value.to_string();
                    }
                }
                Some(StandardTagKey::AlbumArtist) => {
                    if artist.is_empty() {
                        artist = tag.value.to_string();
                    }
                }
                Some(StandardTagKey::Album) if album.is_empty() => {
                    album = tag.value.to_string();
                }
                _ => {}
            }
        }
        if cover_art.is_none()
            && let Some(visual) = rev.visuals().first()
        {
            cover_art = Some(visual.data.to_vec());
        }
    }

    if (artist.is_empty() || title == fallback_name)
        && let Ok(id3tag) = id3::Tag::read_from_path(path)
    {
        if artist.is_empty()
            && let Some(a) = id3tag.artist()
        {
            artist = a.to_string();
        }
        if title == fallback_name
            && let Some(t) = id3tag.title()
        {
            title = t.to_string();
        }
        if album.is_empty()
            && let Some(a) = id3tag.album()
        {
            album = a.to_string();
        }
        if cover_art.is_none()
            && let Some(pic) = id3tag.pictures().next()
        {
            cover_art = Some(pic.data.to_vec());
        }
    }

    if (artist.is_empty() || title == fallback_name)
        && let Ok(flac_tag) = metaflac::Tag::read_from_path(path)
    {
        if let Some(vorbis) = flac_tag.vorbis_comments() {
            if artist.is_empty()
                && let Some(artist_list) = vorbis.artist()
                && let Some(a) = artist_list.first()
            {
                artist = a.to_string();
            }
            if title == fallback_name
                && let Some(title_list) = vorbis.title()
                && let Some(t) = title_list.first()
            {
                title = t.to_string();
            }
            if album.is_empty()
                && let Some(album_list) = vorbis.album()
                && let Some(a) = album_list.first()
            {
                album = a.to_string();
            }
        }
        if cover_art.is_none()
            && let Some(pic) = flac_tag.pictures().next()
        {
            cover_art = Some(pic.data.to_vec());
        }
    }

    (
        sanitize_control_chars(&title).into_owned(),
        sanitize_control_chars(&artist).into_owned(),
        sanitize_control_chars(&album).into_owned(),
        duration_ms,
        cover_art,
    )
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[path = "../../tests/app/metadata.rs"]
mod tests;
