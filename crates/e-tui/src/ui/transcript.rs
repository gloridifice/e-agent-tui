use super::*;
use crate::ui::component::{card, text, working};

/// Prompt-injection events render as plain text (no card shell): the
/// `提示词注入` label in the activity label tone (umber in the ferra theme)
/// followed by the injected content in the activity detail tone (bark),
/// capped at this many width-aware rows with a trailing ellipsis when
/// overflowing. The card retains its full raw source for copying.
const MAX_INJECTION_DISPLAY_LINES: usize = 2;

/// First-seen-ordered per-file counts over full paths.
/// `foo.rs x2, bar.rs` — the `xN` suffix appears only for repeats.
fn activity_row_parts(
    row: &ActivityRow,
    state: &TuiApp,
    color_override: Option<Color>,
) -> (Line<'static>, Option<Span<'static>>) {
    let theme = state.theme();
    let color = working::activity_color(&theme, state, row, color_override);
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

    let mut metadata = String::new();
    if let Some(lines) = row.output_lines {
        let noun = if lines == 1 { "line" } else { "lines" };
        if row.output_lines_truncated {
            metadata.push_str(&format!(" · {lines}+ {noun}"));
        } else {
            metadata.push_str(&format!(" · {lines} {noun}"));
        }
    }
    if state.config.show_tool_duration {
        let duration_ms = row.duration_ms.or_else(|| {
            row.live_duration_since
                .map(|started| started.elapsed().as_millis() as u64)
        });
        if let Some(duration_ms) = duration_ms {
            metadata.push_str(&format!(" · {:.1}s", duration_ms as f64 / 1000.0));
        }
    }
    let metadata =
        (!metadata.is_empty()).then(|| Span::styled(metadata, theme.activity.metadata.style()));
    (Line::from(spans), metadata)
}

/// Fit a one-row activity by truncating its command/summary first. Tool line
/// count and elapsed time are a stable trailing status and remain visible.
fn fitted_activity_row_line(
    row: &ActivityRow,
    state: &TuiApp,
    color_override: Option<Color>,
    width: usize,
) -> Line<'static> {
    let (prefix, metadata) = activity_row_parts(row, state, color_override);
    let Some(metadata) = metadata else {
        return truncate_activity_line(prefix, width);
    };
    let metadata_width = metadata.content.width();
    if prefix.width() + metadata_width <= width {
        let mut line = prefix;
        line.push_span(metadata);
        return line;
    }
    if metadata_width >= width {
        return truncate_activity_line(Line::from(metadata), width);
    }
    let mut line = truncate_activity_line(prefix, width - metadata_width);
    line.push_span(metadata);
    line
}

fn transcript_block_lines(block: &TranscriptBlock, state: &TuiApp) -> Vec<Line<'static>> {
    let theme = state.theme();
    let color = text::tone_color(&theme, block.tone);
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

/// Reasoning content is folded (zero rows) in `Compact`, bounded to the
/// configured first-N DISPLAY rows (after width-aware wrapping) in `Lines`,
/// and shown completely in `Full`.
fn reasoning_block_lines(
    block: &TranscriptBlock,
    state: &TuiApp,
    area_width: usize,
) -> Vec<Line<'static>> {
    reasoning_content_lines(&block.content, state, area_width)
}

/// Render accumulated reasoning text according to the display mode, with the
/// muted (Bark) tone used by the main transcript.
fn reasoning_content_lines(content: &str, state: &TuiApp, area_width: usize) -> Vec<Line<'static>> {
    let limit = match state.config.thinking_display_mode() {
        ThinkingDisplayMode::Compact => return Vec::new(),
        ThinkingDisplayMode::Lines => state.config.thinking_lines.max(1),
        ThinkingDisplayMode::Full => usize::MAX,
    };
    let style = Style::default().fg(state.theme().surface.muted_text.fg);
    let mut out = Vec::new();
    for source_line in content.lines() {
        for wrapped in wrap_line(Line::from(source_line.to_owned()), area_width.max(1)) {
            out.push(wrapped.patch_style(style));
            if out.len() == limit {
                return out;
            }
        }
    }
    out
}

/// Render the merged Thinking node: the breathing `• Thinking... xN`
/// indicator row in `compact` (or while no reasoning has streamed in yet);
/// the accumulated reasoning content in `lines`/`full`.
fn thinking_node_lines(
    node: &ThinkingNode,
    state: &TuiApp,
    area_width: usize,
) -> Vec<Line<'static>> {
    if state.config.thinking_display_mode().shows_reasoning() && !node.content.is_empty() {
        reasoning_content_lines(&node.content, state, area_width)
    } else {
        vec![fitted_activity_row_line(&node.row, state, None, area_width)]
    }
}

fn content_card_lines(card: &ContentCard, state: &TuiApp, area_width: usize) -> Vec<Line<'static>> {
    if card.role == CardRole::Context {
        return context_injection_lines(card, state, area_width);
    }
    let theme = state.theme();
    let gutter = card.horizontal_padding.min(area_width);
    let avail = area_width.saturating_sub(gutter).max(1);
    let style = card::shell_style(&theme, card.role);
    let fg = style.fg;
    let bg = style.bg.unwrap_or(theme.bg_soft);
    let fill_row = || {
        Line::from(Span::styled(
            " ".repeat(area_width),
            Style::default().fg(fg).bg(bg),
        ))
    };
    let content_row = |content: String| {
        let mut row = Line::from(vec![
            Span::styled(" ".repeat(gutter), Style::default().fg(fg).bg(bg)),
            Span::styled(content, Style::default().fg(fg).bg(bg)),
        ]);
        let used = row.width();
        if used < area_width {
            row.push_span(Span::styled(
                " ".repeat(area_width - used),
                Style::default().fg(fg).bg(bg),
            ));
        }
        row
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
        let chunks = if line.is_empty() {
            vec![String::new()]
        } else {
            wrap_text(line, avail)
        };
        for chunk in chunks {
            out.push(content_row(chunk));
        }
    }
    out.push(fill_row());
    out
}

/// Prompt-injection events render as plain text instead of a card shell: a
/// `提示词注入` label in the activity label tone (umber in the ferra theme)
/// followed by the injected content in the activity detail tone (bark),
/// capped at `MAX_INJECTION_DISPLAY_LINES` wrapped rows with a trailing `…`
/// marker when the content overflows. The card's `copy_source` keeps the full
/// original text.
fn context_injection_lines(
    card: &ContentCard,
    state: &TuiApp,
    area_width: usize,
) -> Vec<Line<'static>> {
    const LABEL: &str = "提示词注入 ";
    let theme = state.theme();
    let label_style = theme.activity.label.style();
    let content_style = theme.activity.detail.style();
    let avail = area_width.max(1);
    let first_avail = avail.saturating_sub(UnicodeWidthStr::width(LABEL)).max(1);

    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut truncated = false;
    let mut first = true;
    'content: for source_line in card.content.lines() {
        if source_line.is_empty() {
            continue;
        }
        let mut text = source_line.to_owned();
        loop {
            if rows.len() == MAX_INJECTION_DISPLAY_LINES {
                truncated = true;
                break 'content;
            }
            // The first row shares its columns with the label; later rows use
            // the full width.
            let budget = if first { first_avail } else { avail };
            let mut chunks = wrap_text(&text, budget);
            let head = chunks.remove(0);
            if first {
                rows.push(Line::from(vec![
                    Span::styled(LABEL.to_owned(), label_style),
                    Span::styled(head.clone(), content_style),
                ]));
                first = false;
            } else {
                rows.push(Line::from(Span::styled(head.clone(), content_style)));
            }
            if chunks.is_empty() {
                break;
            }
            text = chunks.join("");
        }
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(LABEL.to_owned(), label_style)));
    }
    if truncated {
        // End the final permitted row with an explicit ellipsis marker.
        if let Some(last) = rows.last_mut() {
            let text: String = last
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect();
            let kept = trim_text_to_width(&text, avail.saturating_sub(1));
            *last = Line::from(Span::styled(format!("{kept}…"), content_style));
        }
    }
    rows
}

/// Trim `text` to at most `width` display columns without adding a marker.
fn trim_text_to_width(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + w > width {
            break;
        }
        used += w;
        out.push(ch);
    }
    out
}

fn markdown_block_lines(
    block: &TranscriptBlock,
    state: &TuiApp,
    area_width: usize,
) -> Vec<Line<'static>> {
    let theme = state.theme();
    let Some(lines) = state.render.markdown_layout.lines(&block.id) else {
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

fn display_item_lines(item: &DisplayItem, state: &TuiApp, area_width: usize) -> Vec<Line<'static>> {
    match item {
        DisplayItem::Activity(row) => {
            vec![fitted_activity_row_line(row, state, None, area_width)]
        }
        DisplayItem::Block(block) if block.format == TranscriptFormat::Reasoning => {
            reasoning_block_lines(block, state, area_width)
        }
        DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
            markdown_block_lines(block, state, area_width)
        }
        DisplayItem::Block(block) => transcript_block_lines(block, state),
        DisplayItem::Card(card) => content_card_lines(card, state, area_width),
        DisplayItem::Thinking(node) => thinking_node_lines(node, state, area_width),
        DisplayItem::Composite { activity, detail } => {
            let mut lines = vec![fitted_activity_row_line(activity, state, None, area_width)];
            lines.extend(content_card_lines(detail, state, area_width));
            lines
        }
    }
}

fn is_activity_item(item: &DisplayItem) -> bool {
    item.is_activity()
}

fn is_hidden_item(item: &DisplayItem, state: &TuiApp) -> bool {
    matches!(
        item,
        DisplayItem::Block(block)
            if block.format == TranscriptFormat::Reasoning
                && !state.config.thinking_display_mode().shows_reasoning()
    )
}

fn is_hidden_node(nodes: &[TranscriptNode], index: usize, state: &TuiApp) -> bool {
    is_hidden_item(&nodes[index].item, state)
}

fn next_visible_item_is_activity(state: &TuiApp, index: usize) -> bool {
    let nodes = state.transcript.nodes();
    nodes
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(position, _)| !is_hidden_node(nodes, *position, state))
        .is_some_and(|(_, node)| is_activity_item(&node.item))
}

/// Tool cards, Thinking nodes, and read/edit file groups are "activity"
/// rows: consecutive ones render glued together with no gap row between
/// them. Whether the next visible message is another activity row. Hidden
/// messages are transparent to layout adjacency, just as they are to
/// rendering/copy.
/// One message rendered to transcript lines, including the full-width soft
/// background of user blocks and of `fill`-flagged code/mermaid rows. Shared
/// by the full cache rebuild and the incremental tail splice so both produce
/// identical rows.
/// Copy/navigation provenance derived from the exact same message layout used
/// to build the transcript cache. The shared row contract and wrapping rules
/// live in `transcript_layout`; this renderer supplies message presentation.
pub fn provenance_layout_rows(state: &TuiApp) -> Vec<ProvenanceLayoutRow> {
    if state.session.new_conversation.is_some() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut global_row = 0usize;
    let width = state.render.transcript_cache.width.max(1);
    let nodes = state.transcript.nodes();
    for (index, node) in nodes.iter().enumerate() {
        let item = &node.item;
        if is_hidden_node(nodes, index, state) {
            continue;
        }
        let layout_lines = display_item_lines(item, state, width);
        match item {
            DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
                if let Some(render_lines) = state.render.markdown_layout.lines(&block.id) {
                    for (render_line, layout_line) in render_lines.iter().zip(layout_lines.iter()) {
                        for wrapped in wrap_line(layout_line.clone(), width) {
                            rows.push(ProvenanceLayoutRow {
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
            DisplayItem::Thinking(node) => {
                append_unit_rows(&mut rows, &mut global_row, node.unit, &layout_lines, width);
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
    rows: &mut Vec<ProvenanceLayoutRow>,
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
            rows.push(ProvenanceLayoutRow {
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

fn rebuild_transcript_cache(state: &mut TuiApp, width: usize) {
    let _zone = crate::tracy_zone!("transcript rebuild");
    let mut base = Vec::new();
    let mut ranges = vec![None; state.transcript.len()];
    let mut tail_len = 0usize;
    let nodes = state.transcript.nodes();
    for (index, node) in nodes.iter().enumerate() {
        let item = &node.item;
        if is_hidden_node(nodes, index, state) {
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
    let cache = &mut state.render.transcript_cache;
    cache.lines = base;
    cache.message_ranges = ranges;
    cache.tail_len = tail_len;
    cache.valid = true;
    cache.tail_dirty = false;
    cache.dirty_messages.clear();
    cache.structural_rebuilt();
}

fn refresh_transcript_cache(state: &mut TuiApp, width: usize) {
    if state.render.transcript_cache.width != width {
        state.render.transcript_cache.width = width;
        state.render.transcript_cache.invalidate();
    }
    crate::presentation::materialize_transcript(state);
    if !state.render.transcript_cache.valid {
        rebuild_transcript_cache(state, width);
        return;
    }

    if state.render.transcript_cache.tail_dirty {
        let keep = state
            .render
            .transcript_cache
            .lines
            .len()
            .saturating_sub(state.render.transcript_cache.tail_len);
        let last_index = state.transcript.len().checked_sub(1);
        let rendered = last_index
            .map(|index| display_item_lines(&state.transcript.nodes()[index].item, state, width));
        let transcript_len = state.transcript.len();
        let cache = &mut state.render.transcript_cache;
        cache.lines.truncate(keep);
        if let (Some(index), Some(lines)) = (last_index, rendered) {
            let count = lines.len();
            cache.lines.extend(lines);
            cache.lines.push(Line::default());
            if cache.message_ranges.len() != transcript_len {
                cache.message_ranges.resize(transcript_len, None);
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

    if !state.render.transcript_cache.dirty_messages.is_empty() {
        let indices = state
            .render
            .transcript_cache
            .dirty_messages
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let patches = indices
            .iter()
            .filter_map(|index| {
                let nodes = state.transcript.nodes();
                if *index >= nodes.len() || is_hidden_node(nodes, *index, state) {
                    return None;
                }
                nodes
                    .get(*index)
                    .map(|node| (*index, display_item_lines(&node.item, state, width)))
            })
            .collect::<Vec<_>>();
        let mut fallback = false;
        let mut applied = 0usize;
        for (index, lines) in patches {
            let Some(range) = state
                .render
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
            state.render.transcript_cache.lines[range.start..range.end].clone_from_slice(&lines);
            applied += 1;
        }
        if fallback {
            rebuild_transcript_cache(state, width);
        } else {
            state.render.transcript_cache.dirty_messages.clear();
            state.render.transcript_cache.patched(applied);
        }
    }
}

pub(super) fn render_transcript(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
) {
    let _ = render_transcript_impl(frame, area, state, scroll, theme, help_visible, 0);
}

/// Render transcript with the bottom stack (accessories + input + status +
/// title) participating in the scroll. `area` is the full content viewport;
/// `bottom_rows` is the total height of the stack that follows the transcript.
/// Returns the y offset (within `area`) where that stack begins, so the caller
/// can draw it at the content-bottom position.
pub(super) fn render_transcript_combined(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    bottom_rows: usize,
) -> usize {
    render_transcript_impl(frame, area, state, scroll, theme, help_visible, bottom_rows)
}

fn render_transcript_impl(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    bottom_rows: usize,
) -> usize {
    let screen_height = area.height as usize;
    let width = area.width as usize;
    if let Some(draft) = state.session.new_conversation.as_ref() {
        let bottom_rows = bottom_rows.min(screen_height);
        let bottom_y = if bottom_rows == 0 {
            screen_height
        } else {
            screen_height.saturating_sub(bottom_rows).max(1)
        };
        let mut display = if help_visible {
            help_overlay(theme)
        } else {
            draft
                .notice
                .as_ref()
                .map(|notice| {
                    vec![Line::from(Span::styled(
                        notice.clone(),
                        theme.log.warning.style(),
                    ))]
                })
                .unwrap_or_default()
        };
        display.truncate(bottom_y);
        frame.render_widget(
            Paragraph::new(Text::from(display)).style(Style::default().fg(theme.fg)),
            area,
        );
        return bottom_y;
    }
    refresh_transcript_cache(state, width);
    state
        .render
        .transcript_cache
        .ensure_layout(width, wrapped_rows);
    state.rebuild_reading_model();
    // History prepend anchors are display-row totals, not unwrapped base lines.
    if let Some(anchor) = state.render.transcript_cache.prepend_anchor.take() {
        let delta = state
            .render
            .transcript_cache
            .layout
            .total_rows()
            .saturating_sub(anchor);
        scroll.offset = scroll.offset.saturating_add(delta);
    }
    let len = state.render.transcript_cache.layout.total_rows();
    let bottom_rows = bottom_rows.min(screen_height);
    let follow = scroll.follow;
    let show_hint = !follow && scroll.offset == 0;
    // Where the bottom stack starts on screen. When following, it is pinned at
    // the screen bottom. When scrolled back, it moves down/off-screen according
    // to the transcript offset.
    let bottom_y = if bottom_rows == 0 {
        screen_height
    } else if follow {
        screen_height.saturating_sub(bottom_rows).max(1)
    } else {
        len.saturating_sub(scroll.offset).min(screen_height)
    };
    let mut available = bottom_y;
    if show_hint {
        available = available.saturating_sub(1).max(1);
    }
    let start = if follow {
        len.saturating_sub(bottom_y)
    } else {
        scroll.offset.min(len.saturating_sub(1))
    };
    scroll.offset = start;
    let end = start.saturating_add(available).min(len);
    let (mut base_index, _) = state.render.transcript_cache.layout.locate(start);
    let mut display: Vec<Line<'static>> = Vec::with_capacity(available);
    let mut reading_rail_rows = Vec::new();
    while base_index < state.render.transcript_cache.lines.len() && display.len() < available {
        let base_start = state.render.transcript_cache.layout.prefix[base_index];
        let wrapped = wrap_line(
            state.render.transcript_cache.lines[base_index].clone(),
            width,
        );
        for (row_index, mut row) in wrapped.into_iter().enumerate() {
            let global_row = base_start + row_index;
            if global_row < start {
                continue;
            }
            if global_row >= end {
                break;
            }
            let reading_selected = state
                .reading
                .as_ref()
                .and_then(|reading| state.reading_layout.block(&reading.block_cursor))
                .is_some_and(|block| block.rows.contains(&global_row));
            if reading_selected {
                // A line-level Night background leaves explicit span-local
                // backgrounds (inline code, diff chips) authoritative.
                row = row.patch_style(Style::default().bg(theme.bg));
                reading_rail_rows.push(display.len());
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
        .render
        .transcript_cache
        .record_materialized_rows(display.len());
    if help_visible {
        if bottom_rows == 0 {
            display.extend(help_overlay(theme));
        } else {
            display.clear();
            display.extend(help_overlay(theme));
        }
    }
    // Lazy scroll-back hint at the top of the transcript (display-only).
    if show_hint {
        let hint = if state.session.history_loading {
            "（正在加载更早的消息…）"
        } else if state.session.history_exhausted {
            "（已到最早的消息）"
        } else {
            "（PageUp 加载更早的消息）"
        };
        display.insert(
            0,
            Line::from(Span::styled(hint, Style::default().fg(theme.dim))),
        );
    }
    // In combined mode the bottom stack owns the rows below `bottom_y`, so the
    // transcript/help/hint paragraph must never paint into that area.
    display.truncate(bottom_y);
    let paragraph = Paragraph::new(Text::from(display)).style(Style::default().fg(theme.fg));
    frame.render_widget(paragraph, area);
    if area.x > 0 {
        let rail_x = area.x - 1;
        let buffer = frame.buffer_mut();
        for row in reading_rail_rows {
            let y = area.y.saturating_add(row as u16);
            if y < area.y.saturating_add(area.height) {
                buffer[(rail_x, y)]
                    .set_symbol("│")
                    .set_fg(theme.overlay.border.fg)
                    .set_bg(theme.bg);
            }
        }
    }
    bottom_y
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
