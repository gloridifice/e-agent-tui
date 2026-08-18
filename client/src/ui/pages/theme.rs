use super::*;

pub(super) fn render_theme_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ThemePage,
    focus: &crate::input_page::FocusState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled("主题", Style::default().fg(theme.fg)),
        ])),
        regions.header,
    );
    let focused = focus.current.as_ref().and_then(|id| {
        id.0.strip_prefix("theme:")
            .and_then(|name| page.themes.iter().position(|item| item.name == name))
    });
    if let Some(index) = focused {
        viewport.ensure_visible(index, regions.body.height as usize, page.themes.len());
    }
    let mut rows = Vec::new();
    for option in page
        .themes
        .iter()
        .skip(viewport.start)
        .take(regions.body.height as usize)
    {
        let id = FocusId::new(format!("theme:{}", option.name));
        let selected = option.name == page.current;
        let style = if focus.is(&id) {
            Style::default().fg(theme.fg).bg(theme.bg)
        } else {
            Style::default().fg(theme.fg)
        };
        let mut spans = vec![
            Span::styled(
                if selected { "● " } else { "○ " },
                Style::default()
                    .fg(if selected { theme.ok } else { theme.dim })
                    .bg(style.bg.unwrap_or(ratatui::style::Color::Reset)),
            ),
            Span::styled(format!("{:<18}", option.name), style),
        ];
        for color in [
            option.palette.bg,
            option.palette.fg,
            option.palette.user,
            option.palette.ok,
            option.palette.err,
            option.palette.running,
        ] {
            spans.push(Span::styled("  ", Style::default().bg(color)));
            spans.push(Span::raw(" "));
        }
        rows.push(Line::from(spans));
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(
            "（无可用主题）",
            Style::default().fg(theme.dim),
        )));
    }
    frame.render_widget(Paragraph::new(rows), regions.body);
    frame.render_widget(
        Paragraph::new("hjkl/方向键移动   Enter 应用   Esc 退出")
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}
