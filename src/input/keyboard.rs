use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::utils::error::AppResult;

pub enum InputEvent {
    Bytes(Vec<u8>),
    Resize(u16, u16),
}

// FIX #3: track application cursor key mode; when true, arrow keys send SS3
// sequences (ESC O A..D) instead of CSI sequences (ESC [ A..D).
#[derive(Debug, Clone, Copy, Default)]
pub struct InputMode {
    pub application_cursor_keys: bool,
}

pub fn encode_key(ev: KeyEvent, mode: InputMode) -> Option<Vec<u8>> {
    match ev.code {
        KeyCode::Char(c) => {
            if ev.modifiers.contains(KeyModifiers::CONTROL) {
                let lower = c.to_ascii_lowercase();
                if lower.is_ascii_lowercase() {
                    return Some(vec![(lower as u8) - b'a' + 1]);
                }
            }
            let mut buf = [0u8; 4];
            Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Esc => Some(vec![0x1b]),
        // FIX #3: arrow keys respect application cursor key mode
        KeyCode::Up => {
            if mode.application_cursor_keys {
                Some(vec![0x1b, b'O', b'A'])
            } else {
                Some(vec![0x1b, b'[', b'A'])
            }
        }
        KeyCode::Down => {
            if mode.application_cursor_keys {
                Some(vec![0x1b, b'O', b'B'])
            } else {
                Some(vec![0x1b, b'[', b'B'])
            }
        }
        KeyCode::Right => {
            if mode.application_cursor_keys {
                Some(vec![0x1b, b'O', b'C'])
            } else {
                Some(vec![0x1b, b'[', b'C'])
            }
        }
        KeyCode::Left => {
            if mode.application_cursor_keys {
                Some(vec![0x1b, b'O', b'D'])
            } else {
                Some(vec![0x1b, b'[', b'D'])
            }
        }
        // FIX #2: missing keys — Home, End, Delete, PageUp, PageDown, F1-F12
        KeyCode::Home => Some(vec![0x1b, b'[', b'H']),
        KeyCode::End => Some(vec![0x1b, b'[', b'F']),
        KeyCode::Delete => Some(vec![0x1b, b'[', b'3', b'~']),
        KeyCode::PageUp => Some(vec![0x1b, b'[', b'5', b'~']),
        KeyCode::PageDown => Some(vec![0x1b, b'[', b'6', b'~']),
        KeyCode::F(1) => Some(vec![0x1b, b'O', b'P']),
        KeyCode::F(2) => Some(vec![0x1b, b'O', b'Q']),
        KeyCode::F(3) => Some(vec![0x1b, b'O', b'R']),
        KeyCode::F(4) => Some(vec![0x1b, b'O', b'S']),
        KeyCode::F(5) => Some(vec![0x1b, b'[', b'1', b'5', b'~']),
        KeyCode::F(6) => Some(vec![0x1b, b'[', b'1', b'7', b'~']),
        KeyCode::F(7) => Some(vec![0x1b, b'[', b'1', b'8', b'~']),
        KeyCode::F(8) => Some(vec![0x1b, b'[', b'1', b'9', b'~']),
        KeyCode::F(9) => Some(vec![0x1b, b'[', b'2', b'0', b'~']),
        KeyCode::F(10) => Some(vec![0x1b, b'[', b'2', b'1', b'~']),
        KeyCode::F(11) => Some(vec![0x1b, b'[', b'2', b'3', b'~']),
        KeyCode::F(12) => Some(vec![0x1b, b'[', b'2', b'4', b'~']),
        _ => None,
    }
}

// FIX #1: bounded channel (256) to avoid unbounded memory growth when the
// consumer is slow; unbounded_channel() had no backpressure at all.
pub fn spawn_input_task() -> AppResult<mpsc::Receiver<InputEvent>> {
    let (tx, rx) = mpsc::channel(256);
    enable_raw_mode()?;

    tokio::spawn(async move {
        // FIX #3: input mode is tracked per-task; callers that need to toggle
        // application cursor key mode should extend this via a shared Arc<Mutex<InputMode>>.
        let mode = InputMode::default();

        loop {
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
                    if let Some(bytes) = encode_key(key, mode) {
                        if tx.send(InputEvent::Bytes(bytes)).await.is_err() {
                            break;
                        }
                    }
                }
                Some(Event::Resize(cols, rows))
                    if tx.send(InputEvent::Resize(cols, rows)).await.is_err() =>
                {
                    break;
                }
                _ => {}
            }
        }

        // FIX: always restore raw mode when the input loop exits, whether the
        // channel closed normally or the task was dropped, preventing a raw
        // mode leak that the original code left to the caller.
        let _ = disable_raw_mode();
    });

    Ok(rx)
}

// restore_terminal is kept as a best-effort fallback for panic paths or any
// external code that needs to force-restore the terminal outside the task
// lifecycle (e.g. a panic hook).
pub fn restore_terminal() {
    let _ = disable_raw_mode();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn normal() -> InputMode {
        InputMode { application_cursor_keys: false }
    }
    fn app() -> InputMode {
        InputMode { application_cursor_keys: true }
    }

    #[test]
    fn arrow_keys_normal_mode() {
        assert_eq!(encode_key(key(KeyCode::Up), normal()), Some(vec![0x1b, b'[', b'A']));
        assert_eq!(encode_key(key(KeyCode::Down), normal()), Some(vec![0x1b, b'[', b'B']));
        assert_eq!(encode_key(key(KeyCode::Right), normal()), Some(vec![0x1b, b'[', b'C']));
        assert_eq!(encode_key(key(KeyCode::Left), normal()), Some(vec![0x1b, b'[', b'D']));
    }

    #[test]
    fn arrow_keys_application_mode() {
        assert_eq!(encode_key(key(KeyCode::Up), app()), Some(vec![0x1b, b'O', b'A']));
        assert_eq!(encode_key(key(KeyCode::Down), app()), Some(vec![0x1b, b'O', b'B']));
        assert_eq!(encode_key(key(KeyCode::Right), app()), Some(vec![0x1b, b'O', b'C']));
        assert_eq!(encode_key(key(KeyCode::Left), app()), Some(vec![0x1b, b'O', b'D']));
    }

    #[test]
    fn missing_keys_encoded() {
        assert_eq!(encode_key(key(KeyCode::Home), normal()), Some(vec![0x1b, b'[', b'H']));
        assert_eq!(encode_key(key(KeyCode::End), normal()), Some(vec![0x1b, b'[', b'F']));
        assert_eq!(encode_key(key(KeyCode::Delete), normal()), Some(vec![0x1b, b'[', b'3', b'~']));
        assert_eq!(encode_key(key(KeyCode::PageUp), normal()), Some(vec![0x1b, b'[', b'5', b'~']));
        assert_eq!(
            encode_key(key(KeyCode::PageDown), normal()),
            Some(vec![0x1b, b'[', b'6', b'~'])
        );
    }

    #[test]
    fn f_keys_encoded() {
        assert_eq!(encode_key(key(KeyCode::F(1)), normal()), Some(vec![0x1b, b'O', b'P']));
        assert_eq!(
            encode_key(key(KeyCode::F(5)), normal()),
            Some(vec![0x1b, b'[', b'1', b'5', b'~'])
        );
        assert_eq!(
            encode_key(key(KeyCode::F(12)), normal()),
            Some(vec![0x1b, b'[', b'2', b'4', b'~'])
        );
    }

    #[test]
    fn ctrl_keys_encoded() {
        assert_eq!(encode_key(ctrl('c'), normal()), Some(vec![3]));
        assert_eq!(encode_key(ctrl('a'), normal()), Some(vec![1]));
    }

    #[test]
    fn basic_keys() {
        assert_eq!(encode_key(key(KeyCode::Enter), normal()), Some(vec![b'\r']));
        assert_eq!(encode_key(key(KeyCode::Tab), normal()), Some(vec![b'\t']));
        assert_eq!(encode_key(key(KeyCode::Backspace), normal()), Some(vec![0x7f]));
        assert_eq!(encode_key(key(KeyCode::Esc), normal()), Some(vec![0x1b]));
    }
}
