use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};

use super::{LIBRARY_ITEMS, Ui, UiState};

impl Ui {
    pub fn render_welcome(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if state.compact_effective {
            let mut items: Vec<ListItem> = Vec::new();

            items.push(ListItem::new(Line::from(Span::styled(
                " Default",
                Style::default()
                    .fg(self.theme.text_secondary)
                    .add_modifier(Modifier::BOLD),
            ))));

            for name in LIBRARY_ITEMS {
                items.push(ListItem::new(Line::from(vec![Span::raw(format!(
                    "  {name} "
                ))])));
            }

            if !state.playlists.is_empty() {
                items.push(ListItem::new(Line::from(Span::styled(
                    " Playlists",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::BOLD),
                ))));

                for p in &state.playlists {
                    items.push(ListItem::new(Line::from(vec![Span::raw(format!(
                        "  {} ",
                        p.name
                    ))])));
                }
            }

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .bg(self.theme.highlight_bg)
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(self.theme.highlight_symbol.as_str());
            frame.render_stateful_widget(list, area, &mut state.library_list);
            return;
        }

        let block = self.build_panel_block(UiWidget::MainContent, false, "");
        frame.render_widget(&block, area);
        let inner = block.inner(area);

        let lines = if state.loading {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Loading...",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::SLOW_BLINK),
                )),
                Line::from(""),
            ]
        } else if !state.spotify_authenticated && state.local_tree.visible_len() == 0 {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Welcome to isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Choose how to get started:",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " 1. Spotify streaming",
                    Style::default()
                        .fg(self.theme.text_primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "   Run: isi-music setup-spotify",
                    Style::default().fg(self.theme.accent_color),
                )),
                Line::from(Span::styled(
                    "   Then select Liked Songs or a playlist from the left panel",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " 2. Local files",
                    Style::default()
                        .fg(self.theme.text_primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "   Set [local] music_dir in config.toml",
                    Style::default().fg(self.theme.accent_color),
                )),
                Line::from(Span::styled(
                    "   Then select Local Files from the Library panel and press ENTER",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate   ENTER select   / search   ? help   q quit",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        } else if state.spotify_authenticated && state.tracks.is_empty() {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Select a playlist from the Library or Playlists panel,",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(Span::styled(
                    "or press / to search Spotify.",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate   ENTER select   / search   Ctrl+F quick search",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        } else if !state.spotify_authenticated && state.local_tree.visible_len() > 0 {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Local files loaded! Select Local Files and press ENTER to play.",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Want Spotify streaming? Run: isi-music setup-spotify",
                    Style::default().fg(self.theme.accent_color),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate   ENTER select   / search   ? help   q quit",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        } else {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "isi-music",
                    Style::default()
                        .fg(self.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Select a playlist from the Library or Playlists panel,",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(Span::styled(
                    "or press / to search Spotify.",
                    Style::default().fg(self.theme.text_secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "TAB navigate panels   ENTER select   / search   Ctrl+F quick search",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                )),
            ]
        };

        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }
}
