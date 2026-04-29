//! Terminal Emulator entry point.
//!
//! This module wires together the four major subsystems of the application:
//! (1) PTY process management, (2) input capture, (3) ANSI parsing, and
//! (4) incremental rendering.

use crossterm::{
    cursor::MoveTo,
    terminal::{size as terminal_size, Clear, ClearType},
    ExecutableCommand,
};
use std::io::{stdout, IsTerminal, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use terminal_emulator::ansi::{AnsiCapabilities, Parser};
use terminal_emulator::config::Config;
use terminal_emulator::pty::{self, reap_child, resize_pty, spawn_shell, write_to_pty, TermSize};
use terminal_emulator::terminal::{render, Grid};
use terminal_emulator::utils::error::AppResult;
use tokio::sync::mpsc;

#[derive(Debug)]
enum TerminalEvent {
    // Future: MouseInput/Clipboard/OSC dispatch can be added here without
    // changing the parser->grid->renderer architecture.
    PtyOutput(Vec<u8>),
    KeyInput(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Tick,
}

struct TerminalModeGuard;
impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        terminal_emulator::input::restore_terminal();
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    if let Err(err) = run().await {
        eprintln!("terminal-emulator error: {err}");
    }
}

async fn spawn_pty_reader(
    master: Arc<pty::pty_master::PtyMaster>,
    tx: mpsc::UnboundedSender<TerminalEvent>,
) {
    let _ = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0_u8; 4096];
        loop {
            match pty::read_from_pty(&master, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(TerminalEvent::PtyOutput(buf[..n].to_vec())).is_err() {
                        break;
                    }
                    log::trace!("pty_read_bytes={n}");
                }
                Err(err) => {
                    log::debug!("pty_read_error={err}");
                    break;
                }
            }
        }
    })
    .await;
}

async fn run() -> AppResult<()> {
    let cfg = Config::load();

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("non-interactive environment detected; skipping runtime loop".into());
    }

    log::info!("starting terminal runtime rows={} cols={}", cfg.rows, cfg.cols);
    let master = Arc::new(spawn_shell(TermSize {
        rows: cfg.rows,
        cols: cfg.cols,
        shell: Some(cfg.shell.clone()),
    })?);
    let mut grid = Grid::new(cfg.rows as usize, cfg.cols as usize);
    let mut parser = Parser::new(AnsiCapabilities::default());
    let mut input_rx = terminal_emulator::input::spawn_input_task()?;
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let _terminal_mode_guard = TerminalModeGuard;

    tokio::spawn(spawn_pty_reader(Arc::clone(&master), event_tx.clone()));

    tokio::spawn({
        let event_tx = event_tx.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(16)).await;
                if event_tx.send(TerminalEvent::Tick).is_err() {
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        while let Some(event) = input_rx.recv().await {
            match event {
                terminal_emulator::input::InputEvent::Bytes(bytes) => {
                    let _ = event_tx.send(TerminalEvent::KeyInput(bytes));
                }
                terminal_emulator::input::InputEvent::Resize(cols, rows) => {
                    let _ = event_tx.send(TerminalEvent::Resize { cols, rows });
                }
            }
        }
    });

    let mut out = stdout();
    out.execute(Clear(ClearType::All))?;
    out.execute(MoveTo(0, 0))?;

    while let Some(event) = event_rx.recv().await {
        match event {
            TerminalEvent::PtyOutput(bytes) => {
                parser.feed(&bytes, &mut grid);
                let started = Instant::now();
                render(&grid, &mut out)?;
                log::trace!("render_us={}", started.elapsed().as_micros());
                grid.clear_dirty();
            }
            TerminalEvent::KeyInput(bytes) => {
                write_to_pty(&master, &bytes)?;
            }
            TerminalEvent::Resize { cols, rows } => {
                if rows > 0 && cols > 0 {
                    grid.resize(rows as usize, cols as usize);
                    resize_pty(&master, TermSize { rows, cols, shell: None })?;
                }
            }
            TerminalEvent::Tick => {}
        }
    }

    log::info!("runtime shutting down; reaping child process");
    let _ = reap_child(&master);
    if let Ok((_cols, rows)) = terminal_size() {
        let _ = out.execute(MoveTo(0, rows.saturating_sub(1)));
        let _ = out.flush();
    }
    Ok(())
}
