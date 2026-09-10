//! Two-layer theme registry.
//!
//! Theme TOML files contain an open-ended `[colors]` palette and a fixed
//! `[semantics.*]` schema. Semantic styles reference palette names; `fg` is
//! required while `bg`, `bold`, `italic`, and `underline` are optional. The
//! built-in files live in `e-tui/assets/themes/`, are embedded with
//! `include_str!`, and parsed through the same value-schema path as user
//! themes. Filesystem discovery and installation belong to the executable.

use std::collections::BTreeMap;

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

const FERRA_SOURCE: &str = include_str!("../assets/themes/ferra.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Padding {
    All(u16),
    Separate { left: u16, right: u16 },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeparatePadding {
    left: u16,
    right: u16,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PaddingWire {
    All(u16),
    Separate(SeparatePadding),
}

impl<'de> Deserialize<'de> for Padding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match PaddingWire::deserialize(deserializer)? {
            PaddingWire::All(value) => Self::All(value),
            PaddingWire::Separate(SeparatePadding { left, right }) => {
                Self::Separate { left, right }
            }
        })
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::All(0)
    }
}

impl Padding {
    pub const MAX: u16 = 64;

    pub fn left(self) -> usize {
        match self {
            Self::All(value) => usize::from(value),
            Self::Separate { left, .. } => usize::from(left),
        }
    }

    pub fn right(self) -> usize {
        match self {
            Self::All(value) => usize::from(value),
            Self::Separate { right, .. } => usize::from(right),
        }
    }

    fn validate(self) -> Result<Self, String> {
        let exceeds = |value| value > Self::MAX;
        match self {
            Self::All(value) if exceeds(value) => {
                Err(format!("padding must be between 0 and {}", Self::MAX))
            }
            Self::Separate { left, right } if exceeds(left) || exceeds(right) => {
                Err(format!("padding must be between 0 and {}", Self::MAX))
            }
            padding => Ok(padding),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThemeStyle {
    pub fg: Color,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub padding: Padding,
}

impl ThemeStyle {
    pub fn style(self) -> Style {
        let mut style = Style::default().fg(self.fg);
        if let Some(bg) = self.bg {
            style = style.bg(bg);
        }
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleRef {
    fg: String,
    #[serde(default)]
    bg: Option<String>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    underline: bool,
    #[serde(default)]
    padding: Padding,
}

impl StyleRef {
    fn resolve(&self, colors: &BTreeMap<String, Color>) -> Result<ThemeStyle, String> {
        let lookup = |name: &str| {
            colors
                .get(name)
                .copied()
                .ok_or_else(|| format!("unknown palette color `{name}`"))
        };
        Ok(ThemeStyle {
            fg: lookup(&self.fg)?,
            bg: self.bg.as_deref().map(lookup).transpose()?,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            padding: self.padding.validate()?,
        })
    }
}

macro_rules! style_group {
    ($raw:ident => $resolved:ident { $($field:ident),+ $(,)? }) => {
        #[derive(Clone, Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $raw {
            $( $field: StyleRef, )+
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $resolved {
            $( pub $field: ThemeStyle, )+
        }

        impl $raw {
            fn resolve(&self, colors: &BTreeMap<String, Color>) -> Result<$resolved, String> {
                Ok($resolved {
                    $( $field: self.$field.resolve(colors)?, )+
                })
            }
        }
    };
}

style_group!(SurfaceRef => SurfaceTheme {
    base,
    panel,
    selection,
    primary_text,
    muted_text,
});

style_group!(MarkdownRef => MarkdownTheme {
    text,
    heading1,
    heading2,
    heading3,
    heading4,
    heading5,
    heading6,
    emphasis,
    strong,
    strikethrough,
    inline_code,
    link_text,
    link_url,
    image,
    quote_marker,
    rule,
    code_block_bg,
    table_border,
    table_header,
    list_marker,
    task_checked,
    task_unchecked,
    mermaid_border,
    mermaid_node,
    mermaid_edge,
    mermaid_edge_label,
    mermaid_title,
});

// Dedicated code-coloring semantic group. The token roles below map onto
// syntect scope families (see `syntax.rs`) and transfer only
// foreground/bold/italic/underline; backgrounds stay at the block level
// (`markdown.code_block_bg`) or the diff level (`semantics.diff`). `text` is
// the syntax default/plain fallback foreground and `meta` colors the
// `lang · N 行` header and diff metadata rows.
style_group!(CodeRef => CodeTheme {
    text,
    comment,
    keyword,
    r#type,
    function,
    string,
    constant,
    attribute,
    escape,
    invalid,
    meta,
});

style_group!(InputRef => InputTheme {
    background,
    text,
    prompt,
    cursor,
    hint,
    placeholder,
    selection,
    status,
    status_hint,
    status_accent,
});

style_group!(WorkingStatusRef => WorkingStatusTheme {
    idle,
    waiting,
    running,
    success,
    failure,
    cancelled,
});

style_group!(LogRef => LogTheme { info, warning, error });
style_group!(ActivityRef => ActivityTheme { label, detail, metadata });
style_group!(CardRef => CardTheme { user, context, detail, attachment });
style_group!(OverlayRef => OverlayTheme {
    background,
    border,
    text,
    muted,
    selection,
    accent,
    selected_marker,
    unselected_marker,
});

style_group!(DiffRef => DiffTheme {
    text,
    added,
    removed,
    added_accent,
    removed_accent,
    context_accent,
    separator,
});

// Pane-separator drag affordances: the idle grip (`bar`), the full-height
// guide drawn while dragging (`line`), and the margin-inset placeholder boxes
// shown during a drag (`placeholder`, text foreground + fill background).
style_group!(SeparatorRef => SeparatorTheme {
    bar,
    line,
    placeholder,
});

style_group!(HistoryOperationRef => HistoryOperationTheme {
    model, read, edit, bash, search, other,
});

style_group!(HistoryDurationRef => HistoryDurationTheme {
    highest, second, top_five, remaining, unknown,
});

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRef {
    text: StyleRef,
    heading: StyleRef,
    metadata: StyleRef,
    total_elapsed: StyleRef,
    separator: StyleRef,
    hint: StyleRef,
    progress: StyleRef,
    bar_text: StyleRef,
    operation: HistoryOperationRef,
    duration: HistoryDurationRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HistoryTheme {
    pub text: ThemeStyle,
    pub heading: ThemeStyle,
    pub metadata: ThemeStyle,
    pub total_elapsed: ThemeStyle,
    pub separator: ThemeStyle,
    pub hint: ThemeStyle,
    pub progress: ThemeStyle,
    pub bar_text: ThemeStyle,
    pub operation: HistoryOperationTheme,
    pub duration: HistoryDurationTheme,
}

impl HistoryRef {
    fn resolve(&self, colors: &BTreeMap<String, Color>) -> Result<HistoryTheme, String> {
        Ok(HistoryTheme {
            text: self.text.resolve(colors)?,
            heading: self.heading.resolve(colors)?,
            metadata: self.metadata.resolve(colors)?,
            total_elapsed: self.total_elapsed.resolve(colors)?,
            separator: self.separator.resolve(colors)?,
            hint: self.hint.resolve(colors)?,
            progress: self.progress.resolve(colors)?,
            bar_text: self.bar_text.resolve(colors)?,
            operation: self.operation.resolve(colors)?,
            duration: self.duration.resolve(colors)?,
        })
    }
}

impl HistoryTheme {
    fn from_existing(
        surface: SurfaceTheme,
        code: CodeTheme,
        status: WorkingStatusTheme,
        separator: SeparatorTheme,
    ) -> Self {
        let foreground = |style: ThemeStyle| ThemeStyle {
            bg: None,
            padding: Padding::All(0),
            ..style
        };
        Self {
            text: foreground(surface.primary_text),
            heading: foreground(surface.primary_text),
            metadata: foreground(surface.muted_text),
            total_elapsed: foreground(surface.primary_text),
            separator: foreground(separator.line),
            hint: foreground(separator.line),
            progress: foreground(code.r#type),
            bar_text: ThemeStyle {
                fg: surface.base.bg.unwrap_or(Color::Reset),
                ..foreground(surface.primary_text)
            },
            operation: HistoryOperationTheme {
                model: foreground(surface.muted_text),
                read: foreground(code.string),
                edit: foreground(code.keyword),
                bash: foreground(code.r#type),
                search: foreground(code.constant),
                other: foreground(surface.muted_text),
            },
            duration: HistoryDurationTheme {
                highest: foreground(status.failure),
                second: foreground(code.r#type),
                top_five: foreground(surface.primary_text),
                remaining: foreground(surface.muted_text),
                unknown: foreground(surface.muted_text),
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticsRef {
    surface: SurfaceRef,
    markdown: MarkdownRef,
    markdown_weak: MarkdownRef,
    code: CodeRef,
    code_weak: CodeRef,
    input: InputRef,
    working_status: WorkingStatusRef,
    log: LogRef,
    activity: ActivityRef,
    card: CardRef,
    overlay: OverlayRef,
    diff: DiffRef,
    separator: SeparatorRef,
    #[serde(default)]
    history: Option<HistoryRef>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeDocument {
    name: String,
    colors: BTreeMap<String, String>,
    semantics: SemanticsRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StatusFlashTheme {
    pub model: Color,
    pub effort_max: Color,
    pub effort_xhigh: Color,
    pub effort_high: Color,
}

/// Fully resolved, render-time theme. The nested fields are the public semantic
/// API. The flat color aliases remain temporarily for existing render helpers;
/// each is derived from a semantic role rather than directly from a palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Theme {
    pub surface: SurfaceTheme,
    pub markdown: MarkdownTheme,
    pub markdown_weak: MarkdownTheme,
    pub code: CodeTheme,
    pub code_weak: CodeTheme,
    pub input: InputTheme,
    pub working_status: WorkingStatusTheme,
    pub log: LogTheme,
    pub activity: ActivityTheme,
    pub card: CardTheme,
    pub overlay: OverlayTheme,
    pub diff: DiffTheme,
    pub separator: SeparatorTheme,
    pub history: HistoryTheme,
    pub status_flash: StatusFlashTheme,

    // Compatibility aliases for render paths that combine semantic roles.
    pub bg: Color,
    pub bg_soft: Color,
    pub selection: Color,
    pub dim: Color,
    pub fg: Color,
    pub ok: Color,
    pub link: Color,
    pub user: Color,
    pub rose: Color,
    pub err: Color,
    pub running: Color,
    /// Flat palette alias for the `coral` palette color (top-level list
    /// bullets). Themes without a `coral` palette entry fall back to the
    /// `card.detail` tone, keeping the alias safe for older user themes.
    pub coral: Color,
}

impl Theme {
    pub fn ferra() -> Self {
        builtin_theme("ferra")
    }

    /// Built-in fallback by name (unknown names fall back to Ferra).
    pub fn from_name(name: &str) -> Self {
        builtin_theme(name)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::ferra()
    }
}

#[derive(Clone, Debug)]
pub struct ThemeFile {
    pub name: String,
    theme: Theme,
}

impl ThemeFile {
    /// Test/helper constructor for an already-resolved theme.
    pub fn from_theme(name: &str, theme: Theme) -> Self {
        Self {
            name: name.to_string(),
            theme,
        }
    }

    pub fn is_legal(&self) -> bool {
        !self.name.trim().is_empty()
    }

    pub fn to_theme(&self) -> Option<Theme> {
        self.is_legal().then_some(self.theme)
    }
}

fn parse_hex(value: &str) -> Option<Color> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&value[0..2], 16).ok()?;
    let g = u8::from_str_radix(&value[2..4], 16).ok()?;
    let b = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn resolve_document(document: ThemeDocument) -> Result<ThemeFile, String> {
    if document.name.trim().is_empty() {
        return Err("theme name must not be empty".into());
    }
    if document.colors.is_empty() {
        return Err("theme palette must not be empty".into());
    }

    let mut colors = BTreeMap::new();
    for (name, value) in &document.colors {
        if name.trim().is_empty() {
            return Err("palette color names must not be empty".into());
        }
        let color = parse_hex(value)
            .ok_or_else(|| format!("palette color `{name}` is not a 6-digit hex color"))?;
        colors.insert(name.clone(), color);
    }

    let surface = document.semantics.surface.resolve(&colors)?;
    let markdown = document.semantics.markdown.resolve(&colors)?;
    let markdown_weak = document.semantics.markdown_weak.resolve(&colors)?;
    let code = document.semantics.code.resolve(&colors)?;
    let code_weak = document.semantics.code_weak.resolve(&colors)?;
    let input = document.semantics.input.resolve(&colors)?;
    let working_status = document.semantics.working_status.resolve(&colors)?;
    let log = document.semantics.log.resolve(&colors)?;
    let activity = document.semantics.activity.resolve(&colors)?;
    let card = document.semantics.card.resolve(&colors)?;
    let overlay = document.semantics.overlay.resolve(&colors)?;
    let diff = document.semantics.diff.resolve(&colors)?;
    let separator = document.semantics.separator.resolve(&colors)?;
    let history = document
        .semantics
        .history
        .as_ref()
        .map(|history| history.resolve(&colors))
        .transpose()?
        .unwrap_or_else(|| HistoryTheme::from_existing(surface, code, working_status, separator));

    // `fg` is the only required style property. Background-oriented roles
    // gracefully inherit when `bg` is omitted rather than making the schema
    // stricter for those roles.
    let bg = surface.base.bg.unwrap_or(Color::Reset);
    let bg_soft = surface.panel.bg.unwrap_or(bg);
    let selection = surface.selection.bg.unwrap_or(bg_soft);
    let theme = Theme {
        bg,
        bg_soft,
        selection,
        dim: surface.muted_text.fg,
        fg: surface.primary_text.fg,
        ok: working_status.success.fg,
        link: markdown.link_url.fg,
        user: input.prompt.fg,
        rose: markdown.emphasis.fg,
        err: log.error.fg,
        running: working_status.running.fg,
        coral: colors.get("coral").copied().unwrap_or(card.detail.fg),
        status_flash: StatusFlashTheme {
            model: colors
                .get("mist")
                .copied()
                .unwrap_or(surface.primary_text.fg),
            effort_max: colors
                .get("ember")
                .copied()
                .unwrap_or(working_status.failure.fg),
            effort_xhigh: colors
                .get("honey")
                .copied()
                .unwrap_or(working_status.running.fg),
            effort_high: colors.get("blush").copied().unwrap_or(code.r#type.fg),
        },
        surface,
        markdown,
        markdown_weak,
        code,
        code_weak,
        input,
        working_status,
        log,
        activity,
        card,
        overlay,
        diff,
        separator,
        history,
    };

    Ok(ThemeFile {
        name: document.name,
        theme,
    })
}

fn parse_theme_result(text: &str) -> Result<ThemeFile, String> {
    let document: ThemeDocument = toml::from_str(text).map_err(|error| error.to_string())?;
    resolve_document(document)
}

/// Parse and validate one two-layer theme file.
pub fn parse_theme(text: &str) -> Option<ThemeFile> {
    parse_theme_result(text).ok()
}

pub fn builtin_theme_sources() -> [(&'static str, &'static str); 6] {
    [
        ("ferra", FERRA_SOURCE),
        (
            "rider-dark",
            include_str!("../assets/themes/rider-dark.toml"),
        ),
        ("dracula", include_str!("../assets/themes/dracula.toml")),
        (
            "catppuccin",
            include_str!("../assets/themes/catppuccin.toml"),
        ),
        ("one-dark", include_str!("../assets/themes/one-dark.toml")),
        (
            "synthwave-84",
            include_str!("../assets/themes/synthwave-84.toml"),
        ),
    ]
}

fn builtin_theme_file(name: &str) -> ThemeFile {
    let source = builtin_theme_sources()
        .into_iter()
        .find_map(|(candidate, source)| (candidate == name).then_some(source))
        .unwrap_or(FERRA_SOURCE);
    parse_theme_result(source).expect("embedded theme must be valid")
}

fn builtin_theme(name: &str) -> Theme {
    builtin_theme_file(name).theme
}

pub fn default_themes() -> Vec<(&'static str, Theme)> {
    builtin_theme_sources()
        .into_iter()
        .map(|(name, _)| (name, builtin_theme(name)))
        .collect()
}

/// Resolve a discovered theme, then fall back to an embedded built-in.
pub fn resolve(name: &str, themes: &[ThemeFile]) -> Theme {
    themes
        .iter()
        .find(|theme| theme.name == name)
        .and_then(ThemeFile::to_theme)
        .unwrap_or_else(|| Theme::from_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_names_are_open_ended_and_style_flags_are_optional() {
        let source = FERRA_SOURCE
            .replace("night = \"#2b292d\"", "my_custom_night = \"#010203\"")
            .replace("bg = \"night\"", "bg = \"my_custom_night\"")
            .replace("fg = \"night\"", "fg = \"my_custom_night\"");
        let theme = parse_theme(&source)
            .expect("arbitrary palette key parses")
            .theme;
        assert_eq!(theme.bg, Color::Rgb(1, 2, 3));
        assert!(theme.markdown.heading3.bold);
        assert!(!theme.markdown.heading3.italic);
        assert_eq!(theme.markdown.heading3.bg, None);
    }

    #[test]
    fn unknown_palette_reference_and_missing_semantic_are_rejected() {
        let unknown = FERRA_SOURCE.replacen("fg = \"mist\"", "fg = \"missing\"", 1);
        assert!(parse_theme(&unknown).is_none());
        let missing = FERRA_SOURCE.replacen("heading6 = { fg = \"bark\" }", "", 1);
        assert!(parse_theme(&missing).is_none());
        let missing_weak =
            FERRA_SOURCE.replace("[semantics.markdown_weak]", "[ignored.markdown_weak]");
        assert!(parse_theme(&missing_weak).is_none());
        let illegal_weak = FERRA_SOURCE.replacen(
            "[semantics.markdown_weak]",
            "[semantics.markdown_weak]\nunknown = { fg = \"bark\" }",
            1,
        );
        assert!(parse_theme(&illegal_weak).is_none());
        let missing_code = FERRA_SOURCE.replace("[semantics.code]", "[ignored.code]");
        assert!(parse_theme(&missing_code).is_none());
        let missing_code_weak =
            FERRA_SOURCE.replace("[semantics.code_weak]", "[ignored.code_weak]");
        assert!(parse_theme(&missing_code_weak).is_none());
    }

    #[test]
    fn history_schema_accepts_builtins_and_legacy_custom_palettes() {
        fn rename_references(value: &mut toml::Value) {
            if let Some(table) = value.as_table_mut() {
                for (key, value) in table {
                    if key == "fg" || key == "bg" {
                        *value = toml::Value::String(format!("custom_{}", value.as_str().unwrap()));
                    } else {
                        rename_references(value);
                    }
                }
            }
        }
        for (name, source) in builtin_theme_sources() {
            let mut document: toml::Value = toml::from_str(source).unwrap();
            assert!(document["semantics"].get("history").is_some(), "{name}");
            assert!(parse_theme(source).is_some(), "{name}");
            document["semantics"]
                .as_table_mut()
                .unwrap()
                .remove("history");
            let palette = document["colors"].as_table_mut().unwrap();
            *palette = std::mem::take(palette)
                .into_iter()
                .map(|(key, value)| (format!("custom_{key}"), value))
                .collect();
            rename_references(&mut document["semantics"]);
            assert!(
                parse_theme(&toml::to_string(&document).unwrap()).is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn explicit_history_schema_rejects_partial_unknown_and_unresolved_roles() {
        let document: toml::Value = toml::from_str(FERRA_SOURCE).unwrap();
        for group in [None, Some("operation"), Some("duration")] {
            let roles = match group {
                Some(group) => &document["semantics"]["history"][group],
                None => &document["semantics"]["history"],
            }
            .as_table()
            .unwrap();
            for key in roles.keys() {
                let mut incomplete = document.clone();
                let target = &mut incomplete["semantics"]["history"];
                let target = match group {
                    Some(group) => &mut target[group],
                    None => target,
                };
                target.as_table_mut().unwrap().remove(key);
                assert!(parse_theme(&toml::to_string(&incomplete).unwrap()).is_none());
            }
            let mut unknown = document.clone();
            let target = &mut unknown["semantics"]["history"];
            let target = match group {
                Some(group) => &mut target[group],
                None => target,
            };
            target
                .as_table_mut()
                .unwrap()
                .insert("unexpected".into(), roles.values().next().unwrap().clone());
            assert!(parse_theme(&toml::to_string(&unknown).unwrap()).is_none());
        }
        let mut unresolved = document;
        unresolved["semantics"]["history"]["operation"]["bash"]["fg"] =
            "absent_palette_entry".into();
        assert!(parse_theme(&toml::to_string(&unresolved).unwrap()).is_none());
    }

    #[test]
    fn missing_separator_group_is_rejected() {
        let missing = FERRA_SOURCE.replace("[semantics.separator]", "[ignored.separator]");
        assert!(parse_theme(&missing).is_none());
    }

    #[test]
    fn padding_accepts_scalar_and_separate_tables() {
        // `Padding`'s wire format is theme-file independent: deserialize it
        // from a TOML value directly instead of string-replacing a line inside
        // an embedded theme, which breaks whenever the built-in theme sources
        // change.
        let scalar: std::collections::BTreeMap<String, Padding> =
            toml::from_str("padding = 1").unwrap();
        assert_eq!(scalar["padding"], Padding::All(1));

        let separate: std::collections::BTreeMap<String, Padding> =
            toml::from_str("padding = { left = 2, right = 3 }").unwrap();
        assert_eq!(separate["padding"], Padding::Separate { left: 2, right: 3 });
    }

    #[test]
    fn padding_defaults_to_zero_when_omitted() {
        // `StyleRef.padding` is `#[serde(default)]` and resolves through
        // `Padding::default()`; both are theme-file independent.
        assert_eq!(Padding::default(), Padding::All(0));
        let omitted: StyleRef = toml::from_str("fg = \"rose\"").unwrap();
        assert_eq!(omitted.padding, Padding::All(0));
    }

    #[test]
    fn padding_rejects_invalid_and_unknown_fields() {
        type PaddingMap = std::collections::BTreeMap<String, Padding>;

        // Unknown field in the table form must be rejected (strict table).
        assert!(
            toml::from_str::<PaddingMap>("padding = { left = 1, right = 1, bogus = 1 }").is_err()
        );
        // Missing a required table field is rejected.
        assert!(toml::from_str::<PaddingMap>("padding = { left = 1 }").is_err());
        // Out-of-range values are rejected at resolve time (`validate()`), not
        // at deserialization.
        assert!(Padding::All(65).validate().is_err());
        assert!(Padding::Separate { left: 1, right: 65 }.validate().is_err());
    }

    #[test]
    fn resolve_prefers_discovered_then_builtin() {
        let mut custom = Theme::ferra();
        custom.input.prompt.fg = Color::Rgb(1, 2, 3);
        custom.user = custom.input.prompt.fg;
        let themes = vec![ThemeFile::from_theme("mine", custom)];
        assert_eq!(
            resolve("mine", &themes).input.prompt.fg,
            Color::Rgb(1, 2, 3)
        );
        assert_eq!(resolve("ferra", &themes).user, Theme::ferra().user);
        assert_eq!(resolve("nope", &themes).user, Theme::ferra().user);
    }
}
