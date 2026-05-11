use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Api,   
    Audio,
}

impl LogLevel {
    fn label(&self) -> &'static str {
        match self {
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERR  ",
            Self::Api => "API  ",
            Self::Audio => "AUDIO",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Info => Color::Cyan,
            Self::Warn => Color::Yellow,
            Self::Error => Color::Red,
            Self::Api => Color::Green,
            Self::Audio => Color::Magenta,
        }
    }
}

#[derive(Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub elapsed: Duration,
}

#[derive(Clone, Default, Debug)]
pub struct ProcessMetrics {
    pub cpu_percent: f32,
    pub rss_mb: f32,
    pub thread_count: u32,
    pub audio_backend: String,
}

const LOG_CAPACITY: usize = 200;

#[derive(Clone)]
pub struct DebugOverlay {
    inner: Arc<Mutex<OverlayInner>>,
    start: Instant,
}

struct OverlayInner {
    logs: VecDeque<LogEntry>,
    metrics: ProcessMetrics,
    last_metric_update: Instant,
    pub visible: bool,
    prev_utime: u64,
    prev_stime: u64,
    prev_wall: Instant,
}

impl Default for DebugOverlay {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            inner: Arc::new(Mutex::new(OverlayInner {
                logs: VecDeque::with_capacity(LOG_CAPACITY),
                metrics: ProcessMetrics {
                    audio_backend: "rodio/ALSA".to_string(),
                    ..Default::default()
                },
                visible: false,
                last_metric_update: now,
                prev_utime: 0,
                prev_stime: 0,
                prev_wall: now,
            })),
            start: now,
        }
    }
}

impl DebugOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&self) -> DebugHandle {
        DebugHandle {
            inner: Arc::clone(&self.inner),
            start: self.start,
        }
    }

    pub fn log(&self, level: LogLevel, msg: impl Into<String>) {
        self.handle().log(level, msg);
    }

    pub fn log_api(&self, method: &str, url: &str, status: u16) {
        self.log(
            LogLevel::Api,
            format!("{} {}  → {}", method, url, status),
        );
    }

    pub fn log_audio(&self, msg: impl Into<String>) {
        self.log(LogLevel::Audio, msg);
    }

    pub fn update_metrics(&self) {
        let mut g = self.inner.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(g.last_metric_update) < Duration::from_millis(800) {
            return;
        }
        g.last_metric_update = now;

        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
            let fields: Vec<&str> = stat.split_whitespace().collect();
            if fields.len() > 14 {
                if let (Ok(utime), Ok(stime)) = (
                    fields[13].parse::<u64>(),
                    fields[14].parse::<u64>(),
                ) {
                    let total_jiffies =
                        (utime + stime).saturating_sub(g.prev_utime + g.prev_stime);
                    let wall_secs = now.duration_since(g.prev_wall).as_secs_f64();
                    let cpu = (total_jiffies as f64 / 100.0 / wall_secs * 100.0) as f32;
                    g.metrics.cpu_percent = cpu.clamp(0.0, 999.0);
                    g.prev_utime = utime;
                    g.prev_stime = stime;
                    g.prev_wall = now;
                }
            }
        }

        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    let kb: f32 = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    g.metrics.rss_mb = kb / 1024.0;
                }
                if line.starts_with("Threads:") {
                    g.metrics.thread_count = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
            }
        }
    }

    pub fn toggle_visible(&self) {
        let mut g = self.inner.lock().unwrap();
        g.visible = !g.visible;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let g = self.inner.lock().unwrap();

        if !g.visible {
            return;
        }

        let popup = centered_rect(80, 85, area);

        frame.render_widget(Clear, popup);

        let outer = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .title(" 󰃤 Debug Overlay  [D] fechar ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));

        let inner = outer.inner(popup);
        frame.render_widget(outer, popup);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),  
                Constraint::Length(1), 
                Constraint::Min(0),     
            ])
            .split(inner);

        self.render_metrics(frame, &g.metrics, sections[0]);

        frame.render_widget(
            Paragraph::new("─".repeat(sections[1].width as usize))
                .style(Style::default().fg(Color::DarkGray)),
            sections[1],
        );

        self.render_logs(frame, &g.logs, sections[2]);
    }

    fn render_metrics(&self, frame: &mut Frame, m: &ProcessMetrics, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);

        let proc_lines = vec![
            Line::from(vec![
                Span::styled("CPU   ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.1}%", m.cpu_percent),
                    Style::default()
                        .fg(if m.cpu_percent > 50.0 {
                            Color::Red
                        } else {
                            Color::Green
                        })
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("RSS   ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.1} MB", m.rss_mb),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::styled("Threads ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", m.thread_count),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::styled("Backend ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    m.audio_backend.clone(),
                    Style::default().fg(Color::Magenta),
                ),
            ]),
        ];

        frame.render_widget(Paragraph::new(proc_lines), cols[0]);

    }

    fn render_logs<'a>(&self, frame: &mut Frame, logs: &VecDeque<LogEntry>, area: Rect) {
        let visible = area.height as usize;
        let skip = logs.len().saturating_sub(visible);

        let items: Vec<ListItem> = logs
            .iter()
            .skip(skip)
            .map(|e| {
                let ts = format!("{:>6.1}s ", e.elapsed.as_secs_f64());
                ListItem::new(Line::from(vec![
                    Span::styled(ts, Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("[{}] ", e.level.label()),
                        Style::default()
                            .fg(e.level.color())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(e.message.clone()),
                ]))
            })
            .collect();

        frame.render_widget(List::new(items), area);
    }
}

#[derive(Clone)]
pub struct DebugHandle {
    inner: Arc<Mutex<OverlayInner>>,
    start: Instant,
}

impl DebugHandle {
    pub fn log(&self, level: LogLevel, msg: impl Into<String>) {
        let elapsed = self.start.elapsed();
        let mut g = self.inner.lock().unwrap();
        if g.logs.len() >= LOG_CAPACITY {
            g.logs.pop_front();
        }
        g.logs.push_back(LogEntry {
            level,
            message: msg.into(),
            elapsed,
        });
    }

    pub fn log_api(&self, method: &str, url: &str, status: u16) {
        self.log(LogLevel::Api, format!("{} {}  → {}", method, url, status));
    }

    pub fn log_audio(&self, msg: impl Into<String>) {
        self.log(LogLevel::Audio, msg);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_overlay_creation() {
        let debug = DebugOverlay::new();
        assert!(!debug.visible);
    }

    #[test]
    fn test_logging() {
        let debug = DebugOverlay::new();
        debug.log(LogLevel::Info, "test message");
        debug.log_api("GET", "/api/track", 200);
    }

    #[test]
    fn test_metrics_update() {
        let debug = DebugOverlay::new();
        debug.update_metrics();
        debug.update_eq_state(true, [0.0; 10]);
    }
}