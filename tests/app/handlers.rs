use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::super::App;
use crate::keybinds::Action;
use crate::player::{QueuedTrack, RepeatMode};
use crate::spotify::TrackSummary;
use crate::ui::Focus;

#[path = "mock_player.rs"]
mod mock_player;
use mock_player::MockPlayer;

// ---------------------------------------------------------------------------
// Pure action tests (no player, no spotify)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_toggle_radio_toggles_on_and_off() {
    let mut app = App::new_for_test().await;
    assert!(!app.state.playback.radio_mode);

    app.dispatch(Action::ToggleRadio).await;
    assert!(app.state.playback.radio_mode);
    assert_eq!(app.state.status_msg.as_deref(), Some("Radio mode on"));

    app.dispatch(Action::ToggleRadio).await;
    assert!(!app.state.playback.radio_mode);
    assert_eq!(app.state.status_msg.as_deref(), Some("Radio mode off"));
}

#[tokio::test]
async fn dispatch_toggle_compact_toggles_mode() {
    let mut app = App::new_for_test().await;
    assert!(!app.state.compact_mode);

    app.dispatch(Action::ToggleCompact).await;
    assert!(app.state.compact_mode);

    app.dispatch(Action::ToggleCompact).await;
    assert!(!app.state.compact_mode);
}

#[tokio::test]
async fn dispatch_toggle_compact_when_in_library_focus_switches_to_tracks() {
    let mut app = App::new_for_test().await;
    app.state.compact_mode = false;
    app.state.focus = Focus::Library;

    app.dispatch(Action::ToggleCompact).await;
    assert!(app.state.compact_mode);
    assert_eq!(app.state.focus, Focus::Tracks);
}

#[tokio::test]
async fn dispatch_toggle_compact_keeps_search_focus() {
    let mut app = App::new_for_test().await;
    app.state.compact_mode = false;
    app.state.focus = Focus::Search;

    app.dispatch(Action::ToggleCompact).await;
    assert!(app.state.compact_mode);
    assert_eq!(app.state.focus, Focus::Search);
}

#[tokio::test]
async fn dispatch_toggle_fullscreen_no_title_is_noop() {
    let mut app = App::new_for_test().await;
    app.state.playback.title.clear();
    app.state.fullscreen_player = false;

    app.dispatch(Action::ToggleFullscreen).await;
    assert!(!app.state.fullscreen_player);
}

#[tokio::test]
async fn dispatch_toggle_fullscreen_with_title_toggles() {
    let mut app = App::new_for_test().await;
    app.state.playback.title = "Test Song".to_string();
    app.state.fullscreen_player = false;

    app.dispatch(Action::ToggleFullscreen).await;
    assert!(app.state.fullscreen_player);

    app.dispatch(Action::ToggleFullscreen).await;
    assert!(!app.state.fullscreen_player);
}

#[tokio::test]
async fn dispatch_toggle_visualizer_toggles() {
    let mut app = App::new_for_test().await;
    app.state.show_visualizer = false;

    app.dispatch(Action::ToggleVisualizer).await;
    assert!(app.state.show_visualizer);

    app.dispatch(Action::ToggleVisualizer).await;
    assert!(!app.state.show_visualizer);
}

#[tokio::test]
async fn dispatch_toggle_lyrics_toggles_and_sets_status() {
    let mut app = App::new_for_test().await;
    app.state.show_lyrics = false;

    app.dispatch(Action::ToggleLyrics).await;
    assert!(app.state.show_lyrics);
    assert_eq!(app.state.status_msg.as_deref(), Some("Lyrics panel on"));

    app.dispatch(Action::ToggleLyrics).await;
    assert!(!app.state.show_lyrics);
    assert_eq!(app.state.status_msg.as_deref(), Some("Lyrics panel off"));
}

#[tokio::test]
async fn dispatch_quit_sets_flag() {
    let mut app = App::new_for_test().await;
    assert!(!app.should_quit);

    app.dispatch(Action::Quit).await;
    assert!(app.should_quit);
}

#[tokio::test]
async fn dispatch_search_starts_search() {
    let mut app = App::new_for_test().await;
    app.state.search_active = false;

    app.dispatch(Action::Search).await;
    assert!(app.state.search_active);
}

#[tokio::test]
async fn dispatch_back_exits_fullscreen() {
    let mut app = App::new_for_test().await;
    app.state.fullscreen_player = true;

    app.dispatch(Action::Back).await;
    assert!(!app.state.fullscreen_player);
}

#[tokio::test]
async fn dispatch_back_clears_search_results() {
    let mut app = App::new_for_test().await;
    app.state.search_results = Some(crate::ui::SearchResults::new(
        "test".into(),
        crate::spotify::FullSearchResults {
            tracks: Vec::new(),
            artists: Vec::new(),
            albums: Vec::new(),
            playlists: Vec::new(),
            tracks_total: 0,
            artists_total: 0,
            albums_total: 0,
            playlists_total: 0,
        },
    ));

    app.dispatch(Action::Back).await;
    assert!(app.state.search_results.is_none());
}

#[tokio::test]
async fn dispatch_toggle_debug_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.dispatch(Action::ToggleDebug).await;
    app.dispatch(Action::ToggleDebug).await;
}

#[tokio::test]
async fn dispatch_sort_tracks_no_crash() {
    let mut app = App::new_for_test().await;
    app.state.active_content = crate::ui::ActiveContent::Tracks;
    app.dispatch(Action::SortTracks).await;
}

#[tokio::test]
async fn dispatch_options_panel_toggles() {
    let mut app = App::new_for_test().await;
    let was_visible = app.options_panel.as_ref().unwrap().visible;

    app.dispatch(Action::OptionsPanel).await;
    assert_ne!(app.options_panel.as_ref().unwrap().visible, was_visible);

    app.dispatch(Action::OptionsPanel).await;
    assert_eq!(app.options_panel.as_ref().unwrap().visible, was_visible);
}

#[tokio::test]
async fn dispatch_scroll_unsynced_lyrics() {
    let mut app = App::new_for_test().await;
    app.state.fullscreen_player = true;
    app.state.playback.lyrics = Some(crate::utils::lyrics::LyricsData {
        is_synced: false,
        lines: vec![
            crate::utils::lyrics::LyricLine {
                time_ms: 0,
                text: "line 1".into(),
            },
            crate::utils::lyrics::LyricLine {
                time_ms: 1000,
                text: "line 2".into(),
            },
        ],
    });

    app.dispatch(Action::ScrollDown).await;
    assert_eq!(app.state.playback.lyrics_scroll, 4);

    app.dispatch(Action::ScrollUp).await;
    assert_eq!(app.state.playback.lyrics_scroll, 0);
}

#[tokio::test]
async fn dispatch_scroll_synced_lyrics_is_noop() {
    let mut app = App::new_for_test().await;
    app.state.fullscreen_player = true;
    app.state.playback.lyrics = Some(crate::utils::lyrics::LyricsData {
        is_synced: true,
        lines: vec![
            crate::utils::lyrics::LyricLine {
                time_ms: 0,
                text: "line 1".into(),
            },
            crate::utils::lyrics::LyricLine {
                time_ms: 1000,
                text: "line 2".into(),
            },
        ],
    });

    app.dispatch(Action::ScrollDown).await;
    assert_eq!(app.state.playback.lyrics_scroll, 0);
}

// ---------------------------------------------------------------------------
// Player action tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_play_pause_pauses_playing_track() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.is_playing = true;
    app.player = Some(mock);
    app.state.playback.is_playing = true;

    app.dispatch(Action::PlayPause).await;

    assert!(!app.state.playback.is_playing);
    assert!(!app.player.as_ref().unwrap().is_playing());
}

#[tokio::test]
async fn dispatch_play_pause_resumes_paused_track() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player = Some(mock);
    app.state.playback.is_playing = false;

    app.dispatch(Action::PlayPause).await;

    assert!(app.state.playback.is_playing);
    assert!(app.player.as_ref().unwrap().is_playing());
}

#[tokio::test]
async fn dispatch_next_track_calls_player_next() {
    let mut app = App::new_for_test().await;
    let next_flag = Arc::new(AtomicBool::new(false));
    let mock = Box::new(MockPlayer::new(Arc::clone(&next_flag), Arc::default()));
    app.player = Some(mock);

    app.dispatch(Action::NextTrack).await;

    assert!(next_flag.load(Ordering::Relaxed));
}

#[tokio::test]
async fn dispatch_prev_track_calls_player_prev() {
    let mut app = App::new_for_test().await;
    let prev_flag = Arc::new(AtomicBool::new(false));
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::clone(&prev_flag)));
    app.player = Some(mock);

    app.dispatch(Action::PrevTrack).await;

    assert!(prev_flag.load(Ordering::Relaxed));
}

#[tokio::test]
async fn dispatch_next_track_without_player_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.player = None;

    app.dispatch(Action::NextTrack).await;
}

#[tokio::test]
async fn dispatch_volume_up_increases_volume() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.volume = 40;
    app.player = Some(mock);
    app.state.playback.volume = 40;

    app.dispatch(Action::VolumeUp).await;

    assert_eq!(app.state.playback.volume, 50);
}

#[tokio::test]
async fn dispatch_volume_down_decreases_volume() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.volume = 40;
    app.player = Some(mock);
    app.state.playback.volume = 40;

    app.dispatch(Action::VolumeDown).await;

    assert_eq!(app.state.playback.volume, 30);
}

#[tokio::test]
async fn dispatch_volume_no_player_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.player = None;

    app.dispatch(Action::VolumeUp).await;
}

#[tokio::test]
async fn dispatch_toggle_shuffle_toggles() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.shuffle = false;
    app.player = Some(mock);
    app.state.playback.shuffle = false;

    app.dispatch(Action::ToggleShuffle).await;

    assert!(app.state.playback.shuffle);
}

#[tokio::test]
async fn dispatch_cycle_repeat_cycles_through_modes() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.repeat = RepeatMode::Off;
    app.player = Some(mock);

    app.dispatch(Action::CycleRepeat).await;
    assert_eq!(
        app.state.playback.repeat,
        rspotify::model::RepeatState::Track
    );

    app.dispatch(Action::CycleRepeat).await;
    assert_eq!(
        app.state.playback.repeat,
        rspotify::model::RepeatState::Context
    );

    app.dispatch(Action::CycleRepeat).await;
    assert_eq!(app.state.playback.repeat, rspotify::model::RepeatState::Off);
}

#[tokio::test]
async fn dispatch_add_to_queue_appends_to_player_queue() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player = Some(mock);
    app.state.active_content = crate::ui::ActiveContent::Tracks;
    app.state.tracks = vec![TrackSummary {
        uri: "spotify:track:abc".into(),
        name: "Test Track".into(),
        artist: "Test Artist".into(),
        album: "Test Album".into(),
        duration_ms: 200_000,
        cover_path: None,
    }];
    app.state.track_list.select(Some(0));
    app.state.sorted_track_indices = vec![0];

    app.dispatch(Action::AddToQueue).await;

    let player = app.player.as_ref().unwrap();
    assert_eq!(player.user_queue().len(), 1);
    assert_eq!(player.user_queue()[0].name, "Test Track");
}

#[tokio::test]
async fn dispatch_remove_from_queue_removes_item() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.user_queue.push(QueuedTrack {
        uri: "spotify:track:abc".into(),
        name: "Track 1".into(),
        artist: "Artist".into(),
        duration_ms: 200_000,
        cover_path: None,
    });
    app.player = Some(mock);
    app.state.focus = Focus::Queue;
    app.state.queue_list.select(Some(0));
    app.sync_queue_display();

    app.dispatch(Action::RemoveFromQueue).await;

    let player = app.player.as_ref().unwrap();
    assert!(player.user_queue().is_empty());
}

#[tokio::test]
async fn dispatch_play_pause_without_player_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.player = None;
    app.state.playback.is_playing = false;

    app.dispatch(Action::PlayPause).await;
}
