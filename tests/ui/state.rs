use super::*;

#[test]
fn new_sets_default_state() {
    let s = UiState::new();
    assert_eq!(s.focus, Focus::Library);
    assert!(!s.compact_mode);
    assert!(!s.compact_effective);
    assert!(!s.fullscreen_player);
    assert_eq!(s.library_list.selected(), Some(0));
    assert!(s.show_album_art);
    assert!(s.show_visualizer);
    assert!(!s.show_lyrics);
    assert!(!s.search_active);
    assert!(!s.quick_search_active);
}

#[test]
fn start_search_sets_active() {
    let mut s = UiState::new();
    s.start_search();
    assert!(s.search_active);
    assert!(s.search_query.is_empty());
}

#[test]
fn cancel_search_clears() {
    let mut s = UiState::new();
    s.start_search();
    s.search_query.push('x');
    s.cancel_search();
    assert!(!s.search_active);
    assert!(s.search_query.is_empty());
}

#[test]
fn search_push_pop() {
    let mut s = UiState::new();
    s.search_push('a');
    assert_eq!(s.search_query, "a");
    s.search_push('b');
    assert_eq!(s.search_query, "ab");
    s.search_pop();
    assert_eq!(s.search_query, "a");
}

#[test]
fn start_quick_search_sets_active() {
    let mut s = UiState::new();
    s.start_quick_search();
    assert!(s.quick_search_active);
}

#[test]
fn cancel_quick_search_clears() {
    let mut s = UiState::new();
    s.start_quick_search();
    s.quick_search_query.push('x');
    s.cancel_quick_search();
    assert!(!s.quick_search_active);
    assert!(s.quick_search_query.is_empty());
}

#[test]
fn nav_up_library_wraps() {
    let mut s = UiState::new();
    s.focus = Focus::Library;
    s.library_list.select(Some(0));
    s.nav_up();
    assert_eq!(s.library_list.selected(), Some(LIBRARY_ITEMS.len() - 1));
}

#[test]
fn nav_down_library_wraps() {
    let mut s = UiState::new();
    s.focus = Focus::Library;
    s.library_list.select(Some(LIBRARY_ITEMS.len() - 1));
    s.nav_down();
    assert_eq!(s.library_list.selected(), Some(0));
}

#[test]
fn nav_up_empty_playlist_noop() {
    let mut s = UiState::new();
    s.focus = Focus::Playlists;
    s.playlists.clear();
    s.nav_up();
}

#[test]
fn nav_first_selects_zero() {
    let mut s = UiState::new();
    s.focus = Focus::Library;
    s.library_list.select(Some(3));
    s.nav_first();
    assert_eq!(s.library_list.selected(), Some(0));
}

#[test]
fn nav_last_selects_last() {
    let mut s = UiState::new();
    s.focus = Focus::Library;
    s.nav_last();
    assert_eq!(s.library_list.selected(), Some(LIBRARY_ITEMS.len() - 1));
}

#[test]
fn switch_focus_cycles() {
    let mut s = UiState::new();
    s.focus = Focus::Library;
    s.switch_focus();
    assert_eq!(s.focus, Focus::Playlists);
    s.switch_focus();
    assert_eq!(s.focus, Focus::Tracks);
    s.switch_focus();
    assert_eq!(s.focus, Focus::Queue);
    s.switch_focus();
    assert_eq!(s.focus, Focus::Library);
}

#[test]
fn switch_focus_prev_reverses() {
    let mut s = UiState::new();
    s.focus = Focus::Library;
    s.switch_focus_prev();
    assert_eq!(s.focus, Focus::Queue);
    s.switch_focus_prev();
    assert_eq!(s.focus, Focus::Tracks);
    s.switch_focus_prev();
    assert_eq!(s.focus, Focus::Playlists);
    s.switch_focus_prev();
    assert_eq!(s.focus, Focus::Library);
}

#[test]
fn switch_focus_compact_always_tracks() {
    let mut s = UiState::new();
    s.compact_effective = true;
    s.focus = Focus::Library;
    s.switch_focus();
    assert_eq!(s.focus, Focus::Tracks);
    s.focus = Focus::Queue;
    s.switch_focus();
    assert_eq!(s.focus, Focus::Tracks);
}

#[test]
fn sort_tracks_cycles() {
    use TrackSortBy::*;
    assert_eq!(Default.next(), Title);
    assert_eq!(Title.next(), Artist);
    assert_eq!(Artist.next(), Album);
    assert_eq!(Album.next(), Duration);
    assert_eq!(Duration.next(), Default);
}

#[test]
fn sort_tracks_labels() {
    use TrackSortBy::*;
    assert_eq!(Default.label(), "Default");
    assert_eq!(Title.label(), "Title");
    assert_eq!(Artist.label(), "Artist");
    assert_eq!(Album.label(), "Album");
    assert_eq!(Duration.label(), "Duration");
}

#[test]
fn search_panel_next_cycles() {
    use SearchPanel::*;
    assert_eq!(Tracks.next(), Artists);
    assert_eq!(Artists.next(), Albums);
    assert_eq!(Albums.next(), Playlists);
    assert_eq!(Playlists.next(), Tracks);
}

#[test]
fn active_content_default_is_none() {
    assert_eq!(ActiveContent::default(), ActiveContent::None);
}

#[test]
fn track_sort_cycles_default() {
    let mut s = UiState::new();
    let start = s.track_sort_by;
    s.sort_tracks();
    assert_ne!(s.track_sort_by, start);
    assert_eq!(s.track_sort_by, TrackSortBy::Title);
}

#[test]
fn apply_quick_filter_empty_tracks_populates_all() {
    let mut s = UiState::new();
    s.active_content = ActiveContent::Tracks;
    s.apply_quick_filter();
    assert!(s.sorted_track_indices.is_empty());
}

#[test]
fn nav_first_playlist_empty_noop() {
    let mut s = UiState::new();
    s.focus = Focus::Playlists;
    s.playlists.clear();
    s.nav_first();
}

#[test]
fn nav_last_playlist_empty_noop() {
    let mut s = UiState::new();
    s.focus = Focus::Playlists;
    s.playlists.clear();
    s.nav_last();
}

#[test]
fn selected_track_index_returns_none_initially() {
    let s = UiState::new();
    assert!(s.selected_track_index().is_none());
}

#[test]
fn selected_album_index_returns_none_initially() {
    let s = UiState::new();
    assert!(s.selected_album_index().is_none());
}

#[test]
fn selected_artist_index_returns_none_initially() {
    let s = UiState::new();
    assert!(s.selected_artist_index().is_none());
}

#[test]
fn selected_show_index_returns_none_initially() {
    let s = UiState::new();
    assert!(s.selected_show_index().is_none());
}

#[test]
fn switch_search_panel_no_panels() {
    let mut s = UiState::new();
    s.switch_search_panel();
}

#[test]
fn nav_with_compact_mode_selectable() {
    let mut s = UiState::new();
    s.compact_effective = true;
    s.focus = Focus::Tracks;
    s.active_content = ActiveContent::None;
    s.library_list.select(Some(1));
    s.nav_down();
    let selectable = s.compact_selectable_positions();
    if !selectable.is_empty() {
        assert!(s.library_list.selected().is_some());
    }
}

#[test]
fn cancel_search_also_clears_active() {
    let mut s = UiState::new();
    s.start_search();
    assert!(s.search_active);
    s.cancel_search();
    assert!(!s.search_active);
}

#[test]
fn rebuild_sort_indices_no_tracks() {
    let mut s = UiState::new();
    s.active_content = ActiveContent::Tracks;
    s.tracks = vec![];
    s.rebuild_sort_indices();
    assert!(s.track_list.selected().is_none());
}
