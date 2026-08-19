use super::*;

pub(crate) fn render_settings(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    settings: &mut SettingsState,
    config: &crate::config::Config,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);

    // ---- category tabs: actionable members of the single focus graph ----
    {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, cat) in crate::settings::CATEGORIES.iter().enumerate() {
            let fg = if i == settings.category {
                theme.ok
            } else {
                theme.fg
            };
            let bg = if settings.focus_tabs && i == settings.tab_cursor {
                theme.bg
            } else {
                theme.bg_soft
            };
            spans.push(Span::styled(
                format!(" {cat} "),
                Style::default().fg(fg).bg(bg),
            ));
            spans.push(Span::styled(
                "  ",
                Style::default().fg(theme.fg).bg(theme.bg_soft),
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
    let items = crate::settings::items_in(settings.category);
    let focus_idx = (!settings.focus_tabs).then_some(settings.pos[settings.category]);
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
            (*item).label,
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
        for chunk in wrap_text(item.desc, left_width.max(1)) {
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
        "Enter 确认   Esc 取消修改"
    } else {
        "hjkl/方向键移动   Enter 执行   Esc 退出 · 即改即存"
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
/// cell sits on Ash; while editing, the focused element (cursor option or
/// input buffer) turns Night.
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
        theme.bg
    } else {
        theme.bg_soft
    };
    let line: Line<'static> = match item.kind {
        crate::settings::ItemKind::Choice { options } => {
            let current = (item.get)(config);
            let selected = options
                .iter()
                .position(|o| (*o).starts_with(current.as_str()));
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, opt) in options.iter().enumerate() {
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
                    theme.bg
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
                    theme.bg
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
                // Fill the row so the focused input cell reads as a Night block.
                let used = input_line.width();
                if used < width as usize {
                    input_line.push_span(Span::styled(
                        " ".repeat(width as usize - used),
                        Style::default().fg(theme.fg).bg(theme.bg),
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
