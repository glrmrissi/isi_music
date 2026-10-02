use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::super::App;
use crate::keybinds::Action;
use crate::player::{QueuedTrack, RepeatMode};
use crate::spotify::RepeatState;
use crate::spotify::TrackSummary;
use crate::ui::Focus;

#[allow(clippy::duplicate_mod)]
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
    assert_eq!(app.state.status_msg.as_deref(), Some("Visualizer enabled"));
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.config.ui.show_visualizer),
        Some(true)
    );

    app.dispatch(Action::ToggleVisualizer).await;
    assert!(!app.state.show_visualizer);
    assert_eq!(app.state.status_msg.as_deref(), Some("Visualizer disabled"));
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.config.ui.show_visualizer),
        Some(false)
    );
}

#[tokio::test]
async fn dispatch_toggle_lyrics_toggles_and_sets_status() {
    let mut app = App::new_for_test().await;
    app.state.show_lyrics = false;

    app.dispatch(Action::ToggleLyrics).await;
    assert!(app.state.show_lyrics);
    assert_eq!(app.state.status_msg.as_deref(), Some("Lyrics panel on"));
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.config.ui.show_lyrics),
        Some(true)
    );
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.settings.lock().ok())
            .and_then(|s| s.config.ui.show_lyrics),
        Some(true)
    );

    app.dispatch(Action::ToggleLyrics).await;
    assert!(!app.state.show_lyrics);
    assert_eq!(app.state.status_msg.as_deref(), Some("Lyrics panel off"));
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.config.ui.show_lyrics),
        Some(false)
    );
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.settings.lock().ok())
            .and_then(|s| s.config.ui.show_lyrics),
        Some(false)
    );
}

#[tokio::test]
async fn settings_lyrics_display_toggle_persists() {
    let mut app = App::new_for_test().await;
    app.state.show_lyrics = false;
    {
        let panel = app.settings_panel.as_mut().unwrap();
        panel.visible = true;
        panel.focused_section = crate::ui::options::SettingsSection::General;
        #[cfg(feature = "album-art")]
        {
            panel.selected_item = 2;
        }
        #[cfg(not(feature = "album-art"))]
        {
            panel.selected_item = 1;
        }
    }

    app.handle_key(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    )
    .await
    .expect("handle_key");

    assert!(app.state.show_lyrics);
    assert_eq!(app.state.status_msg.as_deref(), Some("Lyrics panel on"));
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.config.ui.show_lyrics),
        Some(true)
    );
    assert_eq!(
        app.settings_panel
            .as_ref()
            .and_then(|p| p.settings.lock().ok())
            .and_then(|s| s.config.ui.show_lyrics),
        Some(true)
    );
}

#[tokio::test]
async fn settings_general_renders_lyrics_display() {
    let mut app = App::new_for_test().await;
    let panel = app.settings_panel.as_mut().unwrap();
    panel.visible = true;
    panel.focused_section = crate::ui::options::SettingsSection::General;

    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).expect("test terminal");
    let theme = crate::utils::theme::Theme::default();
    terminal
        .draw(|frame| panel.render(frame, &app.state, &theme, true))
        .expect("render settings");

    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Lyrics Display"));
}

#[tokio::test]
async fn dispatch_toggle_lyrics_warns_when_fetching_disabled() {
    let mut app = App::new_for_test().await;
    app.state.show_lyrics = false;
    app.enable_lyrics = false;

    app.dispatch(Action::ToggleLyrics).await;
    assert!(app.state.show_lyrics);
    assert_eq!(
        app.state.status_msg.as_deref(),
        Some("Lyrics panel on (fetching disabled in Settings)")
    );
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
        crate::spotify::FullSearchResults::empty(),
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
    let was_visible = app.settings_panel.as_ref().unwrap().visible;

    app.dispatch(Action::OptionsPanel).await;
    assert_ne!(app.settings_panel.as_ref().unwrap().visible, was_visible);

    app.dispatch(Action::OptionsPanel).await;
    assert_eq!(app.settings_panel.as_ref().unwrap().visible, was_visible);
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
    app.player_mgr.player = Some(mock);
    app.state.playback.is_playing = true;

    app.dispatch(Action::PlayPause).await;

    assert!(!app.state.playback.is_playing);
    assert!(!app.player_mgr.player.as_ref().unwrap().is_playing());
}

#[tokio::test]
async fn dispatch_play_pause_resumes_paused_track() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player_mgr.player = Some(mock);
    app.state.playback.is_playing = false;

    app.dispatch(Action::PlayPause).await;

    assert!(app.state.playback.is_playing);
    assert!(app.player_mgr.player.as_ref().unwrap().is_playing());
}

#[tokio::test]
async fn dispatch_next_track_calls_player_next() {
    let mut app = App::new_for_test().await;
    let next_flag = Arc::new(AtomicBool::new(false));
    let mock = Box::new(MockPlayer::new(Arc::clone(&next_flag), Arc::default()));
    app.player_mgr.player = Some(mock);

    app.dispatch(Action::NextTrack).await;

    assert!(next_flag.load(Ordering::Relaxed));
}

#[tokio::test]
async fn dispatch_prev_track_calls_player_prev() {
    let mut app = App::new_for_test().await;
    let prev_flag = Arc::new(AtomicBool::new(false));
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::clone(&prev_flag)));
    app.player_mgr.player = Some(mock);

    app.dispatch(Action::PrevTrack).await;

    assert!(prev_flag.load(Ordering::Relaxed));
}

#[tokio::test]
async fn dispatch_next_track_without_player_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.player_mgr.player = None;

    app.dispatch(Action::NextTrack).await;
}

#[tokio::test]
async fn dispatch_volume_up_increases_volume() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.volume = 40;
    app.player_mgr.player = Some(mock);
    app.state.playback.volume = 40;

    app.dispatch(Action::VolumeUp).await;

    assert_eq!(app.state.playback.volume, 50);
}

#[tokio::test]
async fn dispatch_volume_down_decreases_volume() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.volume = 40;
    app.player_mgr.player = Some(mock);
    app.state.playback.volume = 40;

    app.dispatch(Action::VolumeDown).await;

    assert_eq!(app.state.playback.volume, 30);
}

#[tokio::test]
async fn dispatch_volume_no_player_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.player_mgr.player = None;

    app.dispatch(Action::VolumeUp).await;
}

#[tokio::test]
async fn dispatch_toggle_shuffle_toggles() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.shuffle = false;
    app.player_mgr.player = Some(mock);
    app.state.playback.shuffle = false;

    app.dispatch(Action::ToggleShuffle).await;

    assert!(app.state.playback.shuffle);
}

#[tokio::test]
async fn dispatch_cycle_repeat_cycles_through_modes() {
    let mut app = App::new_for_test().await;
    let mut mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    mock.repeat = RepeatMode::Off;
    app.player_mgr.player = Some(mock);

    app.dispatch(Action::CycleRepeat).await;
    assert_eq!(app.state.playback.repeat, RepeatState::Track);

    app.dispatch(Action::CycleRepeat).await;
    assert_eq!(app.state.playback.repeat, RepeatState::Context);

    app.dispatch(Action::CycleRepeat).await;
    assert_eq!(app.state.playback.repeat, RepeatState::Off);
}

#[tokio::test]
async fn dispatch_add_to_queue_appends_to_player_queue() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player_mgr.player = Some(mock);
    app.state.active_content = crate::ui::ActiveContent::Tracks;
    app.state.tracks = vec![TrackSummary {
        uri: "spotify:track:abc".into(),
        name: "Test Track".into(),
        artist: "Test Artist".into(),
        album: "Test Album".into(),
        duration_ms: 200_000,
        cover_path: None,
        added_at: None,
    }];
    app.state.track_list.select(Some(0));
    app.state.sorted_track_indices = vec![0];

    app.dispatch(Action::AddToQueue).await;

    let player = app.player_mgr.player.as_ref().unwrap();
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
        album: String::new(),
        duration_ms: 200_000,
        cover_path: None,
    });
    app.player_mgr.player = Some(mock);
    app.state.focus = Focus::Queue;
    app.state.queue_list.select(Some(0));
    app.sync_queue_display();

    app.dispatch(Action::RemoveFromQueue).await;

    let player = app.player_mgr.player.as_ref().unwrap();
    assert!(player.user_queue().is_empty());
}

#[tokio::test]
async fn dispatch_play_pause_without_player_does_not_panic() {
    let mut app = App::new_for_test().await;
    app.player_mgr.player = None;
    app.state.playback.is_playing = false;

    app.dispatch(Action::PlayPause).await;
}

#[tokio::test]
async fn dispatch_seek_forward_and_backward() {
    let mut app = App::new_for_test().await;
    app.state.playback.duration_ms = 100_000;
    app.state.playback.progress_ms = 20_000;

    app.dispatch(Action::SeekForward).await;
    assert_eq!(app.state.playback.progress_ms, 25_000);

    app.dispatch(Action::SeekBackward).await;
    assert_eq!(app.state.playback.progress_ms, 20_000);
}

#[tokio::test]
async fn liked_songs_claims_context_when_reentered_from_artist() {
    let mut app = App::new_for_test().await;
    if let Some(spotify) = Arc::get_mut(&mut app.spotify) {
        spotify.authenticated = true;
    }
    app.spotify_enabled = true;
    app.state.spotify_enabled = true;
    app.state.active_playlist_id = Some("artist:abc".to_string());
    app.state.active_playlist_uri = Some("spotify:artist:abc".to_string());

    app.handle_library_item(0).await;

    assert_eq!(
        app.state.active_playlist_id.as_deref(),
        Some("liked_songs"),
        "liked songs must claim active_playlist_id eagerly or streamed events are dropped as stale"
    );
    assert_eq!(
        app.state.active_playlist_uri.as_deref(),
        Some("liked_songs")
    );
}

#[tokio::test]
async fn spotify_search_submit_clears_stale_playlist_context() {
    let mut app = App::new_for_test().await;
    if let Some(spotify) = Arc::get_mut(&mut app.spotify) {
        spotify.authenticated = true;
    }
    app.spotify_enabled = true;
    app.state.spotify_enabled = true;
    app.state.active_playlist_id = Some("playlist_xyz".to_string());
    app.state.active_playlist_uri = Some("spotify:playlist:xyz".to_string());

    app.state.start_search();
    for c in "metallica".chars() {
        app.state.search_push(c);
    }
    app.handle_search_key(crossterm::event::KeyCode::Enter)
        .await
        .expect("search submit");

    assert!(
        app.state.active_playlist_id.is_none(),
        "search must clear stale playlist context or SearchInitial is dropped by the stale-id guard"
    );
    assert!(app.state.active_playlist_uri.is_none());
}

// ---------------------------------------------------------------------------
// Podcasts / shows
// ---------------------------------------------------------------------------

fn show_summary(id: &str, name: &str) -> crate::spotify::ShowSummary {
    crate::spotify::ShowSummary {
        id: id.into(),
        name: name.into(),
        publisher: "Pub".into(),
        total_episodes: 3,
    }
}

fn episode_track(id: &str, name: &str) -> TrackSummary {
    TrackSummary {
        uri: format!("spotify:episode:{id}"),
        name: name.into(),
        artist: "Show".into(),
        album: String::new(),
        duration_ms: 1_800_000,
        cover_path: None,
        added_at: None,
    }
}

#[tokio::test]
async fn library_item_podcasts_streams_saved_shows() {
    let mut app = App::new_for_test().await;
    if let Some(spotify) = Arc::get_mut(&mut app.spotify) {
        spotify.authenticated = true;
    }
    app.spotify_enabled = true;
    app.state.spotify_enabled = true;

    app.handle_library_item(3).await;

    assert!(
        app.fetcher.stream_rx.is_some(),
        "podcasts library item must spawn a saved-shows stream"
    );
    assert!(app.state.loading);
    assert_eq!(
        app.state.status_msg.as_deref(),
        Some("Loading saved shows…")
    );
    assert!(app.state.active_playlist_id.is_none());
}

#[tokio::test]
async fn shows_stream_events_populate_show_list() {
    let mut app = App::new_for_test().await;
    app.state.loading = true;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    tx.send(crate::app::fetcher::StreamEvent::ShowsInitial {
        shows: vec![show_summary("s1", "Pod One")],
        total: 2,
    })
    .unwrap();
    tx.send(crate::app::fetcher::StreamEvent::ShowsBatch {
        shows: vec![show_summary("s2", "Pod Two")],
        total: 2,
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    assert_eq!(app.state.shows.len(), 2);
    assert_eq!(app.state.shows_total, 2);
    assert_eq!(app.state.shows_offset, 2);
    assert_eq!(app.state.active_content, crate::ui::ActiveContent::Shows);
    assert!(!app.state.loading);
    assert_eq!(app.state.show_list.selected(), Some(0));
}

#[tokio::test]
async fn show_tracks_initial_populates_episodes() {
    let mut app = App::new_for_test().await;
    app.state.active_playlist_id = Some("show:s1".to_string());
    app.state.loading = true;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    tx.send(crate::app::fetcher::StreamEvent::ShowTracksInitial {
        show_id: "s1".into(),
        tracks: vec![episode_track("e1", "Episode 1")],
        total: 1,
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    assert_eq!(app.state.tracks.len(), 1);
    assert_eq!(app.state.tracks[0].uri, "spotify:episode:e1");
    assert_eq!(app.state.active_playlist_id.as_deref(), Some("show:s1"));
    assert_eq!(app.state.active_playlist_uri.as_deref(), Some("show:s1"));
    assert_eq!(app.state.active_content, crate::ui::ActiveContent::Tracks);
    assert!(!app.state.loading);
}

#[tokio::test]
async fn show_tracks_initial_dropped_for_stale_context() {
    let mut app = App::new_for_test().await;
    app.state.active_playlist_id = Some("show:other".to_string());
    app.state.loading = true;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    tx.send(crate::app::fetcher::StreamEvent::ShowTracksInitial {
        show_id: "s1".into(),
        tracks: vec![episode_track("e1", "Episode 1")],
        total: 1,
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    assert!(app.state.tracks.is_empty());
    assert_eq!(app.state.active_playlist_id.as_deref(), Some("show:other"));
}

#[tokio::test]
async fn enter_on_episode_track_queues_all_tracks() {
    let mut app = App::new_for_test().await;
    let mock = Box::new(MockPlayer::new(Arc::default(), Arc::default()));
    app.player_mgr.player = Some(mock);
    app.state.focus = Focus::Tracks;
    app.state.active_content = crate::ui::ActiveContent::Tracks;
    app.state.tracks = vec![
        TrackSummary {
            uri: "spotify:track:t1".into(),
            name: "Song".into(),
            artist: "A".into(),
            album: String::new(),
            duration_ms: 200_000,
            cover_path: None,
            added_at: None,
        },
        episode_track("e1", "Episode 1"),
        episode_track("e2", "Episode 2"),
    ];
    app.state.sorted_track_indices = vec![0, 1, 2];
    app.state.track_list.select(Some(1));

    app.dispatch(Action::Enter).await;

    let player = app.player_mgr.player.as_ref().unwrap();
    let (queue, idx) = player.snapshot_queue();
    assert_eq!(queue.len(), 3);
    assert_eq!(queue[1], "spotify:episode:e1");
    assert_eq!(idx, Some(1));
    assert_eq!(app.state.playback.title, "Episode 1");
    assert_eq!(app.state.playback.artist, "Show");
}

// ---------------------------------------------------------------------------
// Saved episodes (Your Episodes) + podcast search panel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn library_item_your_episodes_streams_saved_episodes() {
    let mut app = App::new_for_test().await;
    if let Some(spotify) = Arc::get_mut(&mut app.spotify) {
        spotify.authenticated = true;
    }
    app.spotify_enabled = true;
    app.state.spotify_enabled = true;

    app.handle_library_item(4).await;

    assert!(
        app.fetcher.stream_rx.is_some(),
        "your episodes library item must spawn a saved-episodes stream"
    );
    assert!(app.state.loading);
    assert_eq!(
        app.state.status_msg.as_deref(),
        Some("Loading saved episodes…")
    );
    assert_eq!(
        app.state.active_playlist_id.as_deref(),
        Some("saved_episodes")
    );
}

#[tokio::test]
async fn episodes_stream_events_populate_tracks() {
    let mut app = App::new_for_test().await;
    app.state.active_playlist_id = Some("saved_episodes".to_string());
    app.state.loading = true;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    tx.send(crate::app::fetcher::StreamEvent::EpisodesInitial {
        tracks: vec![episode_track("e1", "Episode 1")],
        total: 2,
    })
    .unwrap();
    tx.send(crate::app::fetcher::StreamEvent::EpisodesBatch {
        tracks: vec![
            episode_track("e1", "Episode 1"),
            episode_track("e2", "Episode 2"),
        ],
        total: 2,
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    assert_eq!(app.state.tracks.len(), 2, "batch must dedupe e1");
    assert_eq!(app.state.tracks[1].uri, "spotify:episode:e2");
    assert_eq!(
        app.state.active_playlist_id.as_deref(),
        Some("saved_episodes")
    );
    assert_eq!(app.state.active_content, crate::ui::ActiveContent::Tracks);
    assert!(!app.state.loading);
}

#[tokio::test]
async fn episodes_initial_dropped_for_stale_context() {
    let mut app = App::new_for_test().await;
    app.state.active_playlist_id = Some("show:s1".to_string());
    app.state.loading = true;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    tx.send(crate::app::fetcher::StreamEvent::EpisodesInitial {
        tracks: vec![episode_track("e1", "Episode 1")],
        total: 1,
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    assert!(app.state.tracks.is_empty());
    assert_eq!(app.state.active_playlist_id.as_deref(), Some("show:s1"));
}

#[tokio::test]
async fn search_results_selected_podcast_splits_shows_and_episodes() {
    use crate::ui::{PodcastSelection, SearchPanel, SearchResults};

    let results = crate::spotify::FullSearchResults {
        shows: vec![show_summary("s1", "Pod One"), show_summary("s2", "Pod Two")],
        episodes: vec![episode_track("e1", "Episode 1")],
        shows_total: 2,
        episodes_total: 1,
        ..crate::spotify::FullSearchResults::empty()
    };
    let mut sr = SearchResults::new("q".to_string(), results);
    sr.panel = SearchPanel::Podcasts;

    assert_eq!(sr.current_len(), 3);

    sr.podcast_list.select(Some(0));
    match sr.selected_podcast() {
        Some(PodcastSelection::Show(s)) => assert_eq!(s.id, "s1"),
        _ => panic!("index 0 must select a show"),
    }

    sr.podcast_list.select(Some(2));
    match sr.selected_podcast() {
        Some(PodcastSelection::Episode(t)) => assert_eq!(t.uri, "spotify:episode:e1"),
        _ => panic!("index 2 must select an episode"),
    }
}

#[tokio::test]
async fn search_panel_cycle_includes_podcasts() {
    use crate::ui::SearchPanel;

    assert_eq!(SearchPanel::Playlists.next(), SearchPanel::Podcasts);
    assert_eq!(SearchPanel::Podcasts.next(), SearchPanel::Tracks);
    assert_eq!(SearchPanel::Tracks.prev(), SearchPanel::Podcasts);
    assert_eq!(SearchPanel::Podcasts.prev(), SearchPanel::Playlists);
}

#[tokio::test]
async fn search_more_show_episode_merges_and_dedupes() {
    use crate::ui::{SearchPanel, SearchResults};

    let mut app = App::new_for_test().await;
    let initial = crate::spotify::FullSearchResults {
        shows: vec![show_summary("s1", "Pod One")],
        episodes: vec![episode_track("e1", "Episode 1")],
        shows_total: 3,
        episodes_total: 2,
        ..crate::spotify::FullSearchResults::empty()
    };
    let mut sr = SearchResults::new("q".to_string(), initial);
    sr.panel = SearchPanel::Podcasts;
    app.state.search_results = Some(sr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    let page = crate::spotify::FullSearchResults {
        shows: vec![show_summary("s1", "Pod One"), show_summary("s2", "Pod Two")],
        episodes: vec![episode_track("e2", "Episode 2")],
        shows_total: 3,
        episodes_total: 2,
        ..crate::spotify::FullSearchResults::empty()
    };
    tx.send(crate::app::fetcher::StreamEvent::SearchMore {
        stype: "show,episode".to_string(),
        results: Box::new(page),
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    let sr = app.state.search_results.as_ref().unwrap();
    assert_eq!(sr.shows.len(), 2, "s1 duplicate must be dropped");
    assert_eq!(sr.shows[1].id, "s2");
    assert_eq!(sr.episodes.len(), 2);
    assert_eq!(sr.episodes[1].uri, "spotify:episode:e2");
    assert_eq!(sr.shows_total, 3);
    assert_eq!(sr.episodes_total, 2);
    assert_eq!(sr.podcasts_api_offset, 3);
}

#[tokio::test]
async fn search_more_empty_page_marks_podcasts_done() {
    use crate::ui::{SearchPanel, SearchResults};

    let mut app = App::new_for_test().await;
    let initial = crate::spotify::FullSearchResults {
        shows: vec![show_summary("s1", "Pod One"), show_summary("s2", "Pod Two")],
        episodes: vec![episode_track("e1", "Episode 1")],
        shows_total: 2,
        episodes_total: 1,
        ..crate::spotify::FullSearchResults::empty()
    };
    let mut sr = SearchResults::new("q".to_string(), initial);
    sr.panel = SearchPanel::Podcasts;
    app.state.search_results = Some(sr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    app.fetcher.stream_rx = Some(rx);

    let empty_page = crate::spotify::FullSearchResults {
        shows_total: 2,
        episodes_total: 1,
        ..crate::spotify::FullSearchResults::empty()
    };
    tx.send(crate::app::fetcher::StreamEvent::SearchMore {
        stype: "show,episode".to_string(),
        results: Box::new(empty_page),
    })
    .unwrap();

    app.fetcher.poll_pending_fetch(&mut app.state, &app.spotify);

    let sr = app.state.search_results.as_ref().unwrap();
    assert_eq!(
        sr.podcasts_api_offset,
        sr.shows_total + sr.episodes_total,
        "empty page must snap the shared offset to the combined total"
    );
}

#[tokio::test]
async fn maybe_load_more_podcasts_does_not_fetch_when_all_loaded() {
    use crate::ui::{SearchPanel, SearchResults};

    let mut app = App::new_for_test().await;
    let initial = crate::spotify::FullSearchResults {
        shows: vec![show_summary("s1", "Pod One"), show_summary("s2", "Pod Two")],
        episodes: vec![episode_track("e1", "Episode 1")],
        shows_total: 2,
        episodes_total: 1,
        ..crate::spotify::FullSearchResults::empty()
    };
    let mut sr = SearchResults::new("q".to_string(), initial);
    sr.panel = SearchPanel::Podcasts;
    sr.podcast_list.select(Some(2));
    app.state.search_results = Some(sr);
    app.state.focus = Focus::Search;

    app.maybe_load_more().await;

    assert!(
        app.fetcher.stream_rx.is_none(),
        "all podcast items loaded: must not fire another search_more"
    );
}

#[tokio::test]
async fn maybe_load_more_podcasts_fetches_when_items_remain() {
    use crate::ui::{SearchPanel, SearchResults};

    let mut app = App::new_for_test().await;
    if let Some(spotify) = Arc::get_mut(&mut app.spotify) {
        spotify.authenticated = true;
    }
    app.spotify_enabled = true;
    app.state.spotify_enabled = true;
    let initial = crate::spotify::FullSearchResults {
        shows: vec![show_summary("s1", "Pod One")],
        episodes: vec![episode_track("e1", "Episode 1")],
        shows_total: 10,
        episodes_total: 5,
        ..crate::spotify::FullSearchResults::empty()
    };
    let mut sr = SearchResults::new("q".to_string(), initial);
    sr.panel = SearchPanel::Podcasts;
    sr.podcast_list.select(Some(1));
    app.state.search_results = Some(sr);
    app.state.focus = Focus::Search;

    app.maybe_load_more().await;

    assert!(
        app.fetcher.stream_rx.is_some(),
        "items remaining below total: scroll must trigger pagination"
    );
    app.fetcher.stream_rx = None;
}

// ---------------------------------------------------------------------------
// Playlist items containing episodes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn playlist_item_parser_keeps_episodes() {
    let episode_wrapper = serde_json::json!({
        "added_at": "2024-05-01T00:00:00Z",
        "item": {
            "type": "episode",
            "name": "Ep 42",
            "uri": "spotify:episode:ep42",
            "duration_ms": 1_800_000,
            "show": { "name": "Cool Podcast" }
        }
    });
    let track = crate::spotify::playlist_item_to_track(&episode_wrapper)
        .expect("episode must parse into a track row");
    assert_eq!(track.uri, "spotify:episode:ep42");
    assert_eq!(track.name, "Ep 42");
    assert_eq!(track.artist, "Cool Podcast");
    assert_eq!(track.album, "Cool Podcast");
    assert_eq!(track.added_at.as_deref(), Some("2024-05-01T00:00:00Z"));

    let track_wrapper = serde_json::json!({
        "added_at": "2024-05-01T00:00:00Z",
        "item": {
            "type": "track",
            "name": "Song",
            "uri": "spotify:track:t1",
            "duration_ms": 200_000,
            "artists": [{ "name": "A" }, { "name": "B" }],
            "album": { "name": "Album" }
        }
    });
    let track = crate::spotify::playlist_item_to_track(&track_wrapper)
        .expect("regular track must still parse");
    assert_eq!(track.uri, "spotify:track:t1");
    assert_eq!(track.artist, "A, B");
    assert_eq!(track.album, "Album");

    let null_wrapper = serde_json::json!({ "added_at": null, "item": null, "track": null });
    assert!(crate::spotify::playlist_item_to_track(&null_wrapper).is_none());

    let legacy_wrapper = serde_json::json!({
        "track": {
            "type": "track",
            "name": "Legacy",
            "uri": "spotify:track:legacy",
            "duration_ms": 1
        }
    });
    let track = crate::spotify::playlist_item_to_track(&legacy_wrapper)
        .expect("legacy track field must still parse");
    assert_eq!(track.name, "Legacy");
}

#[tokio::test]
async fn on_track_started_skips_lyrics_fetch_for_episodes() {
    let mut app = App::new_for_test().await;
    app.enable_lyrics = true;
    app.current_track_uri = "spotify:episode:ep1".to_string();
    app.state.playback.title = "Episode 1".to_string();
    app.state.playback.artist = "Podcast".to_string();

    app.on_track_started();

    assert!(!app.state.playback.lyrics_loading);
    assert!(app.fetcher.lyrics.is_none());
}

#[tokio::test]
async fn report_remote_play_error_translates_no_active_device() {
    let mut app = App::new_for_test().await;
    let e = anyhow::anyhow!("API error: 404 no_active_device");
    assert!(!app.report_remote_play_error(&e));
    assert_eq!(
        app.state.status_msg.as_deref(),
        Some(
            "No active Spotify device — streaming player unavailable or start Spotify on a device"
        )
    );
}

#[tokio::test]
async fn report_remote_play_error_flags_401_for_reconnect() {
    let mut app = App::new_for_test().await;
    let e = anyhow::anyhow!("SPOTIFY_UNAUTHORIZED");
    assert!(app.report_remote_play_error(&e));
    assert_eq!(
        app.state.status_msg.as_deref(),
        Some("Authorization expired, reconnecting...")
    );
}

#[tokio::test]
async fn report_remote_play_error_shows_generic_message() {
    let mut app = App::new_for_test().await;
    let e = anyhow::anyhow!("some other failure");
    assert!(!app.report_remote_play_error(&e));
    assert_eq!(
        app.state.status_msg.as_deref(),
        Some("Error: some other failure")
    );
}
