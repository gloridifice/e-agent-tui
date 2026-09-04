use super::*;

pub(super) fn render_resume_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ResumePage,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
    language: crate::Language,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.input.hint.fg)),
            Span::styled("/resume", Style::default().fg(theme.fg)),
            Span::styled(
                format!(
                    "  {}  ",
                    crate::i18n::tr(language, "input_page.resume.title")
                ),
                Style::default().fg(theme.dim),
            ),
            Span::styled("> ", Style::default().fg(theme.dim)),
            Span::styled(page.query.clone(), Style::default().fg(theme.fg)),
            Span::styled("█", Style::default().fg(theme.user)),
        ])),
        regions.header,
    );

    let filtered = page.filtered_indices();
    viewport.ensure_visible(page.sel, regions.body.height as usize, filtered.len());
    let width = regions.body.width as usize;
    let mut rows = Vec::new();
    for (filtered_index, session_index) in filtered
        .iter()
        .enumerate()
        .skip(viewport.start)
        .take(regions.body.height as usize)
    {
        let session = &page.sessions[*session_index];
        let focused = filtered_index == page.sel;
        let style = input_page_item_style(theme, focused, false);
        let title = if session.title.is_empty() && page.titles_pending {
            crate::i18n::tr(language, "input_page.resume.title_loading")
        } else if session.title.is_empty() {
            crate::i18n::tr(language, "input_page.resume.unnamed")
        } else {
            session.title.clone()
        };
        let shown_id = trim_to_width(&session.id, 24);
        let id_width = UnicodeWidthStr::width(shown_id.as_str());
        let title_width = width.saturating_sub(id_width + 5);
        rows.push(Line::from(vec![
            Span::styled(if focused { "› " } else { "  " }, style),
            Span::styled(
                if session.live { "● " } else { "  " },
                if focused {
                    style
                } else {
                    Style::default().fg(if session.live { theme.ok } else { theme.dim })
                },
            ),
            Span::styled(trim_to_width(&title, title_width), style),
            Span::styled(
                format!("  {shown_id}"),
                if focused {
                    style
                } else {
                    Style::default().fg(theme.dim)
                },
            ),
        ]));
    }
    if page.loading && page.sessions.is_empty() {
        rows.push(Line::from(Span::styled(
            crate::i18n::tr(language, "input_page.resume.loading"),
            Style::default().fg(theme.dim),
        )));
    } else if filtered.is_empty() {
        rows.push(Line::from(Span::styled(
            crate::i18n::tr(language, "input_page.resume.no_matches"),
            Style::default().fg(theme.dim),
        )));
    }
    frame.render_widget(Paragraph::new(rows), regions.body);
    frame.render_widget(
        Paragraph::new(crate::i18n::tr(language, "input_page.resume.footer"))
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}
