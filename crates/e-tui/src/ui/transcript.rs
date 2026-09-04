use super::*;
use crate::{
    mouse_selection::{SelectionFrame, SelectionSurface},
    reveal::{apply_reveal, RevealSignature},
    ui::component::{card, text, working},
    wrap::stable_wrap_prefix_graphemes,
};
use unicode_segmentation::UnicodeSegmentation;

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
    let color = working::activity_color(&theme, row, color_override);
    let mut spans = vec![
        Span::styled(
            " ".repeat(2 + usize::from(row.depth) * 2),
            theme.activity.detail.style(),
        ),
        Span::styled(
            working::activity_indicator(state, row),
            Style::default().fg(color),
        ),
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
        let noun = crate::i18n::tr(
            state.config.language,
            if lines == 1 {
                "transcript.line"
            } else {
                "transcript.lines"
            },
        );
        metadata.push_str(&crate::i18n::tr_args(
            state.config.language,
            "transcript.output_lines",
            &[
                ("count", lines.to_string()),
                (
                    "suffix",
                    if row.output_lines_truncated {
                        "+".to_owned()
                    } else {
                        String::new()
                    },
                ),
                ("noun", noun),
            ],
        ));
    }
    if state.config.show_tool_duration {
        let duration_ms = row.duration_ms.or_else(|| {
            row.live_duration_since
                .map(|started| started.elapsed().as_millis() as u64)
        });
        if let Some(duration_ms) = duration_ms {
            metadata.push_str(&crate::i18n::tr_args(
                state.config.language,
                "transcript.duration",
                &[("seconds", format!("{:.1}", duration_ms as f64 / 1000.0))],
            ));
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

/// Render the merged Thinking node: a yellow Braille `Thinking... xN`
/// spinner in `compact` (or while no reasoning has streamed in yet);
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
    let label = format!(
        "{} ",
        crate::i18n::tr(state.config.language, "transcript.context_injection")
    );
    let theme = state.theme();
    let label_style = theme.activity.label.style();
    let content_style = theme.activity.detail.style();
    let avail = area_width.max(1);
    let first_avail = avail
        .saturating_sub(UnicodeWidthStr::width(label.as_str()))
        .max(1);

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
            let chunks = crate::wrap::wrap_text_chunks(&text, budget);
            let Some(head) = chunks.first() else {
                break;
            };
            let head_text = head.text.clone();
            if first {
                rows.push(Line::from(vec![
                    Span::styled(label.clone(), label_style),
                    Span::styled(head_text.clone(), content_style),
                ]));
                first = false;
            } else {
                rows.push(Line::from(Span::styled(head_text.clone(), content_style)));
            }
            if chunks.len() <= 1 {
                break;
            }
            // Resume from the next emitted chunk. Whitespace consumed by the
            // row break is deliberately skipped; a hard-split word resumes at
            // exactly the next byte.
            text = text[chunks[1].byte_start..].to_owned();
        }
    }
    if rows.is_empty() {
        rows.push(Line::from(Span::styled(label, label_style)));
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

fn markdown_block_semantic_lines(block: &TranscriptBlock, state: &TuiApp) -> Vec<Line<'static>> {
    let theme = state.theme();
    let Some(lines) = state.render.markdown_layout.lines(&block.id) else {
        return transcript_block_lines(block, state);
    };
    lines
        .iter()
        .map(|render_line| {
            let mut line = render_line.line.clone();
            if render_line.fill {
                line = line.patch_style(theme.markdown.code_block_bg.style());
            }
            line
        })
        .collect()
}

/// Reveal input for a live assistant Markdown block, read from the already
/// rendered layout without cloning the semantic lines. Returns the signature
/// and, for the trailing streaming line, its text and grapheme count.
fn markdown_block_reveal_source(
    block: &TranscriptBlock,
    state: &TuiApp,
) -> (RevealSignature, Option<(String, usize)>) {
    let (signature, last_text) = if let Some(lines) = state.render.markdown_layout.lines(&block.id)
    {
        let signature =
            RevealSignature::from_line_iter(lines.iter().map(|render_line| &render_line.line));
        let last_text = lines.last().map(|render_line| {
            render_line
                .line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        });
        (signature, last_text)
    } else {
        let owned = transcript_block_lines(block, state);
        let last_text = owned.last().map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        });
        let signature = RevealSignature::from_lines(&owned);
        (signature, last_text)
    };
    let last = last_text.map(|text| {
        let count = text.graphemes(true).count();
        (text, count)
    });
    (signature, last)
}

fn pad_visible_markdown_fill_lines(
    block: &TranscriptBlock,
    state: &TuiApp,
    area_width: usize,
    lines: &mut [Line<'static>],
) {
    let Some(layout) = state.render.markdown_layout.lines(&block.id) else {
        return;
    };
    let fill_style = state.theme().markdown.code_block_bg.style();
    for (line, render_line) in lines.iter_mut().zip(layout) {
        if render_line.fill {
            let width = line.width();
            if width < area_width {
                line.push_span(Span::styled(" ".repeat(area_width - width), fill_style));
            }
        }
    }
}

fn markdown_block_lines(
    block: &TranscriptBlock,
    state: &TuiApp,
    area_width: usize,
) -> Vec<Line<'static>> {
    let theme = state.theme();
    let full = markdown_block_semantic_lines(block, state);
    let mut rendered = if let Some(track) = state.render.transcript_reveals.get(&block.id) {
        apply_reveal(
            full,
            track,
            state.config.background_color.color(),
            theme.markdown.text.fg,
            !state.config.plain_color,
        )
    } else {
        full
    };
    // Fill padding is presentation geometry, not rendered source. Add it only
    // after reveal clipping so it neither consumes pacing budget nor changes
    // the logical signature when the terminal width changes.
    pad_visible_markdown_fill_lines(block, state, area_width, &mut rendered);
    if block.streaming {
        if let Some(last) = rendered.last_mut() {
            last.push_span(Span::raw(" "));
            last.push_span(Span::styled(
                state.activity_spinner_frame(),
                Style::default().fg(theme.working_status.running.fg),
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

const ACTIVITY_FOLD_EDGE_ROWS: usize = 3;
const ACTIVITY_FOLD_THRESHOLD: usize = ACTIVITY_FOLD_EDGE_ROWS * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodePresentation {
    Hidden,
    Visible,
    ActivityFold { hidden: usize },
}

fn is_collapsible_activity(item: &DisplayItem, state: &TuiApp) -> bool {
    match item {
        DisplayItem::Activity(_) => true,
        DisplayItem::Thinking(node) => {
            !state.config.thinking_display_mode().shows_reasoning() || node.content.is_empty()
        }
        _ => false,
    }
}

fn transcript_presentations(state: &TuiApp) -> Vec<NodePresentation> {
    transcript_presentations_with_folding(state, state.reading.is_none())
}

fn is_activity_fold_trigger(item: &DisplayItem) -> bool {
    let DisplayItem::Block(block) = item else {
        return false;
    };
    block.format == TranscriptFormat::Markdown
        || block.tone == DisplayTone::Error
        || block.id.0.starts_with("turn-outcome:")
        || block.id.0.ends_with(":turn-outcome")
}

fn fold_activity_run(run: &[usize], presentations: &mut [NodePresentation]) {
    if run.len() <= ACTIVITY_FOLD_THRESHOLD {
        return;
    }
    let hidden = run.len() - ACTIVITY_FOLD_THRESHOLD;
    let middle = &run[ACTIVITY_FOLD_EDGE_ROWS..run.len() - ACTIVITY_FOLD_EDGE_ROWS];
    presentations[middle[0]] = NodePresentation::ActivityFold { hidden };
    for index in &middle[1..] {
        presentations[*index] = NodePresentation::Hidden;
    }
}

fn transcript_presentations_with_folding(
    state: &TuiApp,
    fold_activities: bool,
) -> Vec<NodePresentation> {
    let nodes = state.transcript.nodes();
    let mut presentations = nodes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if is_hidden_node(nodes, index, state) {
                NodePresentation::Hidden
            } else {
                NodePresentation::Visible
            }
        })
        .collect::<Vec<_>>();
    if !fold_activities {
        return presentations;
    }

    let mut run = Vec::new();
    let mut pending_runs = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if is_hidden_node(nodes, index, state) {
            continue;
        }
        if is_collapsible_activity(&node.item, state) {
            run.push(index);
            continue;
        }
        if !run.is_empty() {
            pending_runs.push(std::mem::take(&mut run));
        }
        if is_activity_fold_trigger(&node.item) {
            for pending in pending_runs.drain(..) {
                fold_activity_run(&pending, &mut presentations);
            }
        } else if !is_activity_item(&node.item) {
            pending_runs.clear();
        }
    }
    presentations
}

fn folded_activity_line(hidden: usize, state: &TuiApp) -> Line<'static> {
    let noun = crate::i18n::tr(
        state.config.language,
        if hidden == 1 {
            "transcript.line"
        } else {
            "transcript.lines"
        },
    );
    Line::styled(
        crate::i18n::tr_args(
            state.config.language,
            "transcript.activity_fold",
            &[("count", hidden.to_string()), ("noun", noun)],
        ),
        state.theme().activity.detail.style(),
    )
}

fn presentation_lines(
    item: &DisplayItem,
    presentation: NodePresentation,
    state: &TuiApp,
    width: usize,
) -> Vec<Line<'static>> {
    match presentation {
        NodePresentation::Hidden => Vec::new(),
        NodePresentation::Visible => display_item_lines(item, state, width),
        NodePresentation::ActivityFold { hidden } => vec![folded_activity_line(hidden, state)],
    }
}

fn presentation_is_activity(item: &DisplayItem, presentation: NodePresentation) -> bool {
    match presentation {
        NodePresentation::Hidden => false,
        NodePresentation::Visible => is_activity_item(item),
        NodePresentation::ActivityFold { .. } => true,
    }
}

fn cached_node_presentation(cache: &TranscriptRenderCache, index: usize) -> NodePresentation {
    if let Some(hidden) = cache
        .activity_fold_hidden_counts
        .get(index)
        .copied()
        .flatten()
    {
        NodePresentation::ActivityFold { hidden }
    } else if cache.message_ranges.get(index).is_some_and(Option::is_some) {
        NodePresentation::Visible
    } else {
        NodePresentation::Hidden
    }
}

fn next_presented_item_is_activity(
    nodes: &[TranscriptNode],
    presentations: &[NodePresentation],
    index: usize,
) -> bool {
    nodes
        .iter()
        .zip(presentations)
        .skip(index + 1)
        .find(|(_, presentation)| **presentation != NodePresentation::Hidden)
        .is_some_and(|(node, presentation)| presentation_is_activity(&node.item, *presentation))
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
    let presentations = transcript_presentations(state);
    for (index, node) in nodes.iter().enumerate() {
        let item = &node.item;
        let presentation = presentations[index];
        if presentation == NodePresentation::Hidden {
            continue;
        }
        let layout_lines = presentation_lines(item, presentation, state, width);
        if matches!(presentation, NodePresentation::ActivityFold { .. }) {
            global_row += layout_lines
                .iter()
                .map(|line| wrapped_rows(line, width))
                .sum::<usize>();
        } else {
            match item {
                DisplayItem::Block(block) if block.format == TranscriptFormat::Markdown => {
                    if let Some(render_lines) = state.render.markdown_layout.lines(&block.id) {
                        for (render_line, layout_line) in
                            render_lines.iter().zip(layout_lines.iter())
                        {
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
                        append_unit_rows(
                            &mut rows,
                            &mut global_row,
                            detail.unit,
                            detail_lines,
                            width,
                        );
                    }
                }
                DisplayItem::Activity(_) => {
                    global_row += layout_lines
                        .iter()
                        .map(|line| wrapped_rows(line, width))
                        .sum::<usize>();
                }
            }
        }
        let next_is_activity = next_presented_item_is_activity(nodes, &presentations, index);
        if !(presentation_is_activity(item, presentation) && next_is_activity) {
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

fn refresh_transcript_reveals(state: &mut TuiApp) {
    if state.render.transcript_reveals.is_empty() {
        return;
    }
    let now = std::time::Instant::now();
    let rate = state.config.message_chars_per_second.get();
    let ids = state
        .render
        .transcript_reveals
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut stale = Vec::new();
    for id in ids {
        let Some(index) = state.transcript.position(&id) else {
            stale.push(id);
            continue;
        };
        let (signature, last_line, streaming, settled, source) = {
            let Some(node) = state.transcript.nodes().get(index) else {
                stale.push(id);
                continue;
            };
            let DisplayItem::Block(block) = &node.item else {
                stale.push(id);
                continue;
            };
            if block.format != TranscriptFormat::Markdown {
                stale.push(id);
                continue;
            }
            let (signature, last_line) = markdown_block_reveal_source(block, state);
            (
                signature,
                last_line,
                block.streaming,
                block.content.ends_with('\n'),
                block.content.clone(),
            )
        };
        let admitted = if !streaming || settled {
            signature.grapheme_count()
        } else if let Some((last_text, last_graphemes)) = last_line {
            let prefix_count = signature.grapheme_count().saturating_sub(last_graphemes);
            prefix_count
                + stable_wrap_prefix_graphemes(&last_text, state.render.transcript_cache.width)
        } else {
            0
        };
        let track = state
            .render
            .transcript_reveals
            .get_mut(&id)
            .expect("collected reveal track remains present");
        if track.reconcile_append_only_source(&source, signature, admitted, !streaming, now, rate) {
            state.render.transcript_cache.mark_reveal_dirty(index);
        }
        if track.is_complete() {
            stale.push(id);
            state.render.transcript_cache.mark_reveal_dirty(index);
        }
    }
    for id in stale {
        state.render.transcript_reveals.remove(&id);
    }
}

fn rebuild_transcript_cache(state: &mut TuiApp, width: usize) {
    let _zone = crate::tracy_zone!("transcript rebuild");
    let mut base = Vec::new();
    let mut ranges = vec![None; state.transcript.len()];
    let nodes = state.transcript.nodes();
    let presentations = transcript_presentations(state);
    for (index, node) in nodes.iter().enumerate() {
        let item = &node.item;
        let presentation = presentations[index];
        if presentation == NodePresentation::Hidden {
            continue;
        }
        let start = base.len();
        let lines = presentation_lines(item, presentation, state, width);
        let line_count = lines.len();
        base.extend(lines);
        let next_is_activity = next_presented_item_is_activity(nodes, &presentations, index);
        let gap = !(presentation_is_activity(item, presentation) && next_is_activity);
        ranges[index] = Some(MessageLineRange {
            start,
            end: start + line_count,
            owns_gap: gap,
        });
        if gap {
            base.push(Line::default());
        }
    }
    let fold_hidden_counts = presentations
        .into_iter()
        .map(|presentation| match presentation {
            NodePresentation::ActivityFold { hidden } => Some(hidden),
            _ => None,
        })
        .collect();
    let cache = &mut state.render.transcript_cache;
    cache.lines = base;
    cache.message_ranges = ranges;
    cache.activity_fold_hidden_counts = fold_hidden_counts;
    cache.valid = true;
    cache.tail_dirty = false;
    cache.reveal_dirty_from = None;
    cache.dirty_messages.clear();
    cache.structural_rebuilt();
}

fn refresh_transcript_cache(state: &mut TuiApp, width: usize) {
    if state.render.transcript_cache.width != width {
        state.render.transcript_cache.width = width;
        state.render.transcript_cache.invalidate();
    }
    crate::presentation::materialize_transcript(state);
    refresh_transcript_reveals(state);
    if !state.render.transcript_cache.valid {
        rebuild_transcript_cache(state, width);
        return;
    }

    let tail_index = state
        .render
        .transcript_cache
        .tail_dirty
        .then(|| state.transcript.len().checked_sub(1))
        .flatten();
    let suffix_index = match (tail_index, state.render.transcript_cache.reveal_dirty_from) {
        (Some(tail), Some(reveal)) => Some(tail.min(reveal)),
        (tail, reveal) => tail.or(reveal),
    };
    if let Some(suffix_index) = suffix_index {
        let Some(keep) = state
            .render
            .transcript_cache
            .message_ranges
            .get(suffix_index)
            .and_then(|range| *range)
            .map(|range| range.start)
        else {
            rebuild_transcript_cache(state, width);
            return;
        };
        let nodes = state.transcript.nodes();
        let mut suffix_lines = Vec::new();
        let mut ranges = Vec::new();
        for (index, node) in nodes.iter().enumerate().skip(suffix_index) {
            let presentation = cached_node_presentation(&state.render.transcript_cache, index);
            if presentation == NodePresentation::Hidden {
                ranges.push((index, None));
                continue;
            }
            let start = keep + suffix_lines.len();
            let lines = presentation_lines(&node.item, presentation, state, width);
            let line_count = lines.len();
            suffix_lines.extend(lines);
            let next_is_activity = nodes
                .iter()
                .enumerate()
                .skip(index + 1)
                .map(|(position, node)| {
                    (
                        node,
                        cached_node_presentation(&state.render.transcript_cache, position),
                    )
                })
                .find(|(_, presentation)| *presentation != NodePresentation::Hidden)
                .is_some_and(|(node, presentation)| {
                    presentation_is_activity(&node.item, presentation)
                });
            let gap = !(presentation_is_activity(&node.item, presentation) && next_is_activity);
            ranges.push((
                index,
                Some(MessageLineRange {
                    start,
                    end: start + line_count,
                    owns_gap: gap,
                }),
            ));
            if gap {
                suffix_lines.push(Line::default());
            }
        }
        let transcript_len = state.transcript.len();
        let cache = &mut state.render.transcript_cache;
        cache.lines.truncate(keep);
        cache.lines.extend(suffix_lines);
        cache.message_ranges.resize(transcript_len, None);
        for (index, range) in ranges {
            cache.message_ranges[index] = range;
        }
        cache.tail_dirty = false;
        cache.reveal_dirty_from = None;
        cache.dirty_messages.retain(|index| *index < suffix_index);
        let row_counts = cache.lines[keep..]
            .iter()
            .map(|line| wrapped_rows(line, width))
            .collect::<Vec<_>>();
        cache.structural_tail_updated(keep, width, &row_counts);
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
                let presentation = cached_node_presentation(&state.render.transcript_cache, *index);
                if presentation == NodePresentation::Hidden {
                    return None;
                }
                nodes.get(*index).map(|node| {
                    (
                        *index,
                        presentation_lines(&node.item, presentation, state, width),
                    )
                })
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
    selection_frame: &mut SelectionFrame,
) {
    let _ = render_transcript_impl(
        frame,
        area,
        state,
        scroll,
        theme,
        help_visible,
        0,
        selection_frame,
    );
}

/// Render transcript with the bottom stack (accessories + input + status +
/// title) participating in the scroll. `area` is the full content viewport;
/// `bottom_rows` is the total height of the stack that follows the transcript.
/// Returns the y offset (within `area`) where that stack begins, so the caller
/// can draw it at the content-bottom position.
#[allow(clippy::too_many_arguments)] // The caller owns the shared viewport contract.
pub(super) fn render_transcript_combined(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    bottom_rows: usize,
    selection_frame: &mut SelectionFrame,
) -> usize {
    render_transcript_impl(
        frame,
        area,
        state,
        scroll,
        theme,
        help_visible,
        bottom_rows,
        selection_frame,
    )
}

#[allow(clippy::too_many_arguments)] // Internal implementation mirrors the public viewport contract.
fn render_transcript_impl(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    help_visible: bool,
    bottom_rows: usize,
    selection_frame: &mut SelectionFrame,
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
            help_overlay(state.config.language, theme)
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
    state.reconcile_reading_layout_anchor(scroll);
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
    let visual_row_offset = usize::from(show_hint);
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
                reading_rail_rows.push(display.len() + visual_row_offset);
            }
            if !help_visible {
                super::selection::register_line(
                    selection_frame,
                    SelectionSurface::Transcript,
                    global_row,
                    area.x,
                    area.y
                        .saturating_add((display.len() + visual_row_offset) as u16),
                    &row,
                );
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
            display.extend(help_overlay(state.config.language, theme));
        } else {
            display.clear();
            display.extend(help_overlay(state.config.language, theme));
        }
    }
    // Lazy scroll-back hint at the top of the transcript (display-only).
    if show_hint {
        let hint = if state.session.history_loading {
            crate::i18n::tr(state.config.language, "transcript.history_loading")
        } else if state.session.history_exhausted {
            crate::i18n::tr(state.config.language, "transcript.history_exhausted")
        } else {
            crate::i18n::tr(state.config.language, "transcript.history_load_hint")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DisplayId;

    fn activity(index: usize) -> DisplayItem {
        DisplayItem::Activity(ActivityRow::root(
            DisplayId::correlated("activity", &index.to_string()),
            format!("tool-{index}"),
        ))
    }

    fn block(id: &str, format: TranscriptFormat, tone: DisplayTone) -> DisplayItem {
        DisplayItem::Block(TranscriptBlock {
            id: DisplayId::correlated(id, "test"),
            unit: None,
            content: id.into(),
            format,
            tone,
            copy_source: id.into(),
            streaming: false,
        })
    }

    fn markdown() -> DisplayItem {
        block(
            "assistant-answer",
            TranscriptFormat::Markdown,
            DisplayTone::Normal,
        )
    }

    fn informational_activity(index: usize) -> DisplayItem {
        DisplayItem::Composite {
            activity: ActivityRow::root(
                DisplayId::correlated("rich-activity", &index.to_string()),
                "tool-with-information",
            ),
            detail: ContentCard {
                id: DisplayId::correlated("rich-detail", &index.to_string()),
                unit: None,
                header: Some("Information".into()),
                content: "complete detail".into(),
                role: CardRole::Detail,
                tone: DisplayTone::Info,
                horizontal_padding: 2,
                copy_source: "complete detail".into(),
            },
        }
    }

    fn append_activity_run(state: &mut TuiApp, start: usize, count: usize) {
        for index in start..start + count {
            state.transcript.append(activity(index), None);
        }
    }

    fn fold_counts(presentations: &[NodePresentation]) -> Vec<usize> {
        presentations
            .iter()
            .filter_map(|presentation| match presentation {
                NodePresentation::ActivityFold { hidden } => Some(*hidden),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn activity_run_folds_only_after_markdown_and_only_above_threshold() {
        let mut within_threshold = TuiApp::default();
        append_activity_run(&mut within_threshold, 0, 6);
        within_threshold.transcript.append(markdown(), None);
        assert!(fold_counts(&transcript_presentations(&within_threshold)).is_empty());

        let mut long = TuiApp::default();
        append_activity_run(&mut long, 0, 7);
        assert!(
            fold_counts(&transcript_presentations(&long)).is_empty(),
            "a live trailing run stays expanded"
        );
        long.transcript.append(markdown(), None);
        let presentations = transcript_presentations(&long);
        assert_eq!(fold_counts(&presentations), vec![1]);
        assert_eq!(
            presentations[3],
            NodePresentation::ActivityFold { hidden: 1 }
        );
    }

    #[test]
    fn informational_activity_does_not_trigger_a_pending_fold() {
        let mut state = TuiApp::default();
        append_activity_run(&mut state, 0, 8);
        state.transcript.append(informational_activity(8), None);
        assert!(fold_counts(&transcript_presentations(&state)).is_empty());

        state.transcript.append(markdown(), None);
        let presentations = transcript_presentations(&state);
        assert_eq!(fold_counts(&presentations), vec![2]);
        assert_eq!(presentations[8], NodePresentation::Visible);
    }

    #[test]
    fn interruption_and_error_outcomes_trigger_pending_folds() {
        let mut interrupted = TuiApp::default();
        append_activity_run(&mut interrupted, 0, 7);
        interrupted.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: DisplayId::event(9, "turn-outcome"),
                unit: None,
                content: "Interrupted".into(),
                format: TranscriptFormat::Plain,
                tone: DisplayTone::Warning,
                copy_source: "Interrupted".into(),
                streaming: false,
            }),
            None,
        );
        assert_eq!(
            fold_counts(&transcript_presentations(&interrupted)),
            vec![1]
        );

        let mut network_error = TuiApp::default();
        append_activity_run(&mut network_error, 0, 8);
        network_error.transcript.append(
            block("error", TranscriptFormat::Plain, DisplayTone::Error),
            None,
        );
        assert_eq!(
            fold_counts(&transcript_presentations(&network_error)),
            vec![2]
        );
    }

    #[test]
    fn ordinary_content_does_not_trigger_or_defer_a_fold_across_it() {
        let mut state = TuiApp::default();
        append_activity_run(&mut state, 0, 7);
        state.transcript.append(
            block("notice", TranscriptFormat::Plain, DisplayTone::Normal),
            None,
        );
        state.transcript.append(markdown(), None);
        assert!(fold_counts(&transcript_presentations(&state)).is_empty());
    }

    #[test]
    fn completed_runs_fold_independently_and_reading_expands_them() {
        let mut state = TuiApp::default();
        append_activity_run(&mut state, 0, 7);
        state.transcript.append(markdown(), None);
        append_activity_run(&mut state, 10, 8);
        state.transcript.append(
            block("error", TranscriptFormat::Plain, DisplayTone::Error),
            None,
        );

        assert_eq!(fold_counts(&transcript_presentations(&state)), vec![1, 2]);
        assert!(transcript_presentations_with_folding(&state, false)
            .iter()
            .all(|presentation| *presentation == NodePresentation::Visible));
    }

    #[test]
    fn hidden_reasoning_does_not_split_a_completed_activity_run() {
        let mut state = TuiApp::default();
        append_activity_run(&mut state, 0, 3);
        state.transcript.append(
            block("reasoning", TranscriptFormat::Reasoning, DisplayTone::Dim),
            None,
        );
        append_activity_run(&mut state, 3, 4);
        state.transcript.append(markdown(), None);

        let presentations = transcript_presentations(&state);
        assert_eq!(fold_counts(&presentations), vec![1]);
        assert_eq!(presentations[3], NodePresentation::Hidden);
        assert_eq!(
            presentations[4],
            NodePresentation::ActivityFold { hidden: 1 }
        );
    }
}
