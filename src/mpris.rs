/// MPRIS2 D-Bus server — exposes isi-music to Waybar, playerctl, and Hyprland media keys.
///
/// Architecture:
///   - `Arc<Mutex<MprisState>>` is written by the app each tick and read by D-Bus handlers.
///   - `mpsc` channel carries commands from D-Bus clients (media keys) back to the app.
///   - A `watch` channel triggers `PropertiesChanged` D-Bus signals when state changes.

use anyhow::Result;
use mpris_server::{
    zbus, Metadata, PlayerInterface, Property, RootInterface, Server,
    LoopStatus, PlaybackStatus, Time, TrackId, Volume,
};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Default)]
pub struct MprisState {
    pub title:        String,
    pub artist:       String,
    pub album:        String,
    pub art_url:      Option<String>,
    pub duration_us:  i64,   // microseconds
    pub position_us:  i64,
    pub volume:       f64,   // 0.0 – 1.0
    pub is_playing:   bool,
    pub shuffle:      bool,
    pub repeat_track: bool,
    pub repeat_queue: bool,
}


pub enum MprisCmd {
    Play,
    Pause,
    Next,
    Prev,
    Seek(i64),      // absolute position in microseconds
    SetVolume(f64), // 0.0 – 1.0
}

struct MprisImpl {
    state:  Arc<Mutex<MprisState>>,
    cmd_tx: mpsc::UnboundedSender<MprisCmd>,
}

impl RootInterface for MprisImpl {
    async fn raise(&self) -> zbus::fdo::Result<()>                  { Ok(()) }
    async fn quit(&self) -> zbus::fdo::Result<()>                   { Ok(()) }
    async fn can_quit(&self) -> zbus::fdo::Result<bool>             { Ok(false) }
    async fn fullscreen(&self) -> zbus::fdo::Result<bool>           { Ok(false) }
    async fn set_fullscreen(&self, _: bool) -> zbus::Result<()>     { Ok(()) }
    async fn can_set_fullscreen(&self) -> zbus::fdo::Result<bool>   { Ok(false) }
    async fn can_raise(&self) -> zbus::fdo::Result<bool>            { Ok(false) }
    async fn has_track_list(&self) -> zbus::fdo::Result<bool>       { Ok(false) }
    async fn identity(&self) -> zbus::fdo::Result<String>           { Ok("isi-music".into()) }
    async fn desktop_entry(&self) -> zbus::fdo::Result<String>      { Ok("isi-music".into()) }
    async fn supported_uri_schemes(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(vec!["spotify".into()])
    }
    async fn supported_mime_types(&self) -> zbus::fdo::Result<Vec<String>> { Ok(vec![]) }
}

impl PlayerInterface for MprisImpl {
    async fn next(&self) -> zbus::fdo::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::Next);
        Ok(())
    }
    async fn previous(&self) -> zbus::fdo::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::Prev);
        Ok(())
    }
    async fn pause(&self) -> zbus::fdo::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::Pause);
        Ok(())
    }
    async fn play_pause(&self) -> zbus::fdo::Result<()> {
        let is_playing = self.state.lock().unwrap().is_playing;
        let _ = self.cmd_tx.send(if is_playing { MprisCmd::Pause } else { MprisCmd::Play });
        Ok(())
    }
    async fn stop(&self) -> zbus::fdo::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::Pause);
        Ok(())
    }
    async fn play(&self) -> zbus::fdo::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::Play);
        Ok(())
    }
    async fn seek(&self, offset: Time) -> zbus::fdo::Result<()> {
        let pos = self.state.lock().unwrap().position_us;
        let new_pos = (pos + offset.as_micros()).max(0);
        let _ = self.cmd_tx.send(MprisCmd::Seek(new_pos));
        Ok(())
    }
    async fn set_position(&self, _: TrackId, position: Time) -> zbus::fdo::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::Seek(position.as_micros()));
        Ok(())
    }
    async fn open_uri(&self, _: String) -> zbus::fdo::Result<()> { Ok(()) }

    async fn playback_status(&self) -> zbus::fdo::Result<PlaybackStatus> {
        let s = self.state.lock().unwrap();
        Ok(if s.title.is_empty() {
            PlaybackStatus::Stopped
        } else if s.is_playing {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        })
    }
    async fn loop_status(&self) -> zbus::fdo::Result<LoopStatus> {
        let s = self.state.lock().unwrap();
        Ok(if s.repeat_track      { LoopStatus::Track }
           else if s.repeat_queue { LoopStatus::Playlist }
           else                   { LoopStatus::None })
    }
    async fn set_loop_status(&self, _: LoopStatus) -> zbus::Result<()> { Ok(()) }
    async fn rate(&self) -> zbus::fdo::Result<f64>                     { Ok(1.0) }
    async fn set_rate(&self, _: f64) -> zbus::Result<()>               { Ok(()) }
    async fn shuffle(&self) -> zbus::fdo::Result<bool> {
        Ok(self.state.lock().unwrap().shuffle)
    }
    async fn set_shuffle(&self, _: bool) -> zbus::Result<()> { Ok(()) }

    async fn metadata(&self) -> zbus::fdo::Result<Metadata> {
        let s = self.state.lock().unwrap();
        let mut meta = Metadata::new();
        if !s.title.is_empty() {
            let track_id = TrackId::try_from("/org/isi_music/CurrentTrack")
                .unwrap_or_else(|_| TrackId::try_from("/").unwrap());
            meta.set_trackid(Some(track_id));
            meta.set_title(Some(s.title.clone()));
            meta.set_artist(Some(vec![s.artist.clone()]));
            meta.set_album(Some(s.album.clone()));
            meta.set_length(Some(Time::from_micros(s.duration_us)));
            if let Some(url) = &s.art_url {
                meta.set_art_url(Some(url.clone()));
            }
        }
        Ok(meta)
    }

    async fn volume(&self) -> zbus::fdo::Result<Volume> {
        Ok(self.state.lock().unwrap().volume)
    }
    async fn set_volume(&self, v: Volume) -> zbus::Result<()> {
        let _ = self.cmd_tx.send(MprisCmd::SetVolume(v));
        Ok(())
    }
    async fn position(&self) -> zbus::fdo::Result<Time> {
        Ok(Time::from_micros(self.state.lock().unwrap().position_us))
    }
    async fn minimum_rate(&self) -> zbus::fdo::Result<f64>      { Ok(1.0) }
    async fn maximum_rate(&self) -> zbus::fdo::Result<f64>      { Ok(1.0) }
    async fn can_go_next(&self) -> zbus::fdo::Result<bool>      { Ok(true) }
    async fn can_go_previous(&self) -> zbus::fdo::Result<bool>  { Ok(true) }
    async fn can_play(&self) -> zbus::fdo::Result<bool>         { Ok(true) }
    async fn can_pause(&self) -> zbus::fdo::Result<bool>        { Ok(true) }
    async fn can_seek(&self) -> zbus::fdo::Result<bool>         { Ok(true) }
    async fn can_control(&self) -> zbus::fdo::Result<bool>      { Ok(true) }
}

pub struct MprisHandle {
    /// Written by the app; read by D-Bus handlers.
    pub state: Arc<Mutex<MprisState>>,
    /// Incoming commands from D-Bus clients (media keys, playerctl, Waybar).
    pub cmd_rx: mpsc::UnboundedReceiver<MprisCmd>,
    /// Sending a value here triggers a `PropertiesChanged` D-Bus signal.
    notify_tx: watch::Sender<()>,
}

impl MprisHandle {
    /// Push a new playback state and notify MPRIS clients.
    pub fn update(&self, new_state: MprisState) {
        *self.state.lock().unwrap() = new_state;
        let _ = self.notify_tx.send(());
    }
}


pub async fn spawn() -> Result<MprisHandle> {
    let state  = Arc::new(Mutex::new(MprisState::default()));
    let (cmd_tx, cmd_rx)     = mpsc::unbounded_channel::<MprisCmd>();
    let (notify_tx, mut notify_rx) = watch::channel(());

    let imp = MprisImpl { state: state.clone(), cmd_tx };

    let server = Server::new("isi_music", imp).await
        .map_err(|e| anyhow::anyhow!("MPRIS D-Bus error: {e}"))?;

    // Keep the server alive and emit PropertiesChanged on every state update.
    let state_for_signals = state.clone();
    tokio::spawn(async move {
        loop {
            if notify_rx.changed().await.is_err() { break; }
            let s = state_for_signals.lock().unwrap().clone();

            let playback_status = if s.title.is_empty() { PlaybackStatus::Stopped }
                                  else if s.is_playing  { PlaybackStatus::Playing }
                                  else                  { PlaybackStatus::Paused };
            let loop_status = if s.repeat_track      { LoopStatus::Track }
                              else if s.repeat_queue { LoopStatus::Playlist }
                              else                   { LoopStatus::None };

            let mut meta = Metadata::new();
            if !s.title.is_empty() {
                let track_id = TrackId::try_from("/org/isi_music/CurrentTrack")
                    .unwrap_or_else(|_| TrackId::try_from("/").unwrap());
                meta.set_trackid(Some(track_id));
                meta.set_title(Some(s.title.clone()));
                meta.set_artist(Some(vec![s.artist.clone()]));
                meta.set_album(Some(s.album.clone()));
                meta.set_length(Some(Time::from_micros(s.duration_us)));
                if let Some(url) = &s.art_url {
                    meta.set_art_url(Some(url.clone()));
                }
            }

            let _ = server.properties_changed([
                Property::PlaybackStatus(playback_status),
                Property::LoopStatus(loop_status),
                Property::Shuffle(s.shuffle),
                Property::Metadata(meta),
                Property::Volume(s.volume),
            ]).await;
        }
    });

    Ok(MprisHandle { state, cmd_rx, notify_tx })
}
