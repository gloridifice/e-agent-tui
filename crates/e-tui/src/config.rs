//! UI configuration value schema and embedded defaults.
//!
//! Filesystem locations and persistence belong to the executable adapter. The
//! default configuration is embedded from `e-tui/assets/default_config.toml`;
//! user documents are parsed here as partial overlays over that one schema.

use std::{fmt, str::FromStr};

use ratatui::style::Color;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::i18n::Language;
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

/// Visual treatment of the ordinary composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputStyle {
    /// Existing Ash-filled composer with no visible border.
    Default,
    /// Bark square border with no composer fill.
    Square,
    /// Bark rounded border with no composer fill.
    Rounded,
    /// Bark horizontal rules and prompt arrow with Umber rule ends.
    Line,
}

impl InputStyle {
    pub const fn value(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Square => "square",
            Self::Rounded => "rounded",
            Self::Line => "line",
        }
    }

    pub const fn horizontal_chrome(self) -> usize {
        match self {
            Self::Default => 0,
            Self::Square | Self::Rounded => 2,
            Self::Line => 1,
        }
    }
}

// ---------- validated persisted values ----------

/// A persisted `#RRGGBB` color. TOML representation remains a string while
/// direct Config deserialization rejects malformed known values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexRgb {
    red: u8,
    green: u8,
    blue: u8,
}

impl HexRgb {
    pub const fn color(self) -> Color {
        Color::Rgb(self.red, self.green, self.blue)
    }
}

impl fmt::Display for HexRgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue
        )
    }
}

impl FromStr for HexRgb {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 7
            || !value.starts_with('#')
            || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("expected #RRGGBB".into());
        }
        // The validation above guarantees that every sliced byte is ASCII and
        // therefore a UTF-8 character boundary.
        let parse = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&value[range], 16).map_err(|_| "expected #RRGGBB".to_string())
        };
        Ok(Self {
            red: parse(1..3)?,
            green: parse(3..5)?,
            blue: parse(5..7)?,
        })
    }
}

impl Serialize for HexRgb {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HexRgb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Valid paced-reveal rate. Zero disables pacing and reveals content
/// immediately; positive values are limited to 1024 graphemes per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevealRate(u16);

impl RevealRate {
    pub const MAX: u16 = 1024;

    pub fn new(value: u16) -> Result<Self, String> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err("reveal rate must be between 0 and 1024".into())
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn is_disabled(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for RevealRate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RevealRate {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value
            .parse::<u16>()
            .map_err(|_| "reveal rate must be a whole number".to_string())?;
        Self::new(value)
    }
}

impl Serialize for RevealRate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}

impl<'de> Deserialize<'de> for RevealRate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// A persisted message-pane width represented as percentage basis points.
///
/// The public TOML value is a percentage such as `60.0`; storing hundredths
/// internally keeps layout arithmetic deterministic while retaining useful
/// precision on wide terminals. Values are limited to 25.00% through 100.00%.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneWidthPercent(u16);

impl PaneWidthPercent {
    pub const MIN_BASIS_POINTS: u16 = 2_500;
    pub const MAX_BASIS_POINTS: u16 = 10_000;
    pub const DEFAULT_BASIS_POINTS: u16 = 6_000;

    pub const fn from_basis_points(value: u16) -> Option<Self> {
        if value >= Self::MIN_BASIS_POINTS && value <= Self::MAX_BASIS_POINTS {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn from_percent(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err("message pane percentage must be finite".into());
        }
        let basis_points = value * 100.0;
        let rounded = basis_points.round();
        if (basis_points - rounded).abs() > 1e-7 {
            return Err("message pane percentage supports at most two decimals".into());
        }
        let basis_points = rounded as i64;
        if !(i64::from(Self::MIN_BASIS_POINTS)..=i64::from(Self::MAX_BASIS_POINTS))
            .contains(&basis_points)
        {
            return Err("message pane percentage must be between 25 and 100".into());
        }
        Ok(Self(basis_points as u16))
    }

    pub const fn basis_points(self) -> u16 {
        self.0
    }

    pub fn as_percent(self) -> f64 {
        f64::from(self.0) / 100.0
    }

    pub fn display(self) -> String {
        format!("{:.2}%", self.as_percent())
    }

    /// Convert this percentage to a terminal-column count using nearest-cell
    /// rounding. The result is always within `0..=total_columns`.
    pub fn columns(self, total_columns: u16) -> u16 {
        let total = u32::from(total_columns);
        ((total * u32::from(self.0) + 5_000) / 10_000).min(total) as u16
    }

    /// Convert a committed terminal-column position back to the closest
    /// representable percentage, clamped to the message-pane minimum.
    pub fn from_columns(columns: u16, total_columns: u16) -> Self {
        if total_columns == 0 {
            return Self::from_basis_points(Self::MIN_BASIS_POINTS)
                .expect("percentage minimum is valid");
        }
        let columns = columns.min(total_columns);
        let basis_points = ((u32::from(columns) * 10_000 + u32::from(total_columns) / 2)
            / u32::from(total_columns))
        .clamp(
            u32::from(Self::MIN_BASIS_POINTS),
            u32::from(Self::MAX_BASIS_POINTS),
        ) as u16;
        Self::from_basis_points(basis_points).expect("clamped percentage is valid")
    }
}

impl Default for PaneWidthPercent {
    fn default() -> Self {
        Self::from_basis_points(Self::DEFAULT_BASIS_POINTS).expect("default percentage is valid")
    }
}

impl fmt::Display for PaneWidthPercent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:.2}%", self.as_percent())
    }
}

impl Serialize for PaneWidthPercent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.as_percent())
    }
}

struct PaneWidthPercentVisitor;

impl<'de> de::Visitor<'de> for PaneWidthPercentVisitor {
    type Value = PaneWidthPercent;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a message pane percentage from 25.00 to 100.00")
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        PaneWidthPercent::from_percent(value).map_err(E::custom)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }
}

impl<'de> Deserialize<'de> for PaneWidthPercent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(PaneWidthPercentVisitor)
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
    /// Foreground-fade interpolation origin; does not replace theme surfaces.
    pub background_color: HexRgb,
    /// Composer chrome: `default`, `square`, `rounded`, or `line`.
    pub input_style: String,
    // Behavior
    pub language: Language,
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
    /// Maximum visible assistant-reply graphemes per second.
    pub message_chars_per_second: RevealRate,
    /// Maximum visible wrapped Preview display rows per second.
    pub preview_lines_per_second: RevealRate,
    /// Horizontal gutter (in columns) of user message blocks and the input
    /// box — live-editable via /settings.
    pub user_input_padding: usize,
    /// Persisted message-pane share. Pane columns are derived from the
    /// current terminal width; Preview may collapse responsively below its
    /// minimum without changing this committed percentage.
    pub message_pane_percent: PaneWidthPercent,
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

    pub fn input_style_mode(&self) -> InputStyle {
        match self.input_style.as_str() {
            "square" => InputStyle::Square,
            "rounded" => InputStyle::Rounded,
            "line" => InputStyle::Line,
            _ => InputStyle::Default,
        }
    }

    /// Stable persisted value used by settings choices.
    pub fn input_style_value(&self) -> &'static str {
        self.input_style_mode().value()
    }

    /// Stable persisted value used by settings choices.
    pub fn thinking_display_value(&self) -> &'static str {
        match self.thinking_display_mode() {
            ThinkingDisplayMode::Compact => "compact",
            ThinkingDisplayMode::Lines => "lines",
            ThinkingDisplayMode::Full => "full",
        }
    }

    /// Stable persisted value used by the page-alignment choice.
    pub fn page_align_value(&self) -> &'static str {
        match self.page_align.as_str() {
            "left" => "left",
            "right" => "right",
            _ => "center",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(config.language, Language::English);
        assert_eq!(config.spinner_frame_ms, 250);
        let defaults = Config::default();
        assert_eq!(config.background_color, defaults.background_color);
        assert_eq!(
            config.message_chars_per_second,
            defaults.message_chars_per_second
        );
        assert_eq!(
            config.preview_lines_per_second,
            defaults.preview_lines_per_second
        );
        assert_eq!(config.page_align, "right");
        assert_eq!(config.spinner_style, "A");
        assert_eq!(config.history_limit, 1000);
        assert_eq!(config.thinking_display, "compact");
        assert_eq!(config.thinking_lines, 2);
        assert_eq!(config.resolved_theme.user, Theme::ferra().user);
    }

    #[test]
    fn language_inherits_validates_round_trips_and_falls_back_safely() {
        assert_eq!(Config::default().language, Language::English);
        assert_eq!(
            Config::from_user_toml("theme = \"ferra\"")
                .unwrap()
                .language,
            Language::English
        );

        let chinese = Config::from_user_toml("language = \"zh-CN\"").unwrap();
        assert_eq!(chinese.language, Language::SimplifiedChinese);
        let persisted = toml::to_string(&chinese).unwrap();
        assert!(persisted.contains("language = \"zh-CN\""));
        assert_eq!(
            Config::from_user_toml(&persisted).unwrap().language,
            Language::SimplifiedChinese
        );

        assert!(Config::from_user_toml("language = \"fr\"").is_err());
        assert_eq!(
            Config::user_toml_or_default("language = \"fr\"").language,
            Language::English
        );
    }

    #[test]
    fn pane_width_percent_uses_basis_points_and_rounds_columns_consistently() {
        let percent = PaneWidthPercent::from_percent(61.25).unwrap();
        assert_eq!(percent.basis_points(), 6_125);
        assert_eq!(percent.columns(800), 490);
        assert_eq!(PaneWidthPercent::from_columns(490, 800), percent);
        assert_eq!(
            PaneWidthPercent::from_columns(0, 800),
            PaneWidthPercent::from_basis_points(2_500).unwrap()
        );
        assert_eq!(
            PaneWidthPercent::from_columns(800, 800),
            PaneWidthPercent::from_basis_points(10_000).unwrap()
        );
        assert!(PaneWidthPercent::from_percent(24.99).is_err());
        assert!(PaneWidthPercent::from_percent(100.01).is_err());
        assert!(PaneWidthPercent::from_percent(61.251).is_err());
    }

    #[test]
    fn pane_width_percent_round_trips_as_a_percentage_value() {
        let config = Config::from_user_toml("message_pane_percent = 61.25").unwrap();
        assert_eq!(config.message_pane_percent.display(), "61.25%");
        let persisted = toml::to_string(&config).unwrap();
        assert!(persisted.contains("message_pane_percent = 61.25"));
    }

    #[test]
    fn pane_width_percent_rejects_invalid_values() {
        assert!(Config::from_user_toml("message_pane_percent = 24.99").is_err());
        assert!(Config::from_user_toml("message_pane_percent = 100.01").is_err());
        assert!(Config::from_user_toml("message_pane_percent = \"wide\"").is_err());
    }

    #[test]
    fn valid_reveal_values_override_and_normalize() {
        let config = Config::from_user_toml(
            r##"
                background_color = "#1A2b3C"
                message_chars_per_second = 7
                preview_lines_per_second = 1024
            "##,
        )
        .unwrap();
        assert_eq!(config.background_color.to_string(), "#1a2b3c");
        assert_eq!(
            config.background_color.color(),
            Color::Rgb(0x1a, 0x2b, 0x3c)
        );
        assert_eq!(config.message_chars_per_second.get(), 7);
        assert_eq!(config.preview_lines_per_second.get(), 1024);
        let persisted = toml::to_string(&config).unwrap();
        assert!(persisted.contains("background_color = \"#1a2b3c\""));
        assert!(persisted.contains("message_chars_per_second = 7"));
    }

    #[test]
    fn invalid_reveal_values_are_strict_known_value_errors() {
        assert!(Config::from_user_toml("background_color = \"black\"").is_err());
        assert!(Config::from_user_toml("background_color = \"#aééb\"").is_err());
        assert!("#aééb".parse::<HexRgb>().is_err());
        assert_eq!(
            Config::from_user_toml("message_chars_per_second = 0")
                .unwrap()
                .message_chars_per_second
                .get(),
            0
        );
        assert!(Config::from_user_toml("preview_lines_per_second = 1025").is_err());
        assert!("-1".parse::<RevealRate>().is_err());
        assert!("1.5".parse::<RevealRate>().is_err());
    }

    #[test]
    fn known_invalid_types_and_malformed_toml_take_the_safe_full_fallback() {
        assert!(Config::from_user_toml("spinner_frame_ms = \"fast\"").is_err());
        assert!(Config::from_user_toml("theme = [").is_err());
        let fallback = Config::user_toml_or_default("history_limit = \"many\"");
        assert_eq!(fallback.history_limit, Config::default().history_limit);
        assert_eq!(fallback.theme, Config::default().theme);
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
        assert!(text.contains(&format!("theme = {:?}", Config::default().theme)));
    }
}
