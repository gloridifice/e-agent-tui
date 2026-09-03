use super::*;

pub(crate) fn render_settings(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    settings: &mut SettingsState,
    config: &crate::config::Config,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);

    // ---- display-only category navigation ----
    // The active page is marked by color; the strip never enters focus.
    {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.bg)),
            regions.header,
        );
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, cat) in crate::settings::CATEGORIES.iter().enumerate() {
            let fg = if i == settings.category {
                theme.ok
            } else {
                theme.fg
            };
            spans.push(Span::styled(
                format!(" {} ", crate::i18n::tr(config.language, cat)),
                Style::default().fg(fg).bg(theme.bg),
            ));
            spans.push(Span::styled(
                "  ",
                Style::default().fg(theme.fg).bg(theme.bg),
            ));
        }
        let line = Line::from(spans);
        let width = (line.width() as u16).min(regions.header.width);
        let x = regions.header.x + regions.header.width.saturating_sub(width) / 2;
        let buffer = frame.buffer_mut();
        buffer.set_line(x, regions.header.y, &line, width);
    }

    // ---- two columns with a 2-space gap: small name/description column
    // ---- left, values right.
    let cols = Layout::horizontal([
        Constraint::Percentage(30),
        Constraint::Length(2),
        Constraint::Min(1),
    ])
    .split(regions.body);
    let left_width = cols[0].width as usize;
    let value_width = cols[2].width;
    // Values form a distinct Night pane, including blank rows and unused
    // space below the last visible item.
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg)),
        cols[2],
    );
    let items = crate::settings::items_in(settings.category);
    let focus_idx = items
        .get(settings.pos[settings.category])
        .filter(|item| item.kind != crate::settings::ItemKind::ReadOnly)
        .map(|_| settings.pos[settings.category]);
    let mut left_rows: Vec<Line<'static>> = Vec::new();
    let mut right_rows: Vec<Line<'static>> = Vec::new();
    // (item index, first row, row count) for scroll anchoring.
    let mut anchors: Vec<(usize, usize, usize)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let focused = focus_idx == Some(idx);
        // While editing, the focus moves to the value: the name loses the
        // Night highlight.
        let editing = focused && settings.editing.is_some();
        let name_bg = if focused && !editing {
            theme.bg
        } else {
            theme.bg_soft
        };
        let first = left_rows.len();
        // Name row: fg text; the focused NAME fills with Night (the
        // description below is never selected).
        let mut name_line = Line::from(Span::styled(
            crate::i18n::tr(config.language, item.label),
            Style::default().fg(theme.fg).bg(name_bg),
        ));
        if focused && !editing {
            let used = name_line.width();
            if used < left_width {
                name_line.push_span(Span::styled(
                    " ".repeat(left_width - used),
                    Style::default().fg(theme.fg).bg(theme.bg),
                ));
            }
        }
        left_rows.push(name_line);
        // Description: Bark foreground on Ash, wrapped under the name.
        let mut height = 1usize;
        for chunk in wrap_text(
            &crate::i18n::tr(config.language, item.desc),
            left_width.max(1),
        ) {
            left_rows.push(Line::from(Span::styled(
                chunk,
                Style::default().fg(theme.dim).bg(theme.bg_soft),
            )));
            height += 1;
        }
        right_rows.push(settings_value_line(
            item,
            config,
            theme,
            settings,
            focused,
            value_width,
        ));
        for _ in 1..height {
            right_rows.push(Line::default());
        }
        anchors.push((idx, first, height));
    }
    // Keep the focused item visible (display-only scroll).
    let visible = regions.body.height as usize;
    if let Some((_, first, height)) = anchors
        .iter()
        .find(|(idx, _, _)| *idx == settings.pos[settings.category])
    {
        if settings.scroll > *first {
            settings.scroll = *first;
        }
        if first + height > settings.scroll + visible {
            settings.scroll = (first + height).saturating_sub(visible);
        }
    }
    settings.scroll = settings.scroll.min(left_rows.len().saturating_sub(1));
    let end = (settings.scroll + visible).min(left_rows.len());
    let left_slice: Vec<Line<'static>> = left_rows[settings.scroll..end].to_vec();
    let right_slice: Vec<Line<'static>> = right_rows[settings.scroll..end].to_vec();
    frame.render_widget(Paragraph::new(Text::from(left_slice)), cols[0]);
    frame.render_widget(Paragraph::new(Text::from(right_slice)), cols[2]);

    // ---- key hints ----
    let hint = if settings.editing.is_some() {
        crate::i18n::tr(config.language, "settings.footer.edit")
    } else {
        crate::i18n::tr(config.language, "settings.footer.browse")
    };
    let buffer = frame.buffer_mut();
    buffer.set_line(
        regions.footer.x,
        regions.footer.y,
        &Line::from(Span::styled(
            hint,
            Style::default().fg(theme.dim).bg(theme.bg_soft),
        )),
        regions.footer.width,
    );
}

/// The value cell of one settings row. Choice values show every option as
/// `○ label` (unselected, default fg) / `● label` (selected, green). The
/// cell sits on Night; while editing, the focused element (cursor option or
/// input buffer) turns Ash so it remains visible on the dark pane.
fn settings_value_line(
    item: &crate::settings::ItemDef,
    config: &crate::config::Config,
    theme: &Theme,
    settings: &SettingsState,
    focused_row: bool,
    width: u16,
) -> Line<'static> {
    let editing_input =
        focused_row && matches!(settings.editing, Some(crate::settings::Edit::Input { .. }));
    let editing_choice = match settings.editing {
        Some(crate::settings::Edit::Choice { cursor }) if focused_row => Some(cursor),
        _ => None,
    };
    let cell_bg = if editing_input {
        theme.bg_soft
    } else {
        theme.bg
    };
    let line: Line<'static> = match item.kind {
        crate::settings::ItemKind::Choice { options } => {
            let current = (item.get)(config);
            let selected = options.iter().position(|(value, _)| *value == current);
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, (_, label_key)) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(
                        "  ",
                        Style::default().fg(theme.fg).bg(cell_bg),
                    ));
                }
                let is_sel = Some(i) == selected;
                let (glyph, fg) = if is_sel {
                    ("●", theme.ok)
                } else {
                    ("○", theme.fg)
                };
                let bg = if Some(i) == editing_choice {
                    theme.bg_soft
                } else {
                    cell_bg
                };
                spans.push(Span::styled(
                    format!("{glyph} {}", crate::i18n::tr(config.language, label_key)),
                    Style::default().fg(fg).bg(bg),
                ));
            }
            Line::from(spans)
        }
        crate::settings::ItemKind::ModeChoice | crate::settings::ItemKind::ThemeChoice => {
            // Same ○/● rendering over the live roster (the current value is
            // appended when the roster no longer lists it).
            let options =
                crate::settings::dynamic_options(item, config, &settings.modes, &settings.themes);
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, opt) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(
                        "  ",
                        Style::default().fg(theme.fg).bg(cell_bg),
                    ));
                }
                let is_sel = *opt == (item.get)(config);
                let (glyph, fg) = if is_sel {
                    ("●", theme.ok)
                } else {
                    ("○", theme.fg)
                };
                let bg = if Some(i) == editing_choice {
                    theme.bg_soft
                } else {
                    cell_bg
                };
                spans.push(Span::styled(
                    format!("{glyph} {opt}"),
                    Style::default().fg(fg).bg(bg),
                ));
            }
            Line::from(spans)
        }
        crate::settings::ItemKind::Input => {
            let text = if editing_input {
                match &settings.editing {
                    Some(crate::settings::Edit::Input { buf }) => format!("{buf}█"),
                    _ => String::new(),
                }
            } else {
                (item.get)(config)
            };
            let fg = if editing_input { theme.ok } else { theme.fg };
            let mut input_line =
                Line::from(Span::styled(text, Style::default().fg(fg).bg(cell_bg)));
            if editing_input {
                // Fill the row so the focused input cell reads as an Ash block.
                let used = input_line.width();
                if used < width as usize {
                    input_line.push_span(Span::styled(
                        " ".repeat(width as usize - used),
                        Style::default().fg(theme.fg).bg(theme.bg_soft),
                    ));
                }
            }
            input_line
        }
        crate::settings::ItemKind::ReadOnly => Line::from(Span::styled(
            trim_to_width(&(item.get)(config), width as usize),
            Style::default().fg(theme.dim).bg(cell_bg),
        )),
    };
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn settings_header_and_value_pane_use_night_background() {
        let width = 80;
        let height = 24;
        let theme = Theme::ferra();
        let config = crate::config::Config::default();
        let mut settings = SettingsState::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();

        terminal
            .draw(|frame| {
                render_settings(
                    frame,
                    ratatui::layout::Rect::new(0, 0, width, height),
                    &mut settings,
                    &config,
                    &theme,
                );
            })
            .unwrap();

        let inner = ratatui::layout::Rect::new(2, 1, width - 4, height - 2);
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);
        let columns = Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(rows[2]);
        let buffer = terminal.backend().buffer();

        let is_wide_continuation = |x: u16, y: u16, left: u16| {
            x > left && UnicodeWidthStr::width(buffer[(x - 1, y)].symbol()) > 1
        };
        for x in rows[0].x..rows[0].right() {
            if !is_wide_continuation(x, rows[0].y, rows[0].x) {
                assert_eq!(buffer[(x, rows[0].y)].bg, theme.bg, "header x={x}");
            }
        }
        for y in columns[2].y..columns[2].bottom() {
            for x in columns[2].x..columns[2].right() {
                if !is_wide_continuation(x, y, columns[2].x) {
                    assert_eq!(buffer[(x, y)].bg, theme.bg, "value pane ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn settings_uses_explicit_language_without_changing_focus_identity() {
        let mut config = crate::config::Config::default();
        config.language = crate::Language::SimplifiedChinese;
        let mut settings = SettingsState::default();
        settings.category = 1;
        settings.pos[1] = 1;
        let focus_key = crate::settings::items_in(1)[1].key;
        let theme = Theme::ferra();
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

        terminal
            .draw(|frame| {
                render_settings(
                    frame,
                    ratatui::layout::Rect::new(0, 0, 100, 24),
                    &mut settings,
                    &config,
                    &theme,
                );
            })
            .unwrap();

        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        let compact = text
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        assert!(compact.contains("行为"));
        assert!(compact.contains("语言"));
        assert!(compact.contains("英语"));
        assert_eq!(focus_key, "language");
        assert_eq!(settings.pos[1], 1);
    }
}
