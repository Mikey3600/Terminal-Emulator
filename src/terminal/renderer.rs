//! Dirty-cell renderer.
//!
//! The renderer converts logical grid state into concrete `crossterm` drawing
//! commands. It intentionally repaints only cells marked dirty to minimize I/O,
//! because terminal writes are often the dominant cost in emulators.

use crate::terminal::screen_buffer::{Color, Grid};
use crossterm::{
    cursor::MoveTo,
    style::{
        Attribute, Color as CrosstermColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
};
use std::io::Write;

/// Renders all dirty cells and moves the visible cursor to the grid cursor.
///
/// Errors from queued operations are ignored intentionally to keep rendering
/// best-effort while the main loop remains responsive.
pub fn render(grid: &Grid, out: &mut impl Write) {
    for (row, col, cell) in grid.dirty_cells() {
        let _ = crossterm::queue!(out, MoveTo(col as u16, row as u16));
        let _ = crossterm::queue!(out, SetForegroundColor(to_crossterm_color(cell.attrs.fg)));
        let _ = crossterm::queue!(out, SetBackgroundColor(to_crossterm_color(cell.attrs.bg)));
        let _ = crossterm::queue!(
            out,
            SetAttribute(if cell.attrs.bold {
                Attribute::Bold
            } else {
                Attribute::NormalIntensity
            })
        );
        let _ = write!(out, "{}", cell.ch);
    }

    let _ = crossterm::queue!(out, MoveTo(grid.cursor_col as u16, grid.cursor_row as u16));
    let _ = out.flush();
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
