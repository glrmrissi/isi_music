use super::{ActiveContent, Focus, UiState};

impl UiState {
    pub fn current_label(&self) -> String {
        if let Some(ref sr) = self.search_results {
            return format!("Search: \"{}\"", sr.query);
        }
        if let Some(ref uri) = self.active_playlist_uri {
            if uri == "liked_songs" {
                return "Liked Songs".to_string();
            }
            if uri.starts_with("artist:") {
                return self
                    .active_artist_name
                    .clone()
                    .unwrap_or_else(|| "Artist".to_string());
            }
            if let Some(p) = self
                .playlists
                .iter()
                .find(|p| p.uri == *uri || p.id == *uri)
            {
                return p.name.clone();
            }
        }
        match self.active_content {
            ActiveContent::None => "Library".to_string(),
            ActiveContent::Tracks => "Tracks".to_string(),
            ActiveContent::Albums => "Albums".to_string(),
            ActiveContent::Artists => "Artists".to_string(),
            ActiveContent::Shows => "Shows".to_string(),
            ActiveContent::LocalFiles => "Local Files".to_string(),
        }
    }

    pub fn restore_session(&mut self, session: &crate::config::SessionState) {
        self.focus = Focus::Library;
        if let Some(ref content) = session.active_content {
            match content.as_str() {
                "tracks" => self.active_content = ActiveContent::Tracks,
                "albums" => self.active_content = ActiveContent::Albums,
                "artists" => self.active_content = ActiveContent::Artists,
                "shows" => self.active_content = ActiveContent::Shows,
                "local_files" => self.active_content = ActiveContent::LocalFiles,
                _ => {}
            }
        }
        if let Some(compact) = session.compact_mode {
            self.compact_mode = compact;
            self.compact_effective = compact;
        }
        if let Some(sel) = session.library_selected {
            self.library_list.select(Some(sel));
        }
    }

    fn focus_cycle(&self) -> Vec<Focus> {
        let mut cycle = vec![Focus::Library, Focus::Playlists];
        if self.search_results.is_some() {
            cycle.push(Focus::Search);
        }
        cycle.push(Focus::Tracks);
        cycle.push(Focus::Queue);
        cycle
    }

    pub fn switch_focus(&mut self) {
        self.search_active = false;
        let cycle = self.focus_cycle();
        let pos = cycle.iter().position(|f| *f == self.focus);
        self.focus = match pos {
            Some(i) => cycle[(i + 1) % cycle.len()],
            None => cycle[0],
        };
    }

    pub fn switch_focus_prev(&mut self) {
        self.search_active = false;
        let cycle = self.focus_cycle();
        let pos = cycle.iter().position(|f| *f == self.focus);
        self.focus = match pos {
            Some(i) => {
                if i == 0 {
                    cycle[cycle.len() - 1]
                } else {
                    cycle[i - 1]
                }
            }
            None => cycle[0],
        };
    }

    pub fn switch_search_panel(&mut self) {
        if let Some(sr) = &mut self.search_results {
            sr.next_panel();
        }
    }

    pub fn switch_search_panel_prev(&mut self) {
        if let Some(sr) = &mut self.search_results {
            sr.prev_panel();
        }
    }
}
