//! Keyboard and terminal-event ingestion.
//!
//! This module translates host terminal events (`crossterm`) into emulator
//! events consumed by the async main loop. It also controls raw mode, which
//! is required for unbuffered key input (no canonical line editing by the OS).

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::time::Duration;
use tokio::sync::mpsc;

pub enum InputEvent {
    /// Opaque byte stream to forward to the PTY master.
    Bytes(Vec<u8>),
    /// Window resize notification `(cols, rows)`.
    Resize(u16, u16),
}

/// Encodes a key event into bytes expected by TTY programs.
///
/// Design notes:
/// - Control-letter combinations map to ASCII control codes (Ctrl+A=0x01, ...).
/// - Arrow/navigation keys use common ANSI CSI sequences.
/// - Unicode `Char` keys are UTF-8 encoded for compatibility with modern shells.
pub fn encode_key(ev: KeyEvent) -> Option<Vec<u8>> {
    match ev.code {
        KeyCode::Char(c) => {
            if ev.modifiers.contains(KeyModifiers::CONTROL) {
                let lower = c.to_ascii_lowercase();
                if ('a'..='z').contains(&lower) {
                    return Some(vec![(lower as u8) - b'a' + 1]);
                }
            }
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            Some(s.as_bytes().to_vec())
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Up => Some(vec![0x1b, b'[', b'A']),
        KeyCode::Down => Some(vec![0x1b, b'[', b'B']),
        KeyCode::Right => Some(vec![0x1b, b'[', b'C']),
        KeyCode::Left => Some(vec![0x1b, b'[', b'D']),
        KeyCode::Home => Some(vec![0x1b, b'[', b'H']),
        KeyCode::End => Some(vec![0x1b, b'[', b'F']),
        KeyCode::PageUp => Some(vec![0x1b, b'[', b'5', b'~']),
        KeyCode::PageDown => Some(vec![0x1b, b'[', b'6', b'~']),
        KeyCode::Delete => Some(vec![0x1b, b'[', b'3', b'~']),
        _ => None,
    }
}

/// Spawns a background task that polls keyboard/resize events and emits `InputEvent`s.
///
/// The task uses `spawn_blocking` because `crossterm` event APIs block the thread.
pub fn spawn_input_task() -> mpsc::UnboundedReceiver<InputEvent> {
    // 1) Create unbounded channel and switch host terminal into raw mode.
    let (tx, rx) = mpsc::unbounded_channel();
    enable_raw_mode().expect("failed to enable raw mode");

    tokio::spawn(async move {
        loop {
            // 2) Poll in a blocking task because crossterm APIs are sync.
            let maybe_event = tokio::task::spawn_blocking(|| {
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    event::read().ok()
                } else {
                    None
                }
            })
            .await
            .ok()
            .flatten();

            match maybe_event {
                Some(Event::Key(key)) => {
                    // 3) Translate key event to PTY bytes and forward.
                    if let Some(bytes) = encode_key(key) {
                        if tx.send(InputEvent::Bytes(bytes)).is_err() {
                            break;
                        }
                    }
                }
                Some(Event::Resize(cols, rows)) => {
                    // 4) Forward resize so both grid and PTY can be resized.
                    if tx.send(InputEvent::Resize(cols, rows)).is_err() {
                        break;
                    }
                }
                _ => {}
            }
        }
    });

    rx
}

/// Restores canonical terminal mode in a best-effort manner.
///
/// Safe to call repeatedly; failures are intentionally ignored during shutdown.
pub fn restore_terminal() {
    let _ = disable_raw_mode();
}
