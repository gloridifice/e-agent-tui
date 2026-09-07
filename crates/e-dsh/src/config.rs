//! Filesystem persistence for the frontend configuration and launcher state.
//!
//! The strict UI schema and embedded defaults are owned by `e-tui`; this
//! adapter owns platform paths and disk effects only.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use e_tui::config::{Config, ThinkingDisplayMode, DEFAULT_CONFIG_SOURCE};
pub use e_tui::theme::Theme;

/// Shared frontend configuration, using `~/.config/e` on macOS too.
pub fn config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| {
            if cfg!(target_os = "macos") {
                dirs.home_dir().join(".config/e")
            } else {
                dirs.config_dir().join("e")
            }
        })
        .unwrap_or_else(|| PathBuf::from(".e"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Theme files live beside the shared config.
pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

pub fn state_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "dshe")
        .map(|dirs| dirs.data_dir().join("state.toml"))
        .unwrap_or_else(|| PathBuf::from("dshe.state.toml"))
}

pub fn load() -> Config {
    let path = config_path();
    let mut config = match std::fs::read_to_string(&path) {
        Ok(text) => Config::user_toml_or_default(&text),
        Err(_) => Config::default(),
    };
    config.config_path_display = path.display().to_string();
    let key_path = config_dir().join("key_mapping.toml");
    let mapping = match std::fs::read_to_string(&key_path) {
        Ok(text) => e_tui::key_mapping::KeyMapping::from_user_toml(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(error) => Err(error.to_string()),
    };
    match mapping {
        Ok(mapping) => config.key_mapping = mapping,
        Err(error) => config.key_mapping_error = Some(format!("{}: {error}", key_path.display())),
    }
    config
}

pub fn save(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let text = toml::to_string_pretty(config).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// Last-attached session id, remembered across runs (D17).
#[derive(Serialize, Deserialize, Default)]
pub struct StateFile {
    pub last_session_id: Option<String>,
}

impl StateFile {
    pub fn load() -> Self {
        match std::fs::read_to_string(state_path()) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = state_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
