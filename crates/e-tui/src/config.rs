//! UI configuration value schema and embedded defaults.
//!
//! Filesystem locations and persistence belong to the executable adapter. The
//! default configuration is embedded from `e-tui/assets/default_config.toml`;
//! user documents are parsed here as partial overlays over that one schema.

use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
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
    /// Host-provided display path for the read-only settings row.
    #[serde(skip)]
    pub config_path_display: String,
    pub plain_color: bool,
    // 行为
    pub remember_last_session: bool,
    /// Agent-preset mode for bare `/new` and the session a fresh TUI process
    /// opens (the bridge falls back to `standard` when this id is stale).
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
    /// Preferred total main-pane width in wide two-pane mode. The Screen
    /// still enforces measured minimums for both panes.
    pub main_pane_width: usize,
    /// Maximum page width in columns (0 = unlimited, use the terminal width
    /// minus the side margins). The content area is capped at this width;
    /// longer text wraps.
    pub page_max_width: usize,
    /// Horizontal alignment of the capped content page: `center` (default),
    /// `left`, or `right`. Unknown values fall back to `center`.
    pub page_align: String,
}

/// Recursively overlay only keys present in the embedded schema. Unknown keys
/// from older config files are ignored, while known keys retain their user
/// value (including an invalid type, which the one strict deserialize rejects).
fn overlay_known(base: &mut toml::Value, user: toml::Value) {
    match (base, user) {
        (toml::Value::Table(base), toml::Value::Table(user)) => {
            for (key, value) in user {
                if let Some(base_value) = base.get_mut(&key) {
                    overlay_known(base_value, value);
                }
            }
        }
        (base, user) => *base = user,
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::parse_complete(DEFAULT_CONFIG_SOURCE)
            .expect("embedded default_config.toml must be a complete valid Config")
    }
}

impl Config {
    fn parse_complete(source: &str) -> Result<Self, String> {
        let mut config: Self = toml::from_str(source).map_err(|error| error.to_string())?;
        config.resolved_theme = Theme::from_name(&config.theme);
        Ok(config)
    }

    /// Merge one partial user document over the embedded schema, ignore
    /// obsolete keys, then deserialize exactly once into the strict Config.
    pub fn from_user_toml(source: &str) -> Result<Self, String> {
        let mut merged: toml::Value =
            toml::from_str(DEFAULT_CONFIG_SOURCE).map_err(|error| error.to_string())?;
        let user: toml::Value = toml::from_str(source).map_err(|error| error.to_string())?;
        overlay_known(&mut merged, user);
        let mut config: Self = merged
            .try_into()
            .map_err(|error: toml::de::Error| error.to_string())?;
        config.resolved_theme = Theme::from_name(&config.theme);
        Ok(config)
    }

    pub fn user_toml_or_default(source: &str) -> Self {
        Self::from_user_toml(source).unwrap_or_else(|error| {
            eprintln!("[dshe] config parse failed ({error}); using embedded defaults");
            Self::default()
        })
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

    /// Display label of the page horizontal alignment (居中/左对齐/右对齐).
    /// Unknown values fall back to 居中, matching the layout behavior.
    pub fn page_align_label(&self) -> &'static str {
        match self.page_align.as_str() {
            "left" => "左对齐",
            "right" => "右对齐",
            _ => "居中",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_default_config_is_the_default_source() {
        let direct: Config =
            toml::from_str(DEFAULT_CONFIG_SOURCE).expect("embedded defaults are the full schema");
        let config = Config::default();
        assert_eq!(config.spinner_style, direct.spinner_style);
        assert_eq!(config.spinner_frame_ms, 120);
        assert_eq!(config.theme, "deepseek-e");
        assert_eq!(config.default_mode, "standard");
        assert_eq!(config.paste_placeholder_chars, 64);
        assert_eq!(config.main_pane_width, 120);
        assert_eq!(config.page_max_width, 0);
        assert_eq!(config.page_align, "center");
        assert_eq!(config.thinking_display, "compact");
        assert_eq!(config.thinking_lines, 2);
        assert_eq!(config.thinking_display_mode(), ThinkingDisplayMode::Compact);
    }

    #[test]
    fn partial_user_config_overlays_embedded_defaults() {
        let config = Config::from_user_toml(
            r#"
                theme = "ferra"
                spinner_frame_ms = 250
                page_align = "right"
            "#,
        )
        .expect("partial config overlays defaults");
        assert_eq!(config.theme, "ferra");
        assert_eq!(config.spinner_frame_ms, 250);
        assert_eq!(config.page_align, "right");
        assert_eq!(config.spinner_style, "A");
        assert_eq!(config.history_limit, 1000);
        assert_eq!(config.thinking_display, "compact");
        assert_eq!(config.thinking_lines, 2);
        assert_eq!(config.resolved_theme.user, Theme::ferra().user);
    }

    #[test]
    fn obsolete_unknown_fields_are_filtered_without_losing_valid_overrides() {
        let config = Config::from_user_toml(
            r#"
                theme = "ferra"
                removed_legacy_option = true
            "#,
        )
        .expect("unknown legacy key is ignored");
        assert_eq!(config.theme, "ferra");
        assert_eq!(config.spinner_frame_ms, 120);
    }

    #[test]
    fn known_invalid_types_and_malformed_toml_take_the_safe_full_fallback() {
        assert!(Config::from_user_toml("spinner_frame_ms = \"fast\"").is_err());
        assert!(Config::from_user_toml("theme = [").is_err());
        let fallback = Config::user_toml_or_default("history_limit = \"many\"");
        assert_eq!(fallback.history_limit, 1000);
        assert_eq!(fallback.theme, "deepseek-e");
    }

    #[test]
    fn unknown_theme_name_is_persisted_but_runtime_palette_has_a_safe_fallback() {
        let config = Config::from_user_toml("theme = \"removed-theme\"").unwrap();
        assert_eq!(config.theme, "removed-theme");
        assert_eq!(config.resolved_theme.user, Theme::deepseek_e().user);
    }

    #[test]
    fn direct_config_deserialization_is_strict_and_complete() {
        assert!(toml::from_str::<Config>("theme = \"ferra\"").is_err());
        let with_unknown = format!("{DEFAULT_CONFIG_SOURCE}\nunknown = true\n");
        assert!(toml::from_str::<Config>(&with_unknown).is_err());
    }

    #[test]
    fn persisted_config_omits_the_resolved_theme_cache() {
        let text = toml::to_string(&Config::default()).expect("config serializes");
        assert!(!text.contains("resolved_theme"));
        assert!(text.contains("theme = \"deepseek-e\""));
    }
}
