use super::*;

#[test]
fn test_debug_overlay_creation() {
    let debug = DebugOverlay::default();
    assert!(!debug.inner.lock().unwrap().visible);
}

#[test]
fn test_logging() {
    let debug = DebugOverlay::default();
    debug.log(LogLevel::Info, "test message");
    debug.log(LogLevel::Api, "GET /api/track → 200");
}

#[test]
fn test_log_level_labels() {
    assert_eq!(LogLevel::Info.label(), "INFO ");
    assert_eq!(LogLevel::Warn.label(), "WARN ");
    assert_eq!(LogLevel::Error.label(), "ERR  ");
    assert_eq!(LogLevel::Api.label(), "API  ");
    assert_eq!(LogLevel::Audio.label(), "AUDIO");
}
