//! Spinner, settle, and reveal-adjacent animation state helpers.

use super::{AgentStatus, DisplayItem, RuntimeState, TranscriptFormat, SETTLE_TRANSITION_MS};
#[cfg(test)]
use super::{Msg, ThinkState, ToolState};
#[cfg(test)]
use ratatui::style::Color;

/// Whether an animation deadline is needed. This is separate from advancing
/// the clock so the event-driven main loop can remain asleep when idle.
pub fn animation_active(state: &RuntimeState, _now: std::time::Instant) -> bool {
    #[cfg(test)]
    if state.transcript.is_empty() && !state.msgs.is_empty() {
        return legacy_animation_active(state);
    }
    state
        .transcript
        .nodes()
        .iter()
        .any(|node| match &node.item {
            DisplayItem::Activity(row) => row.state.is_active(),
            DisplayItem::Block(block) => {
                block.streaming && block.format != TranscriptFormat::Reasoning
            }
            DisplayItem::Composite { activity, .. } => activity.state.is_active(),
            DisplayItem::Thinking(node) => node.row.state.is_active(),
            DisplayItem::Card(_) => false,
        })
        || !state.render.activity_transitions.is_empty()
        || state.session.working
        || state.session.status == AgentStatus::Running
}

/// Advance the breathing/transition animation clock and mark only the public
/// display ranges whose colors can change. Expired transitions submit one
/// final exact-color patch before their sidecar entry is removed.
pub fn tick_spinners(state: &mut RuntimeState, now: std::time::Instant) -> bool {
    #[cfg(test)]
    if state.transcript.is_empty() && !state.msgs.is_empty() {
        return tick_legacy_spinners(state, now);
    }

    let mut any_pending = state.session.working || state.session.status == AgentStatus::Running;
    let mut dirty = Vec::new();
    for (index, node) in state.transcript.nodes().iter().enumerate() {
        let pending = match &node.item {
            DisplayItem::Activity(row) => row.state.is_active(),
            DisplayItem::Block(block) => {
                block.streaming && block.format != TranscriptFormat::Reasoning
            }
            DisplayItem::Composite { activity, .. } => activity.state.is_active(),
            DisplayItem::Thinking(node) => node.row.state.is_active(),
            DisplayItem::Card(_) => false,
        };
        if pending {
            any_pending = true;
            dirty.push(index);
        }
    }

    let transitions = state
        .render
        .activity_transitions
        .iter()
        .map(|(id, transition)| {
            (
                id.clone(),
                now.saturating_duration_since(transition.done_since)
                    .as_millis()
                    >= SETTLE_TRANSITION_MS,
            )
        })
        .collect::<Vec<_>>();
    let mut finalized = Vec::new();
    for (id, expired) in transitions {
        if let Some(index) = state.transcript.position(&id) {
            dirty.push(index);
        }
        if expired {
            finalized.push(id);
        }
    }
    for id in finalized {
        state.render.activity_transitions.remove(&id);
    }

    if any_pending {
        state.session.activity_epoch.get_or_insert(now);
    } else if state.render.activity_transitions.is_empty() {
        state.session.activity_epoch = None;
    }
    dirty.sort_unstable();
    dirty.dedup();
    let animation_changed = !dirty.is_empty();
    for index in dirty {
        state.render.transcript_cache.mark_message_dirty(index);
    }
    any_pending || animation_changed
}

#[cfg(test)]
fn legacy_animation_active(state: &RuntimeState) -> bool {
    let any_pending = state.msgs.iter().any(|message| match message {
        Msg::Tool(card) => card.state == ToolState::Running,
        Msg::FileGroup(group) => group.pending(),
        Msg::Thinking(card) => card.state == ThinkState::Running,
        Msg::Activity(row) => row.state.is_active(),
        Msg::Block(block) => block.streaming && block.format != TranscriptFormat::Reasoning,
        _ => false,
    }) || matches!(state.msgs.last(), Some(Msg::Streaming { .. }))
        || state.session.working
        || state.session.status == AgentStatus::Running;
    let transitioning = state.msgs.iter().any(|message| match message {
        Msg::Tool(card) => card.done_since.is_some() && card.done_from.is_some(),
        Msg::FileGroup(group) => group.done_since.is_some() && group.done_from.is_some(),
        Msg::Thinking(card) => card.done_since.is_some() && card.done_from.is_some(),
        _ => false,
    });
    any_pending || transitioning
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyTransitionTick {
    None,
    Active,
    Finalize,
}

#[cfg(test)]
fn advance_legacy_transition(
    done_since: &mut Option<std::time::Instant>,
    done_from: &mut Option<Color>,
    now: std::time::Instant,
) -> LegacyTransitionTick {
    let (Some(at), Some(_)) = (*done_since, *done_from) else {
        return LegacyTransitionTick::None;
    };
    if now.saturating_duration_since(at).as_millis() < SETTLE_TRANSITION_MS {
        LegacyTransitionTick::Active
    } else {
        *done_from = None;
        LegacyTransitionTick::Finalize
    }
}

#[cfg(test)]
fn tick_legacy_spinners(state: &mut RuntimeState, now: std::time::Instant) -> bool {
    let mut any_pending = state.session.working || state.session.status == AgentStatus::Running;
    let mut animation_changed = false;
    let msg_len = state.msgs.len();
    let mut dirty = Vec::new();
    for (index, message) in state.msgs.iter_mut().enumerate() {
        let (pending, transition) = match message {
            Msg::Tool(card) => (
                card.state == ToolState::Running,
                advance_legacy_transition(&mut card.done_since, &mut card.done_from, now),
            ),
            Msg::FileGroup(group) => (
                group.pending(),
                advance_legacy_transition(&mut group.done_since, &mut group.done_from, now),
            ),
            Msg::Thinking(card) => (
                card.state == ThinkState::Running,
                advance_legacy_transition(&mut card.done_since, &mut card.done_from, now),
            ),
            Msg::Activity(row) => (row.state.is_active(), LegacyTransitionTick::None),
            Msg::Block(block) => (
                block.streaming && block.format != TranscriptFormat::Reasoning,
                LegacyTransitionTick::None,
            ),
            Msg::Streaming { .. } if index + 1 == msg_len => (true, LegacyTransitionTick::None),
            _ => (false, LegacyTransitionTick::None),
        };
        any_pending |= pending;
        if pending || transition != LegacyTransitionTick::None {
            dirty.push(index);
            animation_changed = true;
        }
    }
    if any_pending {
        state.session.activity_epoch.get_or_insert(now);
    } else {
        state.session.activity_epoch = None;
    }
    for index in dirty {
        state.render.transcript_cache.mark_message_dirty(index);
    }
    any_pending || animation_changed
}
