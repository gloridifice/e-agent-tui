use std::collections::HashSet;

use crate::{
    display::{DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat},
    protocol::{HostEvent, HostEventKind, HostSurfaceOp},
};

use super::{AccessoryStateEffect, EventProjector, PageStateEffect, ProjectionEffect};

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
        for index in self.surface_owners.values_mut() {
            if *index > removed {
                *index -= 1;
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
