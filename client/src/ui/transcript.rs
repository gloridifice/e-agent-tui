use super::*;

/// First-seen-ordered per-file counts over full paths.
#[cfg(test)]
fn counted_files(files: &[String]) -> Vec<(String, usize)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for f in files {
        if !counts.contains_key(f) {
            order.push(f.clone());
        }
        *counts.entry(f.clone()).or_insert(0) += 1;
    }
    order
        .into_iter()
        .map(|f| {
            let n = counts[&f];
            let name = f
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(f.as_str())
                .to_string();
            (name, n)
        })
        .collect()
}

/// `foo.rs x2, bar.rs` — the `xN` suffix appears only for repeats.
#[cfg(test)]
fn file_list(files: &[(String, usize)]) -> String {
    files
        .iter()
        .map(|(f, n)| {
            if *n > 1 {
                format!("{f} x{n}")
            } else {
                f.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn activity_row_line(
    row: &ActivityRow,
    state: &AppState,
    color_override: Option<Color>,
) -> Line<'static> {
    let theme = state.theme();
    let target = match row.state {
        ActivityState::Waiting => theme.working_status.waiting.fg,
        ActivityState::Running => breathing_color(&theme, state.breath_phase()),
        ActivityState::Success => theme.working_status.success.fg,
        ActivityState::Failure => theme.working_status.failure.fg,
        ActivityState::Cancelled => theme.working_status.cancelled.fg,
    };
    let color = color_override.unwrap_or_else(|| {
        state
            .activity_transitions
            .get(&row.id)
            .map_or(target, |transition| {
                settle_color(transition.from, target, transition.done_since.elapsed())
            })
    });
    let mut spans = vec![
        Span::styled(
            " ".repeat(2 + usize::from(row.depth) * 2),
            theme.activity.detail.style(),
        ),
        Span::styled("•", Style::default().fg(color)),
        Span::styled(" ", theme.activity.detail.style()),
        Span::styled(row.label.clone(), theme.activity.label.style()),
    ];
    if !row.summary.is_empty() {
        spans.push(Span::styled(" ", theme.activity.detail.style()));
        spans.push(Span::styled(
            row.summary.clone(),
            theme.activity.detail.style(),
        ));
    }
    for continuation in &row.continuations {
        spans.push(Span::styled(
            continuation.separator.clone(),
            theme.activity.detail.style(),
        ));
        spans.push(Span::styled(
            continuation.label.clone(),
            theme.activity.label.style(),
        ));
        if !continuation.summary.is_empty() {
            spans.push(Span::styled(" ", theme.activity.detail.style()));
            spans.push(Span::styled(
                continuation.summary.clone(),
                theme.activity.detail.style(),
            ));
        }
    }
    if row.count > 1 {
        spans.push(Span::styled(
            format!(" x{}", row.count),
            theme.activity.metadata.style(),
        ));
    }
    if state.config.show_tool_duration {
        if let Some(duration_ms) = row.duration_ms {
            spans.push(Span::styled(
                format!(" · {:.1}s", duration_ms as f64 / 1000.0),
                theme.activity.metadata.style(),
            ));
        }
    }
    Line::from(spans)
}

fn transcript_block_lines(block: &TranscriptBlock, state: &AppState) -> Vec<Line<'static>> {
    let theme = state.theme();
    let color = match block.tone {
        DisplayTone::Normal => theme.surface.primary_text.fg,
        DisplayTone::Dim => theme.surface.muted_text.fg,
        DisplayTone::Info => theme.log.info.fg,
        DisplayTone::Warning => theme.log.warning.fg,
        DisplayTone::Error => theme.log.error.fg,
    };
    let prefix = if block.tone == DisplayTone::Error {
        "✗ "
    } else {
        ""
    };
    block
        .content
        .lines()
        .take(MAX_RENDER_LINES_PER_MSG)
        .map(|line| {
            Line::from(Span::styled(
                format!("{prefix}{line}"),
                Style::default().fg(color),
            ))
        })
        .collect()
}

/// Reasoning content is folded by default, bounded to the configured first-N
/// lines in `Lines` mode, and shown completely in `Full` mode. The breathing
/// `Thinking...` row remains visible in every mode.
fn reasoning_block_lines(block: &TranscriptBlock, state: &AppState) -> Vec<Line<'static>> {
    let limit = match state.config.thinking_display_mode() {
        ThinkingDisplayMode::Compact => return Vec::new(),
        ThinkingDisplayMode::Lines => state.config.thinking_lines.max(1),
        ThinkingDisplayMode::Full => usize::MAX,
    };
    let color = state.theme().surface.muted_text.fg;
    block
        .content
        .lines()
        .take(limit)
        .map(|line| Line::from(Span::styled(line.to_owned(), Style::default().fg(color))))
        .collect()
}

fn content_card_lines(
    card: &ContentCard,
    state: &AppState,
    area_width: usize,
) -> Vec<Line<'static>> {
    let theme = state.theme();
    let gutter = card.horizontal_padding.min(area_width);
    let avail = area_width.saturating_sub(gutter).max(1);
    let style = match card.role {
        CardRole::User => theme.card.user,
        CardRole::Context => theme.card.context,
        CardRole::Detail => theme.card.detail,
        CardRole::Attachment => theme.card.attachment,
    };
    let fg = style.fg;
    let bg = style.bg.unwrap_or(theme.bg_soft);
    let fill_row = || {
        Line::from(Span::styled(
            " ".repeat(area_width),
            Style::default().fg(fg).bg(bg),
        ))
    };
    let mut out = vec![fill_row()];
    if let Some(header) = &card.header {
        let mut row = Line::from(vec![
            Span::styled(" ".repeat(gutter), Style::default().fg(fg).bg(bg)),
            Span::styled(
                header.clone(),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            ),
        ]);
        let used = row.width();
        if used < area_width {
            row.push_span(Span::styled(
                " ".repeat(area_width - used),
                Style::default().fg(fg).bg(bg),
            ));
        }
        out.push(row);
    }
    for line in card.content.lines() {
        if line.is_empty() {
            out.push(fill_row());
            continue;
        }
        for chunk in wrap_text(line, avail) {
            let mut row = Line::from(vec![
                Span::styled(" ".repeat(gutter), Style::default().fg(fg).bg(bg)),
                Span::styled(chunk, Style::default().fg(fg).bg(bg)),
            ]);
            let used = row.width();
            if used < area_width {
                row.push_span(Span::styled(
                    " ".repeat(area_width - used),
                    Style::default().fg(fg).bg(bg),
                ));
            }
            out.push(row);
        }
    }
    out.push(fill_row());
    out
}

fn markdown_block_lines(
    block: &TranscriptBlock,
    state: &AppState,
    area_width: usize,
) -> Vec<Line<'static>> {
    let theme = state.theme();
    let Some(lines) = state.markdown_layout.lines(&block.id) else {
        return transcript_block_lines(block, state);
    };
    let mut rendered = lines
        .iter()
        .map(|render_line| {
            let mut line = render_line.line.clone();
            if render_line.fill {
                let fill_style = theme.markdown.code_background.style();
                line = line.patch_style(fill_style);
                let width = line.width();
                if width < area_width {
                    line.push_span(Span::styled(" ".repeat(area_width - width), fill_style));
                }
            }
            line
        })
        .collect::<Vec<_>>();
    if block.streaming {
        if let Some(last) = rendered.last_mut() {
            last.push_span(Span::raw(" "));
            last.push_span(Span::styled(
                "•",
                Style::default().fg(breathing_color(&theme, state.breath_phase())),
            ));
        }
    }
    rendered
}

fn display_item_lines(
    item: &DisplayItem,
    state: &AppState,
    area_width: usize,
) -> Vec<Line<'static>> {
    match item {
        DisplayItem::Activity(row) => vec![truncate_activity_line(
            activity_row_line(row, state, None),
            area_width,
        )],
        DisplayItem::Block(block) if block.format == TranscriptFormat::Reasoning => {
            reasoning_block_lines(block, state)
        }
        DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
            markdown_block_lines(block, state, area_width)
        }
        DisplayItem::Block(block) => transcript_block_lines(block, state),
        DisplayItem::Card(card) => content_card_lines(card, state, area_width),
        DisplayItem::Composite { activity, detail } => {
            let mut lines = vec![truncate_activity_line(
                activity_row_line(activity, state, None),
                area_width,
            )];
            lines.extend(content_card_lines(detail, state, area_width));
            lines
        }
    }
}

fn is_activity_item(item: &DisplayItem) -> bool {
    item.is_activity()
}

fn is_hidden_item(item: &DisplayItem, state: &AppState) -> bool {
    matches!(
        item,
        DisplayItem::Block(block)
            if block.format == TranscriptFormat::Reasoning
                && !state.config.thinking_display_mode().shows_reasoning()
    )
}

fn next_visible_item_is_activity(state: &AppState, index: usize) -> bool {
    state
        .transcript
        .nodes()
        .iter()
        .skip(index + 1)
        .find(|node| !is_hidden_item(&node.item, state))
        .is_some_and(|node| is_activity_item(&node.item))
}

#[cfg(test)]
pub(super) fn legacy_test_lines(msg: &Msg, state: &AppState) -> Vec<Line<'static>> {
    let theme = state.theme();
    match msg {
        // Compact reasoning output stays folded into the breathing
        // `• Thinking... xN` row; `Lines`/`Full` render the content here.
        Msg::Block(_) if is_hidden_msg(msg, state) => Vec::new(),
        Msg::Block(block) if block.format == TranscriptFormat::Reasoning => {
            reasoning_block_lines(block, state)
        }
        Msg::Block(block) => transcript_block_lines(block, state),
        // Cards are built width-aware in legacy_test_styled_lines.
        Msg::Card(_) => Vec::new(),
        Msg::Activity(row) => vec![activity_row_line(row, state, None)],
        // User blocks are built width-aware (wrapped with the gutter on
        // every row) in legacy_test_styled_lines.
        Msg::User { .. } => Vec::new(),
        Msg::Assistant { lines, .. } => lines.iter().map(|r| r.line.clone()).collect(),
        Msg::Streaming { text } => {
            let block = TranscriptBlock {
                id: DisplayId::correlated("assistant", "streaming"),
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: text.clone(),
                streaming: true,
            };
            let mut lines = transcript_block_lines(&block, state);
            // Breathing `•` on the tail while the model streams.
            if let Some(last) = lines.last_mut() {
                last.push_span(Span::styled(" ", Style::default().fg(theme.dim)));
                last.push_span(Span::styled(
                    "•",
                    Style::default().fg(breathing_color(&theme, state.breath_phase())),
                ));
            }
            lines
        }
        Msg::Tool(card) => {
            let (activity_state, summary, duration_ms, color) = match &card.state {
                ToolState::Running => (ActivityState::Running, card.summary.clone(), None, None),
                ToolState::Done {
                    ok,
                    lines,
                    lines_truncated,
                    duration_ms,
                } => {
                    let target = if *ok { theme.ok } else { theme.err };
                    let color = match (&card.done_since, &card.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, target, since.elapsed()),
                        _ => target,
                    };
                    let concise_create = card.name == "create";
                    (
                        if *ok {
                            ActivityState::Success
                        } else {
                            ActivityState::Failure
                        },
                        if concise_create {
                            card.summary.clone()
                        } else if *lines_truncated {
                            format!("{} · 末尾 {lines} 行", card.summary)
                        } else {
                            format!("{} · {lines} 行", card.summary)
                        },
                        (!concise_create).then_some(*duration_ms),
                        Some(color),
                    )
                }
            };
            let mut row = ActivityRow::root(
                DisplayId::correlated("tool", &card.call_id),
                card.name.clone(),
            );
            row.summary = summary;
            row.state = activity_state;
            row.start_ms = Some(card.start_ms);
            row.duration_ms = duration_ms;
            vec![activity_row_line(&row, state, color)]
        }
        Msg::Thinking(card) => {
            let (activity_state, color) = match card.state {
                crate::model::ThinkState::Running => (ActivityState::Running, None),
                crate::model::ThinkState::Done => {
                    let color = match (&card.done_since, &card.done_from) {
                        (Some(since), Some(from)) => settle_color(*from, theme.ok, since.elapsed()),
                        _ => theme.ok,
                    };
                    (ActivityState::Success, Some(color))
                }
            };
            let mut row =
                ActivityRow::root(DisplayId::correlated("thinking", "current"), "Thinking...");
            row.state = activity_state;
            row.count = card.count;
            vec![activity_row_line(&row, state, color)]
        }
        Msg::FileGroup(group) => {
            use crate::model::FileAction;

            let files_for = |action: FileAction, ok: Option<bool>| -> Vec<String> {
                group
                    .items
                    .iter()
                    .filter(|item| item.action == action && item.ok == ok)
                    .map(|item| item.file.clone())
                    .collect()
            };
            let group_id = group
                .items
                .first()
                .map(|item| item.call_id.as_str())
                .unwrap_or("group");
            let make_row =
                |suffix: &str, label: &str, summary: String, activity_state: ActivityState| {
                    let mut row = ActivityRow::root(
                        DisplayId::correlated("file-group", &format!("{group_id}:{suffix}")),
                        label,
                    );
                    row.summary = summary;
                    row.state = activity_state;
                    row
                };
            let combined_row = |suffix: &str,
                                actions: &[FileAction],
                                ok: Option<bool>,
                                activity_state: ActivityState,
                                trailing_semicolon: bool|
             -> Option<ActivityRow> {
                let mut parts = actions.iter().filter_map(|action| {
                    let files = files_for(*action, ok);
                    (!files.is_empty()).then(|| (action.label(), file_list(&counted_files(&files))))
                });
                let (label, summary) = parts.next()?;
                let mut row = make_row(suffix, label, summary, activity_state);
                for (label, summary) in parts {
                    row.continuations.push(ActivityContinuation {
                        separator: "; ".into(),
                        label: label.into(),
                        summary,
                    });
                }
                if trailing_semicolon {
                    if let Some(last) = row.continuations.last_mut() {
                        last.summary.push(';');
                    } else {
                        row.summary.push(';');
                    }
                }
                Some(row)
            };
            let push_failures = |actions: &[FileAction], out: &mut Vec<Line<'static>>| {
                for action in actions {
                    let files = files_for(*action, Some(false));
                    for (index, (name, count)) in counted_files(&files).into_iter().enumerate() {
                        let summary = if count > 1 {
                            format!("{name} x{count}")
                        } else {
                            name
                        };
                        let row = make_row(
                            &format!("{}-failed:{index}", action.label()),
                            action.label(),
                            summary,
                            ActivityState::Failure,
                        );
                        out.push(activity_row_line(&row, state, None));
                    }
                }
            };

            let read_actions = [FileAction::Read, FileAction::View];
            let write_actions = [FileAction::Edit, FileAction::Replace, FileAction::Insert];
            let pending_reads = group
                .items
                .iter()
                .any(|item| item.action.is_read_like() && item.ok.is_none());
            let pending_writes = group
                .items
                .iter()
                .any(|item| !item.action.is_read_like() && item.ok.is_none());
            let mut lines = Vec::new();
            if pending_reads {
                if let Some(row) = combined_row(
                    "read-running",
                    &read_actions,
                    None,
                    ActivityState::Running,
                    false,
                ) {
                    lines.push(activity_row_line(&row, state, None));
                }
                push_failures(&read_actions, &mut lines);
            } else if pending_writes {
                if let Some(row) = combined_row(
                    "read-done",
                    &read_actions,
                    Some(true),
                    ActivityState::Success,
                    true,
                ) {
                    lines.push(activity_row_line(&row, state, None));
                }
                push_failures(&read_actions, &mut lines);
                if let Some(row) = combined_row(
                    "write-running",
                    &write_actions,
                    None,
                    ActivityState::Running,
                    false,
                ) {
                    lines.push(activity_row_line(&row, state, None));
                }
                push_failures(&write_actions, &mut lines);
            } else {
                if let Some(row) = combined_row(
                    "done",
                    &FileAction::FOLD_ORDER,
                    Some(true),
                    ActivityState::Success,
                    false,
                ) {
                    let color = match (&group.done_since, &group.done_from) {
                        (Some(since), Some(from)) => {
                            Some(settle_color(*from, theme.ok, since.elapsed()))
                        }
                        _ => None,
                    };
                    lines.push(activity_row_line(&row, state, color));
                }
                push_failures(&FileAction::FOLD_ORDER, &mut lines);
            }
            lines
        }
        Msg::System { text } => transcript_block_lines(
            &TranscriptBlock {
                id: DisplayId::correlated("system", "legacy"),
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Dim,
                copy_source: text.clone(),
                streaming: false,
            },
            state,
        ),
        Msg::Error { text } => transcript_block_lines(
            &TranscriptBlock {
                id: DisplayId::correlated("error", "legacy"),
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Error,
                copy_source: text.clone(),
                streaming: false,
            },
            state,
        ),
    }
}

/// Tool cards, Thinking rows, and read/edit file groups are "activity"
/// rows: consecutive ones render glued together with no gap row between them.
#[cfg(test)]
fn is_activity_msg(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::Activity(_) | Msg::Tool(_) | Msg::FileGroup(_) | Msg::Thinking(_)
    )
}

/// Thinking output (reasoning blocks) is collapsed into the breathing
/// `• Thinking... xN` row: it contributes zero transcript rows and zero
/// inter-message gap, so the cache builder and copy provenance must skip it.
#[cfg(test)]
fn is_hidden_msg(msg: &Msg, state: &AppState) -> bool {
    matches!(
        msg,
        Msg::Block(block)
            if block.format == TranscriptFormat::Reasoning
                && !state.config.thinking_display_mode().shows_reasoning()
    )
}

/// Whether the next visible message is another activity row. Hidden messages
/// are transparent to layout adjacency, just as they are to rendering/copy.
#[cfg(test)]
fn next_visible_is_activity(msgs: &[Msg], index: usize, state: &AppState) -> bool {
    msgs.iter()
        .skip(index + 1)
        .find(|msg| !is_hidden_msg(msg, state))
        .is_some_and(is_activity_msg)
}

/// One message rendered to transcript lines, including the full-width soft
/// background of user blocks and of `fill`-flagged code/mermaid rows. Shared
/// by the full cache rebuild and the incremental tail splice so both produce
/// identical rows.
#[cfg(test)]
pub(super) fn legacy_test_styled_lines(
    msg: &Msg,
    state: &AppState,
    area_width: usize,
) -> Vec<Line<'static>> {
    let theme = state.theme();
    if let Msg::Assistant { lines, .. } = msg {
        // Assistant lines carry per-row fill flags (code/mermaid blocks);
        // those fill with the Night background (#2b292d, the bg slot).
        return lines
            .iter()
            .map(|r| {
                let mut line = r.line.clone();
                if r.fill {
                    let fill_style = theme.markdown.code_background.style();
                    line = line.patch_style(fill_style);
                    let width = line.width();
                    if width < area_width {
                        line.push_span(Span::styled(" ".repeat(area_width - width), fill_style));
                    }
                }
                line
            })
            .collect();
    }
    if let Msg::Card(card) = msg {
        return content_card_lines(card, state, area_width);
    }
    if let Msg::User { text } = msg {
        let card = ContentCard {
            id: DisplayId::correlated("user", "legacy"),
            unit: None,
            header: None,
            content: text.clone(),
            role: CardRole::User,
            tone: DisplayTone::Normal,
            horizontal_padding: state.config.user_input_padding,
            copy_source: text.clone(),
        };
        return content_card_lines(&card, state, area_width);
    }
    let lines = legacy_test_lines(msg, state);
    if is_activity_msg(msg) {
        lines
            .into_iter()
            .map(|line| truncate_activity_line(line, area_width))
            .collect()
    } else {
        lines
    }
}

#[cfg(test)]
fn legacy_copy_layout_rows(state: &AppState) -> Vec<CopyLayoutRow> {
    let mut rows = Vec::new();
    let mut global_row = 0usize;
    let width = state.transcript_cache.width.max(1);
    for (index, msg) in state.msgs.iter().enumerate() {
        if is_hidden_msg(msg, state) {
            continue;
        }
        let layout_lines = legacy_test_styled_lines(msg, state, width);
        if let Msg::Assistant { lines, .. } = msg {
            for (render_line, layout_line) in lines.iter().zip(layout_lines.iter()) {
                for wrapped in wrap_line(layout_line.clone(), width) {
                    rows.push(CopyLayoutRow {
                        unit: render_line.unit,
                        raw_line: render_line.raw_line,
                        atomic: render_line.atomic,
                        text: wrapped
                            .spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect(),
                        global_row,
                    });
                    global_row += 1;
                }
            }
        } else if let Some(unit) = match msg {
            Msg::Block(block) => block.unit,
            Msg::Card(card) => card.unit,
            _ => None,
        } {
            for (raw_line, line) in layout_lines.iter().enumerate() {
                for wrapped in wrap_line(line.clone(), width) {
                    rows.push(CopyLayoutRow {
                        unit,
                        raw_line: Some(raw_line),
                        atomic: false,
                        text: wrapped
                            .spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect(),
                        global_row,
                    });
                    global_row += 1;
                }
            }
        } else {
            global_row += layout_lines
                .iter()
                .map(|line| wrapped_rows(line, width))
                .sum::<usize>();
        }
        let next_is_activity = next_visible_is_activity(&state.msgs, index, state);
        if !(is_activity_msg(msg) && next_is_activity) {
            global_row += 1;
        }
    }
    rows
}

/// Copy/navigation provenance derived from the exact same message layout used
/// to build the transcript cache. The shared row contract and wrapping rules
/// live in `transcript_layout`; this renderer supplies message presentation.
pub fn copy_layout_rows(state: &AppState) -> Vec<CopyLayoutRow> {
    #[cfg(test)]
    if state.transcript.is_empty() && !state.msgs.is_empty() {
        return legacy_copy_layout_rows(state);
    }
    let mut rows = Vec::new();
    let mut global_row = 0usize;
    let width = state.transcript_cache.width.max(1);
    for (index, node) in state.transcript.nodes().iter().enumerate() {
        let item = &node.item;
        if is_hidden_item(item, state) {
            continue;
        }
        let layout_lines = display_item_lines(item, state, width);
        match item {
            DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
                if let Some(render_lines) = state.markdown_layout.lines(&block.id) {
                    for (render_line, layout_line) in render_lines.iter().zip(layout_lines.iter()) {
                        for wrapped in wrap_line(layout_line.clone(), width) {
                            rows.push(CopyLayoutRow {
                                unit: render_line.unit,
                                raw_line: render_line.raw_line,
                                atomic: render_line.atomic,
                                text: wrapped
                                    .spans
                                    .iter()
                                    .map(|span| span.content.as_ref())
                                    .collect(),
                                global_row,
                            });
                            global_row += 1;
                        }
                    }
                } else {
                    global_row += layout_lines
                        .iter()
                        .map(|line| wrapped_rows(line, width))
                        .sum::<usize>();
                }
            }
            DisplayItem::Block(block) => {
                append_unit_rows(&mut rows, &mut global_row, block.unit, &layout_lines, width);
            }
            DisplayItem::Card(card) => {
                append_unit_rows(&mut rows, &mut global_row, card.unit, &layout_lines, width);
            }
            DisplayItem::Composite { detail, .. } => {
                if let Some((activity, detail_lines)) = layout_lines.split_first() {
                    global_row += wrapped_rows(activity, width);
                    append_unit_rows(&mut rows, &mut global_row, detail.unit, detail_lines, width);
                }
            }
            DisplayItem::Activity(_) => {
                global_row += layout_lines
                    .iter()
                    .map(|line| wrapped_rows(line, width))
                    .sum::<usize>();
            }
        }
        let next_is_activity = next_visible_item_is_activity(state, index);
        if !(is_activity_item(item) && next_is_activity) {
            global_row += 1;
        }
    }
    rows
}

fn append_unit_rows(
    rows: &mut Vec<CopyLayoutRow>,
    global_row: &mut usize,
    unit: Option<u64>,
    layout_lines: &[Line<'static>],
    width: usize,
) {
    let Some(unit) = unit else {
        *global_row += layout_lines
            .iter()
            .map(|line| wrapped_rows(line, width))
            .sum::<usize>();
        return;
    };
    for (raw_line, line) in layout_lines.iter().enumerate() {
        for wrapped in wrap_line(line.clone(), width) {
            rows.push(CopyLayoutRow {
                unit,
                raw_line: Some(raw_line),
                atomic: false,
                text: wrapped
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect(),
                global_row: *global_row,
            });
            *global_row += 1;
        }
    }
}

#[cfg(test)]
fn sync_legacy_test_transcript(state: &mut AppState) {
    let owns_store = !state.transcript.is_empty()
        && state
            .transcript
            .nodes()
            .iter()
            .all(|node| node.id().0.starts_with("legacy-test:"));
    if state.msgs.is_empty() || (!state.transcript.is_empty() && !owns_store) {
        return;
    }
    let messages = state.msgs.clone();
    state.transcript.clear();
    for (index, message) in messages.into_iter().enumerate() {
        let id = DisplayId::correlated("legacy-test", &index.to_string());
        let item = match message {
            Msg::Block(mut block) => {
                block.id = id;
                DisplayItem::Block(block)
            }
            Msg::Card(mut card) => {
                card.id = id;
                DisplayItem::Card(card)
            }
            Msg::Activity(mut row) => {
                row.id = id;
                DisplayItem::Activity(row)
            }
            Msg::User { text } => DisplayItem::Card(ContentCard {
                id,
                unit: None,
                header: None,
                content: text.clone(),
                role: CardRole::User,
                tone: DisplayTone::Normal,
                horizontal_padding: state.config.user_input_padding,
                copy_source: text,
            }),
            Msg::Assistant { text, .. } => DisplayItem::Block(TranscriptBlock {
                id,
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: text,
                streaming: false,
            }),
            Msg::Streaming { text } => DisplayItem::Block(TranscriptBlock {
                id,
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: text,
                streaming: true,
            }),
            Msg::Tool(card) => {
                let mut row = ActivityRow::root(id, card.name);
                row.summary = card.summary;
                row.start_ms = Some(card.start_ms);
                match card.state {
                    ToolState::Running => row.state = ActivityState::Running,
                    ToolState::Done {
                        ok,
                        lines,
                        lines_truncated,
                        duration_ms,
                    } => {
                        row.state = if ok {
                            ActivityState::Success
                        } else {
                            ActivityState::Failure
                        };
                        if row.label != "create" {
                            row.summary.push_str(&if lines_truncated {
                                format!(" · 末尾 {lines} 行")
                            } else {
                                format!(" · {lines} 行")
                            });
                            row.duration_ms = Some(duration_ms);
                        }
                    }
                }
                DisplayItem::Activity(row)
            }
            Msg::Thinking(card) => {
                let mut row = ActivityRow::root(id, "Thinking...");
                row.state = if card.state == crate::model::ThinkState::Running {
                    ActivityState::Running
                } else {
                    ActivityState::Success
                };
                row.count = card.count;
                DisplayItem::Activity(row)
            }
            Msg::FileGroup(group) => {
                let mut items = group.items.into_iter();
                let mut row = if let Some(first) = items.next() {
                    let mut row = ActivityRow::root(id, first.action.label());
                    row.summary = first.file;
                    row.state = match first.ok {
                        None => ActivityState::Running,
                        Some(true) => ActivityState::Success,
                        Some(false) => ActivityState::Failure,
                    };
                    row
                } else {
                    ActivityRow::root(id, "files")
                };
                for item in items {
                    row.continuations.push(ActivityContinuation {
                        separator: "; ".into(),
                        label: item.action.label().into(),
                        summary: item.file,
                    });
                    if item.ok.is_none() {
                        row.state = ActivityState::Running;
                    } else if item.ok == Some(false) && row.state != ActivityState::Running {
                        row.state = ActivityState::Failure;
                    }
                }
                DisplayItem::Activity(row)
            }
            Msg::System { text } => DisplayItem::Block(TranscriptBlock {
                id,
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Dim,
                copy_source: text,
                streaming: false,
            }),
            Msg::Error { text } => DisplayItem::Block(TranscriptBlock {
                id,
                unit: None,
                content: text.clone(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Error,
                copy_source: text,
                streaming: false,
            }),
        };
        state.transcript.append(item, None);
    }
}

fn rebuild_transcript_cache(state: &mut AppState, width: usize) {
    let _zone = crate::tracy_zone!("transcript rebuild");
    let mut base = Vec::new();
    let mut ranges = vec![None; state.transcript.len()];
    let mut tail_len = 0usize;
    for (index, node) in state.transcript.nodes().iter().enumerate() {
        let item = &node.item;
        if is_hidden_item(item, state) {
            continue;
        }
        let start = base.len();
        let lines = display_item_lines(item, state, width);
        let line_count = lines.len();
        base.extend(lines);
        let next_is_activity = next_visible_item_is_activity(state, index);
        let gap = !(is_activity_item(item) && next_is_activity);
        ranges[index] = Some(MessageLineRange {
            start,
            end: start + line_count,
            owns_gap: gap,
        });
        tail_len = line_count + usize::from(gap);
        if gap {
            base.push(Line::default());
        }
    }
    let cache = &mut state.transcript_cache;
    cache.lines = base;
    cache.message_ranges = ranges;
    cache.tail_len = tail_len;
    cache.valid = true;
    cache.tail_dirty = false;
    cache.dirty_messages.clear();
    cache.structural_rebuilt();
}

fn refresh_transcript_cache(state: &mut AppState, width: usize) {
    #[cfg(test)]
    sync_legacy_test_transcript(state);
    if state.transcript_cache.width != width {
        state.transcript_cache.width = width;
        state.transcript_cache.invalidate();
    }
    crate::presentation::materialize_transcript(state);
    if !state.transcript_cache.valid {
        rebuild_transcript_cache(state, width);
        return;
    }

    if state.transcript_cache.tail_dirty {
        let keep = state
            .transcript_cache
            .lines
            .len()
            .saturating_sub(state.transcript_cache.tail_len);
        let last_index = state.transcript.len().checked_sub(1);
        let rendered = last_index
            .map(|index| display_item_lines(&state.transcript.nodes()[index].item, state, width));
        let cache = &mut state.transcript_cache;
        cache.lines.truncate(keep);
        if let (Some(index), Some(lines)) = (last_index, rendered) {
            let count = lines.len();
            cache.lines.extend(lines);
            cache.lines.push(Line::default());
            if cache.message_ranges.len() != state.transcript.len() {
                cache.message_ranges.resize(state.transcript.len(), None);
            }
            cache.message_ranges[index] = Some(MessageLineRange {
                start: keep,
                end: keep + count,
                owns_gap: true,
            });
            cache.dirty_messages.remove(&index);
        } else {
            cache.tail_len = 0;
        }
        cache.tail_dirty = false;
        let tail_row_counts = cache.lines[keep..]
            .iter()
            .map(|line| wrapped_rows(line, width))
            .collect::<Vec<_>>();
        cache.structural_tail_updated(keep, width, &tail_row_counts);
    }

    if !state.transcript_cache.dirty_messages.is_empty() {
        let indices = state
            .transcript_cache
            .dirty_messages
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let patches = indices
            .iter()
            .filter_map(|index| {
                state
                    .transcript
                    .nodes()
                    .get(*index)
                    .map(|node| (*index, display_item_lines(&node.item, state, width)))
            })
            .collect::<Vec<_>>();
        let mut fallback = false;
        let mut applied = 0usize;
        for (index, lines) in patches {
            let Some(range) = state
                .transcript_cache
                .message_ranges
                .get(index)
                .and_then(|range| *range)
            else {
                fallback = true;
                break;
            };
            if range.len() != lines.len() {
                fallback = true;
                break;
            }
            state.transcript_cache.lines[range.start..range.end].clone_from_slice(&lines);
            applied += 1;
        }
        if fallback {
            rebuild_transcript_cache(state, width);
        } else {
            state.transcript_cache.dirty_messages.clear();
            state.transcript_cache.patched(applied);
        }
    }
}

pub(super) fn render_transcript(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut AppState,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    overlay: Option<&CopyOverlay>,
) {
    let visible = area.height as usize;
    let width = area.width as usize;
    refresh_transcript_cache(state, width);
    state.transcript_cache.ensure_layout(width, wrapped_rows);
    // History prepend anchors are display-row totals, not unwrapped base lines.
    if let Some(anchor) = state.transcript_cache.prepend_anchor.take() {
        let delta = state
            .transcript_cache
            .layout
            .total_rows()
            .saturating_sub(anchor);
        scroll.offset = scroll.offset.saturating_add(delta);
    }
    let len = state.transcript_cache.layout.total_rows();
    let mut available = visible;
    let show_hint = !scroll.follow && scroll.offset == 0;
    if show_hint {
        available = available.saturating_sub(1).max(1);
    }
    let start = if scroll.follow {
        len.saturating_sub(available)
    } else {
        scroll.offset.min(len.saturating_sub(1))
    };
    scroll.offset = start;
    let end = start.saturating_add(available).min(len);
    let (mut base_index, _) = state.transcript_cache.layout.locate(start);
    let mut display: Vec<Line<'static>> = Vec::with_capacity(available);
    while base_index < state.transcript_cache.lines.len() && display.len() < available {
        let base_start = state.transcript_cache.layout.prefix[base_index];
        let wrapped = wrap_line(state.transcript_cache.lines[base_index].clone(), width);
        for (row_index, mut row) in wrapped.into_iter().enumerate() {
            let global_row = base_start + row_index;
            if global_row < start {
                continue;
            }
            if global_row >= end {
                break;
            }
            if let Some(overlay) = overlay {
                let mut style = Style::default();
                if overlay
                    .sel
                    .is_some_and(|(lo, hi)| global_row >= lo && global_row <= hi)
                {
                    style = style.bg(theme.selection);
                }
                if overlay.cursor_row == global_row {
                    style = style.fg(theme.bg).bg(theme.fg);
                }
                if style != Style::default() {
                    row = row.patch_style(style);
                }
            }
            // Only an explicit row-level background makes a solid row.
            // Span backgrounds (notably inline-code chips) must stay local;
            // treating any span bg as a row fill leaks that color through the
            // source separator and every trailing terminal cell.
            if let Some(bg) = row.style.bg {
                let used = row.width();
                if used < width {
                    row.push_span(Span::styled(
                        " ".repeat(width - used),
                        Style::default().fg(theme.fg).bg(bg),
                    ));
                }
            }
            display.push(row);
        }
        base_index += 1;
    }
    state
        .transcript_cache
        .record_materialized_rows(display.len());
    if help_visible {
        display.extend(help_overlay(theme));
    }
    // Lazy scroll-back hint at the top of the transcript (display-only).
    if show_hint {
        let hint = if state.history_loading {
            "（正在加载更早的消息…）"
        } else if state.history_exhausted {
            "（已到最早的消息）"
        } else {
            "（PageUp 加载更早的消息）"
        };
        display.insert(
            0,
            Line::from(Span::styled(hint, Style::default().fg(theme.dim))),
        );
    }
    let paragraph = Paragraph::new(Text::from(display)).style(Style::default().fg(theme.fg));
    frame.render_widget(paragraph, area);
}

/// Scroll the transcript by a bounded number of visible rows.
pub fn scroll_lines(
    scroll: &mut ScrollState,
    area_height: usize,
    lines_total: usize,
    up: bool,
    rows: usize,
) {
    let rows = rows.max(1);
    if up {
        scroll.follow = false;
        scroll.offset = scroll.offset.saturating_sub(rows);
    } else {
        let max = lines_total.saturating_sub(area_height);
        let next = scroll.offset.saturating_add(rows).min(max);
        scroll.offset = next;
        if next >= max {
            scroll.follow = true;
        }
    }
}

/// One visible transcript page (used by PgUp/PgDn).
pub fn scroll_page(scroll: &mut ScrollState, area_height: usize, lines_total: usize, up: bool) {
    scroll_lines(
        scroll,
        area_height,
        lines_total,
        up,
        area_height.saturating_sub(1),
    );
}

#[derive(Clone, Copy)]
pub(super) struct InputPageRegions {
    pub(super) header: ratatui::layout::Rect,
    pub(super) body: ratatui::layout::Rect,
    pub(super) footer: ratatui::layout::Rect,
}
