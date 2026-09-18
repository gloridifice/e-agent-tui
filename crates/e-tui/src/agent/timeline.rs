//! Kernel-neutral timeline facts produced by an agent adapter.

use super::tool::ToolActivity;

use crate::preview::{MutationDiff, MutationHunk};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceOperation {
    Append,
    Replace { start: u64, end: u64 },
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
    Text(String),
    Reasoning(String),
    Image { label: String },
    Custom { namespace: String, kind: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageSource {
    pub kind: Option<String>,
    pub form: Option<String>,
    pub summary: Option<String>,
    pub producer: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOutcome {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolExecutionMetrics {
    pub duration_ms: Option<u64>,
    pub output_lines: Option<usize>,
    pub output_lines_truncated: bool,
    pub started_unix_ms: Option<u64>,
    pub ended_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimelineFact {
    UserMessage {
        text: String,
        source_kind: Option<String>,
        content: Vec<ContentBlock>,
        source: MessageSource,
    },
    AssistantChunk {
        text: String,
        reasoning: String,
        turn: Option<u64>,
        step: Option<u64>,
        usage: Option<TokenUsage>,
    },
    AssistantMessage {
        text: String,
        reasoning: String,
        content: Vec<ContentBlock>,
        turn: Option<u64>,
        step: Option<u64>,
        usage: Option<TokenUsage>,
    },
    /// Provider-reported price for the immediately following final assistant
    /// message. This is capture metadata and never a transcript surface.
    UsageCost {
        usd_nanos: u64,
    },
    ToolCall(ToolActivity),
    ToolResult {
        activity_id: String,
        output: String,
        state: super::tool::ActivityState,
        output_truncated: bool,
        /// Present only on native history/snapshot records. `Some` with an
        /// unknown duration prevents replay time from masquerading as zero.
        execution_metrics: Option<ToolExecutionMetrics>,
        /// Whether this result immediately begins a new model-thinking phase.
        /// Backends that emit the next model call as an explicit `TurnStart`
        /// leave this false so the lifecycle is not counted twice.
        starts_thinking: bool,
        /// Event-supplied unified mutation text (e.g. Pi `details.patch`).
        mutation_diff: Option<MutationDiff>,
        /// Event-supplied mutation fragments (e.g. DSH edit `meta.diffs`).
        mutation_hunks: Vec<MutationHunk>,
    },
    TurnStart,
    StepStart {
        turn: Option<u64>,
        step: Option<u64>,
    },
    StepEnd {
        turn: Option<u64>,
        step: Option<u64>,
    },
    TurnEnd {
        reason: Option<String>,
        error_message: Option<String>,
        error_code: Option<String>,
    },
    SessionTitle {
        title: Option<String>,
    },
    TodoWrite {
        todos: Vec<(String, String)>,
    },
    RetryScheduled {
        id: String,
        retry: u64,
        max_retries: Option<u64>,
        delay_ms: u64,
        message: String,
    },
    RetryStarted {
        id: String,
        retry: u64,
    },
    CommandStarted {
        id: String,
        name: String,
        args: Option<String>,
    },
    CommandFinished {
        id: String,
        success: bool,
        text: Option<String>,
    },
    SubagentStarted {
        root_id: String,
        parent_id: String,
        id: String,
        name: String,
        summary: String,
    },
    SubagentFinished {
        id: String,
        failed: bool,
    },
    WorkflowStarted {
        id: String,
        name: String,
    },
    WorkflowMemberStarted {
        workflow_id: String,
        sequence: u64,
        label: String,
    },
    WorkflowMemberFinished {
        workflow_id: String,
        sequence: u64,
        outcome: LifecycleOutcome,
    },
    WorkflowFinished {
        id: String,
        outcome: LifecycleOutcome,
    },
    CompactionStarted {
        id: String,
        model_name: Option<String>,
    },
    AutoCompactionStarted {
        id: String,
    },
    CompactionSummary {
        id: String,
        summary: String,
    },
    CompactionFinished {
        id: String,
        model_name: Option<String>,
        error: Option<String>,
    },
    AutoCompactionFinished {
        id: String,
        error: Option<String>,
    },
    GoalChanged {
        summary: String,
    },
    ModeChanged {
        mode: String,
    },
    PresetSelected {
        preset: String,
    },
    SessionState {
        state: String,
    },
    Audit {
        namespace: String,
        kind: String,
    },
    Custom {
        namespace: String,
        kind: Option<String>,
        summary: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRecord {
    pub sequence: Option<u64>,
    pub time_ms: Option<u64>,
    pub surface: Option<SurfaceOperation>,
    pub source_sequences: Vec<u64>,
    pub fact: TimelineFact,
}

impl TimelineRecord {
    pub fn is_replay_relevant(&self) -> bool {
        self.is_surface() || self.surface.is_some()
    }

    pub fn is_surface(&self) -> bool {
        matches!(
            self.fact,
            TimelineFact::UserMessage { .. }
                | TimelineFact::AssistantMessage { .. }
                | TimelineFact::ToolCall(_)
                | TimelineFact::ToolResult { .. }
                | TimelineFact::TurnStart
                | TimelineFact::TurnEnd { .. }
                | TimelineFact::TodoWrite { .. }
                | TimelineFact::RetryScheduled { .. }
                | TimelineFact::RetryStarted { .. }
                | TimelineFact::CommandStarted { .. }
                | TimelineFact::CommandFinished { .. }
                | TimelineFact::SubagentStarted { .. }
                | TimelineFact::SubagentFinished { .. }
                | TimelineFact::WorkflowStarted { .. }
                | TimelineFact::WorkflowMemberStarted { .. }
                | TimelineFact::WorkflowMemberFinished { .. }
                | TimelineFact::WorkflowFinished { .. }
                | TimelineFact::CompactionStarted { .. }
                | TimelineFact::AutoCompactionStarted { .. }
                | TimelineFact::CompactionSummary { .. }
                | TimelineFact::CompactionFinished { .. }
                | TimelineFact::AutoCompactionFinished { .. }
                | TimelineFact::GoalChanged { .. }
                | TimelineFact::ModeChanged { .. }
                | TimelineFact::PresetSelected { .. }
                | TimelineFact::SessionState { .. }
        )
    }
}
