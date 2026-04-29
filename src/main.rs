mod ansi;
mod buffer;
mod config;
mod input;
mod pty;
mod terminal;
mod utils;

use ansi::Parser;
use buffer::RingBuffer;
use config::Config;
use crossterm::{cursor::MoveTo, terminal::{Clear, ClearType}, ExecutableCommand};
use pty::{spawn_shell, write_to_pty, TermSize};
use std::io::stdout;
use std::sync::Arc;
use terminal::{render, Grid};

#[tokio::main]
async fn main() {
    env_logger::init();

    let cfg = Config::load();
    let master = Arc::new(spawn_shell(TermSize { rows: cfg.rows, cols: cfg.cols }).expect("Failed to spawn shell"));

    let mut grid = Grid::new(cfg.rows as usize, cfg.cols as usize);
    let mut parser = Parser::new();
    let mut _scrollback: RingBuffer<Vec<char>> = RingBuffer::new(1000);
    let mut input_rx = input::spawn_input_task();

    let mut out = stdout();
    out.execute(Clear(ClearType::All)).unwrap();
    out.execute(MoveTo(0, 0)).unwrap();

    loop {
        tokio::select! {
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
            Some(input::InputBytes(bytes)) = input_rx.recv() => {
                write_to_pty(&master, &bytes).ok();
            }
        }
    }

    input::restore_terminal();
}
