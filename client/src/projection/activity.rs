use crate::display::{ActivityRow, ActivityState, DisplayId};

use super::EventProjector;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivityMutation {
    Upsert(ActivityRow),
    Settle {
        id: DisplayId,
        state: ActivityState,
        summary: Option<String>,
    },
    Enrich {
        id: DisplayId,
        summary: String,
        start_ms: Option<u64>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingActivityEnrichment {
    pub summary: String,
    pub start_ms: Option<u64>,
}

impl EventProjector {
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
}
