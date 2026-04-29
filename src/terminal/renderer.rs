use crate::terminal::screen_buffer::{Color, Grid};
use crossterm::{
    cursor::MoveTo,
    style::{
        Attribute, Color as CrosstermColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    QueueableCommand,
};
use std::io::Write;

pub fn render(grid: &Grid, out: &mut impl Write) -> std::io::Result<()> {
    for (row, col, cell) in grid.dirty_cells() {
        out.queue(MoveTo(col as u16, row as u16))?;
        out.queue(SetForegroundColor(to_crossterm_color(cell.attrs.fg)))?;
        out.queue(SetBackgroundColor(to_crossterm_color(cell.attrs.bg)))?;
        out.queue(SetAttribute(if cell.attrs.bold {
            Attribute::Bold
        } else {
            Attribute::NormalIntensity
        }))?;
        write!(out, "{}", cell.ch)?;
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
