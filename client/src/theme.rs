//! Theme registry: the resolved palette (`Theme`), the two built-in default
//! themes (deepseek-e & ferra), and discovery/validation of `.toml` theme
//! files in `%APPDATA%\dshe\themes\`.
//!
//! A theme file is flat TOML:
//! ```toml
//! name = "deepseek-e"
//! bg = "#141828"      # transcript background
//! bg_soft = "#1e2438" # cards / input bar / user block
//! selection = "#2a3352"
//! dim = "#5c6480"     # secondary text / tool cards
//! fg = "#d8dceb"      # body foreground
//! ok = "#7ad88f"      # success
//! link = "#7aa2ff"    # links
//! user = "#4d6bfe"    # user `❯` / user highlight
//! rose = "#c792ea"    # inline code / emphasis
//! err = "#ff6b7a"     # errors / failures
//! running = "#ffcc66" # running / approvals / warnings
//! ```
//! All eleven colors must parse as 6-digit hex; a file that fails to parse is
//! not a legal theme and is skipped by discovery.

use std::fs;
use std::path::Path;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

// ---------- resolved palette ----------

#[derive(Clone, Copy, Debug)]
pub struct Theme {
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
    /// DeepSeek-branded cool dark palette (the default theme).
    pub fn deepseek_e() -> Self {
        Self {
            bg: Color::Rgb(0x14, 0x18, 0x28),
            bg_soft: Color::Rgb(0x1e, 0x24, 0x38),
            selection: Color::Rgb(0x2a, 0x33, 0x52),
            dim: Color::Rgb(0x5c, 0x64, 0x80),
            fg: Color::Rgb(0xd8, 0xdc, 0xeb),
            ok: Color::Rgb(0x7a, 0xd8, 0x8f),
            link: Color::Rgb(0x7a, 0xa2, 0xff),
            user: Color::Rgb(0x4d, 0x6b, 0xfe),
            rose: Color::Rgb(0xc7, 0x92, 0xea),
            err: Color::Rgb(0xff, 0x6b, 0x7a),
            running: Color::Rgb(0xff, 0xcc, 0x66),
        }
    }

    /// Ferra palette (casperstorm/ferra).
    pub fn ferra() -> Self {
        Self {
            bg: Color::Rgb(0x2b, 0x29, 0x2d),
            bg_soft: Color::Rgb(0x38, 0x35, 0x39),
            selection: Color::Rgb(0x4d, 0x42, 0x4b),
            dim: Color::Rgb(0x6f, 0x5d, 0x63),
            fg: Color::Rgb(0xd1, 0xd1, 0xe0),
            ok: Color::Rgb(0xb1, 0xb6, 0x95),
            link: Color::Rgb(0xfe, 0xcd, 0xb2),
            user: Color::Rgb(0xff, 0xa0, 0x7a),
            rose: Color::Rgb(0xf6, 0xb6, 0xc9),
            err: Color::Rgb(0xe0, 0x6b, 0x75),
            running: Color::Rgb(0xf5, 0xd7, 0x6e),
        }
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

fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

// ---------- theme files ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ThemeFile {
    pub name: String,
    pub bg: String,
    pub bg_soft: String,
    pub selection: String,
    pub dim: String,
    pub fg: String,
    pub ok: String,
    pub link: String,
    pub user: String,
    pub rose: String,
    pub err: String,
    pub running: String,
}

impl Default for ThemeFile {
    fn default() -> Self {
        Self {
            name: String::new(),
            bg: String::new(),
            bg_soft: String::new(),
            selection: String::new(),
            dim: String::new(),
            fg: String::new(),
            ok: String::new(),
            link: String::new(),
            user: String::new(),
            rose: String::new(),
            err: String::new(),
            running: String::new(),
        }
    }
}

impl ThemeFile {
    pub fn from_theme(name: &str, theme: Theme) -> Self {
        Self {
            name: name.to_string(),
            bg: hex(theme.bg),
            bg_soft: hex(theme.bg_soft),
            selection: hex(theme.selection),
            dim: hex(theme.dim),
            fg: hex(theme.fg),
            ok: hex(theme.ok),
            link: hex(theme.link),
            user: hex(theme.user),
            rose: hex(theme.rose),
            err: hex(theme.err),
            running: hex(theme.running),
        }
    }

    /// Legal theme = non-empty name and every color parsing as 6-digit hex.
    pub fn is_legal(&self) -> bool {
        !self.name.trim().is_empty()
            && self.to_theme().is_some()
    }

    pub fn to_theme(&self) -> Option<Theme> {
        Some(Theme {
            bg: parse_hex(&self.bg)?,
            bg_soft: parse_hex(&self.bg_soft)?,
            selection: parse_hex(&self.selection)?,
            dim: parse_hex(&self.dim)?,
            fg: parse_hex(&self.fg)?,
            ok: parse_hex(&self.ok)?,
            link: parse_hex(&self.link)?,
            user: parse_hex(&self.user)?,
            rose: parse_hex(&self.rose)?,
            err: parse_hex(&self.err)?,
            running: parse_hex(&self.running)?,
        })
    }
}

fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        other => format!("{other:?}"),
    }
}

/// The two built-in themes, written to the themes directory on first load.
pub fn default_themes() -> Vec<(&'static str, Theme)> {
    vec![("deepseek-e", Theme::deepseek_e()), ("ferra", Theme::ferra())]
}

/// Ensure the themes directory exists and contains the two default theme
/// files (never overwriting a user's edits to an existing default file).
pub fn ensure_default_themes(dir: &Path) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    for (name, theme) in default_themes() {
        let path = dir.join(format!("{name}.toml"));
        if path.exists() {
            continue;
        }
        let file = ThemeFile::from_theme(name, theme);
        if let Ok(text) = toml::to_string_pretty(&file) {
            let _ = fs::write(&path, text);
        }
    }
}

/// Parse one theme file's TOML text into a legal theme (None if invalid).
pub fn parse_theme(text: &str) -> Option<ThemeFile> {
    let file: ThemeFile = toml::from_str(text).ok()?;
    if file.is_legal() {
        Some(file)
    } else {
        None
    }
}

/// Discover every legal theme in the directory, sorted by name.
pub fn discover_themes(dir: &Path) -> Vec<ThemeFile> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if let Some(file) = parse_theme(&text) {
            out.push(file);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Discover themes, ensuring the two defaults exist first.
pub fn load_themes(dir: &Path) -> Vec<ThemeFile> {
    ensure_default_themes(dir);
    discover_themes(dir)
}

/// Resolve a theme name to a palette: a discovered theme wins, then the
/// built-in fallback (unknown names fall back to deepseek-e).
pub fn resolve(name: &str, themes: &[ThemeFile]) -> Theme {
    if let Some(file) = themes.iter().find(|t| t.name == name) {
        if let Some(theme) = file.to_theme() {
            return theme;
        }
    }
    Theme::from_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_accepts_hash_and_bare() {
        assert_eq!(parse_hex("#4d6bfe"), Some(Color::Rgb(0x4d, 0x6b, 0xfe)));
        assert_eq!(parse_hex("4d6bfe"), Some(Color::Rgb(0x4d, 0x6b, 0xfe)));
        assert_eq!(parse_hex("#xyz"), None);
        assert_eq!(parse_hex("4d6b"), None);
    }

    #[test]
    fn theme_file_roundtrips_hex() {
        let file = ThemeFile::from_theme("deepseek-e", Theme::deepseek_e());
        let text = toml::to_string_pretty(&file).unwrap();
        let parsed = parse_theme(&text).expect("legal theme parses");
        assert_eq!(parsed.name, "deepseek-e");
        assert_eq!(parsed.to_theme().unwrap().user, Theme::deepseek_e().user);
    }

    #[test]
    fn illegal_theme_is_rejected() {
        // Missing/invalid colors ⇒ not legal.
        let bad = ThemeFile {
            name: "broken".into(),
            bg: "not-a-color".into(),
            ..ThemeFile::from_theme("x", Theme::ferra())
        };
        assert!(!bad.is_legal());
        // Empty name ⇒ not legal.
        let no_name = ThemeFile {
            name: "  ".into(),
            ..ThemeFile::from_theme("x", Theme::ferra())
        };
        assert!(!no_name.is_legal());
        // Garbage TOML ⇒ None.
        assert!(parse_theme("this is { not toml").is_none());
    }

    #[test]
    fn resolve_prefers_discovered_then_builtin() {
        let themes = vec![
            ThemeFile::from_theme("mine", Theme {
                user: Color::Rgb(1, 2, 3),
                ..Theme::deepseek_e()
            }),
        ];
        assert_eq!(resolve("mine", &themes).user, Color::Rgb(1, 2, 3));
        assert_eq!(resolve("ferra", &themes).user, Theme::ferra().user);
        // Unknown name falls back to deepseek-e.
        assert_eq!(resolve("nope", &themes).user, Theme::deepseek_e().user);
    }
}
