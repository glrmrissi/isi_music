use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LyricsData {
    pub lines: Vec<LyricLine>,
    pub is_synced: bool,
}

impl LyricsData {
    pub fn active_idx(&self, progress_ms: u64) -> Option<usize> {
        if !self.is_synced || self.lines.is_empty() {
            return None;
        }
        self.lines.iter().rposition(|l| l.time_ms <= progress_ms)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
