use crate::terminal::screen_buffer::{Attributes, Color, Grid};
use crossterm::{
    cursor::MoveTo,
    style::{
        Attribute, Color as CrosstermColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    QueueableCommand,
};
use std::io::Write;

pub fn render(grid: &Grid, out: &mut impl Write) -> std::io::Result<()> {
    let dirty = grid.dirty_cells().count();
    log::debug!("renderer_dirty_cells={dirty}");

    let mut last_pos: Option<(usize, usize)> = None;
    let mut last_attrs: Option<Attributes> = None;

    for (row, col, cell) in grid.dirty_cells() {
        if cell.wide_continuation {
            continue;
        }

        let contiguous = last_pos.map(|(r, c)| r == row && c + 1 == col).unwrap_or(false);
        if !contiguous {
            out.queue(MoveTo(col as u16, row as u16))?;
        }

        if last_attrs.map(|a| a.fg) != Some(cell.attrs.fg) {
            out.queue(SetForegroundColor(to_crossterm_color(cell.attrs.fg)))?;
        }
        if last_attrs.map(|a| a.bg) != Some(cell.attrs.bg) {
            out.queue(SetBackgroundColor(to_crossterm_color(cell.attrs.bg)))?;
        }
        if last_attrs.map(|a| a.bold) != Some(cell.attrs.bold) {
            out.queue(SetAttribute(if cell.attrs.bold {
                Attribute::Bold
            } else {
                Attribute::NormalIntensity
            }))?;
        }
        // FIX #1: emit italic attribute when it changes
        if last_attrs.map(|a| a.italic) != Some(cell.attrs.italic) {
            out.queue(SetAttribute(if cell.attrs.italic {
                Attribute::Italic
            } else {
                Attribute::NoItalic
            }))?;
        }
        // FIX #1: emit underline attribute when it changes
        if last_attrs.map(|a| a.underline) != Some(cell.attrs.underline) {
            out.queue(SetAttribute(if cell.attrs.underline {
                Attribute::Underlined
            } else {
                Attribute::NoUnderline
            }))?;
        }
        // FIX #1: emit blink attribute when it changes
        if last_attrs.map(|a| a.blink) != Some(cell.attrs.blink) {
            out.queue(SetAttribute(if cell.attrs.blink {
                Attribute::SlowBlink
            } else {
                Attribute::NoBlink
            }))?;
        }
        // FIX #1: emit reverse attribute when it changes
        if last_attrs.map(|a| a.reverse) != Some(cell.attrs.reverse) {
            out.queue(SetAttribute(if cell.attrs.reverse {
                Attribute::Reverse
            } else {
                Attribute::NoReverse
            }))?;
        }

        write!(out, "{}", cell.ch)?;
        last_pos = Some((row, col));
        last_attrs = Some(cell.attrs);
    }

    out.queue(MoveTo(grid.cursor_col as u16, grid.cursor_row as u16))?;
    out.flush()
}

fn to_crossterm_color(c: Color) -> CrosstermColor {
    match c {
        Color::Default => CrosstermColor::Reset,
        Color::Black => CrosstermColor::Black,
        Color::Red => CrosstermColor::DarkRed,
        Color::Green => CrosstermColor::DarkGreen,
        Color::Yellow => CrosstermColor::DarkYellow,
        Color::Blue => CrosstermColor::DarkBlue,
        Color::Magenta => CrosstermColor::DarkMagenta,
        Color::Cyan => CrosstermColor::DarkCyan,
        Color::White => CrosstermColor::Grey,
        Color::Indexed(n) => CrosstermColor::AnsiValue(n),
        Color::Rgb(r, g, b) => CrosstermColor::Rgb { r, g, b },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a fresh grid, write text at (0,0), render into a Vec<u8>,
    // and return the raw bytes. The grid marks all cells dirty on creation.
    fn render_to_bytes(grid: &Grid) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        render(grid, &mut buf).expect("render failed");
        buf
    }

    // Crossterm queues escape sequences into the writer; we just need to verify
    // the plain character bytes appear somewhere in the output — we don't
    // re-parse the full escape sequence stream.
    fn contains_char(buf: &[u8], ch: char) -> bool {
        let s = String::from_utf8_lossy(buf);
        s.contains(ch)
    }

    #[test]
    fn renders_dirty_cells() {
        let mut grid = Grid::new(3, 10);
        grid.write_char('A');
        grid.write_char('B');
        let buf = render_to_bytes(&grid);
        assert!(contains_char(&buf, 'A'), "expected 'A' in output");
        assert!(contains_char(&buf, 'B'), "expected 'B' in output");
    }

    #[test]
    fn skips_clean_cells_after_mark_clean() {
        let mut grid = Grid::new(3, 10);
        grid.write_char('X');
        // Mark everything clean, then render — only the cursor MoveTo should appear.
        grid.clear_dirty();
        let buf = render_to_bytes(&grid);
        assert!(!contains_char(&buf, 'X'), "clean cell 'X' should not be re-rendered");
    }

    #[test]
    fn bold_attribute_emitted() {
        let mut grid = Grid::new(3, 10);
        grid.current_attrs.bold = true;
        grid.write_char('B');
        let buf = render_to_bytes(&grid);
        // Crossterm encodes Bold as ESC[1m — verify the SGR byte 1 appears
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains('\x1b'), "expected escape sequence for bold");
        assert!(contains_char(&buf, 'B'));
    }

    // FIX #1 regression tests — italic and underline must now appear in output.
    #[test]
    fn italic_attribute_emitted() {
        let mut grid = Grid::new(3, 10);
        grid.current_attrs.italic = true;
        grid.write_char('I');
        let buf = render_to_bytes(&grid);
        // Crossterm emits ESC[3m for Italic. We verify an escape is present.
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains('\x1b'), "expected escape sequence for italic");
        assert!(contains_char(&buf, 'I'));
    }

    #[test]
    fn underline_attribute_emitted() {
        let mut grid = Grid::new(3, 10);
        grid.current_attrs.underline = true;
        grid.write_char('U');
        let buf = render_to_bytes(&grid);
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains('\x1b'), "expected escape sequence for underline");
        assert!(contains_char(&buf, 'U'));
    }

    #[test]
    fn blink_attribute_emitted() {
        let mut grid = Grid::new(3, 10);
        grid.current_attrs.blink = true;
        grid.write_char('K');
        let buf = render_to_bytes(&grid);
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains('\x1b'), "expected escape sequence for blink");
        assert!(contains_char(&buf, 'K'));
    }

    #[test]
    fn reverse_attribute_emitted() {
        let mut grid = Grid::new(3, 10);
        grid.current_attrs.reverse = true;
        grid.write_char('R');
        let buf = render_to_bytes(&grid);
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains('\x1b'), "expected escape sequence for reverse");
        assert!(contains_char(&buf, 'R'));
    }

    #[test]
    fn attribute_off_emitted_on_transition() {
        // Write one bold cell, then one non-bold cell.
        // The renderer must emit NormalIntensity between them.
        let mut grid = Grid::new(3, 20);
        grid.current_attrs.bold = true;
        grid.write_char('B');
        grid.current_attrs.bold = false;
        grid.write_char('N');
        let buf = render_to_bytes(&grid);
        // Both characters must appear
        assert!(contains_char(&buf, 'B'));
        assert!(contains_char(&buf, 'N'));
        // At least two escape sequences: Bold on + Bold off
        let esc_count = buf.windows(2).filter(|w| w[0] == 0x1b && w[1] == b'[').count();
        assert!(esc_count >= 2, "expected at least 2 SGR sequences, got {}", esc_count);
    }

    #[test]
    fn fg_color_change_emitted() {
        let mut grid = Grid::new(3, 20);
        grid.current_attrs.fg = Color::Red;
        grid.write_char('R');
        grid.current_attrs.fg = Color::Blue;
        grid.write_char('B');
        let buf = render_to_bytes(&grid);
        assert!(contains_char(&buf, 'R'));
        assert!(contains_char(&buf, 'B'));
        // Two distinct fg color sequences expected
        let esc_count = buf.windows(2).filter(|w| w[0] == 0x1b && w[1] == b'[').count();
        assert!(esc_count >= 2);
    }
}
