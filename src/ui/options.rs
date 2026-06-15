use crate::config::AppOptionsConfig;
use crate::utils::cache::{CacheManager, CacheStats};
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
};

use super::UiState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelAction {
    None,
    Close,
    ToggleItem,
    ClearAllCache,
    CleanupExpired,
    RefreshStats,
    RefreshPlaylists,
}

pub struct OptionsPanel {
    pub visible: bool,
    pub focused_section: OptionsSection,
    pub selected_item: usize,
    pub cache_manager: CacheManager,
    pub config: AppOptionsConfig,
    pub cache_stats: Option<CacheStats>,
    pub loading: bool,
    pub help_text: Vec<String>,
    pub help_scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptionsSection {
    Features,
    Cache,
    QuickAccess,
    Help,
}

const SECTIONS: &[OptionsSection] = &[
    OptionsSection::Features,
    OptionsSection::Cache,
    OptionsSection::QuickAccess,
    OptionsSection::Help,
];

fn bg_style() -> Style {
    Style::default().bg(Color::Rgb(20, 20, 20))
}

fn section_block(title: &str) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(bg_style())
}

impl OptionsPanel {
    pub fn new(cache_manager: CacheManager) -> Self {
        Self {
            visible: false,
            focused_section: OptionsSection::Features,
            selected_item: 0,
            cache_manager,
            config: AppOptionsConfig::default(),
            cache_stats: None,
            loading: false,
            help_text: Vec::new(),
            help_scroll: 0,
        }
    }

    pub fn set_help_text(&mut self, text: Vec<String>) {
        self.help_text = text;
        self.help_scroll = 0;
    }

    pub async fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.selected_item = 0;
            self.load_cache_stats().await;
        } else {
            self.cache_stats = None;
        }
    }

    pub async fn load_cache_stats(&mut self) {
        self.loading = true;
        let stats = self.cache_manager.get_stats().await;
        self.cache_stats = Some(stats);
        self.loading = false;
    }

    fn items_in_section(&self) -> usize {
        match self.focused_section {
            OptionsSection::Features => 5,
            OptionsSection::Cache => 8,
            OptionsSection::QuickAccess => 1,
            OptionsSection::Help => 1,
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) -> PanelAction {
        if self.focused_section == OptionsSection::Help {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                    return PanelAction::None;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                    return PanelAction::None;
                }
                KeyCode::Esc => return PanelAction::Close,
                _ => {}
            }
        }

        match code {
            KeyCode::Esc => PanelAction::Close,
            KeyCode::Up => {
                if self.selected_item == 0 {
                    self.selected_item = self.items_in_section().saturating_sub(1);
                } else {
                    self.selected_item -= 1;
                }
                PanelAction::None
            }
            KeyCode::Down => {
                self.selected_item = (self.selected_item + 1) % self.items_in_section().max(1);
                PanelAction::None
            }
            KeyCode::Left => {
                self.navigate_sections(true, false);
                self.selected_item = 0;
                PanelAction::None
            }
            KeyCode::Right => {
                self.navigate_sections(false, true);
                self.selected_item = 0;
                PanelAction::None
            }
            KeyCode::Tab => {
                self.navigate_sections(false, true);
                self.selected_item = 0;
                PanelAction::None
            }
            KeyCode::Enter => match self.focused_section {
                OptionsSection::Cache => match self.selected_item {
                    4 => PanelAction::ClearAllCache,
                    5 => PanelAction::CleanupExpired,
                    6 => PanelAction::RefreshStats,
                    7 => PanelAction::RefreshPlaylists,
                    _ => PanelAction::None,
                },
                _ => PanelAction::ToggleItem,
            },
            KeyCode::Char('c') | KeyCode::Char('C')
                if self.focused_section == OptionsSection::Cache =>
            {
                PanelAction::ClearAllCache
            }
            KeyCode::Char('r') | KeyCode::Char('R')
                if self.focused_section == OptionsSection::Cache =>
            {
                PanelAction::RefreshStats
            }
            _ => PanelAction::None,
        }
    }

    pub fn navigate_sections(&mut self, up: bool, down: bool) {
        if let Some(current) = SECTIONS.iter().position(|s| *s == self.focused_section) {
            let mut new = current;
            if up && new == 0 {
                new = SECTIONS.len() - 1;
            } else if up {
                new -= 1;
            }
            if down && new == SECTIONS.len() - 1 {
                new = 0;
            } else if down {
                new += 1;
            }
            self.focused_section = SECTIONS[new];
        }
    }

    pub fn render(&self, frame: &mut Frame, state: &UiState) {
        if !self.visible {
            return;
        }

        let bg = Style::default().bg(Color::Rgb(20, 20, 20));
        let area = frame.area();

        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(area);

        let content_area = popup_layout[1];
        let footer_area = popup_layout[2];

        // Opaque backdrop for the full popup
        frame.render_widget(Clear, content_area);
        frame.render_widget(Paragraph::new("").style(bg), content_area);

        let block = Block::default()
            .title(" Options Panel ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(bg);

        let inner_area = block.inner(content_area);
        frame.render_widget(block, content_area);

        let sections_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(0)])
            .split(inner_area);

        let sections_area = sections_layout[0];
        let content_area = sections_layout[1];

        // Opaque backdrop for sidebar and content
        frame.render_widget(Clear, sections_area);
        frame.render_widget(Paragraph::new("").style(bg), sections_area);
        frame.render_widget(Clear, content_area);
        frame.render_widget(Paragraph::new("").style(bg), content_area);

        self.render_sections(frame, sections_area);
        self.render_content(frame, state, content_area);

        // Opaque footer
        frame.render_widget(Clear, footer_area);
        frame.render_widget(Paragraph::new("").style(bg), footer_area);

        let footer_text = Line::from(vec![
            Span::styled(
                " [\u{2191}\u{2193}] Items ",
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                " [\u{2190}\u{2192}/Tab] Sections ",
                Style::default().fg(Color::Gray),
            ),
            Span::styled(" [Enter] Select ", Style::default().fg(Color::Yellow)),
            Span::styled(" [Esc] Close ", Style::default().fg(Color::Gray)),
        ]);

        frame.render_widget(
            Paragraph::new(footer_text)
                .style(bg)
                .alignment(Alignment::Center),
            footer_area,
        );
    }

    fn render_sections(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = SECTIONS
            .iter()
            .map(|section| {
                let label = match section {
                    OptionsSection::Features => "  Features",
                    OptionsSection::Cache => "  Cache",
                    OptionsSection::QuickAccess => "  Quick Access",
                    OptionsSection::Help => "  Help",
                };
                let is_focused = self.focused_section == *section;
                let style = if is_focused {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();

        let mut list_state = ratatui::widgets::ListState::default();
        if let Some(idx) = SECTIONS.iter().position(|s| *s == self.focused_section) {
            list_state.select(Some(idx));
        }

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Sections ")
                    .style(Style::default().bg(Color::Rgb(20, 20, 20))),
            )
            .style(Style::default().bg(Color::Rgb(20, 20, 20)))
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(50, 50, 50))
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("\u{25b6} ");

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn render_content(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        match self.focused_section {
            OptionsSection::Features => self.render_features_section(frame, state, area),
            OptionsSection::Cache => self.render_cache_section(frame, area),
            OptionsSection::QuickAccess => self.render_quick_access_section(frame, area),
            OptionsSection::Help => self.render_help_section(frame, area),
        }
    }

    fn render_item_list(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        items: &[(&str, &str, bool)],
    ) {
        let block = section_block(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(i, &(label, _, enabled))| {
                let is_selected = i == self.selected_item;
                let prefix = if is_selected { "\u{25b6} " } else { "  " };
                let status_str = if enabled { "On" } else { "Off" };
                let status_color = if enabled { Color::Green } else { Color::Red };
                let line_style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}{}: ", prefix, label), line_style),
                    Span::styled(status_str, Style::default().fg(status_color)),
                ]))
            })
            .collect();

        let list = List::new(list_items)
            .style(bg_style())
            .highlight_style(Style::default().bg(Color::Rgb(50, 50, 50)));

        let mut list_state =
            ratatui::widgets::ListState::default().with_selected(Some(self.selected_item));

        frame.render_stateful_widget(list, inner, &mut list_state);
    }

    fn render_features_section(&self, frame: &mut Frame, state: &UiState, area: Rect) {
        let items = vec![
            (
                "Cover Images",
                "",
                self.config.show_cover_images.unwrap_or(true),
            ),
            (
                "Lyrics Fetching",
                "",
                self.config.enable_lyrics.unwrap_or(true),
            ),
            (
                "Visualizer Display",
                "",
                self.config.show_visualizer.unwrap_or(true),
            ),
            (
                "Compact Mode",
                "",
                self.config.compact_mode_default.unwrap_or(false),
            ),
            ("Breadcrumb", "", state.show_breadcrumb),
        ];
        self.render_item_list(frame, area, "Feature Toggles", &items);
    }

    fn render_cache_section(&self, frame: &mut Frame, area: Rect) {
        let block = section_block("Cache Management");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.loading {
            let loading_text = Paragraph::new("Loading cache statistics...")
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(Color::Rgb(20, 20, 20)),
                );
            frame.render_widget(loading_text, inner);
            return;
        }

        let mut rows: Vec<ListItem> = Vec::new();

        let stats_items = if let Some(stats) = &self.cache_stats {
            vec![
                format!("  Search Cache: {} entries", stats.search_cache_entries),
                format!("  Library Cache: {} entries", stats.library_cache_entries),
                format!("  Lyrics Cache: {} entries", stats.lyrics_cache_entries),
            ]
        } else {
            vec![
                "  Search Cache: N/A".into(),
                "  Library Cache: N/A".into(),
                "  Lyrics Cache: N/A".into(),
            ]
        };

        for (i, line) in stats_items.iter().enumerate() {
            let is_sel = i == self.selected_item && i < 3;
            let prefix = if is_sel { "\u{25b6} " } else { "  " };
            let style = if is_sel {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            rows.push(ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, line),
                style,
            ))));
        }

        rows.push(ListItem::new(Line::from("")));

        let actions = vec![
            (" Clear All Caches", "c"),
            (" Cleanup Expired", "f"),
            (" Refresh Stats", "r"),
            (" Refresh Playlists", "p"),
        ];

        for (i, (label, key)) in actions.iter().enumerate() {
            let idx = i + 4;
            let is_sel = idx == self.selected_item && idx >= 4;
            let prefix = if is_sel { "\u{25b6} " } else { "  " };
            let style = if is_sel {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            rows.push(ListItem::new(Line::from(Span::styled(
                format!("{}{} [{}]", prefix, label, key),
                style,
            ))));
        }

        let list = List::new(rows)
            .style(bg_style())
            .highlight_style(Style::default().bg(Color::Rgb(50, 50, 50)));

        let mut list_state =
            ratatui::widgets::ListState::default().with_selected(Some(self.selected_item));

        frame.render_stateful_widget(list, inner, &mut list_state);
    }

    fn render_quick_access_section(&self, frame: &mut Frame, area: Rect) {
        let block = section_block("Quick Access Setup");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let content = vec![
            Line::from("  Configure quick access search for:"),
            Line::from(""),
            Line::from("  \u{2022} Playlists (Spotify)"),
            Line::from("  \u{2022} Albums (Spotify)"),
            Line::from("  \u{2022} Artists (Spotify)"),
            Line::from("  \u{2022} Liked Songs (Spotify)"),
            Line::from("  \u{2022} Local Files"),
        ];

        frame.render_widget(
            Paragraph::new(content)
                .style(bg_style())
                .wrap(Wrap { trim: false }),
            inner,
        );
    }

    fn render_help_section(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let scroll = self.help_scroll;
        let lines: Vec<Line> = self
            .help_text
            .iter()
            .map(|line| {
                if line.starts_with('#') {
                    Line::from(Span::styled(
                        &line[1..],
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(Span::styled(line, Style::default().fg(Color::White)))
                }
            })
            .collect();

        let total = lines.len();
        let visible = inner.height.saturating_sub(2) as usize;
        let max_scroll = total.saturating_sub(visible);
        let offset = scroll.min(max_scroll);

        let title = if total > visible {
            let pct = if max_scroll > 0 {
                (offset * 100) / max_scroll
            } else {
                0
            };
            let n = (pct / 10).clamp(0, 10);
            let bar: String = (0..10).map(|i| if i < n { '█' } else { '░' }).collect();
            format!(" Help {bar} ")
        } else {
            " Help ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .title_alignment(Alignment::Left)
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let visible_lines: Vec<&Line> = lines.iter().skip(offset).take(visible).collect();
        let text: Vec<Line> = visible_lines.into_iter().cloned().collect();

        let paragraph = Paragraph::new(Text::from(text))
            .block(Block::default().padding(Padding::new(2, 2, 1, 1)));
        frame.render_widget(paragraph, inner);
    }
}
