use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::utils::error::AppResult;

pub enum InputEvent {
    Bytes(Vec<u8>),
    Resize(u16, u16),
}

pub fn encode_key(ev: KeyEvent) -> Option<Vec<u8>> {
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
        KeyCode::Up => Some(vec![0x1b, b'[', b'A']),
        KeyCode::Down => Some(vec![0x1b, b'[', b'B']),
        KeyCode::Right => Some(vec![0x1b, b'[', b'C']),
        KeyCode::Left => Some(vec![0x1b, b'[', b'D']),
        _ => None,
    }
}

pub fn spawn_input_task() -> AppResult<mpsc::UnboundedReceiver<InputEvent>> {
    let (tx, rx) = mpsc::unbounded_channel();
    enable_raw_mode()?;

    tokio::spawn(async move {
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
                    if let Some(bytes) = encode_key(key) {
                        if tx.send(InputEvent::Bytes(bytes)).is_err() {
                            break;
                        }
                    }
                }
                Some(Event::Resize(cols, rows))
                    if tx.send(InputEvent::Resize(cols, rows)).is_err() =>
                {
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(rx)
}

pub fn restore_terminal() {
    let _ = disable_raw_mode();
}
