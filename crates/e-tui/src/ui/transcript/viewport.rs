//! Visible transcript row materialization and viewport painting.

use super::refresh_transcript_cache;
use super::*;

#[allow(clippy::too_many_arguments)] // Internal implementation mirrors the public viewport contract.
pub(super) fn render_transcript_impl(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &mut TuiApp,
    scroll: &mut ScrollState,
    theme: &Theme,
    bottom_rows: usize,
) -> usize {
    let screen_height = area.height as usize;
    let width = area.width as usize;
    if let Some(draft) = state
        .session
        .new_conversation
        .as_ref()
        .filter(|draft| state.shell.draft_ids.is_empty() || draft.pending_input.is_some())
    {
        let cards: Vec<_> = draft
            .pending_card
            .iter()
            .cloned()
            .flat_map(|card| {
                let message = draft.pending_input.as_ref().and_then(|prompt| {
                    crate::runtime::RuntimeState::submission_message_card(prompt, &card)
                });
                std::iter::once(card).chain(message)
            })
            .collect();
        for card in &cards {
            if matches!(card.role, CardRole::User | CardRole::Attachment) {
                let (_, content_width) =
                    crate::transcript_layout::user_message_geometry(card, width);
                let options = crate::render::transcript_options(&state.config, content_width);
                state
                    .render
                    .markdown_layout
                    .materialize_card(card, theme, &options);
            }
        }
        let bottom_rows = bottom_rows.min(screen_height);
        let bottom_y = if bottom_rows == 0 {
            screen_height
        } else {
            screen_height.saturating_sub(bottom_rows).max(1)
        };
        let mut display: Vec<_> = cards
            .into_iter()
            .flat_map(|card| display_item_lines(&DisplayItem::Card(card), state, width))
            .collect();
        if let Some(notice) = &draft.notice {
            display.push(Line::from(Span::styled(
                notice.clone(),
                theme.log.warning.style(),
            )));
        }
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
    let attached = state.shell.active.as_ref().and_then(|id| {
        let node = state.transcript.get(id)?;
        if state.session.new_conversation.is_some() && !state.shell.draft_ids.contains(id) {
            return None;
        }
        display_item_lines(&node.item, state, width)
            .into_iter()
            .next()
    });
    // The running command owns a viewport row, not a second transcript node.
    let attached_rows = usize::from(attached.is_some());
    let bottom_y = if bottom_rows == 0 {
        screen_height
    } else if follow {
        screen_height.saturating_sub(bottom_rows).max(1)
    } else {
        len.saturating_sub(scroll.offset)
            .saturating_add(attached_rows)
            .min(screen_height)
    };
    let content_height = bottom_y.saturating_sub(attached_rows);
    let available = content_height.saturating_sub(usize::from(show_hint));
    let start = if follow {
        len.saturating_sub(content_height)
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
    // transcript/hint paragraph must never paint into that area.
    display.truncate(content_height);
    let paragraph = Paragraph::new(Text::from(display)).style(Style::default().fg(theme.fg));
    frame.render_widget(paragraph, area);
    if let Some(line) = attached.filter(|_| bottom_y > 0) {
        frame.render_widget(
            Paragraph::new(line),
            ratatui::layout::Rect::new(area.x, area.y + bottom_y as u16 - 1, area.width, 1),
        );
    }
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
