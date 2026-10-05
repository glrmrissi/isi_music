use crate::spotify::TrackSummary;
use anyhow::{Context, Result};
use librespot_core::spotify_uri::SpotifyUri;
use librespot_metadata::{
    Metadata, Playlist as MercuryPlaylist, Track as MercuryTrack,
    playlist::list::SelectedListContent,
};
use librespot_protocol::playlist4_external::SelectedListContent as SelectedListContentMsg;
use protobuf::Message as _;
use tracing::{info, warn};

use super::NativePlayer;

impl NativePlayer {
    pub async fn fetch_playlist_tracks_via_mercury(
        &self,
        playlist_uri: &str,
    ) -> Result<Vec<TrackSummary>> {
        let uri = SpotifyUri::from_uri(playlist_uri)
            .with_context(|| format!("invalid playlist URI: {playlist_uri}"))?;
        let SpotifyUri::Playlist {
            id: playlist_id, ..
        } = &uri
        else {
            anyhow::bail!("not a playlist URI: {playlist_uri}");
        };

        info!("Fetching playlist via Mercury protocol: {playlist_uri}");

        let initial = MercuryPlaylist::get(&self._session, &uri)
            .await
            .with_context(|| format!("Mercury playlist fetch failed: {playlist_uri}"))?;

        let total_tracks = initial.length;
        let playlist_name = initial.name().to_string();
        info!(
            "Mercury playlist '{}' has {} total tracks, initial items: {}",
            playlist_name,
            total_tracks,
            initial.contents.items.len(),
        );

        let mut all_track_uris: Vec<SpotifyUri> = initial
            .tracks()
            .filter(|u| matches!(u, SpotifyUri::Track { .. }))
            .cloned()
            .collect();

        let page_size = 100u32;
        let mut from = all_track_uris.len() as u32;
        while from < total_tracks as u32 {
            let length = page_size.min(total_tracks as u32 - from);
            info!(
                "Mercury pagination: fetching from={} length={}",
                from, length
            );
            let endpoint = format!(
                "/playlist/v2/playlist/{}?from={}&length={}",
                playlist_id.to_base62()?,
                from,
                length,
            );
            let response = self
                ._session
                .spclient()
                .request(&hyper::Method::GET, &endpoint, None, None)
                .await
                .with_context(|| format!("Mercury playlist range fetch failed at from={from}"))?;
            let msg = SelectedListContentMsg::parse_from_bytes(&response)
                .with_context(|| "failed to parse playlist protobuf")?;
            let selected = SelectedListContent::try_from(&msg)
                .with_context(|| "failed to convert playlist protobuf")?;
            let page_count = selected.contents.items.len();
            for item in selected.contents.items.iter() {
                if matches!(item.id, SpotifyUri::Track { .. }) {
                    all_track_uris.push(item.id.clone());
                }
            }
            info!(
                "Mercury pagination: got {} items, {} tracks (total so far: {})",
                page_count,
                selected.contents.items.len(),
                all_track_uris.len(),
            );
            if page_count == 0 {
                warn!("Mercury pagination returned 0 items, stopping");
                break;
            }
            from += page_count as u32;
        }

        info!(
            "Mercury playlist has {} track URIs total (episodes filtered)",
            all_track_uris.len()
        );

        use futures::stream::StreamExt;

        let session = self._session.clone();
        let concurrency = 20;
        let results: Vec<(usize, TrackSummary)> =
            futures::stream::iter(all_track_uris.into_iter().enumerate())
                .map(|(idx, track_uri)| {
                    let session = session.clone();
                    async move {
                        let uri_str = track_uri.to_uri().unwrap_or_default();
                        match MercuryTrack::get(&session, &track_uri).await {
                            Ok(t) => {
                                let artist = t
                                    .artists
                                    .iter()
                                    .map(|a| a.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                let name = if t.name.is_empty() {
                                    "Unknown".to_string()
                                } else {
                                    t.name
                                };
                                let artist = if artist.is_empty() {
                                    "Unknown".to_string()
                                } else {
                                    artist
                                };
                                let album = if t.album.name.is_empty() {
                                    "Unknown".to_string()
                                } else {
                                    t.album.name
                                };
                                if name == "Unknown" || artist == "Unknown" {
                                    warn!(
                                        "Mercury track has incomplete metadata: {uri_str} (name={}, artist={}, album={})",
                                        name, artist, album
                                    );
                                }
                                Some((
                                    idx,
                                    TrackSummary {
                                        name,
                                        artist,
                                        album,
                                        duration_ms: t.duration.max(0) as u64,
                                        uri: uri_str,
                                        cover_path: None,
                                        added_at: None,
                                    },
                                ))
                            }
                            Err(e) => {
                                warn!(
                                    "Mercury track metadata failed for {uri_str}: {e}; using URI fallback"
                                );
                                Some((
                                    idx,
                                    TrackSummary {
                                        name: "Unknown".to_string(),
                                        artist: "Unknown".to_string(),
                                        album: "Unknown".to_string(),
                                        duration_ms: 0,
                                        uri: uri_str,
                                        cover_path: None,
                                        added_at: None,
                                    },
                                ))
                            }
                        }
                    }
                })
                .buffer_unordered(concurrency)
                .filter_map(std::future::ready)
                .collect()
                .await;

        let mut indexed: Vec<(usize, TrackSummary)> = results;
        indexed.sort_by_key(|(idx, _)| *idx);
        let tracks: Vec<TrackSummary> = indexed.into_iter().map(|(_, t)| t).collect();

        info!(
            "Mercury playlist fetch complete: {} tracks with metadata",
            tracks.len()
        );
        Ok(tracks)
    }
}
