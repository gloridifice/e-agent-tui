use super::*;

pub(super) fn render_theme_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ThemePage,
    focus: &crate::input_page::FocusState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
    language: crate::Language,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.input.hint.fg)),
            Span::styled("/theme", Style::default().fg(theme.fg)),
            Span::styled(
                format!("  {}", crate::i18n::tr(language, "input_page.theme.title")),
                Style::default().fg(theme.dim),
            ),
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
        let style = input_page_item_style(theme, focus.is(&id), selected);
        let mut spans = vec![
            Span::styled(if selected { "● " } else { "○ " }, style),
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
            spans.push(Span::styled("██", Style::default().fg(color)));
            spans.push(Span::raw(" "));
        }
        rows.push(Line::from(spans));
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(
            crate::i18n::tr(language, "input_page.theme.no_themes"),
            Style::default().fg(theme.dim),
        )));
    }
    frame.render_widget(Paragraph::new(rows), regions.body);
    frame.render_widget(
        Paragraph::new(crate::i18n::tr(language, "input_page.theme.footer"))
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}
