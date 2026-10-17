use crate::config;

use super::NativePlayer;
use crate::player::types::RepeatMode;

impl NativePlayer {
    pub fn play(&mut self) {
        self.player.play();
        self.is_playing = true;
    }

    pub fn pause(&mut self) {
        if self.is_playing {
            self.player.pause();
            self.is_playing = false;
        }
    }

    pub fn toggle(&mut self) {
        if self.is_playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.play_history.clear();
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn volume_up(&mut self) {
        self.volume = self.volume.saturating_add(5).min(100);
        self.apply_volume();
        config::save_volume(self.volume);
    }

    pub fn volume_down(&mut self) {
        self.volume = self.volume.saturating_sub(5);
        self.apply_volume();
        config::save_volume(self.volume);
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    pub(super) fn apply_volume(&self) {
        let v = (self.volume as u32 * 65535 / 100) as u16;
        self.mixer.set_volume(v);
    }
}
