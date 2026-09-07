//! Filesystem persistence for `pie` frontend-only configuration.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use e_tui::config::{Config, ThinkingDisplayMode, DEFAULT_CONFIG_SOURCE};
pub use e_tui::theme::Theme;

pub fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "pie")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".pie"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

pub fn state_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "pie")
        .map(|dirs| dirs.data_dir().join("state.toml"))
        .unwrap_or_else(|| PathBuf::from("pie.state.toml"))
}

pub fn load() -> Config {
    let path = config_path();
    let mut config = match std::fs::read_to_string(&path) {
        Ok(text) => Config::user_toml_or_default(&text),
        Err(_) => Config::default(),
    };
    // The shared frontend still exposes a mode label for deferred `/new`
    // drafts; Pi has one adapter-owned mode rather than DSH mode presets.
    config.default_mode = "pi".into();
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

#[derive(Serialize, Deserialize, Default)]
pub struct StateFile {
    pub last_session_path: Option<String>,
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
