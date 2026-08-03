use std::time::SystemTime;

#[test]
fn unix_now_returns_recent_timestamp() {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let result = crate::app::metadata::unix_now();

    // Should be within 2 seconds of our reference
    let diff = if result > now {
        result - now
    } else {
        now - result
    };
    assert!(diff < 2, "unix_now() off by {diff}s");
}

#[test]
fn unix_now_is_monotonic() {
    let a = crate::app::metadata::unix_now();
    let b = crate::app::metadata::unix_now();
    assert!(b >= a);
}

// ---------------------------------------------------------------------------
// Opus (Ogg) metadata
// ---------------------------------------------------------------------------

fn opus_head_packet(channels: u8, preskip: u16) -> Vec<u8> {
    let mut p = b"OpusHead".to_vec();
    p.push(1); // version
    p.push(channels);
    p.extend_from_slice(&preskip.to_le_bytes());
    p.extend_from_slice(&48_000u32.to_le_bytes()); // input sample rate
    p.extend_from_slice(&0i16.to_le_bytes()); // output gain
    p.push(0); // channel mapping family 0
    p
}

fn opus_tags_packet(comments: &[&str]) -> Vec<u8> {
    let mut p = b"OpusTags".to_vec();
    let vendor = b"isi-music-test";
    p.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    p.extend_from_slice(vendor);
    p.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for c in comments {
        p.extend_from_slice(&(c.len() as u32).to_le_bytes());
        p.extend_from_slice(c.as_bytes());
    }
    p
}

/// FLAC METADATA_BLOCK_PICTURE structure (big-endian fields).
fn flac_picture_block(data: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&3u32.to_be_bytes()); // type: front cover
    let mime = b"image/png";
    b.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    b.extend_from_slice(mime);
    b.extend_from_slice(&0u32.to_be_bytes()); // description length
    for v in [1u32, 1, 24, 0] {
        b.extend_from_slice(&v.to_be_bytes()); // width, height, depth, colors
    }
    b.extend_from_slice(&(data.len() as u32).to_be_bytes());
    b.extend_from_slice(data);
    b
}

fn write_test_opus(path: &std::path::Path, comments: &[String], last_granulepos: u64) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = ogg::writing::PacketWriter::new(file);
    let serial = 0x6973_6d75;
    let refs: Vec<&str> = comments.iter().map(|s| s.as_str()).collect();

    writer
        .write_packet(
            opus_head_packet(2, 312),
            serial,
            ogg::writing::PacketWriteEndInfo::EndPage,
            0,
        )
        .unwrap();
    writer
        .write_packet(
            opus_tags_packet(&refs),
            serial,
            ogg::writing::PacketWriteEndInfo::EndPage,
            0,
        )
        .unwrap();
    // One packet is enough to give the stream a final granule position
    writer
        .write_packet(
            vec![0u8; 4],
            serial,
            ogg::writing::PacketWriteEndInfo::EndStream,
            last_granulepos,
        )
        .unwrap();
}

#[test]
fn opus_metadata_reads_tags_cover_and_duration() {
    use base64::Engine;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.opus");

    let cover_bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 1, 2, 3, 4]; // fake PNG
    let picture_b64 =
        base64::engine::general_purpose::STANDARD.encode(flac_picture_block(&cover_bytes));
    let comments = vec![
        "TITLE=Opus Song".to_string(),
        "ARTIST=Opus Artist".to_string(),
        "ALBUM=Opus Album".to_string(),
        format!("METADATA_BLOCK_PICTURE={picture_b64}"),
    ];
    // 3 seconds at 48 kHz
    write_test_opus(&path, &comments, 48_000 * 3);

    let (title, artist, album, duration_ms, cover) =
        crate::app::metadata::read_audio_metadata(&path);

    assert_eq!(title, "Opus Song");
    assert_eq!(artist, "Opus Artist");
    assert_eq!(album, "Opus Album");
    assert_eq!(cover.as_deref(), Some(cover_bytes.as_slice()));
    assert!(
        (2500..=3500).contains(&duration_ms),
        "duration should be ~3000ms, got {duration_ms}ms"
    );
}
