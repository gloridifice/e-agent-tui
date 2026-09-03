use super::*;
use crate::input_page::EffortPage;

pub(super) fn render_effort_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &EffortPage,
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
                crate::i18n::tr(language, "input_page.effort.title"),
                Style::default().fg(theme.fg),
            ),
        ])),
        regions.header,
    );
    if page.loading {
        frame.render_widget(
            Paragraph::new(crate::i18n::tr(language, "input_page.effort.loading"))
                .style(Style::default().fg(theme.dim)),
            regions.body,
        );
    } else if page.unavailable || page.efforts.is_empty() {
        frame.render_widget(
            Paragraph::new(crate::i18n::tr(language, "input_page.effort.unavailable"))
                .style(Style::default().fg(theme.dim)),
            regions.body,
        );
    } else {
        let visible = regions.body.height as usize;
        let focus_index = focus.current.as_ref().and_then(|id| {
            id.0.strip_prefix("effort:").and_then(|effort_id| {
                page.efforts
                    .iter()
                    .position(|effort| effort.id == effort_id)
            })
        });
        if let Some(index) = focus_index {
            viewport.ensure_visible(index, visible, page.efforts.len());
        }
        let explicit = page
            .current
            .as_ref()
            .and_then(|current| current.reasoning_effort.as_deref());
        let mut rows = Vec::new();
        for effort in page.efforts.iter().skip(viewport.start).take(visible) {
            let id = FocusId::new(format!("effort:{}", effort.id));
            let style = if focus.is(&id) {
                Style::default().fg(theme.fg).bg(theme.bg)
            } else {
                Style::default().fg(theme.fg)
            };
            let selected = explicit == Some(effort.id.as_str());
            let default =
                explicit.is_none() && page.default_effort.as_deref() == Some(effort.id.as_str());
            let mut label = effort.name.clone();
            if default {
                label.push_str(&crate::i18n::tr(language, "input_page.effort.default"));
            }
            rows.push(Line::from(vec![
                Span::styled(
                    if selected { "● " } else { "○ " },
                    Style::default()
                        .fg(if selected { theme.ok } else { theme.dim })
                        .bg(style.bg.unwrap_or(theme.bg_soft)),
                ),
                Span::styled(
                    trim_to_width(&label, regions.body.width.saturating_sub(2) as usize),
                    style,
                ),
            ]));
        }
        if explicit.is_none() && page.default_effort.is_none() {
            rows.insert(
                0,
                Line::from(Span::styled(
                    crate::i18n::tr(language, "input_page.effort.current_default"),
                    Style::default().fg(theme.dim),
                )),
            );
        }
        frame.render_widget(Paragraph::new(rows), regions.body);
    }
    frame.render_widget(
        Paragraph::new(crate::i18n::tr(language, "input_page.effort.footer"))
            .style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}
