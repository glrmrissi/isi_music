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
            // Sanitize remote-controlled strings at the sink: a cell must
            // never emit control chars (ANSI/OSC injection). Cells with
            // CellDiffOption::ForcedWidth are exempt: ratatui-image smuggles
            // sixel/kitty/iterm2 sequences through one cell's symbol, and
            // diff_option can only be set by render code, never by string data.
            let symbol = cell.symbol();
            if matches!(
                cell.diff_option,
                ratatui::buffer::CellDiffOption::ForcedWidth(_)
            ) {
                queue!(writer, Print(symbol))?;
                continue;
            }
            if symbol.chars().any(|c| {
                let code = c as u32;
                code < 0x20 || code == 0x7f || (0x80..0xa0).contains(&code)
            }) {
                let clean: String = symbol
                    .chars()
                    .map(|c| {
                        let code = c as u32;
                        if code < 0x20 || code == 0x7f || (0x80..0xa0).contains(&code) {
                            ' '
                        } else {
                            c
                        }
                    })
                    .collect();
                queue!(writer, Print(clean))?;
            } else {
                queue!(writer, Print(symbol))?;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::CellDiffOption;
    use std::num::NonZeroU16;

    fn draw_cells(cells: Vec<(u16, u16, Cell)>) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        {
            let mut backend = StableCrosstermBackend::new(&mut out);
            let refs: Vec<(u16, u16, &Cell)> = cells.iter().map(|(x, y, c)| (*x, *y, c)).collect();
            backend.draw(refs.into_iter()).unwrap();
            Backend::flush(&mut backend).unwrap();
        }
        out
    }

    #[test]
    fn draw_sanitizes_control_chars_in_normal_cells() {
        let mut cell = Cell::default();
        cell.set_symbol("hi\x1b]52;c;AAAA\x07\nthere");
        let out = draw_cells(vec![(0, 0, cell)]);
        let out = String::from_utf8_lossy(&out);
        assert!(!out.contains("\x1b]"), "OSC sequence must be stripped");
        assert!(out.contains("hi ]52;c;AAAA  there"), "got: {out:?}");
    }

    #[test]
    fn draw_passes_through_image_protocol_cells() {
        let mut cell = Cell::default();
        cell.set_symbol("\x1bPq#0;2;0;0;0~\x1b\\");
        cell.set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap()));
        let out = draw_cells(vec![(0, 0, cell)]);
        let out = String::from_utf8_lossy(&out);
        assert!(
            out.contains("\x1bPq#0;2;0;0;0~\x1b\\"),
            "sixel payload must reach the terminal intact, got: {out:?}"
        );
    }
}
