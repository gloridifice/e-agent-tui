//! Incremental transcript cache and reveal-suffix updates.

use super::*;

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

pub(super) fn rebuild_transcript_cache(state: &mut TuiApp, width: usize) {
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

pub(super) fn refresh_transcript_cache(state: &mut TuiApp, width: usize) {
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
