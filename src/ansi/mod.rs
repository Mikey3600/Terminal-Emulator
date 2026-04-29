//! ANSI parsing subsystem.
//!
//! This module isolates terminal-control-sequence decoding from rendering and
//! PTY I/O. Bytes from the shell are streamed into [`Parser`], which mutates
//! the terminal screen model (`Grid`) according to ASCII control bytes and
//! ANSI/VT escape sequences.

pub mod parser;

pub use parser::{AnsiCapabilities, Parser};
