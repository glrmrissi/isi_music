# Progress Tracking & Lyrics Sync Architecture

## Three Progress Sources

### 1. NativePlayer (Spotify/librespot)
- **Source of truth:** librespot `PlayerEvent::Playing{position_ms}` + `PositionChanged{position_ms}` (every 250ms)
- **Stored in:** `NativePlayer.server_position: Arc<Mutex<(u64, Instant)>>`
- **Updated by events:** `Playing`, `PositionChanged`, `PositionCorrection`, `Seeked`
- **Interpolation:** `current_playback_state()` returns `base + elapsed_since_last_event`
- **Configured in:** `PlayerConfig { position_update_interval: Some(250ms) }` (`src/player/mod.rs`)
- **Synced in app:** non-local branch copies `pb.progress_ms` every tick and resets `playing_started_at`

### 2. LocalPlayer (rodio)
- **Source of truth:** Wall-clock via `Instant::elapsed()` auto-detected in progress update
- **Trigger:** When `is_playing` becomes true and `playing_started_at` is `None`
- **Accuracy:** ~50ms error (audio pipeline latency), acceptable for local files with no network buffering
- **Key:** `on_track_started()` does NOT start the timer — it only starts when the first tick detects `is_playing=true`

### 3. Web API (no player, Spotify device control)
- **Source of truth:** `fetch_playback()` server response
- **Synced in:** `needs_sync` path sets `playing_started_at` and `progress_at_play_start` from server data

## App-Level Tracking (`src/app.rs`)

### Fields
- `playing_started_at: Option<Instant>` — when current playback period started
- `progress_at_play_start: u64` — progress_ms at that moment

### Logic (runs every tick, lines 754-774)
```
if is_playing:
    if playing_started_at is None:  # first tick or resume
        start timer now, save current progress_ms as base
    progress_ms = progress_at_play_start + started_at.elapsed()
    if progress >= duration && no player:  # end-of-track for local-only
        stop, freeze

elif playing_started_at is Some:  # just paused
    freeze: add elapsed to progress_at_play_start, clear started_at
```

### Resets
- **Track start** (`on_track_started`): `playing_started_at = None`, `progress_at_play_start = 0`
- **NativePlayer sync** (each tick): resets from librespot position
- **Playing notification** (first audio): resets if `playing_started_at` is None
- **Seek** (handler + seek_rx + MPRIS): `progress_at_play_start = target`, `playing_started_at = now`
- **Pause freeze**: accumulated elapsed saved to `progress_at_play_start`, `playing_started_at = None`

## Key Files

| File | What |
|------|------|
| `src/player/mod.rs` | NativePlayer: `server_position`, event handlers (Playing/PositionChanged), `current_playback_state()` with interpolation |
| `src/app.rs` | App struct fields, progress update logic, non-local sync, seek reset, Playing notification handler |
| `src/app/player.rs` | `on_track_started()` — clears tracking, doesn't start timer |
| `src/app/handlers.rs` | Seek handler — resets `progress_at_play_start` and `playing_started_at` |

## History

### 2026-06-08: Fix lyrics drift + sync issues
- **Problem 1:** Lyrics drifted behind over time. Root cause: `delta_ms = now - last_tick` truncated to ms each frame, losing ~0.33ms/frame. Over 3min at 60fps: ~1.8s accumulated error.
- **Fix 1:** Replaced `progress_ms += delta_ms` with Instant-based: `progress_ms = progress_at_play_start + started_at.elapsed()`. No cumulative truncation (max 1ms error per frame, resets each frame).
- **Problem 2:** Lyrics were ahead of audio at first. Root cause: Instant-based tracking started when `PlayerEvent::Playing` fired, but actual audio was delayed by librespot's buffer + PulseAudio.
- **Fix 2:** Added `server_position` to NativePlayer using librespot's own `position_ms` from `Playing`/`PositionChanged` events. Progress now reflects actual decoded position, not wall-clock.
