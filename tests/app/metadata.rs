use std::time::{Duration, SystemTime};

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
