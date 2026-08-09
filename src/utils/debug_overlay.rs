use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
};

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
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
    process_system: System,
    process_pid: Pid,
}

#[cfg(windows)]
fn process_thread_count(pid: u32) -> u32 {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32,
        TH32CS_SNAPTHREAD,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return 0;
        }

        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut count = 0;
        if Thread32First(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32OwnerProcessID == pid {
                    count += 1;
                }
                if Thread32Next(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        count
    }
}

#[cfg(not(windows))]
fn process_thread_count(_pid: u32) -> u32 {
    0
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
                process_system: System::new(),
                process_pid: Pid::from_u32(std::process::id()),
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

    pub fn update_metrics(&self) {
        let mut g = self.inner.lock().unwrap();
        let now = Instant::now();

        if now.duration_since(g.last_metric_update) < Duration::from_millis(800) {
            return;
        }
        g.last_metric_update = now;

        let pid = g.process_pid;
        g.process_system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_tasks(),
        );

        let process_metrics = g.process_system.process(pid).map(|process| {
            let cpu_count = std::thread::available_parallelism()
                .map(|n| n.get() as f32)
                .unwrap_or(1.0);
            let cpu_percent = (process.cpu_usage() / cpu_count).clamp(0.0, 999.0);
            let rss_mb = process.memory() as f32 / (1024.0 * 1024.0);
            let thread_count = process
                .tasks()
                .map(|tasks| tasks.len() as u32)
                .unwrap_or_else(|| process_thread_count(pid.as_u32()));
            (cpu_percent, rss_mb, thread_count)
        });
        if let Some((cpu_percent, rss_mb, thread_count)) = process_metrics {
            g.metrics.cpu_percent = cpu_percent;
            g.metrics.rss_mb = rss_mb;
            g.metrics.thread_count = thread_count;
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

        let height = 30u16;
        let debug_area = Rect {
            x: area.x + 1,
            y: area.y + area.height.saturating_sub(height),
            width: area.width.saturating_sub(2),
            height: height.min(area.height),
        };

        frame.render_widget(Clear, debug_area);

        let outer = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .title(" Debug Overlay [D] Close ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray));

        let inner = outer.inner(debug_area);
        frame.render_widget(outer, debug_area);

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
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
                Span::styled(m.audio_backend.clone(), Style::default().fg(Color::Magenta)),
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
}

#[cfg(test)]
#[path = "../../tests/utils/debug_overlay.rs"]
mod tests;
