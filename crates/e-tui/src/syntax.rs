//! Embedded, theme-semantic syntax highlighting for code presentation.
//!
//! This module owns no layout and performs no filesystem I/O. It converts
//! bounded code into Ratatui spans; Markdown and diff renderers retain
//! ownership of backgrounds, gutters, provenance, wrapping, and clipping.

use std::{path::Path, str::FromStr, sync::OnceLock};

use ratatui::{
    style::Color,
    text::{Line, Span},
};
use syntect::{
    highlighting::{
        Color as SyntectColor, FontStyle, ScopeSelectors, StyleModifier, Theme as SyntectTheme,
        ThemeItem, ThemeSettings,
    },
    parsing::{SyntaxReference, SyntaxSet},
};
use tui_syntax_highlight::Highlighter;

use crate::theme::{CodeTheme, ThemeStyle};

/// Match the existing bounded deferred Preview budget.
pub const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;
/// Match the existing bounded deferred Preview row budget.
pub const MAX_HIGHLIGHT_LINES: usize = 2_000;
/// Avoid pathological regex work on one contiguous source line.
pub const MAX_HIGHLIGHT_LINE_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxHint<'a> {
    Token(&'a str),
    Path(&'a str),
}

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(|| {
        let _zone = crate::tracy_zone!("syntax assets init");
        SyntaxSet::load_defaults_newlines()
    })
}

/// Initialize embedded syntax assets before the interactive frame loop. The
/// composition root may overlap this pure CPU work with bridge connection.
pub fn warm_up() {
    let _ = syntaxes();
}

/// Highlight complete logical lines. Unknown syntaxes, engine failures, and
/// bounded-limit breaches return one semantic plain-code line per input row.
pub fn highlight_lines(
    lines: &[&str],
    hint: SyntaxHint<'_>,
    code: &CodeTheme,
) -> Vec<Line<'static>> {
    let fallback = || plain_lines(lines, code.text);
    if exceeds_limits(lines) {
        return fallback();
    }
    let syntax_set = syntaxes();
    let Some(syntax) = resolve_syntax(syntax_set, hint) else {
        return fallback();
    };

    let _zone = crate::tracy_zone!("syntax highlight");
    let highlighter = Highlighter::new(syntect_theme(code)).line_numbers(false);
    match highlighter.highlight_lines(lines.iter().copied(), syntax, syntax_set) {
        Ok(text) if text.lines.len() == lines.len() => text.lines,
        _ => fallback(),
    }
}

/// Convenience for a complete source string. This follows `str::lines`, which
/// is also the line contract used by the existing code and diff renderers.
pub fn highlight_source(
    source: &str,
    hint: SyntaxHint<'_>,
    code: &CodeTheme,
) -> Vec<Line<'static>> {
    let lines = source.lines().collect::<Vec<_>>();
    highlight_lines(&lines, hint, code)
}

fn plain_lines(lines: &[&str], style: ThemeStyle) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|line| Line::from(Span::styled((*line).to_owned(), style.style())))
        .collect()
}

fn exceeds_limits(lines: &[&str]) -> bool {
    lines.len() > MAX_HIGHLIGHT_LINES
        || lines
            .iter()
            .any(|line| line.len() > MAX_HIGHLIGHT_LINE_BYTES)
        || lines
            .iter()
            .try_fold(0usize, |total, line| total.checked_add(line.len() + 1))
            .is_none_or(|total| total > MAX_HIGHLIGHT_BYTES)
}

fn resolve_syntax<'a>(set: &'a SyntaxSet, hint: SyntaxHint<'_>) -> Option<&'a SyntaxReference> {
    match hint {
        SyntaxHint::Token(token) => {
            let token = normalize_token(token);
            (!token.is_empty())
                .then(|| set.find_syntax_by_token(token))
                .flatten()
        }
        SyntaxHint::Path(path) => Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| set.find_syntax_by_extension(extension)),
    }
}

fn normalize_token(token: &str) -> &str {
    let token = token
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['{', '}', '.'])
        .split([',', ';'])
        .next()
        .unwrap_or_default();
    match token.to_ascii_lowercase().as_str() {
        "c++" => "cpp",
        "c#" => "cs",
        "console" | "shell" | "bash" => "sh",
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "plaintext" | "text" | "none" => "txt",
        _ => token,
    }
}

fn syntect_theme(code: &CodeTheme) -> SyntectTheme {
    SyntectTheme {
        name: Some("dshe-semantic-code".into()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(syntect_color(code.text.fg)),
            // Alpha 1 is tui-syntax-highlight's transparent/no-color marker.
            background: Some(SyntectColor {
                r: 0,
                g: 0,
                b: 0,
                a: 1,
            }),
            ..ThemeSettings::default()
        },
        scopes: vec![
            theme_item("comment", code.comment),
            theme_item("keyword, storage", code.keyword),
            theme_item(
                "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, support.type, storage.type - storage.type.function",
                code.r#type,
            ),
            theme_item(
                "entity.name.function, support.function, variable.function, entity.name.function.preprocessor",
                code.function,
            ),
            theme_item("string", code.string),
            theme_item("constant", code.constant),
            theme_item(
                "entity.other.attribute-name, meta.annotation, storage.type.annotation, variable.annotation",
                code.attribute,
            ),
            theme_item(
                "constant.character.escape, punctuation.section.interpolation, meta.interpolation",
                code.escape,
            ),
            theme_item("invalid", code.invalid),
        ],
    }
}

fn theme_item(selector: &str, style: ThemeStyle) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(selector).expect("static syntax scope selector is valid"),
        style: StyleModifier {
            foreground: Some(syntect_color(style.fg)),
            background: None,
            font_style: Some(font_style(style)),
        },
    }
}

fn font_style(style: ThemeStyle) -> FontStyle {
    let mut font = FontStyle::empty();
    if style.bold {
        font |= FontStyle::BOLD;
    }
    if style.italic {
        font |= FontStyle::ITALIC;
    }
    if style.underline {
        font |= FontStyle::UNDERLINE;
    }
    font
}

fn syntect_color(color: Color) -> SyntectColor {
    let (r, g, b, a) = match color {
        Color::Reset => (0, 0, 0, 1),
        Color::Black => (0, 0, 0, 0),
        Color::Red => (1, 0, 0, 0),
        Color::Green => (2, 0, 0, 0),
        Color::Yellow => (3, 0, 0, 0),
        Color::Blue => (4, 0, 0, 0),
        Color::Magenta => (5, 0, 0, 0),
        Color::Cyan => (6, 0, 0, 0),
        Color::Gray => (7, 0, 0, 0),
        Color::DarkGray => (8, 0, 0, 0),
        Color::LightRed => (9, 0, 0, 0),
        Color::LightGreen => (10, 0, 0, 0),
        Color::LightYellow => (11, 0, 0, 0),
        Color::LightBlue => (12, 0, 0, 0),
        Color::LightMagenta => (13, 0, 0, 0),
        Color::LightCyan => (14, 0, 0, 0),
        Color::White => (15, 0, 0, 0),
        Color::Indexed(index) => (index, 0, 0, 0),
        Color::Rgb(r, g, b) => (r, g, b, 255),
    };
    SyntectColor { r, g, b, a }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span_for<'a>(lines: &'a [Line<'static>], text: &str) -> &'a Span<'static> {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content.contains(text))
            .expect("highlighted span exists")
    }

    #[test]
    fn aliases_and_paths_resolve_without_io() {
        let theme = crate::theme::Theme::ferra();
        let js = highlight_lines(
            &["if (answer) console.log(answer);"],
            SyntaxHint::Token("js title=demo"),
            &theme.code,
        );
        let rust = highlight_lines(
            &["pub struct Demo;"],
            SyntaxHint::Path("src/demo.rs"),
            &theme.code,
        );
        assert_eq!(span_for(&js, "if").style.fg, Some(theme.code.keyword.fg));
        assert_eq!(span_for(&rust, "Demo").style.fg, Some(theme.code.r#type.fg));
    }

    #[test]
    fn unknown_and_oversized_inputs_fall_back_to_code_text_role() {
        let theme = crate::theme::Theme::ferra();
        let unknown = highlight_lines(
            &["mystery token"],
            SyntaxHint::Token("not-a-language"),
            &theme.code,
        );
        assert_eq!(unknown[0].spans[0].style, theme.code.text.style());

        let long = "x".repeat(MAX_HIGHLIGHT_LINE_BYTES + 1);
        let limited = highlight_lines(&[long.as_str()], SyntaxHint::Token("rust"), &theme.code);
        assert_eq!(limited[0].spans[0].style, theme.code.text.style());
    }
}
