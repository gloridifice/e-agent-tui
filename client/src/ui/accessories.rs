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
    approval: &crate::model::ApprovalCard,
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

/// Pending user-question panel: fixed above the input bar (design §4.4).
/// Shows the current question, and the description of the highlighted option.
pub(super) fn render_question(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    batch: &crate::model::QuestionBatch,
    theme: &Theme,
) {
    let current = &batch.questions[batch.current];
    let header = current
        .header
        .as_deref()
        .filter(|h| !h.is_empty())
        .unwrap_or("问题");
    let title = Line::from(vec![
        Span::styled(
            "❓ ",
            Style::default().fg(theme.user).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            header,
            Style::default().fg(theme.user).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ({}/{})", batch.current + 1, batch.questions.len()),
            Style::default().fg(theme.dim),
        ),
    ]);
    let mut rows = vec![title];
    rows.push(Line::from(Span::styled(
        current.question.clone(),
        Style::default().fg(theme.fg),
    )));
    let detail = if batch.is_free_text() {
        "无预设选项 · 直接输入文本，Enter 提交".to_string()
    } else {
        batch
            .current_options()
            .get(batch.sel)
            .and_then(|o| o.description.clone())
            .unwrap_or_else(|| "←→ 切换选项 · Enter 选中".to_string())
    };
    rows.push(Line::from(Span::styled(
        detail,
        Style::default().fg(theme.dim),
    )));
    // Borderless soft-background panel, matching the input bar's look.
    let panel = Block::default().style(Style::default().bg(theme.bg_soft));
    frame.render_widget(panel, area);
    frame.render_widget(Paragraph::new(Text::from(rows)), area);
}

/// Selection bar replacing the input bar while a question pends: `◄ A · B · C ►`
/// with the highlighted option reversed, or a `❯` draft line for free-text
/// questions. The right side carries the key hints.
pub(super) fn render_question_bar(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    batch: &crate::model::QuestionBatch,
    theme: &Theme,
    padding: u16,
) -> Option<Position> {
    let block = Block::default()
        .style(Style::default().bg(theme.bg_soft))
        .padding(Padding::new(padding, padding, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let hint = format!(
        "{}/{} Enter {} · Esc 取消",
        batch.current + 1,
        batch.questions.len(),
        if batch.current + 1 < batch.questions.len() {
            "下一项"
        } else {
            "确定"
        }
    );
    let options = batch.current_options();
    let left: Line<'static>;
    if options.is_empty() {
        left = Line::from(vec![
            Span::styled("❯ ", Style::default().fg(theme.user)),
            Span::styled(batch.draft.clone(), Style::default().fg(theme.fg)),
            // Software cursor: the hardware cursor stays hidden during all
            // terminal diff writes to avoid jumping through animated rows.
            Span::styled(" ", Style::default().fg(theme.bg).bg(theme.fg)),
        ]);
    } else {
        let mut spans = Vec::new();
        spans.push(Span::styled("◄ ", Style::default().fg(theme.dim)));
        for (i, option) in options.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            }
            if i == batch.sel {
                spans.push(Span::styled(
                    option.label.clone(),
                    Style::default().fg(theme.bg).bg(theme.fg),
                ));
            } else {
                spans.push(Span::styled(
                    option.label.clone(),
                    Style::default().fg(theme.fg),
                ));
            }
        }
        spans.push(Span::styled(" ►", Style::default().fg(theme.dim)));
        left = Line::from(spans);
    }
    // Render through the buffer directly: no wrapping, hard clip at edges.
    let buffer = frame.buffer_mut();
    buffer.set_line(inner.x, inner.y, &left, inner.width);
    let hint_line = Line::from(Span::styled(hint, Style::default().fg(theme.dim)))
        .alignment(ratatui::layout::Alignment::Right);
    // +4 slack: Line::width() under-counts CJK by a couple of cells vs the
    // buffer writer (same allowance as the status bar).
    let hint_width = (hint_line.width() + 4).min(inner.width as usize) as u16;
    let hint_x = inner.x + inner.width.saturating_sub(hint_width);
    buffer.set_line(hint_x, inner.y, &hint_line, hint_width);
    if options.is_empty() {
        // Keep the hidden terminal cursor anchored to the draft so IME
        // candidate windows still open beside the software cursor.
        let col = unicode_width::UnicodeWidthStr::width(batch.draft.as_str()) as u16;
        Some(Position::new(inner.x + 2 + col, inner.y))
    } else {
        None
    }
}
