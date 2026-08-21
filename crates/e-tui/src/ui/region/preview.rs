use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
    Frame,
};

use crate::{
    config::Config,
    preview::{
        LineSelection, PreviewContent, PreviewPaneState, PreviewState, ToolMetrics, ToolPreview,
        ToolPreviewPrimary, ToolPreviewSecondary,
    },
    render::{render_markdown, RenderOptions},
    reveal::{apply_line_reveal, LineRevealTrack},
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
) {
    let inner_width = usize::from(area.width).saturating_sub(2).max(1);
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
            let full = content_lines(content, theme, inner_width)
                .into_iter()
                .flat_map(|line| wrap_line(line, inner_width))
                .collect::<Vec<_>>();
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
    // Wrap every row to the padded content width first so long reasoning
    // lines stay fully visible instead of truncating at the pane edge.
    let mut lines = lines
        .into_iter()
        .flat_map(|line| wrap_line(line, inner_width))
        .collect::<Vec<_>>();
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
    let mut centered = Vec::with_capacity(usize::from(area.height));
    centered.extend(std::iter::repeat_n(Line::raw(""), top_padding));
    centered.extend(lines);
    frame.render_widget(
        Paragraph::new(centered).block(Block::default().padding(Padding::horizontal(1))),
        area,
    );
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
        PreviewContent::Diff(source) => source
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') {
                    diff::added(theme)
                } else if line.starts_with('-') {
                    diff::removed(theme)
                } else {
                    theme.markdown.code_text.style()
                };
                Line::styled(line.to_owned(), style)
            })
            .collect(),
        PreviewContent::Lines { path, start, lines } => {
            std::iter::once(Line::styled(path.clone(), theme.markdown.code_meta.style()))
                .chain(lines.iter().enumerate().map(|(index, line)| {
                    Line::from(vec![
                        Span::styled(
                            format!("{:>4} ", start + index),
                            theme.markdown.code_meta.style(),
                        ),
                        Span::styled(line.clone(), theme.markdown.code_text.style()),
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
            Span::styled(command.clone(), theme.markdown.code_text.style()),
        ])],
        PreviewContent::Path(path) => {
            vec![Line::styled(path.clone(), theme.markdown.link_url.style())]
        }
        PreviewContent::Markdown(source) => source
            .lines()
            .map(|line| Line::styled(line.to_owned(), theme.markdown.text.style()))
            .collect(),
        PreviewContent::Reasoning(source) | PreviewContent::MutedMarkdown(source) => {
            muted_markdown_lines(source, theme, width)
        }
        PreviewContent::Tool(preview) => tool_lines(preview, theme, width),
        PreviewContent::Hunks(hunks) => hunks
            .iter()
            .flat_map(|hunk| hunk_lines(hunk, theme))
            .collect(),
        PreviewContent::PlainText(text) => text
            .lines()
            .map(|line| Line::styled(line.to_owned(), theme.surface.primary_text.style()))
            .collect(),
    }
}

/// Full Markdown rendering with every foreground forced to the muted (Bark)
/// tone while preserving Markdown modifiers and backgrounds. Shared by the
/// reasoning and injected-context content kinds.
fn muted_markdown_lines(source: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut next_unit = 0u64;
    let mut units = std::collections::HashMap::new();
    let options = RenderOptions {
        collapse_rows: usize::MAX,
        mermaid_enabled: false,
        content_width: Some(width),
        ..Default::default()
    };
    let bark = theme.surface.muted_text.fg;
    render_markdown(source, theme, &mut next_unit, &options, &mut units)
        .into_iter()
        .map(|render_line| {
            let mut line = render_line.line;
            line.style = line.style.fg(bark);
            for span in line.spans.iter_mut() {
                span.style = span.style.fg(bark);
            }
            line
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

fn hunk_lines(hunk: &crate::preview::MutationHunk, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(path) = &hunk.path {
        lines.push(Line::styled(path.clone(), theme.markdown.code_meta.style()));
    }
    if let Some(anchor) = hunk.anchor_line {
        lines.push(Line::styled(
            format!("@ line {anchor}"),
            theme.markdown.code_meta.style(),
        ));
    }
    if let Some(old) = &hunk.old {
        for line in old.lines() {
            lines.push(Line::styled(format!("- {line}"), diff::removed(theme)));
        }
    }
    if let Some(new) = &hunk.new {
        for line in new.lines() {
            lines.push(Line::styled(format!("+ {line}"), diff::added(theme)));
        }
    }
    lines
}
