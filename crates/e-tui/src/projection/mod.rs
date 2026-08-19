//! Normalized agent-event classification and surface ownership.

use std::collections::{HashMap, HashSet};

use crate::display::{DisplayId, DisplayItem};

pub mod activity;
pub mod assistant;
pub mod command;
pub mod lifecycle;
pub mod retry;
mod store;
mod surface;
pub mod tool;
pub mod workflow;

pub use activity::{
    ActivityMutation, PendingActivityEnrichment, PendingActivityResult, PendingToolResult,
};
pub use store::{TranscriptNode, TranscriptStore};
pub use surface::is_surface_node;

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

/// Canonical owner for normalized timeline projection and transcript storage.
///
/// The legacy executable may temporarily forward through this value, but it
/// must not retain a second transcript or projection state.
#[derive(Debug, Default)]
pub struct TimelineModel {
    pub transcript: TranscriptStore,
    pub projector: EventProjector,
}

#[derive(Debug, Default)]
pub struct EventProjector {
    pub shadowed_surface_seqs: HashSet<u64>,
    pub surface_order: Vec<u64>,
    pub surface_owners: HashMap<u64, usize>,
    pub pending_surface_insert_at: Option<usize>,
    pub tool_calls: HashMap<String, DisplayId>,
    pub commands: HashMap<String, DisplayId>,
    pub retries: HashMap<String, DisplayId>,
    pub compactions: HashMap<String, DisplayId>,
    pub nested_calls: HashMap<String, DisplayId>,
    pub workflows: HashMap<String, DisplayId>,
    pub tool_family: tool::ToolProjectionState,
    pub pending_tool_results: HashMap<String, PendingToolResult>,
    pub pending_activity_results: HashMap<DisplayId, PendingActivityResult>,
    pub pending_activity_enrichments: HashMap<DisplayId, PendingActivityEnrichment>,
}

#[cfg(test)]
mod tests {
    use crate::agent::timeline::{SurfaceOperation, TimelineFact, TimelineRecord};

    use super::*;

    fn record(
        sequence: u64,
        surface: Option<SurfaceOperation>,
        fact: TimelineFact,
    ) -> TimelineRecord {
        TimelineRecord {
            sequence: Some(sequence),
            time_ms: Some(sequence * 10),
            surface,
            source_sequences: Vec::new(),
            fact,
        }
    }

    #[test]
    fn replacement_uses_canonical_surface_owners() {
        let mut projector = EventProjector::default();
        for (sequence, index) in [(2, 0), (3, 1), (7, 2), (9, 3)] {
            projector.record_surface_owner(sequence, index, false);
        }
        let replacement = record(
            14,
            Some(SurfaceOperation::Replace { start: 2, end: 9 }),
            TimelineFact::UserMessage {
                text: "summary".into(),
                source_kind: None,
                content: Vec::new(),
                source: Default::default(),
            },
        );
        assert!(matches!(
            projector.effects(&replacement).as_slice(),
            [ProjectionEffect::SurfaceMutation { remove_indices, insert_at: Some(0) }, ProjectionEffect::Reduce { insert_at: Some(0) }]
                if remove_indices == &[0, 1, 2, 3]
        ));
        assert!(projector.is_shadowed(3));
    }

    #[test]
    fn custom_append_is_bounded_and_custom_replace_is_rejected() {
        let custom = TimelineFact::Custom {
            namespace: "future".into(),
            kind: Some("event".into()),
            summary: None,
        };
        let mut projector = EventProjector::default();
        assert!(matches!(
            projector.effects(&record(1, Some(SurfaceOperation::Append), custom.clone()))[0],
            ProjectionEffect::Display(_)
        ));
        assert!(matches!(
            projector.effects(&record(
                2,
                Some(SurfaceOperation::Replace { start: 1, end: 1 }),
                custom,
            ))[0],
            ProjectionEffect::CompatibilityError(_)
        ));
    }
}
