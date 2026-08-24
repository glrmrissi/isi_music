use crate::utils::theme::UiWidget;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};
#[cfg(feature = "album-art")]
use ratatui_image::protocol::StatefulProtocol;

use super::{Ui, UiState};

impl Ui {
    pub fn render_now_playing_widget(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        if area.width < 10 || area.height < 5 {
            return;
        }

        #[cfg(feature = "album-art")]
        let info_area = if state.show_album_art {
            let art_size = area.height.min(18).min(area.width / 4).max(12);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(art_size),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(area);

            let art_area = cols[0];
            if let Some(art) = &mut state.album_art {
                if let Some(img_state) = &mut art.image_state {
                    frame.render_stateful_widget(
                        ratatui_image::StatefulImage::<StatefulProtocol>::default(),
                        art_area,
                        img_state,
                    );
                }
            }
            cols[2]
        } else {
            area
        };

        #[cfg(not(feature = "album-art"))]
        let info_area = area;

        let info_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(info_area);

        let title_area = info_grid[0];
        let artist_area = info_grid[1];
        let album_area = info_grid[2];
        let progress_area = info_grid[4];
        state.store_widget_rect(UiWidget::Progress, progress_area);

        let pb = &state.playback;

        frame.render_widget(
            Paragraph::new(vec![Line::from(Span::styled(
                pb.title.as_str(),
                Style::default()
                    .fg(self.theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ))]),
            title_area,
        );

        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![
                Span::styled(
                    "Artist  ",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    pb.artist.as_str(),
                    Style::default().fg(self.theme.text_primary),
                ),
            ])]),
            artist_area,
        );

        let album_split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(12)])
            .split(album_area);

        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![
                Span::styled(
                    "Album   ",
                    Style::default()
                        .fg(self.theme.text_secondary)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    pb.album.as_str(),
                    Style::default().fg(self.theme.text_primary),
                ),
            ])]),
            album_split[0],
        );

        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![Span::styled(
                format!(" Vol: {}% ", pb.volume),
                Style::default().fg(self.theme.text_secondary),
            )])])
            .alignment(ratatui::layout::Alignment::Right),
            album_split[1],
        );

        self.render_progress(frame, &state.playback, progress_area);
    }

    pub fn render_add_to_playlist(&self, frame: &mut Frame, state: &mut UiState, area: Rect) {
        let block = self.build_panel_block(UiWidget::MainContent, true, "Add to Playlist");

        let mut items: Vec<ListItem> = state
            .playlists
            .iter()
            .map(|p| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {} ", p.name)),
                    Span::styled(
                        format!("({})", p.total_tracks),
                        Style::default().fg(self.theme.text_secondary),
                    ),
                ]))
            })
            .collect();

        items.push(ListItem::new(Line::from(vec![Span::styled(
            " + Create new playlist",
            Style::default()
                .fg(self.theme.accent_color)
                .add_modifier(Modifier::BOLD),
        )])));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        frame.render_stateful_widget(list, area, &mut state.add_to_playlist_list);
    }

    pub fn render_delete_playlist_confirm(
        &self,
        frame: &mut Frame,
        state: &mut UiState,
        area: Rect,
    ) {
        let name = state
            .delete_playlist_target
            .as_deref()
            .unwrap_or("this playlist");
        let title = format!("Delete Playlist: {name}?");

        let block = self.build_panel_block(UiWidget::MainContent, true, &title);

        let items = vec![
            ListItem::new(Line::from(Span::styled(
                "  Yes (y)",
                Style::default().fg(self.theme.text_primary),
            ))),
            ListItem::new(Line::from(Span::styled(
                "  No (n)",
                Style::default().fg(self.theme.text_primary),
            ))),
        ];

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(self.theme.highlight_bg)
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(self.theme.highlight_symbol.as_str());

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        frame.render_stateful_widget(list, area, &mut list_state);
    }
}
