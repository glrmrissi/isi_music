use super::{PlayerManager, PlayerRestore};
use crate::player::{QueuedTrack, RepeatMode};

#[allow(clippy::duplicate_mod)]
#[path = "mock_player.rs"]
mod mock_player;
use mock_player::MockPlayer;

fn sample_restore() -> PlayerRestore {
    PlayerRestore {
        queue: vec!["spotify:track:a".into(), "spotify:track:b".into()],
        index: Some(1),
        user_queue: vec![QueuedTrack {
            uri: "spotify:track:uq".into(),
            name: "Queued".into(),
            artist: "Artist".into(),
            album: String::new(),
            duration_ms: 60_000,
            cover_path: None,
        }],
        volume: 33,
        shuffle: true,
        repeat: RepeatMode::Track,
        progress_ms: 12_000,
        is_playing: false,
    }
}

#[test]
fn restore_apply_rebuilds_queue_and_playback_state() {
    let restore = sample_restore();
    let mut mock = MockPlayer::with_queue(vec![]);

    restore.apply(&mut mock);

    assert_eq!(mock.queue, vec!["spotify:track:a", "spotify:track:b"]);
    assert_eq!(mock.current_index, Some(1));
    assert_eq!(mock.user_queue.len(), 1);
    assert_eq!(mock.user_queue[0].uri, "spotify:track:uq");
    assert_eq!(mock.volume, 33);
    assert!(mock.shuffle);
    assert!(matches!(mock.repeat, RepeatMode::Track));
    assert!(!mock.is_playing);
}

#[test]
fn snapshot_prefers_pending_restore_over_live_player() {
    let mut mgr = PlayerManager::new(50, String::new(), false, None);
    mgr.player = Some(Box::new(MockPlayer::with_queue(vec![])));
    let pending = sample_restore();
    mgr.pending_player_restore = Some(PlayerRestore {
        queue: pending.queue.clone(),
        ..pending
    });

    let snap = mgr.take_restore_snapshot(0).expect("snapshot should exist");
    assert_eq!(snap.queue, vec!["spotify:track:a", "spotify:track:b"]);
    assert!(mgr.pending_player_restore.is_none());
}

#[test]
fn snapshot_captures_live_player_state() {
    let mut mgr = PlayerManager::new(50, String::new(), false, None);
    let mut mock = MockPlayer::with_queue(vec![]);
    mock.queue = vec!["spotify:track:x".into()];
    mock.current_index = Some(0);
    mock.volume = 77;
    mgr.player = Some(Box::new(mock));

    let snap = mgr
        .take_restore_snapshot(5_000)
        .expect("snapshot should capture live player");
    assert_eq!(snap.queue, vec!["spotify:track:x"]);
    assert_eq!(snap.index, Some(0));
    assert_eq!(snap.volume, 77);
    assert_eq!(snap.progress_ms, 5_000);
}

#[test]
fn snapshot_is_none_without_player_or_pending() {
    let mut mgr = PlayerManager::new(50, String::new(), false, None);
    assert!(mgr.take_restore_snapshot(0).is_none());
}
