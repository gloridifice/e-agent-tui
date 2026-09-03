use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    syntax::{self, SyntaxHint},
    theme::Theme,
};

const ELLIPSIS: &str = "\u{2026}";
const ELLIPSIS_MARGIN: usize = 2;
const LINE_NUM_WIDTH: usize = 4;
const GUTTER: &str = "\u{258c}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Context,
}

fn palette(theme: &Theme, kind: DiffKind) -> (Color, Option<Color>) {
    match kind {
        DiffKind::Added => (theme.diff.added_accent.fg, theme.diff.added.bg),
        DiffKind::Removed => (theme.diff.removed_accent.fg, theme.diff.removed.bg),
        DiffKind::Context => (theme.diff.context_accent.fg, None),
    }
}

fn prefix_spans(
    base: Style,
    accent: Color,
    separator: Color,
    number: &str,
) -> (Vec<Span<'static>>, usize) {
    let spans = vec![
        Span::styled(GUTTER, base.fg(accent)),
        Span::styled(" ", base.fg(accent)),
        Span::styled(number.to_owned(), base.fg(accent)),
        Span::styled(" ", base),
        Span::styled("\u{2502}", base.fg(separator)),
        Span::styled(" ", base),
    ];
    let width = spans.iter().map(Span::width).sum();
    (spans, width)
}

/// Render one diff row with a project-owned gutter and syntax-styled body.
/// Body foregrounds/modifiers survive; the row kind owns its background.
pub fn styled_line(
    theme: &Theme,
    kind: DiffKind,
    line_num: Option<usize>,
    content: Vec<Span<'static>>,
    width: usize,
) -> Line<'static> {
    if width == 0 {
        return Line::raw("");
    }
    let (accent, bg) = palette(theme, kind);
    let base = bg.map_or_else(Style::default, |color| Style::default().bg(color));
    let number = line_num.map_or_else(
        || " ".repeat(LINE_NUM_WIDTH),
        |number| format!("{number:>LINE_NUM_WIDTH$}"),
    );
    let (prefix, prefix_width) = prefix_spans(base, accent, theme.diff.separator.fg, &number);
    if width < prefix_width {
        let raw = prefix
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        return Line::from(Span::styled(truncate_text(&raw, width), base.fg(accent)));
    }

    let room = width - prefix_width;
    let content_width = spans_width(&content);
    let mut body = if content_width > room {
        let keep = room.saturating_sub(1 + ELLIPSIS_MARGIN);
        let mut clipped = truncate_spans(content, keep);
        let ellipsis_style = clipped
            .last()
            .map_or_else(|| theme.diff.text.style(), |span| span.style);
        clipped.push(Span::styled(ELLIPSIS, ellipsis_style));
        clipped.push(Span::styled(" ".repeat(ELLIPSIS_MARGIN), ellipsis_style));
        clipped
    } else {
        content
    };

    for span in &mut body {
        if let Some(bg) = bg {
            span.style = span.style.bg(bg);
        }
    }

    let mut spans = prefix;
    spans.extend(body);
    let used = spans_width(&spans);
    if let Some(bg) = bg {
        if used < width {
            spans.push(Span::styled(
                " ".repeat(width - used),
                Style::default().bg(bg),
            ));
        }
    }
    Line::from(spans)
}

/// Plain-body convenience retained for metadata, fallback, and focused tests.
pub fn line(
    theme: &Theme,
    kind: DiffKind,
    line_num: Option<usize>,
    content: &str,
    width: usize,
) -> Line<'static> {
    styled_line(
        theme,
        kind,
        line_num,
        vec![Span::styled(content.to_owned(), theme.diff.text.style())],
        width,
    )
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(Span::width).sum()
}

fn truncate_text(text: &str, width: usize) -> String {
    truncate_span(Span::raw(text.to_owned()), width)
        .content
        .into_owned()
}

fn truncate_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut remaining = width;
    let mut out = Vec::new();
    for span in spans {
        if remaining == 0 {
            break;
        }
        let span_width = span.width();
        if span_width <= remaining {
            remaining -= span_width;
            out.push(span);
        } else {
            let clipped = truncate_span(span, remaining);
            if !clipped.content.is_empty() {
                out.push(clipped);
            }
            break;
        }
    }
    out
}

fn truncate_span(span: Span<'static>, width: usize) -> Span<'static> {
    let text = span.content.as_ref();
    if text.width() <= width {
        return span;
    }
    let mut used = 0;
    let mut end = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let next = used + grapheme.width();
        if next > width {
            break;
        }
        used = next;
        end = index + grapheme.len();
    }
    Span::styled(text[..end].to_owned(), span.style)
}

#[derive(Debug)]
enum ClassifiedBlock {
    Meta(String),
    Hunk(ClassifiedHunk),
}

#[derive(Debug)]
struct ClassifiedHunk {
    header: Option<String>,
    path: Option<String>,
    rows: Vec<ClassifiedRow>,
}

#[derive(Debug)]
enum ClassifiedRow {
    Body {
        kind: DiffKind,
        old_line: Option<usize>,
        new_line: Option<usize>,
        content: String,
    },
    Meta(String),
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@")?.trim();
    let mut parts = rest.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    Some((
        old.split(',').next()?.parse().ok()?,
        new.split(',').next()?.parse().ok()?,
    ))
}

fn diff_header_path(line: &str) -> Option<String> {
    let raw = if let Some(path) = line.strip_prefix("+++ ") {
        path.split('\t').next().unwrap_or(path)
    } else if let Some(rest) = line.strip_prefix("diff --git ") {
        rest.split_whitespace().last()?
    } else {
        return None;
    };
    let raw = raw.trim_matches('"');
    if raw == "/dev/null" {
        None
    } else {
        Some(raw.strip_prefix("b/").unwrap_or(raw).to_owned())
    }
}

fn is_file_meta(line: &str) -> bool {
    line.starts_with("diff --git ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("new file mode")
        || line.starts_with("deleted file mode")
        || line.starts_with("old mode")
        || line.starts_with("new mode")
        || line.starts_with("similarity index")
        || line.starts_with("rename from")
        || line.starts_with("rename to")
}

fn flush_hunk(blocks: &mut Vec<ClassifiedBlock>, hunk: &mut Option<ClassifiedHunk>) {
    if let Some(hunk) = hunk.take() {
        blocks.push(ClassifiedBlock::Hunk(hunk));
    }
}

fn classify(source: &str, fallback_path: Option<&str>) -> Vec<ClassifiedBlock> {
    let mut blocks = Vec::new();
    let mut current_path = fallback_path.map(str::to_owned);
    let mut hunk: Option<ClassifiedHunk> = None;
    let mut old_line = 1usize;
    let mut new_line = 1usize;

    for raw in source.lines() {
        if let Some((old_start, new_start)) = parse_hunk_header(raw) {
            flush_hunk(&mut blocks, &mut hunk);
            old_line = old_start;
            new_line = new_start;
            hunk = Some(ClassifiedHunk {
                header: Some(raw.to_owned()),
                path: current_path.clone(),
                rows: Vec::new(),
            });
            continue;
        }

        if is_file_meta(raw) {
            flush_hunk(&mut blocks, &mut hunk);
            blocks.push(ClassifiedBlock::Meta(raw.to_owned()));
            if let Some(path) = diff_header_path(raw) {
                current_path = Some(path);
            }
            continue;
        }

        let body = match raw.as_bytes().first().copied() {
            Some(b'+') => Some((DiffKind::Added, &raw[1..])),
            Some(b'-') => Some((DiffKind::Removed, &raw[1..])),
            Some(b' ') => Some((DiffKind::Context, &raw[1..])),
            _ => None,
        };
        if let Some((kind, content)) = body {
            let hunk = hunk.get_or_insert_with(|| ClassifiedHunk {
                header: None,
                path: current_path.clone(),
                rows: Vec::new(),
            });
            let (old, new) = match kind {
                DiffKind::Added => {
                    let current = new_line;
                    new_line = new_line.saturating_add(1);
                    (None, Some(current))
                }
                DiffKind::Removed => {
                    let current = old_line;
                    old_line = old_line.saturating_add(1);
                    (Some(current), None)
                }
                DiffKind::Context => {
                    let old = old_line;
                    let new = new_line;
                    old_line = old_line.saturating_add(1);
                    new_line = new_line.saturating_add(1);
                    (Some(old), Some(new))
                }
            };
            hunk.rows.push(ClassifiedRow::Body {
                kind,
                old_line: old,
                new_line: new,
                content: content.to_owned(),
            });
        } else if raw.starts_with('\\') {
            if let Some(hunk) = &mut hunk {
                hunk.rows.push(ClassifiedRow::Meta(raw.to_owned()));
            } else {
                blocks.push(ClassifiedBlock::Meta(raw.to_owned()));
            }
        } else {
            flush_hunk(&mut blocks, &mut hunk);
            blocks.push(ClassifiedBlock::Meta(raw.to_owned()));
        }
    }
    flush_hunk(&mut blocks, &mut hunk);
    blocks
}

fn syntax_hint(path: Option<&str>) -> SyntaxHint<'_> {
    path.map_or(SyntaxHint::Token(""), SyntaxHint::Path)
}

fn render_hunk(hunk: ClassifiedHunk, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let old_source = hunk
        .rows
        .iter()
        .filter_map(|row| match row {
            ClassifiedRow::Body {
                kind: DiffKind::Removed | DiffKind::Context,
                content,
                ..
            } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let new_source = hunk
        .rows
        .iter()
        .filter_map(|row| match row {
            ClassifiedRow::Body {
                kind: DiffKind::Added | DiffKind::Context,
                content,
                ..
            } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let hint = syntax_hint(hunk.path.as_deref());
    let old = syntax::highlight_lines(&old_source, hint, &theme.code);
    let new = syntax::highlight_lines(&new_source, hint, &theme.code);
    let mut old_index = 0usize;
    let mut new_index = 0usize;
    let mut lines = Vec::new();
    if let Some(header) = hunk.header {
        lines.push(styled_line(
            theme,
            DiffKind::Context,
            None,
            vec![Span::styled(header, theme.code.meta.style())],
            width,
        ));
    }
    for row in hunk.rows {
        match row {
            ClassifiedRow::Meta(meta) => lines.push(styled_line(
                theme,
                DiffKind::Context,
                None,
                vec![Span::styled(meta, theme.code.meta.style())],
                width,
            )),
            ClassifiedRow::Body {
                kind,
                old_line,
                new_line,
                ..
            } => {
                let (body, number) = match kind {
                    DiffKind::Removed => {
                        let body = old[old_index].spans.clone();
                        old_index += 1;
                        (body, old_line)
                    }
                    DiffKind::Added => {
                        let body = new[new_index].spans.clone();
                        new_index += 1;
                        (body, new_line)
                    }
                    DiffKind::Context => {
                        old_index += 1;
                        let body = new[new_index].spans.clone();
                        new_index += 1;
                        (body, new_line)
                    }
                };
                lines.push(styled_line(theme, kind, number, body, width));
            }
        }
    }
    lines
}

/// Classify and render event-authored unified diff text. Classification adds
/// presentation metadata only; it never computes changes or reads a file.
pub fn unified(
    source: &str,
    fallback_path: Option<&str>,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    classify(source, fallback_path)
        .into_iter()
        .flat_map(|block| match block {
            ClassifiedBlock::Meta(meta) => vec![styled_line(
                theme,
                DiffKind::Context,
                None,
                vec![Span::styled(meta, theme.code.meta.style())],
                width,
            )],
            ClassifiedBlock::Hunk(hunk) => render_hunk(hunk, theme, width),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concat(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn renders_gutter_line_number_separator_and_content() {
        let theme = Theme::ferra();
        let line = line(&theme, DiffKind::Added, Some(3), "fn added()", 40);
        assert_eq!(
            concat(&line).trim_end(),
            "\u{258c}    3 \u{2502} fn added()"
        );
        assert_eq!(line.spans[0].style.fg, Some(theme.diff.added_accent.fg));
        assert_eq!(line.spans[4].style.fg, Some(theme.diff.separator.fg));
    }

    #[test]
    fn context_rows_have_no_background_or_padding() {
        let theme = Theme::ferra();
        let context = line(&theme, DiffKind::Context, Some(4), "}", 30);
        assert_eq!(concat(&context), "\u{258c}    4 \u{2502} }");
        assert!(context.spans.iter().all(|span| span.style.bg.is_none()));
    }

    #[test]
    fn overflow_preserves_span_style_and_grapheme_boundaries() {
        let theme = Theme::ferra();
        let body = vec![
            Span::styled("fn ", theme.code.keyword.style()),
            Span::styled("👩‍💻界界界界界", theme.code.string.style()),
        ];
        let line = styled_line(&theme, DiffKind::Removed, Some(9), body, 18);
        let text = concat(&line);
        assert_eq!(text.width(), 18);
        assert!(text.contains("…  "));
        assert!(!text.contains('�'));
        assert_eq!(line.spans.last().unwrap().style.bg, theme.diff.removed.bg);
    }

    #[test]
    fn unified_counts_lines_and_highlights_old_and_new_streams() {
        let theme = Theme::ferra();
        let source = "@@ -3,2 +3,3 @@\n fn before() {}\n-fn old() {}\n+fn new() {}\n";
        let lines = unified(source, Some("src/main.rs"), &theme, 48);
        let texts = lines.iter().map(concat).collect::<Vec<_>>();
        assert_eq!(texts[0], "\u{258c}      \u{2502} @@ -3,2 +3,3 @@");
        assert_eq!(texts[1], "\u{258c}    3 \u{2502} fn before() {}");
        assert_eq!(texts[2].trim_end(), "\u{258c}    4 \u{2502} fn old() {}");
        assert_eq!(texts[3].trim_end(), "\u{258c}    4 \u{2502} fn new() {}");
        for row in &lines[1..] {
            let keyword = row
                .spans
                .iter()
                .find(|span| span.content == "fn")
                .expect("Rust keyword");
            assert_eq!(keyword.style.fg, Some(theme.code.keyword.fg));
        }
    }

    #[test]
    fn unified_retains_multifile_and_no_newline_metadata() {
        let theme = Theme::ferra();
        let source = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-fn old() {}\n+fn new() {}\n\\ No newline at end of file\ndiff --git a/b.py b/b.py\n--- a/b.py\n+++ b/b.py\n@@ -1 +1 @@\n-def old(): pass\n+def new(): pass";
        let lines = unified(source, None, &theme, 60);
        let text = lines.iter().map(concat).collect::<Vec<_>>().join("\n");
        for retained in [
            "diff --git a/a.rs b/a.rs",
            "fn old",
            "No newline",
            "b/b.py",
            "def new",
        ] {
            assert!(text.contains(retained), "retains {retained:?}: {text}");
        }
    }
}
