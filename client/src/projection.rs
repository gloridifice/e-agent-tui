//! Typed DSH event classification and surface ownership.

use std::collections::{HashMap, HashSet};

use crate::display::{DisplayId, DisplayItem};

mod activity;
pub(crate) mod assistant;
pub(crate) mod command;
pub(crate) mod lifecycle;
pub(crate) mod retry;
mod store;
mod surface;
pub(crate) mod tool;
pub(crate) mod workflow;

pub(crate) use activity::{
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

#[derive(Debug, Default)]
pub struct EventProjector {
    pub(super) shadowed_surface_seqs: HashSet<u64>,
    pub(super) surface_order: Vec<u64>,
    pub(super) surface_owners: HashMap<u64, usize>,
    pub(super) pending_surface_insert_at: Option<usize>,
    pub tool_calls: HashMap<String, DisplayId>,
    pub commands: HashMap<String, DisplayId>,
    pub retries: HashMap<String, DisplayId>,
    pub compactions: HashMap<String, DisplayId>,
    pub nested_calls: HashMap<String, DisplayId>,
    pub workflows: HashMap<String, DisplayId>,
    pub tool_family: tool::ToolProjectionState,
    pub(super) pending_tool_results: HashMap<String, PendingToolResult>,
    pub(super) pending_activity_results: HashMap<DisplayId, PendingActivityResult>,
    pub(super) pending_activity_enrichments: HashMap<DisplayId, PendingActivityEnrichment>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::HostEvent;
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
