use std::hash::{DefaultHasher, Hash, Hasher};

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
    Frame,
};

use crate::{
    config::Config,
    mouse_selection::{SelectionFrame, SelectionSurface},
    preview::{
        LineSelection, PreviewContent, PreviewLayoutKey, PreviewPaneState, PreviewState,
        ToolMetrics, ToolPreview, ToolPreviewPrimary, ToolPreviewSecondary,
    },
    render::{render_markdown, MarkdownStrength, RenderOptions},
    reveal::{apply_line_reveal, LineRevealTrack},
    syntax::{self, SyntaxHint},
    theme::Theme,
    transcript_layout::wrap_line,
    ui::component::{ansi, diff},
};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    preview: &mut PreviewPaneState,
    config: &Config,
    theme: &Theme,
    selection_frame: &mut SelectionFrame,
    left_padding: u16,
    right_padding: u16,
) {
    let inner_width = usize::from(area.width)
        .saturating_sub(usize::from(left_padding.saturating_add(right_padding)))
        .max(1);
    let lines = match &preview.state {
        PreviewState::Empty => vec![Line::styled("No preview", theme.surface.muted_text.style())],
        PreviewState::Loading { .. } => vec![Line::styled(
            "• Loading preview…",
            theme.working_status.running.style(),
        )],
        PreviewState::Error(error) => vec![Line::styled(
            format!("Preview error: {error}"),
            theme.log.error.style(),
        )],
        PreviewState::Ready(content) => {
            let layout_key = preview_layout_key(preview, content, theme, inner_width);
            let full = if let Some(lines) = preview.cached_layout(&layout_key) {
                lines
            } else {
                let lines = content_lines(content, theme, inner_width)
                    .into_iter()
                    .flat_map(|line| wrap_line(line, inner_width))
                    .collect::<Vec<_>>();
                preview.store_layout(layout_key, lines.clone());
                lines
            };
            if preview.target.is_none() {
                // Direct Ready injection is retained for renderer fixtures;
                // production Ready content always belongs to a selected target.
                full
            } else {
                let track = preview.reveal.get_or_insert_with(LineRevealTrack::default);
                track.reconcile(
                    &full,
                    std::time::Instant::now(),
                    config.preview_lines_per_second.get(),
                );
                apply_line_reveal(
                    full,
                    track,
                    config.background_color.color(),
                    theme.surface.primary_text.fg,
                    !config.plain_color,
                )
            }
        }
    };
    // Ready layouts are wrapped once before entering the cache. Immediate
    // state labels are already bounded single rows.
    let mut lines = lines;
    let total = lines.len();
    let visible = usize::from(area.height);
    // scroll == 0 is the "follow the latest" anchor: when content overflows
    // the pane, bottom-anchor it so streaming reasoning keeps its newest
    // rows visible. A positive scroll (future scroll binding) switches to
    // manual review and keeps the historical `scroll.min(total - 1)` start.
    let start = if preview.scroll == 0 && total > visible {
        total - visible
    } else {
        preview.scroll.min(total.saturating_sub(1))
    };
    lines = lines.into_iter().skip(start).take(visible).collect();
    preview.record_materialized_rows(lines.len());
    // Vertically center content that fits the pane; overflowing content is
    // already bottom-anchored and fills the pane, so no centering applies.
    let top_padding = usize::from(area.height).saturating_sub(lines.len()) / 2;
    for (index, line) in lines.iter().enumerate() {
        crate::ui::selection::register_line(
            selection_frame,
            SelectionSurface::Preview,
            start + index,
            area.x.saturating_add(left_padding),
            area.y.saturating_add((top_padding + index) as u16),
            line,
        );
    }
    let mut centered = Vec::with_capacity(usize::from(area.height));
    centered.extend(std::iter::repeat_n(Line::raw(""), top_padding));
    centered.extend(lines);
    frame.render_widget(
        Paragraph::new(centered).block(Block::default().padding(Padding::new(
            left_padding,
            right_padding,
            0,
            0,
        ))),
        area,
    );
}

fn hash_value(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn preview_layout_key(
    preview: &PreviewPaneState,
    content: &PreviewContent,
    theme: &Theme,
    width: usize,
) -> PreviewLayoutKey {
    let owner = preview
        .target
        .as_ref()
        .map(|target| (target.reference.key().clone(), target.reference.revision()));
    PreviewLayoutKey {
        // Production content identity is already represented by key/revision;
        // hash direct Ready content only for renderer fixtures.
        content_signature: owner.as_ref().map_or_else(|| hash_value(content), |_| 0),
        owner,
        width,
        theme_signature: hash_value(theme),
    }
}

fn content_lines(content: &PreviewContent, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    match content {
        PreviewContent::Link { label, url } => vec![Line::from(vec![
            Span::styled(
                label.clone().unwrap_or_else(|| "Link".into()),
                theme.markdown.link_text.style(),
            ),
            Span::raw(" "),
            Span::styled(url.clone(), theme.markdown.link_url.style()),
        ])],
        PreviewContent::Diff { path, source } => {
            diff::unified(source, path.as_deref(), theme, width)
        }
        PreviewContent::Lines { path, start, lines } => {
            std::iter::once(Line::styled(path.clone(), theme.code.meta.style()))
                .chain(lines.iter().enumerate().map(|(index, line)| {
                    Line::from(vec![
                        Span::styled(format!("{:>4} ", start + index), theme.code.meta.style()),
                        Span::styled(line.clone(), theme.code.text.style()),
                    ])
                }))
                .collect()
        }
        PreviewContent::SearchResult { query, matches } => std::iter::once(Line::styled(
            format!("Search: {query}"),
            theme.markdown.heading3.style(),
        ))
        .chain(
            matches
                .iter()
                .map(|line| Line::styled(line.clone(), theme.markdown.text.style())),
        )
        .collect(),
        PreviewContent::Command(command) => vec![Line::from(vec![
            Span::styled("$ ", theme.input.prompt.style()),
            Span::styled(command.clone(), theme.code.text.style()),
        ])],
        PreviewContent::Path(path) => {
            vec![Line::styled(path.clone(), theme.markdown.link_url.style())]
        }
        PreviewContent::Markdown(source)
        | PreviewContent::Reasoning(source)
        | PreviewContent::MutedMarkdown(source) => weak_markdown_lines(source, theme, width),
        PreviewContent::Tool(preview) => tool_lines(preview, theme, width),
        PreviewContent::Hunks(hunks) => hunks
            .iter()
            .flat_map(|hunk| hunk_lines(hunk, theme, width))
            .collect(),
        PreviewContent::PlainText(text) => text
            .lines()
            .map(|line| Line::styled(line.to_owned(), theme.surface.primary_text.style()))
            .collect(),
    }
}

/// Complete Preview Markdown rendered directly through the weak semantic
/// group. Reasoning and injected context retain distinct semantic content
/// kinds but share this presentation palette.
fn weak_markdown_lines(source: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut next_unit = 0u64;
    let mut units = std::collections::HashMap::new();
    let options = RenderOptions {
        collapse_rows: usize::MAX,
        mermaid_enabled: false,
        markdown_strength: MarkdownStrength::Weak,
        content_width: Some(width),
        ..Default::default()
    };
    render_markdown(source, theme, &mut next_unit, &options, &mut units)
        .into_iter()
        .map(|render_line| {
            if render_line.fill {
                render_line
                    .line
                    .patch_style(theme.markdown_weak.code_block_bg.style())
            } else {
                render_line.line
            }
        })
        .collect()
}

fn tool_lines(preview: &ToolPreview, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::styled(
        preview.name.clone(),
        theme.activity.label.style(),
    ));
    match &preview.primary {
        ToolPreviewPrimary::Location { path, lines: range } => {
            lines.push(Line::styled(
                location_text(path, range),
                theme.surface.primary_text.style(),
            ));
        }
        ToolPreviewPrimary::Command { command, metrics } => {
            lines.push(Line::from(vec![
                Span::styled("$ ", theme.input.prompt.style()),
                Span::styled(command.clone(), theme.surface.primary_text.style()),
            ]));
            lines.push(Line::styled(
                metrics_text(metrics),
                theme.activity.detail.style(),
            ));
        }
        ToolPreviewPrimary::Search { query, path } => {
            lines.push(Line::styled(
                format!("\"{query}\""),
                theme.surface.primary_text.style(),
            ));
            if let Some(path) = path {
                lines.push(Line::from(vec![
                    Span::styled("at ", theme.activity.detail.style()),
                    Span::styled(format!("\"{path}\""), theme.surface.primary_text.style()),
                ]));
            }
        }
        ToolPreviewPrimary::Json { source, truncated } => {
            for line in source.lines() {
                lines.push(Line::styled(
                    line.to_owned(),
                    theme.surface.primary_text.style(),
                ));
            }
            if *truncated {
                lines.push(Line::styled("…", theme.activity.detail.style()));
            }
        }
    }
    if let Some(secondary) = &preview.secondary {
        lines.push(Line::raw(""));
        match secondary {
            ToolPreviewSecondary::Terminal { output, truncated } => {
                lines.extend(ansi::terminal_lines(
                    output,
                    theme.surface.muted_text.style(),
                    theme.activity.label.style(),
                ));
                if *truncated {
                    lines.push(Line::styled("…", theme.activity.detail.style()));
                }
            }
        }
    }
    // Terminal output is the only section rendered as-is; everything else is
    // already width-agnostic logical rows the caller wraps.
    let _ = width;
    lines
}

fn location_text(path: &str, range: &Option<LineSelection>) -> String {
    match range {
        None => path.to_owned(),
        Some(LineSelection { start, end: None }) => format!("{path}:{start}-"),
        Some(LineSelection {
            start,
            end: Some(end),
        }) if end == start => {
            format!("{path}:{start}")
        }
        Some(LineSelection {
            start,
            end: Some(end),
        }) => format!("{path}:{start}-{end}"),
    }
}

fn metrics_text(metrics: &ToolMetrics) -> String {
    let noun = if metrics.output_lines == 1 {
        "line"
    } else {
        "lines"
    };
    let suffix = if metrics.truncated { "+" } else { "" };
    let mut text = format!("{noun} {}{suffix}", metrics.output_lines);
    if let Some(duration_ms) = metrics.duration_ms {
        text.push_str(&format!(", duration {:.1}s", duration_ms as f64 / 1000.0));
    }
    text
}

fn hunk_lines(
    hunk: &crate::preview::MutationHunk,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(path) = &hunk.path {
        lines.push(Line::styled(path.clone(), theme.code.meta.style()));
    }
    if let Some(anchor) = hunk.anchor_line {
        lines.push(Line::styled(
            format!("@ line {anchor}"),
            theme.code.meta.style(),
        ));
    }
    // Old and new fragments are separate logical syntax streams so multiline
    // state cannot leak across mutually exclusive versions.
    let hint = hunk
        .path
        .as_deref()
        .map_or(SyntaxHint::Token(""), SyntaxHint::Path);
    let old_source = hunk
        .old
        .as_deref()
        .map_or_else(Vec::new, |source| source.lines().collect::<Vec<_>>());
    let new_source = hunk
        .new
        .as_deref()
        .map_or_else(Vec::new, |source| source.lines().collect::<Vec<_>>());
    let old = syntax::highlight_lines(&old_source, hint, &theme.code);
    let new = syntax::highlight_lines(&new_source, hint, &theme.code);
    for (offset, highlighted) in old.into_iter().enumerate() {
        lines.push(diff::styled_line(
            theme,
            diff::DiffKind::Removed,
            Some(1 + offset),
            highlighted.spans,
            width,
        ));
    }
    let new_start = hunk.anchor_line.map_or(1, |anchor| anchor + 1);
    for (offset, highlighted) in new.into_iter().enumerate() {
        lines.push(diff::styled_line(
            theme,
            diff::DiffKind::Added,
            Some(new_start + offset),
            highlighted.spans,
            width,
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::MutationHunk;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn hunk_lines_number_added_rows_from_the_insert_anchor() {
        let theme = Theme::ferra();
        let hunk = MutationHunk {
            path: Some("a.rs".into()),
            old: None,
            new: Some("line a\nline b".into()),
            anchor_line: Some(10),
        };
        let lines = hunk_lines(&hunk, &theme, 40);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(texts[0], "a.rs");
        assert_eq!(texts[1], "@ line 10");
        // anchor_line is 0-based; first added row displays as 11.
        assert!(texts[2].starts_with("\u{258c}   11 \u{2502} line a"));
        assert!(texts[3].starts_with("\u{258c}   12 \u{2502} line b"));
    }

    #[test]
    fn hunk_lines_number_removed_and_added_rows_from_one_without_anchor() {
        let theme = Theme::ferra();
        let hunk = MutationHunk {
            path: None,
            old: Some("old1\nold2".into()),
            new: Some("new1\nnew2".into()),
            anchor_line: None,
        };
        let lines = hunk_lines(&hunk, &theme, 40);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts[0].starts_with("\u{258c}    1 \u{2502} old1"));
        assert!(texts[1].starts_with("\u{258c}    2 \u{2502} old2"));
        assert!(texts[2].starts_with("\u{258c}    1 \u{2502} new1"));
        assert!(texts[3].starts_with("\u{258c}    2 \u{2502} new2"));
    }
}
