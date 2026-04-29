// File: src/input.rs
// Milestone 4: Keyboard Input Handling
//
// We need to:
//   1. Put the real terminal into "raw mode" so we get every keypress.
//   2. Read key events from crossterm.
//   3. Encode each event as the byte sequence the shell expects.
//   4. Pump those bytes into the PTY master so the shell sees them.
//
// Done asynchronously via tokio so input doesn't block output rendering.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};
use tokio::sync::mpsc;
use std::time::Duration;

/// One input event as our app sees it after encoding.
/// Carries the raw bytes that should be written to the PTY.
pub struct InputBytes(pub Vec<u8>);

/// Translate a crossterm KeyEvent into the byte sequence
/// that bash expects on its stdin.
pub fn encode_key(ev: KeyEvent) -> Option<Vec<u8>> {
    match ev.code {
        KeyCode::Char(c) => {
            if ev.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+letter — translate to control byte (Ctrl+A = 0x01, etc.)
                let lower = c.to_ascii_lowercase();
                if lower >= 'a' && lower <= 'z' {
                    return Some(vec![(lower as u8) - b'a' + 1]);
                }
            }
            // Plain character — encode as UTF-8
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            Some(s.as_bytes().to_vec())
        }
        KeyCode::Enter      => Some(vec![b'\r']),         // shells expect CR, not LF
        KeyCode::Tab        => Some(vec![b'\t']),
        KeyCode::Backspace  => Some(vec![0x7f]),           // DEL byte
        KeyCode::Esc        => Some(vec![0x1b]),
        KeyCode::Up         => Some(vec![0x1b, b'[', b'A']),
        KeyCode::Down       => Some(vec![0x1b, b'[', b'B']),
        KeyCode::Right      => Some(vec![0x1b, b'[', b'C']),
        KeyCode::Left       => Some(vec![0x1b, b'[', b'D']),
        KeyCode::Home       => Some(vec![0x1b, b'[', b'H']),
        KeyCode::End        => Some(vec![0x1b, b'[', b'F']),
        KeyCode::PageUp     => Some(vec![0x1b, b'[', b'5', b'~']),
        KeyCode::PageDown   => Some(vec![0x1b, b'[', b'6', b'~']),
        KeyCode::Delete     => Some(vec![0x1b, b'[', b'3', b'~']),
        _ => None,
    }
}

/// Spawn the input task. Returns a receiver of byte chunks to forward to the PTY.
pub fn spawn_input_task() -> mpsc::UnboundedReceiver<InputBytes> {
    let (tx, rx) = mpsc::unbounded_channel();

    // Enable raw mode — every keystroke is delivered immediately,
    // no line buffering, no echo, Ctrl+C does not kill us.
    enable_raw_mode().expect("failed to enable raw mode");

    tokio::spawn(async move {
        loop {
            // Poll with a small timeout so the task can be cancelled cleanly.
            // event::poll blocks the OS thread, so we run it inside spawn_blocking
            // to avoid blocking the tokio reactor.
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

            if let Some(Event::Key(key)) = maybe_event {
                if let Some(bytes) = encode_key(key) {
                    if tx.send(InputBytes(bytes)).is_err() {
                        break; // receiver dropped, app is shutting down
                    }
                }
            }
        }
    });

    rx
}

/// Restore the terminal to normal mode on shutdown.
/// Call this from a Drop guard or at the end of main.
pub fn restore_terminal() {
    let _ = disable_raw_mode();
}
