use super::*;
use std::time::{Instant, SystemTime};

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

    let filtered = page.tree_rows();
    viewport.ensure_visible(page.sel, regions.body.height as usize, filtered.len());
    let width = regions.body.width as usize;
    let mut rows = Vec::new();
    let now = SystemTime::now();
    let frame_time = Instant::now();
    page.age_refresh = None;
    for (filtered_index, row) in filtered
        .iter()
        .enumerate()
        .skip(viewport.start)
        .take(regions.body.height as usize)
    {
        let session = &page.sessions[row.index];
        let focused = filtered_index == page.sel;
        let title = if session.title.is_empty() && page.titles_pending {
            crate::i18n::tr(language, "input_page.resume.title_loading")
        } else if session.title.is_empty() {
            crate::i18n::tr(language, "input_page.resume.unnamed")
        } else {
            session.title.clone()
        };
        let title = format!(
            "{}{title}{}",
            row.prefix,
            if page.parents.contains_key(&session.id) {
                " (fork)"
            } else {
                ""
            }
        );
        let age = session.modified_at.map(|modified| {
            let (label, refresh_in) = crate::resume::relative_age(modified, now);
            page.age_refresh = crate::reveal::earliest_deadline(
                page.age_refresh,
                frame_time.checked_add(refresh_in),
            );
            label
        });
        rows.push(resume_row(&title, age.as_deref(), focused, width, theme));
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
    age: Option<&str>,
    focused: bool,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let marker = trim_to_width(if focused { "› " } else { "  " }, width.min(2));
    let available = width.saturating_sub(UnicodeWidthStr::width(marker.as_str()));
    let age = trim_to_width(age.unwrap_or(""), available);
    let age_width = UnicodeWidthStr::width(age.as_str());
    let title_width = available.saturating_sub(age_width + usize::from(age_width > 0));
    let title = trim_to_width(title, title_width);
    let gap = available.saturating_sub(UnicodeWidthStr::width(title.as_str()) + age_width);
    let title_style = input_page_item_style(theme, focused, false);
    Line::from(vec![
        Span::styled(marker, title_style),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(age, Style::default().fg(theme.dim)),
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
            assert_eq!(request.limit, 3);
            page.apply_batch(
                ResumeBatch {
                    next_offset: request.limit,
                    request,
                    sessions: ["Visible title", "Second title", "Third title"]
                        .into_iter()
                        .enumerate()
                        .map(|(i, title)| SessionSummary {
                            id: format!("/private/session-{i}.jsonl"),
                            title: title.into(),
                            live: false,
                            created_at: 0,
                            modified_at: Some(now_modified()),
                        })
                        .collect(),
                    has_more: true,
                    diagnostic: None,
                },
                "/project",
            );
            let rows = render(&mut terminal, &mut page, &mut viewport);
            assert!(rows[3].starts_with("› Visible title"));
            assert!(rows[3].ends_with("3d2h"));
            assert!(rows[4].contains("Second title"));
            assert!(rows[5].contains("Third title"));
            assert!(page.age_refresh.is_some());
            let request = page.take_request("/project").unwrap();
            assert_eq!((request.offset, request.limit), (3, 3));
            page.apply_batch(
                ResumeBatch {
                    next_offset: request.offset + 1,
                    request,
                    sessions: vec![SessionSummary {
                        id: "/private/older.jsonl".into(),
                        title: "Older title".into(),
                        live: false,
                        created_at: 0,
                        modified_at: None,
                    }],
                    has_more: true,
                    diagnostic: None,
                },
                "/project",
            );
            let rows = render(&mut terminal, &mut page, &mut viewport);
            assert!(rows[3].starts_with("› Visible title"));
            assert!(rows[6].trim_end().ends_with("Older title"));
            assert!(!rows.join("\n").contains("/private/"));
            page.query = "missing".into();
            let rows = render(&mut terminal, &mut page, &mut viewport);
            assert!(page.age_refresh.is_none());
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

    fn now_modified() -> SystemTime {
        SystemTime::now() - std::time::Duration::from_secs(266_401)
    }

    #[test]
    fn resume_rows_reserve_age_space_and_never_wrap() {
        let theme = crate::Config::default().theme();
        for width in 0..80 {
            let line = resume_row(
                "Long 中文 session title to truncate",
                Some("3d2h"),
                true,
                width,
                &theme,
            );
            assert!(line.width() <= width);
            if width >= 6 {
                assert_eq!(line.width(), width);
                assert_eq!(line.spans.last().unwrap().content, "3d2h");
            }
        }
        let line = resume_row("A session", None, false, 40, &theme);
        assert!(line.spans.last().unwrap().content.is_empty());
        assert_eq!(line.spans[1].content, "A session");
    }
}
