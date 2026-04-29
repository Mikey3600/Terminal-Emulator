//! PTY subsystem facade.
//!
//! Exposes PTY creation and I/O primitives used by the main loop while keeping
//! low-level Unix details encapsulated in `pty_master`.

pub mod pty_master;
pub mod pty_slave;

pub use pty_master::{read_from_pty, reap_child, resize_pty, spawn_shell, write_to_pty, TermSize};
