use super::*;

pub(super) fn render_resume_page(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    page: &mut ResumePage,
    viewport: &mut crate::input_page::ViewportState,
    theme: &Theme,
    config: &crate::Config,
) {
    let language = config.language;
    let regions = input_page_shell(frame, area, theme);
    page.paging.visible_rows = regions.body.height as usize;
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
        let title = if session.title.is_empty() && page.titles_pending {
            crate::i18n::tr(language, "input_page.resume.title_loading")
        } else if session.title.is_empty() {
            crate::i18n::tr(language, "input_page.resume.unnamed")
        } else {
            session.title.clone()
        };
        rows.push(resume_row(
            &title,
            session.modified_label.as_deref(),
            focused,
            width,
            theme,
        ));
    }
    if filtered.is_empty() {
        let key = if page.search_pending() {
            "input_page.resume.loading"
        } else {
            "input_page.resume.no_matches"
        };
        rows.push(Line::from(Span::styled(
            crate::i18n::tr(language, key),
            Style::default().fg(theme.dim),
        )));
    }
    frame.render_widget(Paragraph::new(rows), regions.body);
    let footer = if page.search_pending() {
        Line::from(crate::i18n::tr(language, "input_page.resume.loading"))
    } else if let Some(diagnostic) = &page.paging.diagnostic {
        Line::from(diagnostic.clone())
    } else {
        Line::from(page_key_hints(config, KeyScope::PageResume))
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}

fn resume_row(
    title: &str,
    date: Option<&str>,
    focused: bool,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let marker = trim_to_width(if focused { "› " } else { "  " }, width.min(2));
    let available = width.saturating_sub(UnicodeWidthStr::width(marker.as_str()));
    let date = trim_to_width(date.unwrap_or(""), available);
    let date_width = UnicodeWidthStr::width(date.as_str());
    let title_width = available.saturating_sub(date_width + usize::from(date_width > 0));
    let title = trim_to_width(title, title_width);
    let gap = available.saturating_sub(UnicodeWidthStr::width(title.as_str()) + date_width);
    Line::from(vec![
        Span::styled(marker, input_page_item_style(theme, focused, false)),
        Span::styled(title, Style::default().fg(theme.fg)),
        Span::raw(" ".repeat(gap)),
        Span::styled(date, Style::default().fg(theme.dim)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_render_drives_batch_size_hides_paths_and_distinguishes_search_progress() {
        use crate::{agent::SessionSummary, resume::ResumeBatch};
        use ratatui::{backend::TestBackend, Terminal};

        for language in [crate::Language::English, crate::Language::SimplifiedChinese] {
            let config = crate::Config {
                language,
                ..Default::default()
            };
            let theme = config.theme();
            let mut page = ResumePage::loading();
            let mut viewport = crate::input_page::ViewportState::default();
            let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
            let render =
                |terminal: &mut Terminal<TestBackend>,
                 page: &mut ResumePage,
                 viewport: &mut crate::input_page::ViewportState| {
                    let rendered = terminal
                        .draw(|frame| {
                            render_resume_page(frame, frame.area(), page, viewport, &theme, &config)
                        })
                        .unwrap();
                    (0..12)
                        .map(|y| {
                            let mut text = String::new();
                            let mut x = 0;
                            while x < 60 {
                                let symbol = rendered.buffer[(x, y)].symbol();
                                text.push_str(symbol);
                                x += UnicodeWidthStr::width(symbol).max(1) as u16;
                            }
                            text
                        })
                        .collect::<Vec<_>>()
                };
            render(&mut terminal, &mut page, &mut viewport);
            let request = page.take_request("/project").unwrap();
            assert_eq!(request.limit, 14);
            page.apply_batch(
                ResumeBatch {
                    next_offset: request.limit,
                    request,
                    sessions: vec![SessionSummary {
                        id: "/private/session.jsonl".into(),
                        title: "Visible title".into(),
                        live: true,
                        created_at: 0,
                        modified_label: Some("2026-09-10 15:30".into()),
                    }],
                    has_more: true,
                    diagnostic: None,
                },
                "/project",
            );
            let rows = render(&mut terminal, &mut page, &mut viewport);
            assert!(rows[3].starts_with("› Visible title"));
            assert!(rows[3].ends_with("2026-09-10 15:30"));
            assert!(!rows.join("\n").contains("/private/session.jsonl"));
            page.query = "missing".into();
            let rows = render(&mut terminal, &mut page, &mut viewport);
            assert!(
                rows[3].contains(&crate::i18n::tr(language, "input_page.resume.loading")),
                "{:?}",
                rows[3]
            );
            let request = page.take_request("/project").unwrap();
            page.apply_batch(
                ResumeBatch {
                    next_offset: request.offset,
                    request,
                    sessions: Vec::new(),
                    has_more: false,
                    diagnostic: None,
                },
                "/project",
            );
            let rows = render(&mut terminal, &mut page, &mut viewport);
            assert!(
                rows[3].contains(&crate::i18n::tr(language, "input_page.resume.no_matches")),
                "{:?}",
                rows[3]
            );
        }
    }

    #[test]
    fn resume_rows_reserve_date_space_and_never_wrap() {
        let theme = crate::Config::default().theme();
        for width in 0..80 {
            let line = resume_row(
                "Long 中文 session title to truncate",
                Some("2026-09-10 15:30"),
                true,
                width,
                &theme,
            );
            assert!(line.width() <= width);
            if width >= 18 {
                assert_eq!(line.width(), width);
                assert_eq!(line.spans.last().unwrap().content, "2026-09-10 15:30");
            }
        }
        let line = resume_row("A session", None, false, 40, &theme);
        assert!(line.spans.last().unwrap().content.is_empty());
        assert_eq!(line.spans[1].content, "A session");
    }
}
