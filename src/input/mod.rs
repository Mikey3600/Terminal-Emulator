//! Input subsystem facade.
//!
//! The terminal emulator keeps input-specific concerns in one place:
//! enabling/disabling raw mode, converting key events into byte sequences
//! expected by TTY applications, and reporting resize events.

pub mod keyboard;

pub use keyboard::{restore_terminal, spawn_input_task, InputEvent};
