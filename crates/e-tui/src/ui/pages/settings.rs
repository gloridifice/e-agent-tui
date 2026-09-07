use super::*;

pub(crate) fn render_settings(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    settings: &mut SettingsState,
    config: &crate::config::Config,
    theme: &Theme,
) {
    let regions = input_page_shell(frame, area, theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.input.hint.fg)),
            Span::styled("/settings", Style::default().fg(theme.fg)),
        ])),
        regions.header,
    );

    let body = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(regions.body);
    render_categories(frame, body[0], settings, config, theme);
    render_settings_grid(frame, body[1], settings, config, theme);

    let hint = page_key_hints(config, settings.key_scope());
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(theme.dim)),
        regions.footer,
    );
}

fn render_categories(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    settings: &SettingsState,
    config: &crate::config::Config,
    theme: &Theme,
) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (index, category) in crate::settings::CATEGORIES.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(
            crate::i18n::tr(config.language, category),
            input_page_item_style(theme, false, index == settings.category),
        ));
    }
    let line = Line::from(spans);
    let width = (line.width() as u16).min(area.width);
    let x = area.x + area.width.saturating_sub(width) / 2;
    frame.buffer_mut().set_line(x, area.y, &line, width);
}

#[derive(Clone, Copy)]
enum GridRule {
    Top,
    Middle,
    Bottom,
}

enum GridRow {
    Rule(GridRule),
    Content(Line<'static>, Line<'static>),
}

fn render_settings_grid(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    settings: &mut SettingsState,
    config: &crate::config::Config,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let columns = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(area);
    let left_width = columns[0].width as usize;
    let items = crate::settings::items_in(settings.category);
    let focus_index = items
        .get(settings.pos[settings.category])
        .filter(|item| item.kind != crate::settings::ItemKind::ReadOnly)
        .map(|_| settings.pos[settings.category]);

    let mut rows = vec![GridRow::Rule(GridRule::Top)];
    let mut anchors = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let focused = focus_index == Some(index);
        let first = rows.len().saturating_sub(1);
        let mut name = Line::from(Span::styled(
            crate::i18n::tr(config.language, item.label),
            input_page_item_style(theme, focused && settings.editing.is_none(), false),
        ));
        let used = name.width();
        if used < left_width {
            name.push_span(Span::raw(" ".repeat(left_width - used)));
        }
        rows.push(GridRow::Content(
            name,
            settings_value_line(item, config, theme, settings, focused),
        ));

        let description = wrap_text(
            &crate::i18n::tr(config.language, item.desc),
            left_width.max(1),
        );
        let height = description.len().saturating_add(1);
        for chunk in description {
            rows.push(GridRow::Content(
                Line::from(Span::styled(chunk, Style::default().fg(theme.dim))),
                Line::default(),
            ));
        }
        let rule = if index + 1 == items.len() {
            GridRule::Bottom
        } else {
            GridRule::Middle
        };
        rows.push(GridRow::Rule(rule));
        anchors.push((index, first, height.saturating_add(2)));
    }

    let visible = area.height as usize;
    if let Some((_, first, height)) = anchors
        .iter()
        .find(|(index, _, _)| *index == settings.pos[settings.category])
    {
        if settings.scroll > *first {
            settings.scroll = *first;
        }
        if first + height > settings.scroll + visible {
            settings.scroll = (first + height).saturating_sub(visible);
        }
    }
    settings.scroll = settings.scroll.min(rows.len().saturating_sub(1));

    for (row_offset, row) in rows.iter().skip(settings.scroll).take(visible).enumerate() {
        let y = area.y.saturating_add(row_offset as u16);
        match row {
            GridRow::Rule(kind) => render_grid_rule(frame, area, columns[1].x, y, *kind, theme),
            GridRow::Content(left, right) => {
                let buffer = frame.buffer_mut();
                buffer.set_line(columns[0].x, y, left, columns[0].width);
                if let Some(cell) = buffer.cell_mut(Position::new(columns[1].x, y)) {
                    cell.set_symbol("│").set_fg(theme.diff.separator.fg);
                }
                buffer.set_line(columns[2].x, y, right, columns[2].width);
            }
        }
    }
}

fn render_grid_rule(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    divider_x: u16,
    y: u16,
    kind: GridRule,
    theme: &Theme,
) {
    let junction = match kind {
        GridRule::Top => "┬",
        GridRule::Middle => "┼",
        GridRule::Bottom => "┴",
    };
    let buffer = frame.buffer_mut();
    for x in area.x..area.right() {
        if let Some(cell) = buffer.cell_mut(Position::new(x, y)) {
            cell.set_symbol(if x == divider_x { junction } else { "─" })
                .set_fg(theme.diff.separator.fg);
        }
    }
}

fn settings_value_line(
    item: &crate::settings::ItemDef,
    config: &crate::config::Config,
    theme: &Theme,
    settings: &SettingsState,
    focused_row: bool,
) -> Line<'static> {
    let editing_input =
        focused_row && matches!(settings.editing, Some(crate::settings::Edit::Input { .. }));
    let editing_choice = match settings.editing {
        Some(crate::settings::Edit::Choice { cursor }) if focused_row => Some(cursor),
        _ => None,
    };
    match item.kind {
        crate::settings::ItemKind::Choice { options } => {
            let current = (item.get)(config);
            let selected = options.iter().position(|(value, _)| *value == current);
            let mut spans = Vec::new();
            for (index, (_, label_key)) in options.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("  "));
                }
                let is_selected = Some(index) == selected;
                let focused = Some(index) == editing_choice;
                let style = input_page_item_style(theme, focused, is_selected);
                spans.push(Span::styled(
                    format!(
                        "{} {}",
                        if is_selected { "●" } else { "○" },
                        crate::i18n::tr(config.language, label_key)
                    ),
                    style,
                ));
            }
            Line::from(spans)
        }
        crate::settings::ItemKind::ModeChoice | crate::settings::ItemKind::ThemeChoice => {
            let options =
                crate::settings::dynamic_options(item, config, &settings.modes, &settings.themes);
            let current = (item.get)(config);
            let mut spans = Vec::new();
            for (index, option) in options.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("  "));
                }
                let is_selected = option == &current;
                let focused = Some(index) == editing_choice;
                spans.push(Span::styled(
                    format!("{} {option}", if is_selected { "●" } else { "○" }),
                    input_page_item_style(theme, focused, is_selected),
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
            Line::from(Span::styled(
                text,
                input_page_item_style(theme, editing_input, false),
            ))
        }
        crate::settings::ItemKind::ReadOnly => Line::from(Span::styled(
            (item.get)(config),
            Style::default().fg(theme.dim),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

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
