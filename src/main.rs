mod ansi;
mod buffer;
mod config;
mod input;
mod pty;
mod terminal;
mod utils;

use ansi::Parser;
use config::Config;
use crossterm::{
    cursor::MoveTo,
    terminal::{size as terminal_size, Clear, ClearType},
    ExecutableCommand,
};
use pty::{reap_child, resize_pty, spawn_shell, write_to_pty, TermSize};
use std::io::{stdout, Write};
use std::sync::Arc;
use terminal::{render, Grid};

struct TerminalModeGuard;

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        input::restore_terminal();
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    if let Err(err) = run().await {
        eprintln!("terminal-emulator error: {err}");
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = Config::load();
    let master = Arc::new(spawn_shell(TermSize {
        rows: cfg.rows,
        cols: cfg.cols,
        shell: Some(cfg.shell.clone()),
    })?);

    let mut grid = Grid::new(cfg.rows as usize, cfg.cols as usize);
    let mut parser = Parser::new();
    let mut input_rx = input::spawn_input_task();
    let _terminal_mode_guard = TerminalModeGuard;

    let mut out = stdout();
    out.execute(Clear(ClearType::All))?;
    out.execute(MoveTo(0, 0))?;

    loop {
        tokio::select! {
            result = tokio::task::spawn_blocking({
                let master = Arc::clone(&master);
                move || {
                    let mut buf = [0u8; 4096];
                    pty::read_from_pty(&master, &mut buf).map(|n| (buf, n))
                }
            }) => {
                match result {
                    Ok(Ok((buf, n))) if n > 0 => {
                        parser.feed(&buf[..n], &mut grid);
                        render(&grid, &mut out);
                        grid.clear_dirty();
                    }
                    _ => break,
                }
            }
            Some(event) = input_rx.recv() => {
                match event {
                    input::InputEvent::Bytes(bytes) => {
                        let _ = write_to_pty(&master, &bytes);
                    }
                    input::InputEvent::Resize(cols, rows) => {
                        if rows > 0 && cols > 0 {
                            grid.resize(rows as usize, cols as usize);
                            let _ = resize_pty(&master, TermSize { rows, cols, shell: None });
                        }
                    }
                }
            }
        }
    }

    let _ = reap_child(&master);

    if let Ok((_cols, rows)) = terminal_size() {
        let _ = out.execute(MoveTo(0, rows.saturating_sub(1)));
        let _ = out.flush();
    }

    Ok(())
}
