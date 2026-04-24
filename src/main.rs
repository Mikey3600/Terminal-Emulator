// File: src/main.rs
// Final: wires all modules together into a working terminal emulator

mod pty;
mod grid;
mod parser;
mod input;
mod buffer;
mod config;

use pty::{spawn_shell, write_to_pty, TermSize};
use grid::Grid;
use parser::Parser;
use buffer::RingBuffer;
use config::Config;
use crossterm::{
    cursor::MoveTo,
    style::{Color as CColor, SetForegroundColor, SetBackgroundColor, Attribute, SetAttribute},
    terminal::{Clear, ClearType},
    ExecutableCommand,
};
use std::io::{stdout, Write};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Initialise logger so Config::load() log::info!/warn! calls are visible
    env_logger::init();

    // Load config — falls back to defaults if no file exists
    let cfg = Config::load();

    // Spawn the shell process connected to a PTY
    let master = Arc::new(spawn_shell(TermSize {
        rows: cfg.rows,
        cols: cfg.cols,
    })
    .expect("Failed to spawn shell"));

    // Initialize screen grid and ANSI parser
    let mut grid = Grid::new(cfg.rows as usize, cfg.cols as usize);
    let mut parser = Parser::new();

    // Scrollback buffer — stores last 1000 rows
    let mut _scrollback: RingBuffer<Vec<char>> = RingBuffer::new(1000);

    // Start the keyboard input task
    let mut input_rx = input::spawn_input_task();

    let mut out = stdout();

    // Clear the screen before we start
    out.execute(Clear(ClearType::All)).unwrap();
    out.execute(MoveTo(0, 0)).unwrap();

    loop {
        tokio::select! {
            // PTY output — shell wrote something
            result = tokio::task::spawn_blocking({
                let master = Arc::clone(&master);
                move || {
                    let mut buf = [0u8; 4096];
                    pty::read_from_pty(&master, &mut buf).map(|n| (buf, n))
                }
            }) => {
                if let Ok(Ok((buf, n))) = result {
                    if n == 0 { break; }
                    parser.feed(&buf[..n], &mut grid);
                    render(&grid, &mut out);
                    grid.clear_dirty();
                }
            }

            // Keyboard input — user pressed a key
            Some(input::InputBytes(bytes)) = input_rx.recv() => {
                write_to_pty(&master, &bytes).ok();
            }
        }
    }

    input::restore_terminal();
}

/// Render only the dirty cells to the real terminal using crossterm.
fn render(grid: &Grid, out: &mut impl Write) {
    for (row, col, cell) in grid.dirty_cells() {
        // Move cursor to cell position
        let _ = crossterm::queue!(out, MoveTo(col as u16, row as u16));

        // Apply color
        let fg = to_crossterm_color(cell.attrs.fg);
        let bg = to_crossterm_color(cell.attrs.bg);
        let _ = crossterm::queue!(out, SetForegroundColor(fg));
        let _ = crossterm::queue!(out, SetBackgroundColor(bg));

        // Apply bold
        if cell.attrs.bold {
            let _ = crossterm::queue!(out, SetAttribute(Attribute::Bold));
        } else {
            let _ = crossterm::queue!(out, SetAttribute(Attribute::NormalIntensity));
        }

        // Write the character
        let _ = write!(out, "{}", cell.ch);
    }

    // Move real cursor to match grid cursor
    let _ = crossterm::queue!(
        out,
        MoveTo(grid.cursor_col as u16, grid.cursor_row as u16)
    );

    let _ = out.flush();
}

/// Convert our Color enum to crossterm's Color type.
fn to_crossterm_color(c: crate::grid::Color) -> CColor {
    match c {
        crate::grid::Color::Default    => CColor::Reset,
        crate::grid::Color::Black      => CColor::Black,
        crate::grid::Color::Red        => CColor::DarkRed,
        crate::grid::Color::Green      => CColor::DarkGreen,
        crate::grid::Color::Yellow     => CColor::DarkYellow,
        crate::grid::Color::Blue       => CColor::DarkBlue,
        crate::grid::Color::Magenta    => CColor::DarkMagenta,
        crate::grid::Color::Cyan       => CColor::DarkCyan,
        crate::grid::Color::White      => CColor::Grey,
        crate::grid::Color::Indexed(n) => CColor::AnsiValue(n),
        crate::grid::Color::Rgb(r,g,b) => CColor::Rgb { r, g, b },
    }
}