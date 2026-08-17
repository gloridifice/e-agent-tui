//! Client configuration: persisted TOML in %APPDATA%\dshe\config.toml,
//! editable live through /settings. The selected theme is a name resolved
//! against the themes directory (see `theme.rs`); the two built-in defaults
//! are deepseek-e and ferra, with deepseek-e the default.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use crate::theme::Theme;

// ---------- full config ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    // 外观
    pub spinner_style: String,
    pub spinner_frame_ms: u64,
    /// Selected theme name ("deepseek-e" | "ferra" | a `<name>` from the
    /// themes directory). The resolved palette is cached in `resolved_theme`
    /// (not persisted) so render-time lookup never touches disk.
    pub theme: String,
    #[serde(skip)]
    pub resolved_theme: Theme,
    pub plain_color: bool,
    // 行为
    pub remember_last_session: bool,
    /// Agent-preset mode for the session a fresh TUI process opens (the
    /// bridge falls back to `standard` when this id is stale).
    pub default_mode: String,
    pub enter_sends: bool,
    pub paste_placeholder_chars: usize,
    pub long_content_lines: usize,
    pub atomic_collapse_rows: usize,
    pub copy_toast_secs: u64,
    pub history_limit: usize,
    // 显示
    pub show_model_in_status: bool,
    pub show_tool_duration: bool,
    pub read_merge: bool,
    pub show_timestamps: bool,
    pub mermaid_enabled: bool,
    /// Horizontal gutter (in columns) of user message blocks and the input
    /// box — live-editable via /settings.
    pub user_input_padding: usize,
    /// Maximum page width in columns (0 = unlimited, use the terminal width
    /// minus the side margins). The content area is centered and capped at
    /// this width; longer text wraps.
    pub page_max_width: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            spinner_style: "A".into(),
            spinner_frame_ms: 120,
            theme: "deepseek-e".into(),
            resolved_theme: Theme::deepseek_e(),
            plain_color: false,
            // A fresh TUI process opens a NEW session by default; resume
            // goes through the CLI session id, `/resume`, or this opt-in.
            remember_last_session: false,
            default_mode: "standard".into(),
            enter_sends: true,
            paste_placeholder_chars: 64,
            long_content_lines: 20,
            atomic_collapse_rows: 40,
            copy_toast_secs: 2,
            history_limit: 1000,
            show_model_in_status: true,
            show_tool_duration: true,
            read_merge: true,
            show_timestamps: false,
            mermaid_enabled: true,
            user_input_padding: 2,
            page_max_width: 0,
        }
    }
}

impl Config {
    /// `%APPDATA%\dshe` on Windows, `~/.config/dshe` elsewhere.
    pub fn config_dir() -> PathBuf {
        directories::ProjectDirs::from("", "", "dshe")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".dshe"))
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Theme files live beside the config: `%APPDATA%\dshe\themes\`.
    pub fn themes_dir() -> PathBuf {
        Self::config_dir().join("themes")
    }

    pub fn state_path() -> PathBuf {
        directories::ProjectDirs::from("", "", "dshe")
            .map(|d| d.data_dir().join("state.toml"))
            .unwrap_or_else(|| PathBuf::from("dshe.state.toml"))
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
                eprintln!("[dshe] config parse failed ({error}); using defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        };
        // Fall back the cached palette by built-in name; the real theme
        // resolution (against the themes directory) happens at startup.
        config.resolved_theme = Theme::from_name(&config.theme);
        config
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn theme(&self) -> Theme {
        self.resolved_theme
    }
}

/// Last-attached session id, remembered across runs (D17).
#[derive(Serialize, Deserialize, Default)]
pub struct StateFile {
    pub last_session_id: Option<String>,
}

impl StateFile {
    pub fn load() -> Self {
        match std::fs::read_to_string(Config::state_path()) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Config::state_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
