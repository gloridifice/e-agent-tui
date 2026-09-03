use super::*;

pub(super) fn render_model_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &ModelPage,
    focus: &crate::input_page::FocusState,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
    language: crate::Language,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled(
                crate::i18n::tr(language, "input_page.model.title"),
                Style::default().fg(theme.fg),
            ),
        ])),
        regions.header,
    );
    if page.loading {
        frame.render_widget(
            Paragraph::new(crate::i18n::tr(language, "input_page.model.loading"))
                .style(Style::default().fg(theme.dim)),
            regions.body,
        );
    } else {
        let columns = Layout::horizontal([
            Constraint::Percentage(38),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(regions.body);
        let visible = regions.body.height as usize;
        let provider_focus = focus.current.as_ref().and_then(|id| {
            id.0.strip_prefix("provider:")
                .and_then(|provider| page.providers.iter().position(|item| item.id == provider))
        });
        if let Some(index) = provider_focus {
            viewport.ensure_visible(index, visible, page.providers.len());
        }
        let mut providers = Vec::new();
        for provider in page.providers.iter().skip(viewport.start).take(visible) {
            let id = FocusId::new(format!("provider:{}", provider.id));
            let active = page.active_provider.as_deref() == Some(provider.id.as_str());
            let style = if focus.is(&id) {
                Style::default().fg(theme.fg).bg(theme.bg)
            } else {
                Style::default().fg(theme.fg)
            };
            providers.push(Line::from(vec![
                Span::styled(
                    if active { "● " } else { "○ " },
                    Style::default()
                        .fg(if active { theme.ok } else { theme.dim })
                        .bg(style.bg.unwrap_or(theme.bg_soft)),
                ),
                Span::styled(
                    trim_to_width(&provider.name, columns[0].width.saturating_sub(2) as usize),
                    style,
                ),
            ]));
        }
        if page.providers.is_empty() {
            providers.push(Line::from(Span::styled(
                crate::i18n::tr(language, "input_page.model.no_providers"),
                Style::default().fg(theme.dim),
            )));
        }
        frame.render_widget(Paragraph::new(providers), columns[0]);

        let active_provider = page.active_provider.as_deref().unwrap_or("");
        let models = page.active_models();
        let model_focus = focus.current.as_ref().and_then(|id| {
            id.0.strip_prefix(&format!("model:{active_provider}:"))
                .and_then(|model| models.iter().position(|item| item.id == model))
        });
        let model_start = model_focus
            .map(|index| index.saturating_sub(visible.saturating_sub(1)))
            .unwrap_or(0);
        let mut model_rows = Vec::new();
        for model in models.iter().skip(model_start).take(visible) {
            let id = FocusId::new(format!("model:{active_provider}:{}", model.id));
            let selected = page.current.as_ref().is_some_and(|(provider, current)| {
                provider == active_provider && current == &model.id
            });
            let style = if focus.is(&id) {
                Style::default().fg(theme.fg).bg(theme.bg)
            } else {
                Style::default().fg(theme.fg)
            };
            model_rows.push(Line::from(vec![
                Span::styled(
                    if selected { "● " } else { "○ " },
                    Style::default()
                        .fg(if selected { theme.ok } else { theme.dim })
                        .bg(style.bg.unwrap_or(theme.bg_soft)),
                ),
                Span::styled(
                    trim_to_width(&model.name, columns[2].width.saturating_sub(2) as usize),
                    style,
                ),
            ]));
        }
        if models.is_empty() && !page.providers.is_empty() {
            model_rows.push(Line::from(Span::styled(
                crate::i18n::tr(language, "input_page.model.no_models"),
                Style::default().fg(theme.dim),
            )));
        }
        frame.render_widget(Paragraph::new(model_rows), columns[2]);
    }
    frame.render_widget(
        Paragraph::new(crate::i18n::tr(language, "input_page.model.footer"))
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}
