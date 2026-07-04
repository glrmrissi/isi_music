use id3::TagLike;
use std::path::Path;

/// Strip control characters from a string, keeping only printable chars, tab, and newline.
/// Removes C0 controls (0x00-0x08, 0x0B-0x1F), DEL (0x7F), and C1 controls (0x80-0x9F).
pub fn sanitize_control_chars(s: &str) -> String {
    s.chars()
        .filter(|c| {
            let code = *c as u32;
            code == 0x09        // TAB
                || code == 0x0A // LF (newline)
                || (code >= 0x20 && code != 0x7F && !(0x80..0xA0).contains(&code))
        })
        .collect()
}

pub fn read_audio_metadata(path: &Path) -> (String, String, String, u64, Option<Vec<u8>>) {
    use symphonia::core::{
        formats::FormatOptions,
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTagKey},
        probe::Hint,
    };

    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

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
            Some((secs * 1000) as u64)
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
                Some(StandardTagKey::Album) => {
                    if album.is_empty() {
                        album = tag.value.to_string();
                    }
                }
                _ => {}
            }
        }
        if cover_art.is_none() {
            if let Some(visual) = rev.visuals().first() {
                cover_art = Some(visual.data.to_vec());
            }
        }
    }

    if artist.is_empty() || title == fallback_name {
        if let Ok(id3tag) = id3::Tag::read_from_path(path) {
            if artist.is_empty() {
                if let Some(a) = id3tag.artist() {
                    artist = a.to_string();
                }
            }
            if title == fallback_name {
                if let Some(t) = id3tag.title() {
                    title = t.to_string();
                }
            }
            if album.is_empty() {
                if let Some(a) = id3tag.album() {
                    album = a.to_string();
                }
            }
            if cover_art.is_none() {
                for pic in id3tag.pictures() {
                    cover_art = Some(pic.data.to_vec());
                    break;
                }
            }
        }
    }

    if artist.is_empty() || title == fallback_name {
        if let Ok(flac_tag) = metaflac::Tag::read_from_path(path) {
            if let Some(vorbis) = flac_tag.vorbis_comments() {
                if artist.is_empty() {
                    if let Some(artist_list) = vorbis.artist() {
                        if let Some(a) = artist_list.first() {
                            artist = a.to_string();
                        }
                    }
                }
                if title == fallback_name {
                    if let Some(title_list) = vorbis.title() {
                        if let Some(t) = title_list.first() {
                            title = t.to_string();
                        }
                    }
                }
                if album.is_empty() {
                    if let Some(album_list) = vorbis.album() {
                        if let Some(a) = album_list.first() {
                            album = a.to_string();
                        }
                    }
                }
            }
            if cover_art.is_none() {
                if let Some(pic) = flac_tag.pictures().next() {
                    cover_art = Some(pic.data.to_vec());
                }
            }
        }
    }

    (
        sanitize_control_chars(&title),
        sanitize_control_chars(&artist),
        sanitize_control_chars(&album),
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
