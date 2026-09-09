use crate::display::{ActivityRow, ActivityState, DisplayId};

use crate::preview::{MutationDiff, MutationHunk};

use super::EventProjector;

#[derive(Debug, Clone)]
pub struct PendingToolResult {
    pub output: String,
    pub is_error: bool,
    pub output_truncated: bool,
    pub execution_metrics: Option<crate::agent::timeline::ToolExecutionMetrics>,
    pub time_ms: u64,
    pub surface_seq: Option<u64>,
    pub mutation_diff: Option<MutationDiff>,
    pub mutation_hunks: Vec<MutationHunk>,
}

#[derive(Debug, Clone)]
pub struct PendingActivityResult {
    pub label: Option<String>,
    pub state: ActivityState,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityMutation {
    Upsert(ActivityRow),
    Settle {
        id: DisplayId,
        label: Option<String>,
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
pub struct PendingActivityEnrichment {
    pub summary: String,
    pub start_ms: Option<u64>,
}

impl EventProjector {
    pub fn remember_tool_result(&mut self, call_id: String, result: PendingToolResult) {
        self.pending_tool_results.insert(call_id, result);
    }

    pub fn take_tool_result(&mut self, call_id: &str) -> Option<PendingToolResult> {
        self.pending_tool_results.remove(call_id)
    }

    pub fn remember_activity_result(&mut self, id: DisplayId, result: PendingActivityResult) {
        self.pending_activity_results.insert(id, result);
    }

    pub fn take_activity_result(&mut self, id: &DisplayId) -> Option<PendingActivityResult> {
        self.pending_activity_results.remove(id)
    }

    pub fn remember_activity_enrichment(
        &mut self,
        id: DisplayId,
        enrichment: PendingActivityEnrichment,
    ) {
        self.pending_activity_enrichments.insert(id, enrichment);
    }

    pub fn take_activity_enrichments(&mut self) -> Vec<(DisplayId, PendingActivityEnrichment)> {
        self.pending_activity_enrichments.drain().collect()
    }
}
