use std::io::{self, Write};

use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, IntoCrossterm, WindowSize},
    buffer::Cell,
    crossterm::{
        cursor::MoveTo,
        queue,
        style::{Attribute, Colors, Print, SetAttribute, SetColors, SetUnderlineColor},
    },
    layout::{Position, Size},
    style::Modifier,
};

pub struct StableCrosstermBackend<W: Write> {
    inner: CrosstermBackend<W>,
}

impl<W: Write> StableCrosstermBackend<W> {
    pub const fn new(writer: W) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
        }
    }
}

impl<W: Write> Write for StableCrosstermBackend<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.writer_mut().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.writer_mut().flush()
    }
}

impl<W: Write> Backend for StableCrosstermBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let writer = self.inner.writer_mut();
        let modifiers = [
            (Modifier::BOLD, Attribute::Bold),
            (Modifier::DIM, Attribute::Dim),
            (Modifier::ITALIC, Attribute::Italic),
            (Modifier::UNDERLINED, Attribute::Underlined),
            (Modifier::SLOW_BLINK, Attribute::SlowBlink),
            (Modifier::RAPID_BLINK, Attribute::RapidBlink),
            (Modifier::REVERSED, Attribute::Reverse),
            (Modifier::HIDDEN, Attribute::Hidden),
            (Modifier::CROSSED_OUT, Attribute::CrossedOut),
        ];

        for (x, y, cell) in content {
            queue!(
                writer,
                MoveTo(x, y),
                SetAttribute(Attribute::Reset),
                SetColors(Colors::new(
                    cell.fg.into_crossterm(),
                    cell.bg.into_crossterm(),
                )),
                SetUnderlineColor(cell.underline_color.into_crossterm()),
            )?;
            for (modifier, attribute) in modifiers {
                if cell.modifier.contains(modifier) {
                    queue!(writer, SetAttribute(attribute))?;
                }
            }
            queue!(writer, Print(cell.symbol()))?;
        }

        queue!(
            writer,
            SetColors(Colors::new(
                ratatui::crossterm::style::Color::Reset,
                ratatui::crossterm::style::Color::Reset,
            )),
            SetUnderlineColor(ratatui::crossterm::style::Color::Reset),
            SetAttribute(Attribute::Reset),
        )
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        Backend::hide_cursor(&mut self.inner)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Backend::show_cursor(&mut self.inner)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Backend::get_cursor_position(&mut self.inner)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        Backend::set_cursor_position(&mut self.inner, position)
    }

    fn clear(&mut self) -> io::Result<()> {
        Backend::clear(&mut self.inner)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        Backend::clear_region(&mut self.inner, clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        Backend::size(&self.inner)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Backend::window_size(&mut self.inner)
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}
