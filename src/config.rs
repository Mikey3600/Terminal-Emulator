//! Configuration loading and defaults.
//!
//! This module defines user-tunable runtime settings and the policy for where
//! configuration files are discovered on Unix-like systems. It exists so the
//! rest of the emulator can depend on a stable `Config` struct instead of
//! environment variables or hardcoded values.
//!
//! Data flow: filesystem TOML -> `serde` deserialization -> validated `Config`
//! with per-field defaults -> consumed by startup (`main`).

use serde::Deserialize;
use std::path::PathBuf;

/// Application configuration. Deserialized from TOML.
/// Every field has a default so a missing config file is fine.
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_shell")]
    pub shell: String,

    #[serde(default = "default_rows")]
    pub rows: u16,

    #[serde(default = "default_cols")]
    pub cols: u16,

    #[serde(default = "default_font_size")]
    pub font_size: u16,

    #[serde(default = "default_scheme")]
    pub color_scheme: String,
}

/// Returns the default shell path from `$SHELL` or `/bin/sh`.
fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}
/// Default terminal height used when no config is provided.
fn default_rows() -> u16 {
    24
}
/// Default terminal width used when no config is provided.
fn default_cols() -> u16 {
    80
}
/// Placeholder default font size for future GUI frontends.
fn default_font_size() -> u16 {
    14
}
/// Default color scheme label used by renderer integrations.
fn default_scheme() -> String {
    "dark".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            shell: default_shell(),
            rows: default_rows(),
            cols: default_cols(),
            font_size: default_font_size(),
            color_scheme: default_scheme(),
        }
    }
}

impl Config {
    /// Load config from the standard XDG location.
    /// Falls back to defaults if the file does not exist or is malformed.
    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(cfg) => {
                    log::info!("loaded config from {:?}", path);
                    cfg
                }
                Err(e) => {
                    log::warn!("config parse error ({}), using defaults", e);
                    Config::default()
                }
            },
            Err(_) => {
                log::info!("no config file at {:?}, using defaults", path);
                Config::default()
            }
        }
    }

    /// The path we look for the config file at:
    /// $XDG_CONFIG_HOME/terminal_emulator/config.toml,
    /// or ~/.config/terminal_emulator/config.toml as fallback.
    fn config_path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".config")
            });
        base.join("terminal_emulator").join("config.toml")
    }
}
