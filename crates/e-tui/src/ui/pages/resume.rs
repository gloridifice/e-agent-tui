use super::*;

pub(super) fn render_resume_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ResumePage,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled("续接会话  ", Style::default().fg(theme.fg)),
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
        let selected = filtered_index == page.sel;
        let style = if selected {
            Style::default().fg(theme.fg).bg(theme.bg)
        } else {
            Style::default().fg(theme.fg)
        };
        let title = if session.title.is_empty() && page.titles_pending {
            "(标题读取中…)"
        } else if session.title.is_empty() {
            "(未命名会话)"
        } else {
            session.title.as_str()
        };
        let shown_id = trim_to_width(&session.id, 24);
        let id_width = UnicodeWidthStr::width(shown_id.as_str());
        let title_width = width.saturating_sub(id_width + 5);
        rows.push(Line::from(vec![
            Span::styled(if selected { "› " } else { "  " }, style),
            Span::styled(
                if session.live { "● " } else { "  " },
                Style::default()
                    .fg(if session.live { theme.ok } else { theme.dim })
                    .bg(style.bg.unwrap_or(theme.bg_soft)),
            ),
            Span::styled(trim_to_width(title, title_width), style),
            Span::styled(
                format!("  {shown_id}"),
                Style::default()
                    .fg(theme.dim)
                    .bg(style.bg.unwrap_or(theme.bg_soft)),
            ),
        ]));
    }
    if page.loading && page.sessions.is_empty() {
        rows.push(Line::from(Span::styled(
            "读取会话列表中…",
            Style::default().fg(theme.dim),
        )));
    } else if filtered.is_empty() {
        rows.push(Line::from(Span::styled(
            "（无匹配会话）",
            Style::default().fg(theme.dim),
        )));
    }
    frame.render_widget(Paragraph::new(rows), regions.body);
    frame.render_widget(
        Paragraph::new("输入文字筛选   ↑↓ 选择   Enter 续接   Esc 退出")
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}
