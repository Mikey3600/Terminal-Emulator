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
