//! Two-layer theme registry.
//!
//! Theme TOML files contain an open-ended `[colors]` palette and a fixed
//! `[semantics.*]` schema. Semantic styles reference palette names; `fg` is
//! required while `bg`, `bold`, `italic`, and `underline` are optional. The
//! built-in files live in `client/assets/themes/`, are embedded with
//! `include_str!`, parsed through the same path as user themes, and copied to
//! `%APPDATA%\dshe\themes\` as editable starting points.

use std::{collections::BTreeMap, fs, path::Path};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

const DEEPSEEK_E_SOURCE: &str = include_str!("../assets/themes/deepseek-e.toml");
const FERRA_SOURCE: &str = include_str!("../assets/themes/ferra.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeStyle {
    pub fg: Color,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
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

        #[derive(Clone, Copy, Debug)]
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
    code_text,
    code_meta,
    code_background,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticsRef {
    surface: SurfaceRef,
    markdown: MarkdownRef,
    input: InputRef,
    working_status: WorkingStatusRef,
    log: LogRef,
    activity: ActivityRef,
    card: CardRef,
    overlay: OverlayRef,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeDocument {
    name: String,
    colors: BTreeMap<String, String>,
    semantics: SemanticsRef,
}

/// Fully resolved, render-time theme. The nested fields are the public semantic
/// API. The flat color aliases remain temporarily for existing render helpers;
/// each is derived from a semantic role rather than directly from a palette.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub surface: SurfaceTheme,
    pub markdown: MarkdownTheme,
    pub input: InputTheme,
    pub working_status: WorkingStatusTheme,
    pub log: LogTheme,
    pub activity: ActivityTheme,
    pub card: CardTheme,
    pub overlay: OverlayTheme,

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
}

impl Theme {
    pub fn deepseek_e() -> Self {
        builtin_theme("deepseek-e")
    }

    pub fn ferra() -> Self {
        builtin_theme("ferra")
    }

    /// Built-in fallback by name (unknown names fall back to deepseek-e).
    pub fn from_name(name: &str) -> Self {
        match name {
            "ferra" => Self::ferra(),
            _ => Self::deepseek_e(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::deepseek_e()
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
    let input = document.semantics.input.resolve(&colors)?;
    let working_status = document.semantics.working_status.resolve(&colors)?;
    let log = document.semantics.log.resolve(&colors)?;
    let activity = document.semantics.activity.resolve(&colors)?;
    let card = document.semantics.card.resolve(&colors)?;
    let overlay = document.semantics.overlay.resolve(&colors)?;

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
        surface,
        markdown,
        input,
        working_status,
        log,
        activity,
        card,
        overlay,
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

fn builtin_sources() -> [(&'static str, &'static str); 2] {
    [("deepseek-e", DEEPSEEK_E_SOURCE), ("ferra", FERRA_SOURCE)]
}

fn builtin_theme_file(name: &str) -> ThemeFile {
    let source = builtin_sources()
        .into_iter()
        .find_map(|(candidate, source)| (candidate == name).then_some(source))
        .unwrap_or(DEEPSEEK_E_SOURCE);
    parse_theme_result(source).expect("embedded theme must be valid")
}

fn builtin_theme(name: &str) -> Theme {
    builtin_theme_file(name).theme
}

pub fn default_themes() -> Vec<(&'static str, Theme)> {
    builtin_sources()
        .into_iter()
        .map(|(name, _)| (name, builtin_theme(name)))
        .collect()
}

/// Copy embedded theme TOML files as editable starting points without
/// overwriting user changes.
pub fn ensure_default_themes(dir: &Path) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    for (name, source) in builtin_sources() {
        let path = dir.join(format!("{name}.toml"));
        if !path.exists() {
            let _ = fs::write(path, source);
        }
    }
}

/// Discover every legal user theme in the directory, sorted by name.
pub fn discover_themes(dir: &Path) -> Vec<ThemeFile> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(file) = parse_theme(&text) {
            out.push(file);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Load embedded themes plus legal user themes. A legal user file with the
/// same `name` overrides the embedded definition; invalid legacy files do not
/// hide the built-in fallback.
pub fn load_themes(dir: &Path) -> Vec<ThemeFile> {
    ensure_default_themes(dir);
    let mut themes: Vec<ThemeFile> = builtin_sources()
        .into_iter()
        .map(|(name, _)| builtin_theme_file(name))
        .collect();
    for file in discover_themes(dir) {
        if let Some(index) = themes.iter().position(|theme| theme.name == file.name) {
            themes[index] = file;
        } else {
            themes.push(file);
        }
    }
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
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
    fn embedded_themes_parse_through_the_public_schema() {
        let deepseek = parse_theme(DEEPSEEK_E_SOURCE).expect("deepseek-e parses");
        let ferra = parse_theme(FERRA_SOURCE).expect("ferra parses");
        assert_eq!(deepseek.name, "deepseek-e");
        assert_eq!(ferra.name, "ferra");
        assert_eq!(ferra.theme.bg, Color::Rgb(0x2b, 0x29, 0x2d));
        assert_eq!(
            ferra.theme.markdown.heading2.fg,
            Color::Rgb(0xb1, 0xb6, 0x95)
        );
        assert!(ferra.theme.markdown.heading2.bold);
        assert_eq!(
            ferra.theme.markdown.heading3.fg,
            Color::Rgb(0xfe, 0xcd, 0xb2)
        );
        assert!(!ferra.theme.markdown.heading3.bold);
    }

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
        assert!(!theme.markdown.heading3.bold);
        assert!(!theme.markdown.heading3.italic);
        assert_eq!(theme.markdown.heading3.bg, None);
    }

    #[test]
    fn unknown_palette_reference_and_missing_semantic_are_rejected() {
        let unknown = FERRA_SOURCE.replacen("fg = \"mist\"", "fg = \"missing\"", 1);
        assert!(parse_theme(&unknown).is_none());
        let missing = FERRA_SOURCE.replace("heading6 = { fg = \"bark\" }\n", "");
        assert!(parse_theme(&missing).is_none());
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
        assert_eq!(resolve("nope", &themes).user, Theme::deepseek_e().user);
    }

    #[test]
    fn embedded_sources_are_copied_verbatim_without_overwrite() {
        let dir = std::env::temp_dir().join(format!(
            "dshe-theme-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        ensure_default_themes(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("ferra.toml")).unwrap(),
            FERRA_SOURCE
        );
        fs::write(dir.join("ferra.toml"), "user edit").unwrap();
        ensure_default_themes(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("ferra.toml")).unwrap(),
            "user edit"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
