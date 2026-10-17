use crate::utils::debug_overlay::{DebugOverlay, LogLevel};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

use super::parse::{normalize_search_query, parse_lrc, parse_plain};
use super::types::LyricsData;

pub(super) async fn fetch_lyrics(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
    musixmatch_api_key: Option<String>,
) -> Option<LyricsData> {
    let normalized_title = normalize_search_query(title);
    let normalized_artist = normalize_search_query(artist);

    if let Some(lyrics) =
        fetch_from_lrclib(http, &normalized_title, &normalized_artist, debug_overlay).await
    {
        return Some(lyrics);
    }

    if let Some(key) = musixmatch_api_key
        && let Some(lyrics) = fetch_from_musixmatch(
            http,
            &normalized_title,
            &normalized_artist,
            debug_overlay,
            &key,
        )
        .await
    {
        return Some(lyrics);
    }

    info!("lyrics: all synced APIs failed, trying fallback -> lyrics.ovh");
    debug_overlay.log(
        LogLevel::Info,
        "lyrics: all synced APIs failed, trying fallback -> lyrics.ovh",
    );
    fetch_from_ovh(http, &normalized_title, &normalized_artist, debug_overlay).await
}

async fn fetch_from_lrclib(
    http: &reqwest::Client,
    track: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
) -> Option<LyricsData> {
    let url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        urlencoding::encode(artist),
        urlencoding::encode(track)
    );

    info!("lyrics: fetching from lrclib -> {} - {}", artist, track);
    debug_overlay.log(
        LogLevel::Info,
        format!("lyrics: fetching from lrclib -> {} - {}", artist, track),
    );

    for attempt in 1..=2 {
        let resp = match tokio::time::timeout(
            Duration::from_secs(10),
            http.get(&url)
                .header(
                    "User-Agent",
                    "isi-music/0.1[](https://github.com/glrmrissi/isi-music)",
                )
                .send(),
        )
        .await
        {
            Ok(Ok(r)) if r.status().is_success() => r,
            Ok(Ok(r)) => {
                if attempt == 2 {
                    debug_overlay.log(
                        LogLevel::Warn,
                        format!("lyrics: lrclib returned status {} (final)", r.status()),
                    );
                }
                continue;
            }
            Ok(Err(e)) => {
                if attempt == 2 {
                    debug_overlay.log(
                        LogLevel::Warn,
                        format!("lyrics: lrclib request failed: {e}"),
                    );
                }
                continue;
            }
            Err(_) => {
                if attempt == 2 {
                    debug_overlay.log(LogLevel::Warn, "lyrics: lrclib request timed out");
                }
                continue;
            }
        };

        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                if attempt == 2 {
                    debug_overlay.log(
                        LogLevel::Warn,
                        format!("lyrics: json parse error from lrclib: {e}"),
                    );
                }
                continue;
            }
        };

        if let Some(lrc) = json["syncedLyrics"].as_str().filter(|s| !s.is_empty()) {
            let parsed = parse_lrc(lrc);
            if !parsed.is_empty() {
                debug_overlay.log(
                    LogLevel::Info,
                    format!(
                        "lyrics: synced lyrics found from lrclib ({} lines)",
                        parsed.lines.len()
                    ),
                );
                info!("lyrics: synced lyrics found ({} lines)", parsed.lines.len());
                return Some(parsed);
            }
        }

        if let Some(plain) = json["plainLyrics"].as_str().filter(|s| !s.is_empty()) {
            let parsed = parse_plain(plain);
            debug_overlay.log(
                LogLevel::Info,
                "lyrics: plain lyrics found from lrclib".to_string(),
            );
            info!("lyrics: plain lyrics found from lrclib");
            return Some(parsed);
        }

        return None;
    }

    None
}

#[derive(Debug, Deserialize)]
struct MusixmatchResponse {
    message: MusixmatchMessage,
}

#[derive(Debug, Deserialize)]
struct MusixmatchMessage {
    header: MusixmatchHeader,
    body: MusixmatchBody,
}

#[derive(Debug, Deserialize)]
struct MusixmatchHeader {
    status_code: i32,
}

#[derive(Debug, Deserialize)]
struct MusixmatchBody {
    #[serde(default)]
    track_list: Vec<MusixmatchTrack>,
}
#[derive(Debug, Deserialize)]
struct MusixmatchTrack {
    track: TrackData,
}

#[derive(Debug, Deserialize)]
struct TrackData {
    track_id: i32,
    track_name: String,
    has_lyrics: bool,
}

async fn fetch_from_musixmatch(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
    api_key: &str,
) -> Option<LyricsData> {
    debug_overlay.log(
        LogLevel::Info,
        format!("lyrics: fetching from musixmatch -> {} - {}", artist, title),
    );

    let search_resp = match tokio::time::timeout(
        Duration::from_secs(5),
        http.get("https://api.musixmatch.com/ws/1.1/track.search")
            .query(&[
                ("q_track", title),
                ("q_artist", artist),
                ("f_has_lyrics", "true"),
                ("apikey", api_key),
            ])
            .send(),
    )
    .await
    {
        Ok(Ok(r)) => {
            if r.status() == reqwest::StatusCode::UNAUTHORIZED {
                debug_overlay.log(
                    LogLevel::Error,
                    "lyrics: Musixmatch HTTP 401 - Unauthorized".to_string(),
                );
                return None;
            }
            r
        }
        _ => return None,
    };

    let search_json: MusixmatchResponse = match search_resp.json().await {
        Ok(j) => j,
        Err(e) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: musixmatch json parse error: {e}"),
            );
            return None;
        }
    };

    match search_json.message.header.status_code {
        401 => {
            debug_overlay.log(
                LogLevel::Error,
                "lyrics: Musixmatch API Key invalid (Internal 401)".to_string(),
            );
            return None;
        }
        200 => {}
        code => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: Musixmatch returned internal code {}", code),
            );
            return None;
        }
    }

    let track = match search_json.message.body.track_list.first() {
        Some(t) => t,
        None => {
            debug_overlay.log(
                LogLevel::Info,
                "lyrics: no tracks found on musixmatch".to_string(),
            );
            return None;
        }
    };

    if !track.track.has_lyrics {
        debug_overlay.log(
            LogLevel::Warn,
            format!("lyrics: track '{}' has no lyrics", track.track.track_name),
        );
        return None;
    }

    let track_id = track.track.track_id;

    let lyrics_resp = match tokio::time::timeout(
        Duration::from_secs(5),
        http.get("https://api.musixmatch.com/ws/1.1/track.lyrics.get")
            .query(&[
                ("track_id", track_id.to_string()),
                ("apikey", api_key.to_string()),
            ])
            .send(),
    )
    .await
    {
        Ok(Ok(r)) => r,
        _ => return None,
    };

    let lyrics_json: serde_json::Value = match lyrics_resp.json().await {
        Ok(j) => j,
        Err(_) => return None,
    };

    if let Some(lyrics_text) = lyrics_json["message"]["body"]["lyrics"]["lyrics_body"].as_str() {
        let cleaned = lyrics_text
            .lines()
            .filter(|l| !l.contains("****") && !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        if !cleaned.is_empty() {
            let parsed = parse_plain(&cleaned);
            debug_overlay.log(
                LogLevel::Info,
                format!(
                    "lyrics: found from musixmatch ({} lines)",
                    parsed.lines.len()
                ),
            );
            return Some(parsed);
        }
    }

    None
}

async fn fetch_from_ovh(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
    debug_overlay: &Arc<DebugOverlay>,
) -> Option<LyricsData> {
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );

    info!("lyrics: fetching from lyrics.ovh -> {} - {}", artist, title);
    debug_overlay.log(
        LogLevel::Info,
        format!("lyrics: fetching from lyrics.ovh -> {} - {}", artist, title),
    );

    let resp = match tokio::time::timeout(Duration::from_secs(6), http.get(&url).send()).await {
        Ok(Ok(r)) if r.status().is_success() => r,
        Ok(Ok(r)) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: lyrics.ovh returned status {}", r.status()),
            );
            return None;
        }
        Ok(Err(e)) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: lyrics.ovh request failed: {e}"),
            );
            return None;
        }
        Err(_) => {
            debug_overlay.log(LogLevel::Warn, "lyrics: lyrics.ovh request timed out");
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            debug_overlay.log(
                LogLevel::Warn,
                format!("lyrics: json parse error from lyrics.ovh: {e}"),
            );
            return None;
        }
    };

    let plain = match json["lyrics"].as_str() {
        Some(l) if !l.is_empty() => l,
        _ => {
            debug_overlay.log(
                LogLevel::Warn,
                "lyrics: lyrics.ovh returned empty or no lyrics field",
            );
            return None;
        }
    };

    let parsed = parse_plain(plain);
    if !parsed.is_empty() {
        debug_overlay.log(
            LogLevel::Info,
            format!(
                "lyrics: plain lyrics found from lyrics.ovh ({} lines)",
                parsed.lines.len()
            ),
        );
        info!(
            "lyrics: plain lyrics found from lyrics.ovh ({} lines)",
            parsed.lines.len()
        );
        Some(parsed)
    } else {
        debug_overlay.log(
            LogLevel::Warn,
            "lyrics: received empty lyrics from lyrics.ovh",
        );
        None
    }
}
