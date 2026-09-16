//! Fenced code, syntax highlighting, and Mermaid block presentation.

use ratatui::text::{Line, Span};

use crate::{
    i18n::tr_args,
    render::{RenderLine, RenderOptions},
    theme::Theme,
};

// ---------------------------------------------------------------------------
// Mermaid (D9): grok-mermaid WASM -> styled box art; trap falls back to the
// fenced source. The block stays atomic: copy mode yields the raw mermaid.
// ---------------------------------------------------------------------------

fn fenced_content<'a>(raw_lines: &'a [&'a str]) -> (&'a [&'a str], usize) {
    let Some(opening) = raw_lines.first().map(|line| line.trim_start()) else {
        return (&[], 0);
    };
    let Some(marker) = opening
        .chars()
        .next()
        .filter(|marker| matches!(marker, '`' | '~'))
    else {
        return (raw_lines, 0);
    };
    let fence_len = opening
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if fence_len < 3 {
        return (raw_lines, 0);
    }
    let has_closing_fence = raw_lines.last().is_some_and(|line| {
        let closing = line.trim();
        closing
            .chars()
            .take_while(|character| *character == marker)
            .count()
            >= fence_len
            && closing.trim_start_matches(marker).trim().is_empty()
    });
    let end = raw_lines
        .len()
        .saturating_sub(usize::from(has_closing_fence));
    (&raw_lines[1.min(end)..end], 1)
}

pub(super) fn render_mermaid_block(
    raw: &str,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let (content, _) = fenced_content(&raw_lines);
    let source = content.join("\n");
    let dim = theme.code.meta.style();
    let source_lines = source.lines().count();
    // Glow-style header: `  mermaid · N lines` on the filled block.
    out.push(RenderLine {
        line: Line::from(vec![
            Span::styled("  ", dim),
            Span::styled("mermaid", dim),
            Span::styled(
                tr_args(
                    options.language,
                    if source_lines == 1 {
                        "markdown.block_line"
                    } else {
                        "markdown.block_lines"
                    },
                    &[("count", source_lines.to_string())],
                ),
                dim,
            ),
        ]),
        unit,
        raw_line: Some(0),
        atomic: true,
        fill: true,
    });

    match crate::mermaid::render(&source, 0) {
        Ok(diagram) => {
            for line in &diagram {
                let spans: Vec<Span<'static>> = line
                    .spans
                    .iter()
                    .map(|s| {
                        let style = match s.class {
                            crate::mermaid::MermaidClass::Border => {
                                theme.markdown.mermaid_border.style()
                            }
                            crate::mermaid::MermaidClass::Node => {
                                theme.markdown.mermaid_node.style()
                            }
                            crate::mermaid::MermaidClass::Edge => {
                                theme.markdown.mermaid_edge.style()
                            }
                            crate::mermaid::MermaidClass::EdgeLabel => {
                                theme.markdown.mermaid_edge_label.style()
                            }
                            crate::mermaid::MermaidClass::Title => {
                                theme.markdown.mermaid_title.style()
                            }
                        };
                        Span::styled(s.text.clone(), style)
                    })
                    .collect();
                let mut base = vec![Span::styled("  ", dim)];
                base.extend(spans);
                let line = Line::from(base);
                // Box art must not reach the outer prose wrapper over-width.
                let line = match options.content_width {
                    Some(width) => crate::wrap::ellipsize_line(line, width),
                    None => line,
                };
                out.push(RenderLine {
                    line,
                    unit,
                    raw_line: None,
                    atomic: true,
                    fill: true,
                });
            }
        }
        Err(error) => {
            // Fall back to the fenced source (D9).
            out.push(RenderLine {
                line: Line::from(vec![
                    Span::styled("  ", dim),
                    Span::styled(
                        tr_args(
                            options.language,
                            "markdown.mermaid_error",
                            &[("error", error.to_string())],
                        ),
                        theme.code.meta.style(),
                    ),
                ]),
                unit,
                raw_line: None,
                atomic: true,
                fill: true,
            });
            for (i, line) in content.iter().enumerate() {
                out.push(RenderLine {
                    line: Line::from(vec![
                        Span::styled("  ", dim),
                        Span::styled((*line).to_string(), theme.code.text.style()),
                    ]),
                    unit,
                    raw_line: Some(i + 1),
                    atomic: true,
                    fill: true,
                });
            }
        }
    }
    block_bottom_pad(unit, theme, out);
}

// ---------------------------------------------------------------------------
// Code block
// ---------------------------------------------------------------------------

pub(super) fn render_code_block(
    raw: &str,
    lang: Option<&str>,
    fenced: bool,
    unit: u64,
    theme: &Theme,
    options: &RenderOptions,
    out: &mut Vec<RenderLine>,
) {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let (content, fence_offset) = if fenced {
        fenced_content(&raw_lines)
    } else {
        (raw_lines.as_slice(), 0)
    };
    let dim = theme.code.meta.style();
    let language_key = if content.len() == 1 {
        "markdown.block_line"
    } else {
        "markdown.block_lines"
    };
    // Glow-style header: `  lang · N lines` — no frame.
    out.push(RenderLine {
        line: Line::from(vec![
            Span::styled("  ", dim),
            Span::styled(
                lang.map_or_else(
                    || crate::i18n::tr(options.language, "markdown.code"),
                    str::to_owned,
                ),
                dim,
            ),
            Span::styled(
                tr_args(
                    options.language,
                    language_key,
                    &[("count", content.len().to_string())],
                ),
                dim,
            ),
        ]),
        unit,
        raw_line: Some(0),
        atomic: true,
        fill: true,
    });

    let highlighted = crate::syntax::highlight_lines(
        content,
        crate::syntax::SyntaxHint::Token(lang.unwrap_or_default()),
        &theme.code,
    );
    for (i, line) in highlighted.into_iter().enumerate() {
        let mut spans = vec![Span::styled("  ", dim)];
        spans.extend(line.spans);
        out.push(RenderLine {
            line: Line::from(spans),
            unit,
            raw_line: Some(i + fence_offset),
            atomic: true,
            fill: true,
        });
    }
    block_bottom_pad(unit, theme, out);
}

/// One row of inner padding below a code/mermaid block: flagged `fill` so the
/// UI paints it with the block background, and carrying a single backgrounded
/// space so blank-normalization keeps it (a width-0 row would be trimmed as
/// a glamour margin blank at the message end).
fn block_bottom_pad(unit: u64, theme: &Theme, out: &mut Vec<RenderLine>) {
    out.push(RenderLine {
        line: Line::from(Span::styled(" ", theme.markdown.code_block_bg.style())),
        unit,
        raw_line: None,
        atomic: true,
        fill: true,
    });
}
