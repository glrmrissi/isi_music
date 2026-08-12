// TODO: modularize this file (~560 lines) into smaller modules
use crate::config::AppConfig;
use crate::settings::Settings;
use crate::utils::cache::{CacheManager, CacheStats};
use crate::utils::theme::Theme;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use std::sync::{Arc, Mutex};

use super::UiState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsAction {
    None,
    Close,
    ToggleItem,
    ClearAllCache,
    CleanupExpired,
    RefreshStats,
    RefreshPlaylists,
    SetupSpotify,
    SetupLastfm,
    EditMusicDir,
    SaveMusicDir,
}

pub struct SettingsPanel {
    pub visible: bool,
    pub focused_section: SettingsSection,
    pub selected_item: usize,
    pub cache_manager: CacheManager,
    pub settings: Arc<Mutex<Settings>>,
    pub config: AppConfig,
    pub cache_stats: Option<CacheStats>,
    pub loading: bool,
    pub help_text: Vec<String>,
    pub help_scroll: usize,
    pub music_dir_editing: bool,
    pub music_dir_input: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsSection {
    General,
    Account,
    Cache,
    QuickAccess,
    Help,
}

const SECTIONS: &[SettingsSection] = &[
    SettingsSection::General,
    SettingsSection::Account,
    SettingsSection::Cache,
    SettingsSection::QuickAccess,
    SettingsSection::Help,
];

fn bg_style(theme: &Theme) -> Style {
    Style::default().bg(theme.background)
}

fn section_block(title: &str, theme: &Theme) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_subtle))
        .style(bg_style(theme))
}

impl SettingsPanel {
    pub fn new(cache_manager: CacheManager, settings: Arc<Mutex<Settings>>) -> Self {
        let config = settings
            .lock()
            .map(|guard| guard.config.clone())
            .unwrap_or_default();
        Self {
            visible: false,
            focused_section: SettingsSection::General,
            selected_item: 0,
            cache_manager,
            settings,
            config,
            cache_stats: None,
            loading: false,
            help_text: Vec::new(),
            help_scroll: 0,
            music_dir_editing: false,
            music_dir_input: String::new(),
        }
    }

    pub fn save_config(&self) {
        if let Ok(mut guard) = self.settings.lock() {
            guard.config = self.config.clone();
            guard.mark_dirty();
        }
        if let Ok(guard) = self.settings.lock() {
            let _ = guard.save();
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
            SettingsSection::General => {
                #[cfg(all(feature = "album-art", feature = "palette"))]
                {
                    8
                }
                #[cfg(all(feature = "album-art", not(feature = "palette")))]
                {
                    7
                }
                #[cfg(not(feature = "album-art"))]
                {
                    6
                }
            }
            SettingsSection::Account => 4,
            SettingsSection::Cache => 8,
            SettingsSection::QuickAccess => 1,
            SettingsSection::Help => 1,
        }
    }

    pub fn handle_key(&mut self, code: KeyCode) -> SettingsAction {
        // Music dir inline text input mode takes priority
        if self.music_dir_editing {
            return match code {
                KeyCode::Esc => {
                    self.music_dir_editing = false;
                    self.music_dir_input.clear();
                    SettingsAction::None
                }
                KeyCode::Enter => {
                    let path = self.music_dir_input.trim().to_string();
                    let path = if cfg!(windows) {
                        path.replace('\\', "/")
                    } else {
                        path
                    };
                    if !path.is_empty() {
                        self.config.local.music_dir = Some(path);
                    } else {
                        self.config.local.music_dir = None;
                    }
                    self.save_config();
                    self.music_dir_editing = false;
                    self.music_dir_input.clear();
                    SettingsAction::SaveMusicDir
                }
                KeyCode::Backspace => {
                    self.music_dir_input.pop();
                    SettingsAction::None
                }
                KeyCode::Char(c) => {
                    self.music_dir_input.push(c);
                    SettingsAction::None
                }
                _ => SettingsAction::None,
            };
        }

        if self.focused_section == SettingsSection::Help {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                    return SettingsAction::None;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                    return SettingsAction::None;
                }
                KeyCode::Esc => return SettingsAction::Close,
                _ => {}
            }
        }

        match code {
            KeyCode::Esc => SettingsAction::Close,
            KeyCode::Up => {
                if self.selected_item == 0 {
                    self.selected_item = self.items_in_section().saturating_sub(1);
                } else {
                    self.selected_item -= 1;
                }
                SettingsAction::None
            }
            KeyCode::Down => {
                self.selected_item = (self.selected_item + 1) % self.items_in_section().max(1);
                SettingsAction::None
            }
            KeyCode::Left => {
                self.navigate_sections(true, false);
                self.selected_item = 0;
                SettingsAction::None
            }
            KeyCode::Right => {
                self.navigate_sections(false, true);
                self.selected_item = 0;
                SettingsAction::None
            }
            KeyCode::Tab => {
                self.navigate_sections(false, true);
                self.selected_item = 0;
                SettingsAction::None
            }
            KeyCode::Enter => match self.focused_section {
                SettingsSection::Cache => match self.selected_item {
                    4 => SettingsAction::ClearAllCache,
                    5 => SettingsAction::CleanupExpired,
                    6 => SettingsAction::RefreshStats,
                    7 => SettingsAction::RefreshPlaylists,
                    _ => SettingsAction::None,
                },
                SettingsSection::Account => match self.selected_item {
                    0 => SettingsAction::SetupSpotify,
                    1 => SettingsAction::SetupLastfm,
                    2 => {
                        self.music_dir_editing = true;
                        self.music_dir_input =
                            self.config.local.music_dir.clone().unwrap_or_default();
                        SettingsAction::EditMusicDir
                    }
                    3 => SettingsAction::ToggleItem,
                    _ => SettingsAction::None,
                },
                _ => SettingsAction::ToggleItem,
            },
            KeyCode::Char('c') | KeyCode::Char('C')
                if self.focused_section == SettingsSection::Cache =>
            {
                SettingsAction::ClearAllCache
            }
            KeyCode::Char('r') | KeyCode::Char('R')
                if self.focused_section == SettingsSection::Cache =>
            {
                SettingsAction::RefreshStats
            }
            _ => SettingsAction::None,
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

    pub fn render(
        &self,
        frame: &mut Frame,
        state: &UiState,
        theme: &Theme,
        autoplay_enabled: bool,
    ) {
        if !self.visible {
            return;
        }

        let bg = Style::default().bg(theme.background);
        let border_color = theme.border_subtle;
        let accent_color = theme.accent_color;
        let muted_color = theme.text_secondary;

        let area = frame.area();

        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .split(area);

        let content_area = popup_layout[1];
        let footer_area = popup_layout[2];

        // Clean backdrop
        frame.render_widget(Clear, content_area);
        frame.render_widget(Paragraph::new("").style(bg), content_area);

        let block = Block::default()
            .title(" Settings ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(bg);

        let inner_area = block.inner(content_area);
        frame.render_widget(block, content_area);

        let sections_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(0)])
            .split(inner_area);

        let sections_area = sections_layout[0];
        let content_area = sections_layout[1];

        // Clean backdrop for sidebar and content
        frame.render_widget(Clear, sections_area);
        frame.render_widget(Paragraph::new("").style(bg), sections_area);
        frame.render_widget(Clear, content_area);
        frame.render_widget(Paragraph::new("").style(bg), content_area);

        self.render_sections(frame, sections_area, theme);
        self.render_content(frame, state, content_area, theme, autoplay_enabled);

        // Clean footer
        frame.render_widget(Clear, footer_area);
        frame.render_widget(Paragraph::new("").style(bg), footer_area);

        let footer_text = Line::from(vec![
            Span::styled(" Arrows: Navigate ", Style::default().fg(muted_color)),
            Span::styled(" Tab: Sections ", Style::default().fg(muted_color)),
            Span::styled(" Enter: Select ", Style::default().fg(accent_color)),
            Span::styled(" Esc: Close ", Style::default().fg(muted_color)),
        ]);

        frame.render_widget(
            Paragraph::new(footer_text)
                .style(bg)
                .alignment(Alignment::Center),
            footer_area,
        );
    }

    fn render_sections(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let accent_color = theme.accent_color;
        let text_color = theme.text_primary;
        let bg_color = theme.background;
        let border_color = theme.border_subtle;

        let items: Vec<ListItem> = SECTIONS
            .iter()
            .map(|section| {
                let label = match section {
                    SettingsSection::General => "Features",
                    SettingsSection::Account => "Account",
                    SettingsSection::Cache => "Cache",
                    SettingsSection::QuickAccess => "Quick Access",
                    SettingsSection::Help => "Help",
                };
                let is_focused = self.focused_section == *section;
                let style = if is_focused {
                    Style::default()
                        .fg(accent_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(text_color)
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
                    .border_style(Style::default().fg(border_color))
                    .style(Style::default().bg(bg_color)),
            )
            .style(Style::default().bg(bg_color))
            .highlight_style(
                Style::default()
                    .bg(theme.highlight_bg)
                    .fg(accent_color)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(" ");

        frame.render_stateful_widget(list, area, &mut list_state);
    }

    fn render_content(
        &self,
        frame: &mut Frame,
        state: &UiState,
        area: Rect,
        theme: &Theme,
        autoplay_enabled: bool,
    ) {
        match self.focused_section {
            SettingsSection::General => {
                self.render_general_section(frame, state, area, theme, autoplay_enabled)
            }
            SettingsSection::Account => self.render_account_section(frame, state, area, theme),
            SettingsSection::Cache => self.render_cache_section(frame, area, theme),
            SettingsSection::QuickAccess => self.render_quick_access_section(frame, area, theme),
            SettingsSection::Help => self.render_help_section(frame, area, theme),
        }
    }

    fn render_item_list(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        items: &[(&str, &str, bool)],
        theme: &Theme,
    ) {
        let accent_color = theme.accent_color;
        let text_color = theme.text_primary;
        let enabled_color = theme.success;
        let disabled_color = theme.error;

        let block = section_block(title, theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(i, &(label, custom_status, enabled))| {
                let is_selected = i == self.selected_item;
                let prefix = if is_selected { " " } else { " " };
                let (status_str, status_color) = if !custom_status.is_empty() {
                    let color = if enabled {
                        enabled_color
                    } else if custom_status.starts_with("Pending") {
                        theme.warning
                    } else {
                        disabled_color
                    };
                    (custom_status, color)
                } else if enabled {
                    ("On", enabled_color)
                } else {
                    ("Off", disabled_color)
                };
                let line_style = if is_selected {
                    Style::default()
                        .fg(accent_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(text_color)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}{}: ", prefix, label), line_style),
                    Span::styled(status_str, Style::default().fg(status_color)),
                ]))
            })
            .collect();

        let list = List::new(list_items)
            .style(bg_style(theme))
            .highlight_style(Style::default().bg(theme.highlight_bg));

        let mut list_state =
            ratatui::widgets::ListState::default().with_selected(Some(self.selected_item));

        frame.render_stateful_widget(list, inner, &mut list_state);
    }

    fn render_general_section(
        &self,
        frame: &mut Frame,
        state: &UiState,
        area: Rect,
        theme: &Theme,
        autoplay_enabled: bool,
    ) {
        let mut items = vec![];
        #[cfg(feature = "album-art")]
        items.push(("Cover Images", "", state.show_album_art));
        items.push(("Lyrics Fetching", "", self.config.enable_lyrics()));
        items.push(("Visualizer Display", "", state.show_visualizer));
        items.push(("Compact Mode", "", state.compact_mode));
        items.push(("Breadcrumb", "", state.show_breadcrumb));

        let lastfm_text = if state.lastfm_connected {
            "Connected"
        } else if state.lastfm_pending {
            "Pending (Press Enter)"
        } else {
            "Not Configured"
        };
        items.push(("Last.fm Scrobbling", lastfm_text, state.lastfm_connected));

        items.push(("Autoplay", "", autoplay_enabled));

        #[cfg(all(feature = "album-art", feature = "palette"))]
        items.push((
            "Reactive Theme (album colors)",
            "",
            state.reactive_theme_enabled,
        ));

        self.render_item_list(frame, area, "General", &items, theme);
    }

    fn render_account_section(
        &self,
        frame: &mut Frame,
        state: &UiState,
        area: Rect,
        theme: &Theme,
    ) {
        let accent_color = theme.accent_color;
        let text_color = theme.text_primary;
        let muted_color = theme.text_secondary;
        let enabled_color = theme.success;
        let disabled_color = theme.error;

        let block = section_block("Account & Integrations", theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let spotify_status = if state.spotify_authenticated {
            ("Connected", enabled_color)
        } else {
            ("Not configured", disabled_color)
        };

        let lastfm_status = if state.lastfm_connected {
            ("Connected", enabled_color)
        } else if state.lastfm_pending {
            ("Pending", theme.warning)
        } else {
            ("Not configured", disabled_color)
        };

        let music_dir_status: String = if self.music_dir_editing {
            format!("Editing: {}", self.music_dir_input)
        } else {
            self.config
                .local
                .music_dir
                .clone()
                .unwrap_or_else(|| "Not set".to_string())
        };

        let discord_enabled = self.config.discord.enabled.unwrap_or(false);
        let discord_status = if discord_enabled {
            ("Enabled", enabled_color)
        } else {
            ("Disabled", disabled_color)
        };

        let rows: Vec<(&str, &str, Color)> = vec![
            ("Spotify", spotify_status.0, spotify_status.1),
            ("Last.fm", lastfm_status.0, lastfm_status.1),
            (
                "Music dir",
                &music_dir_status,
                if self.music_dir_editing {
                    accent_color
                } else {
                    text_color
                },
            ),
            ("Discord", discord_status.0, discord_status.1),
        ];

        let list_items: Vec<ListItem> = rows
            .iter()
            .enumerate()
            .map(|(i, (label, status, color))| {
                let is_selected = i == self.selected_item;
                let prefix = if is_selected { " " } else { " " };
                let line_style = if is_selected {
                    Style::default()
                        .fg(accent_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(text_color)
                };
                let mut spans = vec![
                    Span::styled(format!("{}{}: ", prefix, label), line_style),
                    Span::styled(*status, Style::default().fg(*color)),
                ];
                if is_selected && !self.music_dir_editing {
                    let hint = match i {
                        0 => "  (Enter: setup instructions)",
                        1 => "  (Enter: setup instructions)",
                        2 => "  (Enter: edit path)",
                        3 => "  (Enter: toggle)",
                        _ => "",
                    };
                    spans.push(Span::styled(hint, Style::default().fg(muted_color)));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(list_items)
            .style(bg_style(theme))
            .highlight_style(Style::default().bg(theme.highlight_bg));

        let mut list_state =
            ratatui::widgets::ListState::default().with_selected(Some(self.selected_item));

        frame.render_stateful_widget(list, inner, &mut list_state);

        // Show a hint at the bottom when editing music dir
        if self.music_dir_editing {
            let hint = " Type path, Enter to save, Esc to cancel ";
            frame.render_widget(
                Paragraph::new(hint)
                    .style(Style::default().fg(accent_color).bg(theme.background))
                    .alignment(Alignment::Center),
                area,
            );
        }
    }

    fn render_cache_section(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let accent_color = theme.accent_color;
        let text_color = theme.text_primary;
        let muted_color = theme.text_secondary;

        let block = section_block("Cache Management", theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.loading {
            let loading_text = Paragraph::new("Loading cache statistics...")
                .alignment(Alignment::Center)
                .style(Style::default().fg(accent_color).bg(theme.background));
            frame.render_widget(loading_text, inner);
            return;
        }

        let mut rows: Vec<ListItem> = Vec::new();

        let stats_items = if let Some(stats) = &self.cache_stats {
            vec![
                format!("Search Cache: {} entries", stats.search_cache_entries),
                format!("Library Cache: {} entries", stats.library_cache_entries),
                format!("Lyrics Cache: {} entries", stats.lyrics_cache_entries),
            ]
        } else {
            vec![
                "Search Cache: N/A".into(),
                "Library Cache: N/A".into(),
                "Lyrics Cache: N/A".into(),
            ]
        };

        for (i, line) in stats_items.iter().enumerate() {
            let is_sel = i == self.selected_item && i < 3;
            let prefix = if is_sel { " " } else { " " };
            let style = if is_sel {
                Style::default()
                    .fg(accent_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(text_color)
            };
            rows.push(ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, line),
                style,
            ))));
        }

        rows.push(ListItem::new(Line::from("")));

        let actions = vec![
            ("Clear All Caches", "c"),
            ("Cleanup Expired", "f"),
            ("Refresh Stats", "r"),
            ("Refresh Playlists", "p"),
        ];

        for (i, (label, key)) in actions.iter().enumerate() {
            let idx = i + 4;
            let is_sel = idx == self.selected_item && idx >= 4;
            let prefix = if is_sel { " " } else { " " };
            let style = if is_sel {
                Style::default()
                    .fg(accent_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(muted_color)
            };
            rows.push(ListItem::new(Line::from(Span::styled(
                format!("{}{} [{}]", prefix, label, key),
                style,
            ))));
        }

        let list = List::new(rows)
            .style(bg_style(theme))
            .highlight_style(Style::default().bg(theme.highlight_bg));

        let mut list_state =
            ratatui::widgets::ListState::default().with_selected(Some(self.selected_item));

        frame.render_stateful_widget(list, inner, &mut list_state);
    }

    fn render_quick_access_section(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = section_block("Quick Access Setup", theme);
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
                .style(bg_style(theme))
                .wrap(Wrap { trim: false }),
            inner,
        );
    }

    fn render_help_section(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Clean backdrop
        let backdrop_style = Style::default().bg(theme.background);
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new("").style(backdrop_style), area);

        let scroll = self.help_scroll;

        // Use theme colors
        let accent_color = theme.accent_color;
        let header_color = theme.warning;
        let text_color = theme.text_primary;
        let border_color = theme.border_subtle;

        let lines: Vec<Line> = self
            .help_text
            .iter()
            .map(|line| {
                if line.starts_with('#') {
                    Line::from(Span::styled(
                        &line[1..],
                        Style::default()
                            .fg(header_color)
                            .add_modifier(Modifier::BOLD),
                    ))
                } else if line.contains("  ")
                    && line
                        .chars()
                        .next()
                        .map(|c| !c.is_whitespace())
                        .unwrap_or(false)
                {
                    let parts: Vec<&str> = line.splitn(2, "  ").collect();
                    if parts.len() == 2 {
                        Line::from(vec![
                            Span::styled(
                                parts[0],
                                Style::default()
                                    .fg(accent_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("  {}", parts[1]),
                                Style::default().fg(text_color),
                            ),
                        ])
                    } else {
                        Line::from(Span::styled(line, Style::default().fg(text_color)))
                    }
                } else if line.trim().is_empty() {
                    Line::from("")
                } else {
                    Line::from(Span::styled(line, Style::default().fg(text_color)))
                }
            })
            .collect();

        let total = lines.len();
        let visible = area.height.saturating_sub(3) as usize;
        let max_scroll = total.saturating_sub(visible);
        let offset = scroll.min(max_scroll);

        // Clean title with minimal progress indicator
        let title = if total > visible {
            let pct = if max_scroll > 0 {
                (offset * 100) / max_scroll
            } else {
                0
            };
            format!(" Help [{}%] ", pct)
        } else {
            " Help ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .title_alignment(Alignment::Center)
            .title_style(
                Style::default()
                    .fg(header_color)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(theme.background));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Clear the inner area before rendering to prevent scroll accumulation
        frame.render_widget(Clear, inner);
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(theme.background)),
            inner,
        );

        let text: Vec<Line> = lines.iter().skip(offset).take(visible).cloned().collect();

        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().bg(theme.background))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }
}
