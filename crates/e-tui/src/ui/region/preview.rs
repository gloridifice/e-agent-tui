use std::hash::{DefaultHasher, Hash, Hasher};

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
    Frame,
};

use crate::{
    config::Config,
    i18n::{tr, tr_args, Language},
    preview::{
        LineSelection, PreviewContent, PreviewLayout, PreviewLayoutKey, PreviewPaneState,
        PreviewRevealIntent, PreviewState, ToolMetrics, ToolPreview, ToolPreviewPrimary,
        ToolPreviewSecondary,
    },
    render::{render_markdown, MarkdownStrength, RenderOptions},
    reveal::{apply_line_reveal, LineRevealMode, LineRevealTrack},
    syntax::{self, SyntaxHint},
    theme::Theme,
    transcript_layout::wrap_line,
    ui::component::{ansi, command, diff},
};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    preview: &mut PreviewPaneState,
    config: &Config,
    theme: &Theme,
    left_padding: u16,
    right_padding: u16,
) {
    let inner_width = usize::from(area.width)
        .saturating_sub(usize::from(left_padding.saturating_add(right_padding)))
        .max(1);
    let (lines, pinned_rows, nowrap_from) = match &preview.state {
        PreviewState::Empty => (
            vec![Line::styled(
                tr(config.language, "preview.empty"),
                theme.surface.muted_text.style(),
            )],
            0,
            None,
        ),
        PreviewState::Loading { .. } => (
            vec![Line::styled(
                tr(config.language, "preview.loading"),
                theme.working_status.running.style(),
            )],
            0,
            None,
        ),
        PreviewState::Error(error) => (
            vec![Line::styled(
                tr_args(
                    config.language,
                    "preview.error",
                    &[("error", error.clone())],
                ),
                theme.log.error.style(),
            )],
            0,
            None,
        ),
        PreviewState::Ready(content) => {
            let row_paced = preview.reveal_intent == PreviewRevealIntent::FreshLive
                && matches!(content, PreviewContent::Reasoning(_));
            let layout_key = preview_layout_key(preview, content, theme, inner_width);
            let layout = if let Some(layout) = preview.cached_layout(&layout_key) {
                layout
            } else {
                let layout = content_layout(content, theme, inner_width, config.language);
                preview.store_preview_layout(layout_key, layout.clone());
                layout
            };
            let pinned_rows = layout.pinned_rows;
            let nowrap_from = layout.nowrap_from;
            if preview.target.is_none() {
                // Direct Ready injection is retained for renderer fixtures;
                // production Ready content always belongs to a selected target.
                (layout.lines, pinned_rows, nowrap_from)
            } else {
                let mode = if row_paced {
                    LineRevealMode::Rows
                } else {
                    LineRevealMode::Block
                };
                let source = match &preview.state {
                    PreviewState::Ready(
                        PreviewContent::Markdown(source)
                        | PreviewContent::Reasoning(source)
                        | PreviewContent::MutedMarkdown(source),
                    ) => Some(source.as_str()),
                    _ => None,
                };
                let track = preview.reveal.get_or_insert_with(LineRevealTrack::default);
                track.reconcile_source(
                    &layout.lines,
                    source,
                    std::time::Instant::now(),
                    config.preview_lines_per_second.get(),
                    mode,
                );
                (
                    apply_line_reveal(
                        layout.lines,
                        track,
                        config.background_color.color(),
                        theme.surface.primary_text.fg,
                        !config.plain_color,
                    ),
                    pinned_rows,
                    nowrap_from,
                )
            }
        }
    };
    // Ready layouts are wrapped once before entering the cache. Immediate
    // state labels are already bounded single rows.
    let total = lines.len();
    let visible = usize::from(area.height);
    preview.update_scroll_bounds(total, visible);
    let mut visible_lines: Vec<(usize, Line<'static>)> = Vec::new();
    if preview.follows_tail() && total > visible && pinned_rows > 0 {
        let pinned = pinned_rows.min(visible).min(total);
        visible_lines.extend(lines.iter().take(pinned).cloned().enumerate());
        let output_rows = visible.saturating_sub(pinned);
        let output_start = total.saturating_sub(output_rows).max(pinned_rows);
        visible_lines.extend(
            lines
                .into_iter()
                .enumerate()
                .skip(output_start)
                .take(output_rows),
        );
    } else {
        let start = if preview.follows_tail() {
            total.saturating_sub(visible)
        } else {
            preview.scroll
        };
        visible_lines.extend(lines.into_iter().enumerate().skip(start).take(visible));
    }
    let visible_lines = visible_lines
        .into_iter()
        .map(|(source_index, line)| {
            let line = if nowrap_from.is_some_and(|start| source_index >= start) {
                clip_line(line, inner_width)
            } else {
                line
            };
            (source_index, line)
        })
        .collect::<Vec<_>>();
    preview.record_materialized_rows(visible_lines.len());
    // Vertically center content that fits the pane; overflowing content is
    // anchored and fills the pane, so no centering applies.
    let top_padding = if total <= visible {
        visible.saturating_sub(visible_lines.len()) / 2
    } else {
        0
    };
    let mut centered = Vec::with_capacity(usize::from(area.height));
    centered.extend(std::iter::repeat_n(Line::raw(""), top_padding));
    centered.extend(visible_lines.into_iter().map(|(_, line)| line));
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

fn content_layout(
    content: &PreviewContent,
    theme: &Theme,
    width: usize,
    language: crate::Language,
) -> PreviewLayout {
    if let PreviewContent::Tool(tool) = content {
        let (information, secondary) = tool_sections(tool, theme, language);
        let mut lines = information
            .into_iter()
            .flat_map(|line| wrap_line(line, width))
            .collect::<Vec<_>>();
        if let Some(secondary) = secondary {
            lines.push(Line::raw(""));
            let pinned_rows = lines.len();
            lines.extend(secondary);
            return PreviewLayout {
                lines,
                pinned_rows,
                nowrap_from: Some(pinned_rows),
            };
        }
        return PreviewLayout {
            lines,
            pinned_rows: 0,
            nowrap_from: None,
        };
    }

    PreviewLayout {
        lines: content_lines(content, theme, width, language)
            .into_iter()
            .flat_map(|line| wrap_line(line, width))
            .collect(),
        pinned_rows: 0,
        nowrap_from: None,
    }
}

fn content_lines(
    content: &PreviewContent,
    theme: &Theme,
    width: usize,
    language: crate::Language,
) -> Vec<Line<'static>> {
    match content {
        PreviewContent::Link { label, url } => vec![Line::from(vec![
            Span::styled(
                label
                    .clone()
                    .unwrap_or_else(|| tr(language, "preview.link")),
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
            tr_args(language, "preview.search", &[("query", query.clone())]),
            theme.markdown.heading3.style(),
        ))
        .chain(
            matches
                .iter()
                .map(|line| Line::styled(line.clone(), theme.markdown.text.style())),
        )
        .collect(),
        PreviewContent::Command(command) => command_lines(command, theme),
        PreviewContent::Path(path) => {
            vec![Line::styled(path.clone(), theme.markdown.link_url.style())]
        }
        PreviewContent::Markdown(source)
        | PreviewContent::Reasoning(source)
        | PreviewContent::MutedMarkdown(source) => {
            weak_markdown_lines(source, theme, width, language)
        }
        PreviewContent::Tool(preview) => {
            let (mut information, secondary) = tool_sections(preview, theme, language);
            if let Some(secondary) = secondary {
                information.push(Line::raw(""));
                information.extend(secondary);
            }
            information
        }
        PreviewContent::Hunks(hunks) => hunks
            .iter()
            .flat_map(|hunk| hunk_lines(hunk, theme, width, language))
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
fn weak_markdown_lines(
    source: &str,
    theme: &Theme,
    width: usize,
    language: crate::Language,
) -> Vec<Line<'static>> {
    let mut next_unit = 0u64;
    let mut units = std::collections::HashMap::new();
    let options = RenderOptions {
        language,
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

fn command_lines(source: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = command::highlight(
        source,
        command::CommandColors {
            executable: theme.code.r#type.fg,
            argument: theme.surface.primary_text.fg,
            operator: theme.surface.muted_text.fg,
        },
    );
    lines[0]
        .spans
        .insert(0, Span::styled("$ ", theme.input.prompt.style()));
    lines
}

fn tool_sections(
    preview: &ToolPreview,
    theme: &Theme,
    language: Language,
) -> (Vec<Line<'static>>, Option<Vec<Line<'static>>>) {
    let mut information = Vec::new();
    information.push(Line::styled(
        preview.name.clone(),
        theme.activity.label.style(),
    ));
    match &preview.primary {
        ToolPreviewPrimary::Location { path, lines: range } => {
            information.push(Line::styled(
                location_text(path, range),
                theme.surface.primary_text.style(),
            ));
        }
        ToolPreviewPrimary::Command { command, metrics } => {
            information.extend(command_lines(command, theme));
            information.push(Line::styled(
                metrics_text(metrics, language),
                theme.activity.detail.style(),
            ));
        }
        ToolPreviewPrimary::Search { query, path } => {
            information.push(Line::styled(
                format!("\"{query}\""),
                theme.surface.primary_text.style(),
            ));
            if let Some(path) = path {
                information.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", tr(language, "preview.at")),
                        theme.activity.detail.style(),
                    ),
                    Span::styled(format!("\"{path}\""), theme.surface.primary_text.style()),
                ]));
            }
        }
        ToolPreviewPrimary::Json { source, truncated } => {
            for line in source.lines() {
                information.push(Line::styled(
                    line.to_owned(),
                    theme.surface.primary_text.style(),
                ));
            }
            if *truncated {
                information.push(Line::styled("…", theme.activity.detail.style()));
            }
        }
    }
    let secondary = preview.secondary.as_ref().map(|secondary| match secondary {
        ToolPreviewSecondary::Terminal { output, .. } => ansi::terminal_lines(
            output,
            theme.surface.muted_text.style(),
            theme.activity.label.style(),
        ),
    });
    (information, secondary)
}

fn clip_line(line: Line<'static>, width: usize) -> Line<'static> {
    crate::wrap::clip_line(line, width)
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

fn metrics_text(metrics: &ToolMetrics, language: Language) -> String {
    let noun_key = if metrics.output_lines == 1 {
        "preview.metric.line"
    } else {
        "preview.metric.lines"
    };
    let suffix = if metrics.truncated { "+" } else { "" };
    let mut text = tr_args(
        language,
        "preview.metrics",
        &[
            ("noun", tr(language, noun_key)),
            ("count", metrics.output_lines.to_string()),
            ("suffix", suffix.to_owned()),
        ],
    );
    if let Some(duration_ms) = metrics.duration_ms {
        text.push_str(&format!(", {:.1}s", duration_ms as f64 / 1000.0));
    }
    text
}

fn hunk_lines(
    hunk: &crate::preview::MutationHunk,
    theme: &Theme,
    width: usize,
    language: Language,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(path) = &hunk.path {
        lines.push(Line::styled(path.clone(), theme.code.meta.style()));
    }
    if let Some(anchor) = hunk.anchor_line {
        lines.push(Line::styled(
            tr_args(
                language,
                "preview.line_anchor",
                &[("line", anchor.to_string())],
            ),
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

    #[test]
    fn wheel_preview_reaches_boundaries_without_following_or_relayout() {
        use ratatui::{backend::TestBackend, Terminal};
        let config = Config::default();
        let theme = config.theme();
        let mut preview = PreviewPaneState::default();
        preview.state = PreviewState::Ready(PreviewContent::PlainText(
            (0..20)
                .map(|i| format!("row-{i:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
        let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
        let draw = |terminal: &mut Terminal<TestBackend>, preview: &mut PreviewPaneState| {
            terminal
                .draw(|frame| render(frame, frame.area(), preview, &config, &theme, 0, 0))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..6).map(|x| buffer[(x, 0)].symbol()).collect::<String>()
        };
        assert_eq!(draw(&mut terminal, &mut preview), "row-15");
        assert_eq!(preview.take_work_stats().layout_rebuilds, 1);
        preview.scroll_lines(true, 3);
        assert_eq!(draw(&mut terminal, &mut preview), "row-12");
        preview.scroll_lines(true, 100);
        assert_eq!(draw(&mut terminal, &mut preview), "row-00");
        preview.scroll_lines(true, 3);
        assert_eq!(draw(&mut terminal, &mut preview), "row-00");
        preview.scroll_lines(false, 3);
        assert_eq!(draw(&mut terminal, &mut preview), "row-03");
        preview.scroll_lines(false, 100);
        assert_eq!(draw(&mut terminal, &mut preview), "row-15");
        assert!(!preview.follows_tail());
        preview.scroll_lines(false, 3);
        assert_eq!(draw(&mut terminal, &mut preview), "row-15");
        assert_eq!(preview.take_work_stats().layout_rebuilds, 0);
    }

    #[test]
    fn wheel_bottom_does_not_restore_oversized_command_header() {
        use ratatui::{backend::TestBackend, Terminal};
        let config = Config::default();
        let theme = config.theme();
        let mut preview = PreviewPaneState::default();
        preview.state = PreviewState::Ready(PreviewContent::Tool(ToolPreview {
            name: "bash".into(),
            primary: ToolPreviewPrimary::Command {
                command: "echo header\n".repeat(10),
                metrics: ToolMetrics {
                    output_lines: 20,
                    truncated: false,
                    duration_ms: None,
                },
            },
            secondary: Some(ToolPreviewSecondary::Terminal {
                output: (0..20)
                    .map(|i| format!("row-{i:02}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                truncated: false,
            }),
        }));
        let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
        let draw = |terminal: &mut Terminal<TestBackend>, preview: &mut PreviewPaneState| {
            terminal
                .draw(|frame| render(frame, frame.area(), preview, &config, &theme, 0, 0))
                .unwrap();
        };
        draw(&mut terminal, &mut preview);
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "b");
        preview.take_work_stats();
        preview.scroll_lines(true, 3);
        draw(&mut terminal, &mut preview);
        for _ in 0..2 {
            preview.scroll_lines(false, 3);
            draw(&mut terminal, &mut preview);
            let buffer = terminal.backend().buffer();
            let first = (0..6).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
            let last = (0..6).map(|x| buffer[(x, 4)].symbol()).collect::<String>();
            assert_eq!(first, "row-15");
            assert_eq!(last, "row-19");
            assert!(!preview.follows_tail());
        }
        assert_eq!(preview.take_work_stats().layout_rebuilds, 0);
    }

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn command_previews_share_tokens_and_preserve_multiline_information() {
        use ratatui::style::Modifier;

        let theme = Theme::ferra();
        let source = "cargo --release &&\necho 'a | b' >> output.log";
        let standalone = content_lines(
            &PreviewContent::Command(source.into()),
            &theme,
            80,
            Language::English,
        );
        let tool = ToolPreview {
            name: "bash".into(),
            primary: ToolPreviewPrimary::Command {
                command: source.into(),
                metrics: ToolMetrics::default(),
            },
            secondary: None,
        };
        let (information, secondary) = tool_sections(&tool, &theme, Language::English);
        assert_eq!(&information[1..3], standalone.as_slice());
        assert!(secondary.is_none());
        assert_eq!(line_text(&standalone[0]), "$ cargo --release &&");
        assert_eq!(line_text(&standalone[1]), "echo 'a | b' >> output.log");
        assert!(standalone[0].spans.iter().any(|span| {
            span.content == "--release" && span.style.add_modifier.contains(Modifier::ITALIC)
        }));
        let layout = content_layout(&PreviewContent::Tool(tool), &theme, 8, Language::English);
        assert!(layout.lines.iter().all(|line| line.width() <= 8));
        let italic_text = layout
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| span.style.add_modifier.contains(Modifier::ITALIC))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(italic_text, "--release");
    }

    #[test]
    fn terminal_output_remains_one_cached_row_until_viewport_clipping() {
        let theme = Theme::ferra();
        let layout = content_layout(
            &PreviewContent::Tool(ToolPreview {
                name: "bash".into(),
                primary: ToolPreviewPrimary::Command {
                    command: "x".into(),
                    metrics: ToolMetrics {
                        output_lines: 1,
                        truncated: true,
                        duration_ms: None,
                    },
                },
                secondary: Some(ToolPreviewSecondary::Terminal {
                    output: "0123456789ABCDEFGHIJ-TAIL".into(),
                    truncated: true,
                }),
            }),
            &theme,
            8,
            Language::English,
        );
        let output_start = layout.nowrap_from.expect("terminal row boundary");
        assert_eq!(layout.lines.len(), output_start + 1);
        assert_eq!(
            line_text(&layout.lines[output_start]),
            "0123456789ABCDEFGHIJ-TAIL"
        );
        assert_eq!(
            line_text(&clip_line(layout.lines[output_start].clone(), 8)),
            "01234567"
        );
        assert!(layout.lines.iter().all(|line| line_text(line) != "…"));
    }

    #[test]
    fn preview_chrome_localizes_search_link_and_line_anchor() {
        let theme = Theme::ferra();
        let search = content_lines(
            &PreviewContent::SearchResult {
                query: "needle".into(),
                matches: vec!["src/main.rs:10".into()],
            },
            &theme,
            40,
            Language::SimplifiedChinese,
        );
        assert_eq!(line_text(&search[0]), "搜索：needle");
        assert_eq!(line_text(&search[1]), "src/main.rs:10");

        let link = content_lines(
            &PreviewContent::Link {
                label: None,
                url: "https://example.test".into(),
            },
            &theme,
            40,
            Language::SimplifiedChinese,
        );
        assert_eq!(line_text(&link[0]), "链接 https://example.test");

        let hunk = MutationHunk {
            path: Some("src/main.rs".into()),
            old: None,
            new: Some("new line".into()),
            anchor_line: Some(10),
        };
        let anchored = hunk_lines(&hunk, &theme, 40, Language::SimplifiedChinese);
        assert_eq!(line_text(&anchored[1]), "@ 第 10 行");
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
        let lines = hunk_lines(&hunk, &theme, 40, Language::English);
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
        let lines = hunk_lines(&hunk, &theme, 40, Language::English);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts[0].starts_with("\u{258c}    1 \u{2502} old1"));
        assert!(texts[1].starts_with("\u{258c}    2 \u{2502} old2"));
        assert!(texts[2].starts_with("\u{258c}    1 \u{2502} new1"));
        assert!(texts[3].starts_with("\u{258c}    2 \u{2502} new2"));
    }
}
