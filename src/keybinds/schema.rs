use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::actions::Action;

#[derive(Debug, Deserialize)]
pub(crate) struct KeybindsToml {
    pub(crate) playback: Option<HashMap<String, KeySpec>>,
    pub(crate) navigation: Option<HashMap<String, KeySpec>>,
    pub(crate) modes: Option<HashMap<String, KeySpec>>,
    pub(crate) actions: Option<HashMap<String, KeySpec>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum KeySpec {
    Single(String),
    Multiple(Vec<String>),
}

pub(crate) fn name_to_action(name: &str) -> Option<Action> {
    let name = match name {
        "get_recommendations" => "recommendations",
        other => other,
    };
    Action::all()
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, a)| *a)
}

pub(crate) struct KeybindsTomlOutput {
    pub(crate) playback: Vec<(String, Vec<String>)>,
    pub(crate) navigation: Vec<(String, Vec<String>)>,
    pub(crate) modes: Vec<(String, Vec<String>)>,
    pub(crate) actions: Vec<(String, Vec<String>)>,
}

impl KeybindsTomlOutput {
    pub(crate) fn from_defaults() -> Self {
        let mut playback = Vec::new();
        let mut navigation = Vec::new();
        let mut modes = Vec::new();
        let mut actions = Vec::new();

        for (name, keys, action) in Action::all() {
            let key_strs: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
            let entry = (name.to_string(), key_strs);
            match action {
                Action::PlayPause
                | Action::NextTrack
                | Action::PrevTrack
                | Action::VolumeUp
                | Action::VolumeDown
                | Action::SeekForward
                | Action::SeekBackward
                | Action::SeekMiddle
                | Action::ToggleShuffle
                | Action::CycleRepeat
                | Action::ToggleRadio
                | Action::GetRecommendations
                | Action::LikeTrack => playback.push(entry),
                Action::NavUp
                | Action::NavDown
                | Action::NavFirst
                | Action::NavLast
                | Action::NavMiddle
                | Action::TabNext
                | Action::TabPrev
                | Action::Enter
                | Action::Back
                | Action::FocusLibrary
                | Action::FocusPlaylists
                | Action::FocusTracks
                | Action::FocusQueue
                | Action::JumpToPlaying => navigation.push(entry),
                Action::Search
                | Action::QuickSearch
                | Action::Help
                | Action::ToggleCompact
                | Action::ToggleFullscreen
                | Action::ToggleVisualizer
                | Action::ToggleLyrics
                | Action::ToggleDebug
                | Action::ToggleBreadcrumb
                | Action::ScrollUp
                | Action::ScrollDown
                | Action::OptionsPanel => modes.push(entry),
                Action::AddToQueue
                | Action::RemoveFromQueue
                | Action::SortTracks
                | Action::CopyTrackLink
                | Action::AddToPlaylist
                | Action::RemoveFromPlaylist
                | Action::DeletePlaylist
                | Action::CommandPrompt
                | Action::Quit => actions.push(entry),
            }
        }

        Self {
            playback,
            navigation,
            modes,
            actions,
        }
    }
}

impl Serialize for KeybindsTomlOutput {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(4))?;

        let serialize_section = |map: &mut <S as serde::Serializer>::SerializeMap,
                                 name: &str,
                                 entries: &[(String, Vec<String>)]|
         -> Result<(), S::Error> {
            if entries.is_empty() {
                return Ok(());
            }
            let mut section = std::collections::BTreeMap::new();
            for (k, v) in entries {
                let val = if v.len() == 1 {
                    toml::Value::String(v[0].clone())
                } else {
                    toml::Value::Array(v.iter().map(|s| toml::Value::String(s.clone())).collect())
                };
                section.insert(k.clone(), val);
            }
            map.serialize_entry(name, &section)?;
            Ok(())
        };

        serialize_section(&mut map, "playback", &self.playback)?;
        serialize_section(&mut map, "navigation", &self.navigation)?;
        serialize_section(&mut map, "modes", &self.modes)?;
        serialize_section(&mut map, "actions", &self.actions)?;

        map.end()
    }
}
