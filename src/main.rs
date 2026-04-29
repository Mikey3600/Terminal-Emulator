//! Terminal Emulator entry point.
//!
//! This module wires together the four major subsystems of the application:
//! (1) PTY process management, (2) input capture, (3) ANSI parsing, and
//! (4) incremental rendering. The `run` loop continuously transfers bytes
//! from the shell PTY into the parser, mutates the in-memory screen grid,
//! and paints only changed cells to stdout.
//!
//! Data flow summary:
//! 1. Spawn shell attached to a PTY master.
//! 2. Read shell output bytes from PTY.
//! 3. Parse bytes into terminal operations over `Grid`.
//! 4. Render dirty cells.
//! 5. Capture keyboard/resize input and write encoded bytes back to PTY.

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
    /// Restores the host terminal mode on scope exit (normal return or panic).
    ///
    /// This RAII guard avoids leaving the user terminal in raw mode, which
    /// would otherwise break line editing and echo in the parent shell.
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
    // 1) Load runtime configuration and start shell inside a PTY.
    let cfg = Config::load();
    let master = Arc::new(spawn_shell(TermSize {
        rows: cfg.rows,
        cols: cfg.cols,
        shell: Some(cfg.shell.clone()),
    })?);

    // 2) Initialize in-memory screen model and parser state machine.
    let mut grid = Grid::new(cfg.rows as usize, cfg.cols as usize);
    let mut parser = Parser::new();
    let mut input_rx = input::spawn_input_task();
    let _terminal_mode_guard = TerminalModeGuard;

    // 3) Put terminal in raw mode and clear the visible screen once.
    let mut out = stdout();
    out.execute(Clear(ClearType::All))?;
    out.execute(MoveTo(0, 0))?;

    loop {
        // 4) Drive PTY output and user input concurrently.
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
                        // 4a) Parse shell bytes and repaint only dirty cells.
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
                        // 4b) Forward encoded key bytes to the shell process.
                        let _ = write_to_pty(&master, &bytes);
                    }
                    input::InputEvent::Resize(cols, rows) => {
                        if rows > 0 && cols > 0 {
                            // 4c) Keep UI model and kernel PTY size in sync.
                            grid.resize(rows as usize, cols as usize);
                            let _ = resize_pty(&master, TermSize { rows, cols, shell: None });
                        }
                    }
                }
            }
        }
    }

    // 5) Best-effort child reap and cursor reposition for clean shell return.
    let _ = reap_child(&master);

    if let Ok((_cols, rows)) = terminal_size() {
        let _ = out.execute(MoveTo(0, rows.saturating_sub(1)));
        let _ = out.flush();
    }

    Ok(())
}
