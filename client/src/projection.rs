//! Typed DSH event classification and surface ownership.

use std::collections::{HashMap, HashSet};

use crate::{
    display::{
        ActivityState, DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat,
    },
    protocol::{HostEvent, HostEventKind, HostSurfaceOp},
};

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectionEffect {
    Reduce {
        insert_at: Option<usize>,
    },
    SurfaceMutation {
        remove_indices: Vec<usize>,
        insert_at: Option<usize>,
    },
    Display(DisplayItem),
    PageState(PageStateEffect),
    AccessoryState(AccessoryStateEffect),
    CompatibilityError(String),
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStateEffect {
    Title(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessoryStateEffect {
    Todo(Vec<(String, String)>),
    ClearTodo,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingToolResult {
    pub output: String,
    pub is_error: bool,
    pub output_truncated: bool,
    pub time_ms: u64,
    pub surface_seq: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingActivityResult {
    pub state: ActivityState,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingActivityEnrichment {
    pub summary: String,
    pub start_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub struct EventProjector {
    shadowed_surface_seqs: HashSet<u64>,
    surface_order: Vec<u64>,
    surface_owners: HashMap<u64, usize>,
    display_positions: HashMap<DisplayId, usize>,
    pending_surface_insert_at: Option<usize>,
    pub tool_calls: HashMap<String, DisplayId>,
    pub commands: HashMap<String, DisplayId>,
    pub retries: HashMap<String, DisplayId>,
    pub compactions: HashMap<String, DisplayId>,
    pub nested_calls: HashMap<String, DisplayId>,
    pub workflows: HashMap<String, DisplayId>,
    pending_tool_results: HashMap<String, PendingToolResult>,
    pending_activity_results: HashMap<DisplayId, PendingActivityResult>,
    pending_activity_enrichments: HashMap<DisplayId, PendingActivityEnrichment>,
}

impl EventProjector {
    pub fn effects(&mut self, event: &HostEvent) -> Vec<ProjectionEffect> {
        if event.surface_op_invalid {
            return vec![ProjectionEffect::CompatibilityError(
                "unsupported or malformed DSH surface operation".into(),
            )];
        }
        if event
            .seq
            .is_some_and(|seq| self.shadowed_surface_seqs.contains(&seq))
        {
            return vec![ProjectionEffect::Ignore];
        }

        if let HostEventKind::SessionTitle { title } = &event.kind {
            return vec![ProjectionEffect::PageState(PageStateEffect::Title(
                title.clone(),
            ))];
        }
        if let HostEventKind::TodoWrite { todos } = &event.kind {
            return vec![ProjectionEffect::AccessoryState(
                AccessoryStateEffect::Todo(todos.clone()),
            )];
        }
        if matches!(event.kind, HostEventKind::TurnStart) {
            return vec![
                ProjectionEffect::AccessoryState(AccessoryStateEffect::ClearTodo),
                ProjectionEffect::Reduce { insert_at: None },
            ];
        }

        if let HostEventKind::Unknown { event_type } = &event.kind {
            match event.surface_op {
                Some(HostSurfaceOp::Append) => {
                    let event_type = event_type.as_deref().unwrap_or("unknown");
                    let content: String = format!("Unsupported DSH surface event: {event_type}")
                        .chars()
                        .take(160)
                        .collect();
                    return vec![ProjectionEffect::Display(DisplayItem::Block(
                        TranscriptBlock {
                            id: event.seq.map_or_else(
                                || DisplayId::correlated("unknown", event_type),
                                |seq| DisplayId::event(seq, "unknown"),
                            ),
                            unit: None,
                            copy_source: content.clone(),
                            content,
                            format: TranscriptFormat::UnknownFallback,
                            tone: DisplayTone::Warning,
                            streaming: false,
                        },
                    ))];
                }
                Some(HostSurfaceOp::Replace { .. }) => {
                    return vec![ProjectionEffect::CompatibilityError(format!(
                        "unsupported DSH replacement event: {}",
                        event_type.as_deref().unwrap_or("unknown")
                    ))];
                }
                None => return vec![ProjectionEffect::Ignore],
            }
        }

        if let Some(HostSurfaceOp::Replace { start, end }) = event.surface_op {
            let mut shadowed = if event.source_event_seqs.is_empty() {
                self.surface_range(start, end)
            } else {
                event.source_event_seqs.clone()
            };
            shadowed.sort_unstable();
            shadowed.dedup();
            let mut remove_indices: Vec<usize> = shadowed
                .iter()
                .filter_map(|seq| self.surface_owners.get(seq).copied())
                .collect();
            remove_indices.sort_unstable();
            remove_indices.dedup();
            let insert_at = remove_indices.first().copied();
            self.pending_surface_insert_at = self
                .surface_order
                .iter()
                .enumerate()
                .filter_map(|(index, seq)| shadowed.contains(seq).then_some(index))
                .min();
            self.shadowed_surface_seqs.extend(shadowed.iter().copied());
            self.surface_order
                .retain(|seq| !self.shadowed_surface_seqs.contains(seq));
            for seq in &shadowed {
                self.surface_owners.remove(seq);
            }
            return vec![
                ProjectionEffect::SurfaceMutation {
                    remove_indices,
                    insert_at,
                },
                ProjectionEffect::Reduce { insert_at },
            ];
        }

        vec![ProjectionEffect::Reduce { insert_at: None }]
    }

    fn surface_range(&self, start: u64, end: u64) -> Vec<u64> {
        let Some(start_at) = self.surface_order.iter().position(|seq| *seq == start) else {
            return Vec::new();
        };
        let Some(end_at) = self.surface_order.iter().position(|seq| *seq == end) else {
            return Vec::new();
        };
        let (left, right) = if start_at <= end_at {
            (start_at, end_at)
        } else {
            (end_at, start_at)
        };
        self.surface_order[left..=right].to_vec()
    }

    pub fn record_surface_seq(&mut self, seq: u64) {
        if !self.surface_order.contains(&seq) {
            self.surface_order.push(seq);
        }
    }

    pub fn record_surface_owner(&mut self, seq: u64, display_index: usize, replaced: bool) {
        if replaced {
            let at = self
                .pending_surface_insert_at
                .take()
                .unwrap_or(self.surface_order.len());
            self.surface_order
                .insert(at.min(self.surface_order.len()), seq);
        } else if !self.surface_order.contains(&seq) {
            self.surface_order.push(seq);
        }
        self.surface_owners.insert(seq, display_index);
    }

    pub fn remove_display_index(&mut self, removed: usize) {
        self.surface_owners.retain(|_, index| *index != removed);
        self.display_positions.retain(|_, index| *index != removed);
        for index in self.surface_owners.values_mut() {
            if *index > removed {
                *index -= 1;
            }
        }
        for index in self.display_positions.values_mut() {
            if *index > removed {
                *index -= 1;
            }
        }
    }

    pub fn record_display_position(&mut self, id: DisplayId, index: usize) {
        self.display_positions.insert(id, index);
    }

    pub fn display_position(&self, id: &DisplayId) -> Option<usize> {
        self.display_positions.get(id).copied()
    }

    pub fn display_ids(&self) -> HashSet<DisplayId> {
        self.display_positions.keys().cloned().collect()
    }

    pub fn shift_selected_displays(&mut self, ids: &HashSet<DisplayId>, delta: usize) {
        for (id, index) in &mut self.display_positions {
            if ids.contains(id) {
                *index += delta;
            }
        }
    }

    pub fn owner(&self, seq: u64) -> Option<usize> {
        self.surface_owners.get(&seq).copied()
    }

    pub fn owned_seqs(&self) -> HashSet<u64> {
        self.surface_owners.keys().copied().collect()
    }

    pub fn shift_selected_owners(&mut self, seqs: &HashSet<u64>, delta: usize) {
        for (seq, index) in &mut self.surface_owners {
            if seqs.contains(seq) {
                *index += delta;
            }
        }
    }

    pub(crate) fn remember_tool_result(&mut self, call_id: String, result: PendingToolResult) {
        self.pending_tool_results.insert(call_id, result);
    }

    pub(crate) fn take_tool_result(&mut self, call_id: &str) -> Option<PendingToolResult> {
        self.pending_tool_results.remove(call_id)
    }

    pub(crate) fn remember_activity_result(
        &mut self,
        id: DisplayId,
        result: PendingActivityResult,
    ) {
        self.pending_activity_results.insert(id, result);
    }

    pub(crate) fn take_activity_result(&mut self, id: &DisplayId) -> Option<PendingActivityResult> {
        self.pending_activity_results.remove(id)
    }

    pub(crate) fn remember_activity_enrichment(
        &mut self,
        id: DisplayId,
        enrichment: PendingActivityEnrichment,
    ) {
        self.pending_activity_enrichments.insert(id, enrichment);
    }

    pub(crate) fn take_activity_enrichments(
        &mut self,
    ) -> Vec<(DisplayId, PendingActivityEnrichment)> {
        self.pending_activity_enrichments.drain().collect()
    }

    pub fn is_shadowed(&self, seq: u64) -> bool {
        self.shadowed_surface_seqs.contains(&seq)
    }
}

pub fn is_surface_node(kind: &HostEventKind) -> bool {
    matches!(
        kind,
        HostEventKind::UserMessage { .. }
            | HostEventKind::AssistantMessage { .. }
            | HostEventKind::ToolResult { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replacement_removes_known_owners_and_suppresses_late_history() {
        let mut projector = EventProjector::default();
        for (seq, index) in [(2, 0), (3, 1), (7, 2), (9, 3)] {
            projector.record_surface_owner(seq, index, false);
        }
        let replacement = HostEvent::from_value(json!({
            "seq": 14, "time": 100, "type": "user/message",
            "surfaceOp": {"op":"replace","start":2,"end":9},
            "sourceEventSeqs": [2,3,7,9],
            "data": {"content":[{"type":"text","text":"summary"}],"source":{"kind":"plugin"}}
        }));
        let effects = projector.effects(&replacement);
        assert!(
            matches!(effects[0], ProjectionEffect::SurfaceMutation { ref remove_indices, insert_at: Some(0) } if remove_indices == &[0,1,2,3])
        );
        assert!(projector.is_shadowed(3));
        let older = HostEvent::from_value(json!({
            "seq": 3, "type":"assistant/message", "surfaceOp":"append",
            "data":{"message":{"content":[{"type":"text","text":"old"}]}}
        }));
        assert_eq!(projector.effects(&older), vec![ProjectionEffect::Ignore]);
    }

    #[test]
    fn unknown_append_has_fallback_but_unknown_replace_fails() {
        let mut projector = EventProjector::default();
        let append = HostEvent::from_value(
            json!({"seq":1,"type":"future/event","surfaceOp":"append","data":{}}),
        );
        assert!(matches!(
            projector.effects(&append)[0],
            ProjectionEffect::Display(_)
        ));
        let replacement = HostEvent::from_value(
            json!({"seq":2,"type":"future/event","surfaceOp":{"op":"replace","start":1,"end":1},"sourceEventSeqs":[1],"data":{}}),
        );
        assert!(matches!(
            projector.effects(&replacement)[0],
            ProjectionEffect::CompatibilityError(_)
        ));
        let malformed = HostEvent::from_value(
            json!({"seq":3,"type":"future/event","surfaceOp":{"op":"replace"},"data":{}}),
        );
        assert!(matches!(
            projector.effects(&malformed)[0],
            ProjectionEffect::CompatibilityError(_)
        ));
    }
}
