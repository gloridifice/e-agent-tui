//! Client configuration: persisted TOML in `%APPDATA%\dshe\config.toml` and
//! editable live through `/settings`.
//!
//! The default configuration is `client/assets/default_config.toml`, embedded
//! with `include_str!` and parsed into [`Config`]. User files are partial
//! overlays, so newly added fields inherit the embedded defaults.

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

pub use crate::theme::Theme;

pub const DEFAULT_CONFIG_SOURCE: &str = include_str!("../assets/default_config.toml");

/// How model reasoning content is displayed in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingDisplayMode {
    /// Current behavior: only the breathing `Thinking...` row is visible.
    Compact,
    /// Show at most the first `thinking_lines` lines of reasoning text.
    Lines,
    /// Show the complete reasoning text.
    Full,
}

impl ThinkingDisplayMode {
    pub fn shows_reasoning(self) -> bool {
        !matches!(self, Self::Compact)
    }
}

// ---------- full config ----------

#[derive(Serialize, Clone, Debug)]
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
    /// How reasoning content is shown: `compact` (default), `lines`, or `full`.
    pub thinking_display: String,
    /// Line budget for `ThinkingDisplayMode::Lines`.
    pub thinking_lines: usize,
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

/// Exact schema for the embedded file. Unlike a user overlay, every field is
/// mandatory so an accidental omission in the repository asset fails tests
/// and startup immediately instead of silently changing behavior.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteConfig {
    spinner_style: String,
    spinner_frame_ms: u64,
    theme: String,
    plain_color: bool,
    remember_last_session: bool,
    default_mode: String,
    enter_sends: bool,
    paste_placeholder_chars: usize,
    long_content_lines: usize,
    atomic_collapse_rows: usize,
    copy_toast_secs: u64,
    history_limit: usize,
    show_model_in_status: bool,
    show_tool_duration: bool,
    read_merge: bool,
    thinking_display: String,
    thinking_lines: usize,
    show_timestamps: bool,
    mermaid_enabled: bool,
    user_input_padding: usize,
    page_max_width: usize,
}

impl CompleteConfig {
    fn into_config(self) -> Config {
        let resolved_theme = Theme::from_name(&self.theme);
        Config {
            spinner_style: self.spinner_style,
            spinner_frame_ms: self.spinner_frame_ms,
            theme: self.theme,
            resolved_theme,
            plain_color: self.plain_color,
            remember_last_session: self.remember_last_session,
            default_mode: self.default_mode,
            enter_sends: self.enter_sends,
            paste_placeholder_chars: self.paste_placeholder_chars,
            long_content_lines: self.long_content_lines,
            atomic_collapse_rows: self.atomic_collapse_rows,
            copy_toast_secs: self.copy_toast_secs,
            history_limit: self.history_limit,
            show_model_in_status: self.show_model_in_status,
            show_tool_duration: self.show_tool_duration,
            read_merge: self.read_merge,
            thinking_display: self.thinking_display,
            thinking_lines: self.thinking_lines,
            show_timestamps: self.show_timestamps,
            mermaid_enabled: self.mermaid_enabled,
            user_input_padding: self.user_input_padding,
            page_max_width: self.page_max_width,
        }
    }
}

/// User config files are overlays. Option fields preserve the old
/// `#[serde(default)]` behavior while keeping the default values out of Rust.
#[derive(Deserialize, Default)]
struct PartialConfig {
    spinner_style: Option<String>,
    spinner_frame_ms: Option<u64>,
    theme: Option<String>,
    plain_color: Option<bool>,
    remember_last_session: Option<bool>,
    default_mode: Option<String>,
    enter_sends: Option<bool>,
    paste_placeholder_chars: Option<usize>,
    long_content_lines: Option<usize>,
    atomic_collapse_rows: Option<usize>,
    copy_toast_secs: Option<u64>,
    history_limit: Option<usize>,
    show_model_in_status: Option<bool>,
    show_tool_duration: Option<bool>,
    read_merge: Option<bool>,
    thinking_display: Option<String>,
    thinking_lines: Option<usize>,
    show_timestamps: Option<bool>,
    mermaid_enabled: Option<bool>,
    user_input_padding: Option<usize>,
    page_max_width: Option<usize>,
}

impl PartialConfig {
    fn apply(self, mut config: Config) -> Config {
        macro_rules! apply {
            ($($field:ident),+ $(,)?) => {
                $(if let Some(value) = self.$field {
                    config.$field = value;
                })+
            };
        }
        apply!(
            spinner_style,
            spinner_frame_ms,
            theme,
            plain_color,
            remember_last_session,
            default_mode,
            enter_sends,
            paste_placeholder_chars,
            long_content_lines,
            atomic_collapse_rows,
            copy_toast_secs,
            history_limit,
            show_model_in_status,
            show_tool_duration,
            read_merge,
            thinking_display,
            thinking_lines,
            show_timestamps,
            mermaid_enabled,
            user_input_padding,
            page_max_width,
        );
        config.resolved_theme = Theme::from_name(&config.theme);
        config
    }
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        PartialConfig::deserialize(deserializer).map(|file| file.apply(Self::default()))
    }
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str::<CompleteConfig>(DEFAULT_CONFIG_SOURCE)
            .expect("embedded default_config.toml must be valid")
            .into_config()
    }
}

impl Config {
    /// `%APPDATA%\dshe` on Windows, `~/.config/dshe` elsewhere.
    pub fn config_dir() -> PathBuf {
        directories::ProjectDirs::from("", "", "dshe")
            .map(|dirs| dirs.config_dir().to_path_buf())
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
            .map(|dirs| dirs.data_dir().join("state.toml"))
            .unwrap_or_else(|| PathBuf::from("dshe.state.toml"))
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
                eprintln!("[dshe] config parse failed ({error}); using embedded defaults");
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
            std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        std::fs::write(&path, text).map_err(|error| error.to_string())
    }

    pub fn theme(&self) -> Theme {
        self.resolved_theme
    }

    pub fn thinking_display_mode(&self) -> ThinkingDisplayMode {
        match self.thinking_display.as_str() {
            "lines" => ThinkingDisplayMode::Lines,
            "full" => ThinkingDisplayMode::Full,
            _ => ThinkingDisplayMode::Compact,
        }
    }

    pub fn thinking_display_label(&self) -> &'static str {
        match self.thinking_display_mode() {
            ThinkingDisplayMode::Compact => "Compact",
            ThinkingDisplayMode::Lines => "Lines",
            ThinkingDisplayMode::Full => "Full",
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_default_config_is_the_default_source() {
        let complete: CompleteConfig =
            toml::from_str(DEFAULT_CONFIG_SOURCE).expect("embedded defaults parse");
        let config = Config::default();
        assert_eq!(config.spinner_style, complete.spinner_style);
        assert_eq!(config.spinner_frame_ms, 120);
        assert_eq!(config.theme, "deepseek-e");
        assert_eq!(config.default_mode, "standard");
        assert_eq!(config.paste_placeholder_chars, 64);
        assert_eq!(config.page_max_width, 0);
        assert_eq!(config.thinking_display, "compact");
        assert_eq!(config.thinking_lines, 2);
        assert_eq!(
            config.thinking_display_mode(),
            ThinkingDisplayMode::Compact
        );
    }

    #[test]
    fn partial_user_config_overlays_embedded_defaults() {
        let config: Config = toml::from_str(
            r#"
                theme = "ferra"
                spinner_frame_ms = 250
            "#,
        )
        .expect("partial config parses");
        assert_eq!(config.theme, "ferra");
        assert_eq!(config.spinner_frame_ms, 250);
        assert_eq!(config.spinner_style, "A");
        assert_eq!(config.history_limit, 1000);
        assert_eq!(config.thinking_display, "compact");
        assert_eq!(config.thinking_lines, 2);
        assert_eq!(config.resolved_theme.user, Theme::ferra().user);
    }

    #[test]
    fn persisted_config_omits_the_resolved_theme_cache() {
        let text = toml::to_string(&Config::default()).expect("config serializes");
        assert!(!text.contains("resolved_theme"));
        assert!(text.contains("theme = \"deepseek-e\""));
    }
}
