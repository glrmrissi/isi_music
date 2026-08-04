//! Windows System Media Transport Controls (SMTC) integration.
//!
//! Provides the media overlay (volume OSD, lock screen, action centre) with
//! play/pause/next/previous buttons and track metadata.
//!
//! A dedicated STA thread owns the MediaPlayer / SMTC COM objects and runs a
//! Windows message pump so that overlay button events are delivered.  The
//! application talks to that thread through two mpsc channels:
//!
//! - `state_tx` sends `SmtcState` updates.
//! - `cmd_rx` receives `SmtcCmd` actions from the overlay.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use windows::core::{Result as WinResult, HSTRING};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::Playback::{MediaPlaybackList, MediaPlayer};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::StorageFile;
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    QS_ALLINPUT,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtcCmd {
    Play,
    Pause,
    Next,
    Previous,
    Seek(u64), // milliseconds
}

#[derive(Debug, Clone, Default)]
pub struct SmtcState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub cover_path: Option<String>,
    pub duration_ms: u64,
    pub position_ms: u64,
    pub is_playing: bool,
}

/// Application-facing handle.  All COM objects stay on the SMTC worker thread.
pub struct SmtcHandle {
    pub state_tx: mpsc::Sender<SmtcState>,
    pub cmd_rx: mpsc::Receiver<SmtcCmd>,
    stop_tx: mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
}

impl SmtcHandle {
    pub fn update(&self, state: &SmtcState) {
        let _ = self.state_tx.send(state.clone());
    }
}

impl Drop for SmtcHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn hstr(s: &str) -> HSTRING {
    HSTRING::from(s)
}

fn ms_to_timespan(ms: u64) -> TimeSpan {
    TimeSpan {
        Duration: (ms as i64).saturating_mul(10_000),
    }
}

fn md5_hex(bytes: &[u8]) -> String {
    let digest = md5::compute(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn guess_extension(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.len() >= 12
        && bytes.starts_with(b"RIFF")
        && bytes[8..12].eq_ignore_ascii_case(b"WEBP")
    {
        "webp"
    } else {
        "jpg"
    }
}

fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

/// Downloads a remote cover and stores it in a temp file named by the bytes'
/// MD5 hash.  Because the filename is hash-based, the same cover is written
/// once and reused without comparing bytes on every update.
fn cover_temp_path_from_url(url: &str) -> Option<PathBuf> {
    let bytes = http_client().get(url).send().ok()?.bytes().ok()?.to_vec();
    if bytes.is_empty() {
        return None;
    }
    let hash = md5_hex(&bytes);
    let ext = guess_extension(&bytes);
    let path = std::env::temp_dir().join(format!("isi-music-cover-{hash}.{ext}"));
    if !path.exists() {
        let _ = std::fs::write(&path, &bytes);
    }
    Some(path)
}

fn cover_source_key(art_url: Option<&str>, cover_path: Option<&str>) -> String {
    match (art_url, cover_path) {
        (Some(url), _) => url.to_string(),
        (_, Some(path)) => path.to_string(),
        (None, None) => String::new(),
    }
}

fn resolve_cover_path(
    art_url: Option<&str>,
    cover_path: Option<&str>,
    cache: &mut HashMap<String, PathBuf>,
) -> Option<PathBuf> {
    let source_key = cover_source_key(art_url, cover_path);
    if source_key.is_empty() {
        return None;
    }
    if let Some(path) = cache.get(&source_key) {
        return Some(path.clone());
    }

    let resolved = if let Some(path) = cover_path {
        Some(PathBuf::from(path))
    } else if let Some(url) = art_url {
        if url.starts_with("file:///") {
            Some(PathBuf::from(&url[8..]))
        } else if url.starts_with("http://") || url.starts_with("https://") {
            cover_temp_path_from_url(url)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(ref p) = resolved {
        cache.insert(source_key, p.clone());
    }
    resolved
}

fn set_thumbnail_from_file(
    updater: &windows::Media::SystemMediaTransportControlsDisplayUpdater,
    path: Option<&std::path::Path>,
) -> WinResult<()> {
    if let Some(path) = path {
        let h = HSTRING::from(path.as_os_str());
        let file = StorageFile::GetFileFromPathAsync(&h)?.get()?;
        let thumb = RandomAccessStreamReference::CreateFromFile(&file)?;
        updater.SetThumbnail(&thumb)?;
    } else {
        updater.SetThumbnail(None::<&RandomAccessStreamReference>)?;
    }
    Ok(())
}

fn process_message_pump() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

struct UpdateState {
    last_title: String,
    last_artist: String,
    last_album: String,
    last_art_url: Option<String>,
    last_cover_path: Option<String>,
    last_duration_ms: u64,
    last_position_sent_ms: u64,
    last_is_playing: bool,
    last_timeline_update: Instant,
    last_cover_temp_path: Option<PathBuf>,
    cover_cache: HashMap<String, PathBuf>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            last_title: String::new(),
            last_artist: String::new(),
            last_album: String::new(),
            last_art_url: None,
            last_cover_path: None,
            last_duration_ms: 0,
            last_position_sent_ms: 0,
            last_is_playing: false,
            last_timeline_update: Instant::now(),
            last_cover_temp_path: None,
            cover_cache: HashMap::new(),
        }
    }
}

fn update_smtc(
    smtc: &SystemMediaTransportControls,
    state: &SmtcState,
    update: &mut UpdateState,
) -> WinResult<()> {
    let metadata_changed = state.title != update.last_title
        || state.artist != update.last_artist
        || state.album != update.last_album
        || state.is_playing != update.last_is_playing
        || state.art_url != update.last_art_url
        || state.cover_path != update.last_cover_path;

    let cover_changed =
        state.art_url != update.last_art_url || state.cover_path != update.last_cover_path;

    // Timeline is updated at most once per 500ms and only when the position
    // has advanced by at least 1 second.  Seeking is detected by a backward
    // jump or a forward jump of 2+ seconds, which forces an immediate update.
    let position_forward = state
        .position_ms
        .saturating_sub(update.last_position_sent_ms);
    let position_backward = update
        .last_position_sent_ms
        .saturating_sub(state.position_ms);
    let seek_detected = position_backward > 0 || position_forward >= 2000;
    let normal_timeline_update = position_forward >= 1000
        && update.last_timeline_update.elapsed() >= Duration::from_millis(500);
    let timeline_changed =
        state.duration_ms != update.last_duration_ms || seek_detected || normal_timeline_update;

    // Playback status ( cheap; always set when it changes )
    let status = if state.is_playing {
        MediaPlaybackStatus::Playing
    } else {
        MediaPlaybackStatus::Paused
    };
    smtc.SetPlaybackStatus(status)?;

    // Display metadata + thumbnail
    if metadata_changed || cover_changed {
        let updater = smtc.DisplayUpdater()?;
        updater.SetType(MediaPlaybackType::Music)?;

        let props = updater.MusicProperties()?;
        props.SetTitle(&hstr(&state.title))?;
        props.SetArtist(&hstr(&state.artist))?;
        props.SetAlbumTitle(&hstr(&state.album))?;

        if cover_changed {
            let source_key =
                cover_source_key(state.art_url.as_deref(), state.cover_path.as_deref());
            let resolved = if source_key.is_empty() {
                None
            } else {
                resolve_cover_path(
                    state.art_url.as_deref(),
                    state.cover_path.as_deref(),
                    &mut update.cover_cache,
                )
            };

            if resolved.as_deref() != update.last_cover_temp_path.as_deref() {
                set_thumbnail_from_file(&updater, resolved.as_deref())?;
                update.last_cover_temp_path = resolved;
            }

            update.last_art_url.clone_from(&state.art_url);
            update.last_cover_path.clone_from(&state.cover_path);
        }

        updater.Update()?;

        update.last_title.clone_from(&state.title);
        update.last_artist.clone_from(&state.artist);
        update.last_album.clone_from(&state.album);
        update.last_is_playing = state.is_playing;
    }

    // Timeline / seek bar
    if timeline_changed {
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(ms_to_timespan(0))?;
        timeline.SetMinSeekTime(ms_to_timespan(0))?;
        timeline.SetMaxSeekTime(ms_to_timespan(state.duration_ms))?;
        timeline.SetEndTime(ms_to_timespan(state.duration_ms))?;
        timeline.SetPosition(ms_to_timespan(state.position_ms))?;
        smtc.UpdateTimelineProperties(&timeline)?;

        update.last_position_sent_ms = state.position_ms;
        update.last_duration_ms = state.duration_ms;
        update.last_timeline_update = Instant::now();
    }

    Ok(())
}

fn smtc_worker(
    state_rx: mpsc::Receiver<SmtcState>,
    cmd_tx: mpsc::Sender<SmtcCmd>,
    stop_rx: mpsc::Receiver<()>,
) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let result: WinResult<()> = (|| {
        let player = MediaPlayer::new()?;
        let list = MediaPlaybackList::new()?;
        player.SetSource(&list)?;

        let smtc = player.SystemMediaTransportControls()?;
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsNextEnabled(true)?;
        smtc.SetIsPreviousEnabled(true)?;
        smtc.SetIsStopEnabled(false)?;

        let button_tx = cmd_tx.clone();
        smtc.ButtonPressed(&TypedEventHandler::new(
            move |_,
                  args: windows::core::Ref<
                '_,
                windows::Media::SystemMediaTransportControlsButtonPressedEventArgs,
            >| {
                if let Some(args) = args.as_ref() {
                    let button = args.Button()?;
                    let cmd = match button {
                        SystemMediaTransportControlsButton::Play => Some(SmtcCmd::Play),
                        SystemMediaTransportControlsButton::Pause => Some(SmtcCmd::Pause),
                        SystemMediaTransportControlsButton::Next => Some(SmtcCmd::Next),
                        SystemMediaTransportControlsButton::Previous => Some(SmtcCmd::Previous),
                        _ => None,
                    };
                    if let Some(cmd) = cmd {
                        let _ = button_tx.send(cmd);
                    }
                }
                Ok(())
            },
        ))?;

        let seek_tx = cmd_tx.clone();
        smtc.PlaybackPositionChangeRequested(&TypedEventHandler::new(
            move |_,
                  args: windows::core::Ref<
                '_,
                windows::Media::PlaybackPositionChangeRequestedEventArgs,
            >| {
                if let Some(args) = args.as_ref() {
                    let pos = args.RequestedPlaybackPosition()?;
                    let ms = (pos.Duration / 10_000).max(0) as u64;
                    let _ = seek_tx.send(SmtcCmd::Seek(ms));
                }
                Ok(())
            },
        ))?;

        let mut update_state = UpdateState::default();
        loop {
            process_message_pump();

            let mut latest: Option<SmtcState> = None;
            loop {
                match state_rx.try_recv() {
                    Ok(state) => latest = Some(state),
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }

            if let Some(state) = latest {
                let _ = update_smtc(&smtc, &state, &mut update_state);
            }

            match stop_rx.try_recv() {
                Ok(()) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => {}
            }

            // Wait for Windows messages or a short timeout.
            unsafe {
                let _ = MsgWaitForMultipleObjects(None, false, 16, QS_ALLINPUT);
            }
        }
    })();

    if let Err(e) = result {
        eprintln!("SMTC worker error: {e:?}");
    }

    unsafe {
        CoUninitialize();
    }
}

pub fn spawn() -> Result<SmtcHandle> {
    let (state_tx, state_rx) = mpsc::channel::<SmtcState>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<SmtcCmd>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    let join = thread::Builder::new()
        .name("smtc-worker".into())
        .spawn(move || smtc_worker(state_rx, cmd_tx, stop_rx))?;

    Ok(SmtcHandle {
        state_tx,
        cmd_rx,
        stop_tx,
        join: Some(join),
    })
}
