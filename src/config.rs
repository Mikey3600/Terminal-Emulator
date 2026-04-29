// File: src/config.rs
// Milestone 6: Configuration loading from TOML
//
// We want the user to be able to customize:
//   - Which shell runs
//   - Default rows / cols
//   - Font size (for future GUI rendering)
//   - Color scheme name
// without recompiling.

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

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}
fn default_rows() -> u16 {
    24
}
fn default_cols() -> u16 {
    80
}
fn default_font_size() -> u16 {
    14
}
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
