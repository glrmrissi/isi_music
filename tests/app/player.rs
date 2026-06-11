use std::sync::Arc;

use super::super::App;
use crate::player::QueuedTrack;

#[path = "mock_player.rs"]
mod mock_player;
use mock_player::MockPlayer;

// ---------------------------------------------------------------------------
// sync_queue_display
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sync_queue_display_empty_when_no_players() {
    let mut app = App::new_for_test().await;
    app.player = None;
    app.parked_player = None;

    app.sync_queue_display();

    assert!(app.state.queue_items.is_empty());
}

#[tokio::test]
async fn sync_queue_display_shows_player_queue() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::with_queue(vec![
        QueuedTrack {
            uri: "spotify:track:a".into(),
            name: "Track A".into(),
            artist: "Artist A".into(),
            duration_ms: 100_000,
            cover_path: None,
        },
        QueuedTrack {
            uri: "spotify:track:b".into(),
            name: "Track B".into(),
            artist: "Artist B".into(),
            duration_ms: 200_000,
            cover_path: None,
        },
    ]));
    app.player = Some(mock);
    app.parked_player = None;

    app.sync_queue_display();

    assert_eq!(app.state.queue_items.len(), 2);
    assert_eq!(
        app.state.queue_items[0],
        ("Track A".into(), "Artist A".into())
    );
    assert_eq!(
        app.state.queue_items[1],
        ("Track B".into(), "Artist B".into())
    );
}

#[tokio::test]
async fn sync_queue_display_shows_parked_with_prefix_when_spotify_active() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::with_queue(vec![QueuedTrack {
        uri: "spotify:track:a".into(),
        name: "Track A".into(),
        artist: "Artist A".into(),
        duration_ms: 100_000,
        cover_path: None,
    }]));
    let parked = Box::new(MockPlayer::with_queue(vec![QueuedTrack {
        uri: "file:///music/b".into(),
        name: "Track B".into(),
        artist: "Artist B".into(),
        duration_ms: 200_000,
        cover_path: None,
    }]));
    app.player = Some(mock);
    app.parked_player = Some(parked);
    app.local_active = false;

    app.sync_queue_display();

    assert_eq!(app.state.queue_items.len(), 2);
    assert_eq!(
        app.state.queue_items[0],
        ("Track A".into(), "Artist A".into())
    );
    assert!(app.state.queue_items[1].0.contains("Track B"));
}

#[tokio::test]
async fn sync_queue_display_skips_parked_when_none() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::with_queue(vec![QueuedTrack {
        uri: "spotify:track:a".into(),
        name: "Track A".into(),
        artist: "Artist A".into(),
        duration_ms: 100_000,
        cover_path: None,
    }]));
    app.player = Some(mock);

    app.sync_queue_display();

    assert_eq!(app.state.queue_items.len(), 1);
}

// ---------------------------------------------------------------------------
// activate_local_player / activate_spotify_player
// ---------------------------------------------------------------------------

#[tokio::test]
async fn activate_local_player_already_local_is_noop() {
    let mut app = App::new_for_test().await;
    app.local_active = true;

    app.activate_local_player();

    assert!(app.local_active);
}

#[tokio::test]
async fn activate_local_player_swaps_with_parked() {
    let mut app = App::new_for_test().await;
    let parked = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player = Some(Box::new(MockPlayer::new(Arc::default(), Arc::default())));
    app.parked_player = Some(parked);
    app.local_active = false;

    app.activate_local_player();

    // player = None before swap, so parked moves to player, parked becomes None
    assert!(app.local_active);
    assert!(app.parked_player.is_none());
    assert!(app.player.is_some());
}

#[tokio::test]
async fn activate_local_player_pauses_current_before_swap() {
    let mut app = App::new_for_test().await;
    let mut player = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    player.is_playing = true;
    app.player = Some(player);
    app.parked_player = Some(Box::new(MockPlayer::new(Arc::default(), Arc::default())));
    app.local_active = false;

    app.activate_local_player();

    assert!(!app.player.as_ref().unwrap().is_playing());
}

#[tokio::test]
async fn activate_local_player_no_parked_sets_flag() {
    let mut app = App::new_for_test().await;
    app.player = Some(Box::new(MockPlayer::new(Arc::default(), Arc::default())));
    app.parked_player = None;
    app.local_active = false;

    app.activate_local_player();

    assert!(app.local_active);
    assert!(app.player.is_none());
}

#[tokio::test]
async fn activate_spotify_player_already_spotify_is_noop() {
    let mut app = App::new_for_test().await;
    app.local_active = false;

    app.activate_spotify_player();

    assert!(!app.local_active);
}

#[tokio::test]
async fn activate_spotify_player_swaps_with_parked() {
    let mut app = App::new_for_test().await;
    let parked = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player = Some(Box::new(MockPlayer::new(Arc::default(), Arc::default())));
    app.parked_player = Some(parked);
    app.local_active = true;

    app.activate_spotify_player();

    // player = None before swap, so parked moves to player, parked becomes None
    assert!(!app.local_active);
    assert!(app.player.is_some());
    assert!(app.parked_player.is_none());
}

#[tokio::test]
async fn activate_spotify_player_no_parked_clears_player() {
    let mut app = App::new_for_test().await;
    let player = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player = Some(player);
    app.parked_player = None;
    app.local_active = true;

    app.activate_spotify_player();

    assert!(!app.local_active);
    assert!(app.player.is_none());
}

#[tokio::test]
async fn activate_spotify_player_pauses_current_before_swap() {
    let mut app = App::new_for_test().await;
    let mut player = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    player.is_playing = true;
    app.player = Some(player);
    app.parked_player = Some(Box::new(MockPlayer::new(Arc::default(), Arc::default())));
    app.local_active = true;

    app.activate_spotify_player();

    assert!(!app.player.as_ref().unwrap().is_playing());
}

// ---------------------------------------------------------------------------
// on_track_started
// ---------------------------------------------------------------------------

#[tokio::test]
async fn on_track_started_resets_progress_and_art() {
    let mut app = App::new_for_test().await;
    app.current_track_uri = "spotify:track:abc".into();
    app.state.playback.progress_ms = 50000;
    app.state.playback.artist = "Test Artist".into();
    app.state.playback.title = "Test Song".into();
    app.state.album_art = Some(crate::ui::AlbumArtData { image_state: None });
    app.album_art_pending = Some(tokio::sync::oneshot::channel().1);

    app.on_track_started();

    assert_eq!(app.state.playback.progress_ms, 0);
    assert_eq!(app.progress_at_play_start, 0);
    assert!(app.playing_started_at.is_none());
    assert!(app.state.album_art.is_none());
    assert!(app.album_art_pending.is_none());
    assert!(app.last_art_uri.is_empty());
}

#[tokio::test]
async fn on_track_started_tracks_recent_uris() {
    let mut app = App::new_for_test().await;
    app.current_track_uri = "spotify:track:abc".into();

    app.on_track_started();

    assert_eq!(app.recent_track_uris.len(), 1);
    assert_eq!(app.recent_track_uris[0], "spotify:track:abc");
}

#[tokio::test]
async fn on_track_started_limited_to_five_uris() {
    let mut app = App::new_for_test().await;
    for i in 0..5 {
        app.recent_track_uris
            .push_back(format!("spotify:track:old{i}"));
    }
    app.current_track_uri = "spotify:track:new".into();

    app.on_track_started();

    assert_eq!(app.recent_track_uris.len(), 5);
    assert_eq!(app.recent_track_uris[4], "spotify:track:new");
}

#[tokio::test]
async fn on_track_started_ignores_non_spotify_uris() {
    let mut app = App::new_for_test().await;
    app.current_track_uri = "file:///music/song.mp3".into();

    app.on_track_started();

    assert!(app.recent_track_uris.is_empty());
}

#[tokio::test]
async fn on_track_started_resets_lyrics_state() {
    let mut app = App::new_for_test().await;
    app.state.playback.lyrics = Some(crate::utils::lyrics::LyricsData {
        is_synced: true,
        lines: vec![],
    });
    app.state.playback.lyrics_loading = true;
    app.state.playback.lyrics_scroll = 10;

    app.on_track_started();

    assert!(app.state.playback.lyrics.is_none());
    assert!(!app.state.playback.lyrics_loading);
    assert_eq!(app.state.playback.lyrics_scroll, 0);
}

#[tokio::test]
async fn on_track_started_sets_radio_mode_from_flag() {
    let mut app = App::new_for_test().await;
    app.radio_mode = true;
    app.state.playback.radio_mode = false;

    app.on_track_started();

    assert!(app.state.playback.radio_mode);
}
