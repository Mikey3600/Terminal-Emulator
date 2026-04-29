//! Common application error/result aliases.

/// Convenience result type used by higher-level application code.
///
/// The boxed trait object keeps function signatures simple while still
/// allowing heterogeneous error types from I/O, parsing, PTY calls, etc.
pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
