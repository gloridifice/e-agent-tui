use super::*;

pub(super) fn render_question_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    batch: &crate::question::QuestionBatch,
    focus: &crate::input_page::FocusState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
) -> Option<Position> {
    let regions = input_page_shell(frame, area, theme);
    let Some(question) = batch.questions.get(batch.current) else {
        frame.render_widget(
            Paragraph::new("（没有可显示的问题）").style(Style::default().fg(theme.dim)),
            regions.body,
        );
        return None;
    };
    let title = question
        .header
        .as_deref()
        .filter(|header| !header.is_empty())
        .unwrap_or("问题");
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled(title.to_owned(), Style::default().fg(theme.fg)),
            Span::styled(
                format!("  ({}/{})", batch.current + 1, batch.questions.len()),
                Style::default().fg(theme.dim),
            ),
        ])),
        regions.header,
    );

    let width = regions.body.width.max(1) as usize;
    let question_lines = wrap_text(&question.question, width);
    let question_rows = question_lines.len().max(1);
    let option_top = question_rows.saturating_add(1);
    let visible_options = (regions.body.height as usize)
        .saturating_sub(option_top)
        .max(1);
    let options = batch.current_options();
    if !options.is_empty() {
        viewport.ensure_visible(batch.sel, visible_options, options.len());
    } else {
        viewport.start = 0;
    }

    let mut rows = question_lines
        .into_iter()
        .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.fg))))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(Line::default());
    }
    rows.push(Line::default());

    let mut cursor = None;
    if options.is_empty() {
        let prefix = "❯ ";
        rows.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme.user)),
            Span::styled(batch.draft.clone(), Style::default().fg(theme.fg)),
            Span::styled(" ", Style::default().fg(theme.bg).bg(theme.fg)),
        ]));
        if regions.body.width > 0 && regions.body.height > 0 {
            let y = regions.body.y + rows.len().saturating_sub(1) as u16;
            let x = regions.body.x
                + unicode_width::UnicodeWidthStr::width(prefix) as u16
                + unicode_width::UnicodeWidthStr::width(batch.draft.as_str()) as u16;
            cursor = Some(Position::new(
                x.min(regions.body.right().saturating_sub(1)),
                y.min(regions.body.bottom().saturating_sub(1)),
            ));
        }
    } else {
        for (index, option) in options
            .iter()
            .enumerate()
            .skip(viewport.start)
            .take(visible_options)
        {
            let id = FocusId::new(format!("question:{}:option:{index}", question.id));
            let focused = focus.is(&id);
            let selected = batch.is_option_selected(index);
            let style = if focused {
                Style::default().fg(theme.fg).bg(theme.bg)
            } else {
                Style::default().fg(theme.fg)
            };
            let marker = if selected { "● " } else { "○ " };
            let mut text = option.label.clone();
            if let Some(description) = option
                .description
                .as_deref()
                .filter(|description| !description.is_empty())
            {
                text.push_str(" — ");
                text.push_str(description);
            }
            rows.push(Line::from(vec![
                Span::styled(
                    marker,
                    Style::default()
                        .fg(if selected { theme.ok } else { theme.dim })
                        .bg(style.bg.unwrap_or(theme.bg_soft)),
                ),
                Span::styled(trim_to_width(&text, width.saturating_sub(2)), style),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(rows), regions.body);

    let action = if batch.current + 1 < batch.questions.len() {
        "下一题"
    } else {
        "提交"
    };
    let footer = if options.is_empty() {
        format!("输入答案   ←/→ 切换问题   Enter {action}   Esc 取消")
    } else {
        let select = if question.multi_select {
            "Space 多选/取消"
        } else {
            "Space 选择"
        };
        format!("h/l ←/→ 问题   j/k ↑/↓ 选项   {select}   Enter {action}   Esc 取消")
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(theme.dim)),
        regions.footer,
    );
    cursor
}
