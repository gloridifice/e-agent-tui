use super::*;

pub(super) fn render_info_accessory(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    label: &str,
    value: &str,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {label}: "),
                Style::default().fg(theme.selection).bg(theme.bg),
            ),
            Span::styled(
                trim_to_width(
                    value,
                    area.width.saturating_sub((label.len() + 4) as u16) as usize,
                ),
                Style::default().fg(theme.dim).bg(theme.bg),
            ),
        ]))
        .style(Style::default().bg(theme.bg)),
        area,
    );
}

pub(super) fn render_todo(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    todos: &[(String, String)],
    theme: &Theme,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    if area.height == 0 {
        return;
    }
    let mut rows = vec![Line::from(Span::styled(
        "  Todo",
        Style::default()
            .fg(theme.selection)
            .bg(theme.bg)
            .add_modifier(Modifier::BOLD),
    ))];
    let visible = usize::from(area.height.saturating_sub(1));
    for (content, status) in todos.iter().take(visible) {
        let marker = match status.as_str() {
            "completed" => "✓",
            "in-progress" => "•",
            _ => "○",
        };
        rows.push(Line::from(Span::styled(
            format!(
                "  {marker} {}",
                trim_to_width(content, area.width.saturating_sub(5) as usize)
            ),
            Style::default().fg(theme.dim).bg(theme.bg),
        )));
    }
    frame.render_widget(Paragraph::new(rows), area);
}

/// Pending-prompt queue strip above the input bar: one row per prompt, Night
/// background with Bark text, `  * ` prefix; long prompts truncate to a
/// single row with `…`.
pub(super) fn render_queue(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    queue: &[String],
    visible: usize,
    theme: &Theme,
) {
    frame.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);
    let truncated = queue.len() > visible;
    let shown = visible.saturating_sub(usize::from(truncated));
    let width = (area.width as usize).saturating_sub(4);
    let mut rows: Vec<Line<'static>> = queue
        .iter()
        .take(shown)
        .map(|item| {
            Line::from(Span::styled(
                format!("  * {}", trim_to_width(item, width)),
                Style::default().fg(theme.dim).bg(theme.bg),
            ))
        })
        .collect();
    if truncated {
        rows.push(Line::from(Span::styled(
            format!("  * … 还有 {} 条", queue.len() - shown),
            Style::default().fg(theme.dim).bg(theme.bg),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(rows)), area);
}

/// Slash-command suggestion popup, anchored right above the input bar:
/// borderless soft-background panel with the highlighted row following the
/// selection. `Clear` wipes the transcript cells underneath first — without
/// it, text behind the short rows would show through the panel.
pub(super) fn render_suggest(
    frame: &mut Frame,
    suggest: &Suggestion,
    input_area: ratatui::layout::Rect,
    theme: &Theme,
) {
    // Plugin registries are unbounded. Keep the popup inside the viewport and
    // scroll its visible window around the selected row while retaining every
    // completion in `suggest.matches` for keyboard navigation.
    const MAX_VISIBLE_ROWS: usize = 12;
    let available_rows = input_area.y.saturating_sub(2).max(1) as usize;
    let visible_rows = suggest
        .matches
        .len()
        .min(MAX_VISIBLE_ROWS)
        .min(available_rows);
    let start = suggest
        .sel
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(suggest.matches.len().saturating_sub(visible_rows));
    let width = 46u16.min(input_area.width);
    let rect = ratatui::layout::Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(visible_rows as u16 + 2),
        width,
        height: visible_rows as u16 + 2,
    };
    frame.render_widget(ratatui::widgets::Clear, rect);
    let panel = theme.overlay.background.style();

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("❯ ", theme.overlay.accent.style()),
        Span::styled(
            match suggest.kind {
                SuggestionKind::Commands => "命令",
                SuggestionKind::Modes => "模式",
                SuggestionKind::Skills => "技能",
            },
            theme.overlay.muted.style(),
        ),
    ]));
    for (i, cmd) in suggest
        .matches
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
    {
        let selected = i == suggest.sel;
        let integrated = suggest.sources.get(i).copied() == Some(CommandSource::Integrated);
        let desc = suggest
            .descriptions
            .get(i)
            .map(String::as_str)
            .unwrap_or("");
        let row_style = if selected {
            theme.overlay.selection.style()
        } else {
            theme.overlay.text.style()
        };
        let marker_style = if selected {
            row_style
        } else {
            theme.overlay.border.style()
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "❯ " } else { "  " },
                theme.overlay.accent.style(),
            ),
            Span::styled(if integrated { "↳ " } else { "" }, marker_style),
            Span::styled(cmd.clone(), row_style),
            Span::styled(
                format!("  {desc}"),
                if selected {
                    row_style
                } else {
                    theme.overlay.muted.style()
                },
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "↑↓ 选择 · Enter 发送 · Esc 关闭 · ↳ 插件命令",
        theme.overlay.muted.style(),
    )));
    frame.render_widget(Paragraph::new(Text::from(lines)).style(panel), rect);
}

/// Pending approval card: fixed above the input bar (design §4.4).
pub(super) fn render_approval(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    approval: &crate::interaction::ApprovalCard,
    theme: &Theme,
) {
    let title = Line::from(vec![
        Span::styled(
            "⚠ 审批",
            Style::default()
                .fg(theme.running)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(theme.dim)),
        Span::styled(approval.tool_name.clone(), Style::default().fg(theme.fg)),
    ]);
    let mut rows = vec![title];
    if !approval.reason.is_empty() {
        rows.push(Line::from(Span::styled(
            approval.reason.chars().take(100).collect::<String>(),
            Style::default().fg(theme.dim),
        )));
    }
    rows.push(Line::from(vec![
        Span::styled("[Y] 允许", Style::default().fg(theme.ok)),
        Span::styled("   ", Style::default().fg(theme.dim)),
        Span::styled("[n] 拒绝", Style::default().fg(theme.err)),
        Span::styled("   [i] 详情", Style::default().fg(theme.dim)),
    ]));
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_style(Style::default().fg(theme.running))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Text::from(rows)), inner);
}
