use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
    Frame,
};

use crate::{
    preview::{PreviewContent, PreviewPaneState, PreviewState},
    theme::Theme,
    ui::component::diff,
};

pub fn render(frame: &mut Frame, area: Rect, preview: &mut PreviewPaneState, theme: &Theme) {
    let mut lines = match &preview.state {
        PreviewState::Empty => vec![Line::styled("No preview", theme.surface.muted_text.style())],
        PreviewState::Loading { .. } => vec![Line::styled(
            "• Loading preview…",
            theme.working_status.running.style(),
        )],
        PreviewState::Error(error) => vec![Line::styled(
            format!("Preview error: {error}"),
            theme.log.error.style(),
        )],
        PreviewState::Ready(content) => content_lines(content, theme),
    };
    let start = preview.scroll.min(lines.len().saturating_sub(1));
    let visible = usize::from(area.height.saturating_sub(2));
    lines = lines.into_iter().skip(start).take(visible).collect();
    preview.record_materialized_rows(lines.len());
    let block = Block::bordered()
        .title(Line::styled(" Preview ", theme.overlay.accent.style()))
        .border_style(theme.overlay.border.style())
        .padding(Padding::horizontal(1));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(theme.surface.base.bg.unwrap_or(theme.bg))),
        area,
    );
}

fn content_lines(content: &PreviewContent, theme: &Theme) -> Vec<Line<'static>> {
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
        PreviewContent::PlainText(text) => text
            .lines()
            .map(|line| Line::styled(line.to_owned(), theme.surface.primary_text.style()))
            .collect(),
    }
}
