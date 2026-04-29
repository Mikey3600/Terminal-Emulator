//! Terminal presentation subsystem.
//!
//! This layer contains the in-memory screen representation (`screen_buffer`)
//! and the concrete renderer (`renderer`) that translates changed cells into
//! host terminal drawing commands.

pub mod renderer;
pub mod screen_buffer;

pub use renderer::render;
pub use screen_buffer::Grid;
