use super::super::App;

#[tokio::test]
async fn maybe_fetch_album_art_returns_early_when_disabled() {
    let mut app = App::new_for_test().await;
    app.state.show_album_art = false;
    app.discord = None;
    app.current_track_uri = "spotify:track:abc".into();

    app.maybe_fetch_album_art().await;

    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn maybe_fetch_album_art_returns_early_with_empty_uri() {
    let mut app = App::new_for_test().await;
    app.state.show_album_art = true;
    app.current_track_uri.clear();

    app.maybe_fetch_album_art().await;

    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn maybe_fetch_album_art_returns_early_when_already_fetched() {
    let mut app = App::new_for_test().await;
    app.state.show_album_art = true;
    app.current_track_uri = "spotify:track:abc".into();
    app.last_art_uri = "spotify:track:abc".into();

    app.maybe_fetch_album_art().await;

    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn maybe_fetch_album_art_returns_early_when_pending() {
    let mut app = App::new_for_test().await;
    app.state.show_album_art = true;
    app.current_track_uri = "spotify:track:abc".into();
    let (_, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    app.album_art_pending = Some(rx);

    app.maybe_fetch_album_art().await;

    assert!(app.album_art_pending.is_some());
}

#[tokio::test]
async fn maybe_fetch_album_art_returns_early_for_file_uri_with_no_cover() {
    let mut app = App::new_for_test().await;
    app.state.show_album_art = true;
    app.current_track_uri = "file:///music/test.mp3".into();
    app.last_art_uri.clear();
    app.state.playback.cover_path = None;

    app.maybe_fetch_album_art().await;

    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn maybe_fetch_album_art_does_not_fetch_when_unauthenticated() {
    let mut app = App::new_for_test().await;
    app.state.show_album_art = true;
    app.current_track_uri = "spotify:track:abc".into();
    app.last_art_uri.clear();
    app.state.playback.cover_path = None;

    app.maybe_fetch_album_art().await;

    // With unauthenticated spotify, should return early after the authenticated check
    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn fetch_local_album_art_returns_early_when_already_fetched() {
    let mut app = App::new_for_test().await;
    app.current_track_uri = "file:///music/test.mp3".into();
    app.last_art_uri = "file:///music/test.mp3".into();

    app.fetch_local_album_art();

    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn fetch_local_album_art_returns_early_when_pending() {
    let mut app = App::new_for_test().await;
    app.current_track_uri = "file:///music/test.mp3".into();
    let (_, rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    app.album_art_pending = Some(rx);

    app.fetch_local_album_art();

    assert!(app.album_art_pending.is_some());
}

#[tokio::test]
async fn fetch_local_album_art_returns_early_when_cover_path_missing() {
    let mut app = App::new_for_test().await;
    app.current_track_uri = "file:///music/test.mp3".into();
    app.last_art_uri.clear();
    app.state.playback.cover_path = None;

    app.fetch_local_album_art();

    assert!(app.album_art_pending.is_none());
}

#[tokio::test]
async fn fetch_local_album_art_with_temp_file_sets_pending() {
    let dir = std::env::temp_dir().join("isi-music-test-art");
    let _ = std::fs::create_dir_all(&dir);
    let art_path = dir.join("cover.jpg");
    std::fs::write(&art_path, b"fake-image-bytes").unwrap();

    let mut app = App::new_for_test().await;
    app.current_track_uri = "file:///music/test.mp3".into();
    app.last_art_uri.clear();
    app.state.playback.cover_path = Some(art_path.to_string_lossy().to_string());

    app.fetch_local_album_art();

    assert!(app.album_art_pending.is_some());

    let _ = std::fs::remove_dir_all(&dir);
}
