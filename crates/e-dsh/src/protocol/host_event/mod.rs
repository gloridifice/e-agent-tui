//! Typed bounded DSH HostEvent parser façade.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

mod assistant;
mod content;
mod lifecycle;
mod tool;
mod workflow;

/// Typed anti-corruption view of one DSH session event. The original JSON is
/// retained only for lossless serde/debug compatibility; application state
/// consumes `kind`, so host shape changes are isolated to this parser.
#[derive(Debug, Clone)]
pub struct HostEvent {
    pub seq: Option<u64>,
    pub time_ms: Option<u64>,
    pub surface_op: Option<HostSurfaceOp>,
    pub surface_op_invalid: bool,
    pub source_event_seqs: Vec<u64>,
    pub kind: HostEventKind,
    raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSurfaceOp {
    Append,
    Replace { start: u64, end: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostContentBlock {
    Text(String),
    Reasoning(String),
    Image { label: String },
    Other { block_type: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostMessageSource {
    pub kind: Option<String>,
    pub form: Option<String>,
    pub summary: Option<String>,
    pub producer: Option<String>,
}

/// One DSH-supplied mutation fragment narrowed from opaque result `meta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostMutationHunk {
    pub path: Option<String>,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostLifecycleOutcome {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostEventKind {
    UserMessage {
        text: String,
        source_kind: Option<String>,
        content: Vec<HostContentBlock>,
        source: HostMessageSource,
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
        content: Vec<HostContentBlock>,
        turn: Option<u64>,
        step: Option<u64>,
        usage: Option<TokenUsage>,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
        output_truncated: bool,
        mutation_hunks: Vec<HostMutationHunk>,
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
    LlmRetry {
        retry_id: String,
        retry: u64,
        max_retries: Option<u64>,
        delay_ms: u64,
        message: String,
    },
    LlmRetryStarted {
        retry_id: String,
        retry: u64,
    },
    CommandRun {
        command_id: String,
        name: String,
        args: Option<String>,
    },
    CommandDone {
        command_id: String,
        success: bool,
        text: Option<String>,
    },
    CodeDispatchStart {
        root_call_id: String,
        parent_call_id: String,
        sub_call_id: String,
        name: String,
        arguments: String,
    },
    CodeDispatchEnd {
        sub_call_id: String,
        is_error: bool,
    },
    WorkflowRunStart {
        run_id: String,
        name: String,
    },
    WorkflowAgentStart {
        run_id: String,
        member_seq: u64,
        label: String,
    },
    WorkflowAgentEnd {
        run_id: String,
        member_seq: u64,
        outcome: HostLifecycleOutcome,
    },
    WorkflowRunEnd {
        run_id: String,
        outcome: HostLifecycleOutcome,
    },
    CompactionStart {
        compaction_id: String,
    },
    CompactionSummary {
        compaction_id: String,
        summary: String,
    },
    CompactionEnd {
        compaction_id: String,
        error: Option<String>,
    },
    GoalChange {
        summary: String,
    },
    PlanMode {
        mode: String,
    },
    AgentPresetSelected {
        preset: String,
    },
    SessionState {
        event_type: String,
    },
    AuditOnly {
        event_type: String,
    },
    Unknown {
        event_type: Option<String>,
    },
}

impl HostEventKind {
    pub fn is_surface(&self) -> bool {
        matches!(
            self,
            Self::UserMessage { .. }
                | Self::AssistantMessage { .. }
                | Self::ToolCall { .. }
                | Self::ToolResult { .. }
                | Self::TurnStart
                | Self::TurnEnd { .. }
                | Self::TodoWrite { .. }
                | Self::LlmRetry { .. }
                | Self::LlmRetryStarted { .. }
                | Self::CommandRun { .. }
                | Self::CommandDone { .. }
                | Self::CodeDispatchStart { .. }
                | Self::CodeDispatchEnd { .. }
                | Self::WorkflowRunStart { .. }
                | Self::WorkflowAgentStart { .. }
                | Self::WorkflowAgentEnd { .. }
                | Self::WorkflowRunEnd { .. }
                | Self::CompactionStart { .. }
                | Self::CompactionSummary { .. }
                | Self::CompactionEnd { .. }
                | Self::GoalChange { .. }
                | Self::PlanMode { .. }
                | Self::AgentPresetSelected { .. }
                | Self::SessionState { .. }
        )
    }
}

impl HostEvent {
    /// Transitional constructor used by the DSH anti-corruption adapter while
    /// legacy projection ownership remains in `e-dsh`.
    pub(crate) fn from_normalized_parts(
        seq: Option<u64>,
        time_ms: Option<u64>,
        surface_op: Option<HostSurfaceOp>,
        surface_op_invalid: bool,
        source_event_seqs: Vec<u64>,
        kind: HostEventKind,
    ) -> Self {
        Self {
            seq,
            time_ms,
            surface_op,
            surface_op_invalid,
            source_event_seqs,
            kind,
            raw: Value::Null,
        }
    }

    pub fn from_value(raw: Value) -> Self {
        let seq = raw.get("seq").and_then(Value::as_u64);
        let data = raw.get("data").unwrap_or(&Value::Null);
        // DSH timestamps are event-level. Keep the legacy data.time fallback
        // for old bridge fixtures while preferring the canonical shape.
        let time_ms = raw
            .get("time")
            .and_then(Value::as_u64)
            .or_else(|| data.get("time").and_then(Value::as_u64));
        let surface_op_value = raw.get("surfaceOp");
        let surface_op = match surface_op_value {
            Some(Value::String(op)) if op == "append" => Some(HostSurfaceOp::Append),
            Some(Value::Object(op)) if op.get("op").and_then(Value::as_str) == Some("replace") => {
                match (
                    op.get("start").and_then(Value::as_u64),
                    op.get("end").and_then(Value::as_u64),
                ) {
                    (Some(start), Some(end)) => Some(HostSurfaceOp::Replace { start, end }),
                    _ => None,
                }
            }
            _ => None,
        };
        let surface_op_invalid = surface_op_value.is_some() && surface_op.is_none();
        let source_event_seqs = raw
            .get("sourceEventSeqs")
            .and_then(Value::as_array)
            .map(|seqs| seqs.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        let event_type = raw.get("type").and_then(Value::as_str);
        let kind = match event_type {
            Some(event_type @ ("assistant/chunk" | "assistant/message")) => {
                assistant::parse(event_type, data)
            }
            Some(event_type @ ("tool/call" | "tool/result")) => tool::parse(event_type, data),
            Some(event_type @ ("turn/start" | "step/start" | "step/end" | "turn/end")) => {
                lifecycle::parse(event_type, data)
            }
            Some(
                event_type @ ("llm/retry" | "llm/retry-started" | "command/run" | "command/done"),
            ) => lifecycle::parse(event_type, data),
            Some(event_type @ ("tool/code-dispatch-start" | "tool/code-dispatch")) => {
                tool::parse(event_type, data)
            }
            Some(event_type) if event_type.starts_with("tool-workflow/") => {
                workflow::parse(event_type, data)
            }
            Some(event_type @ ("compaction/start" | "compaction/summary" | "compaction/end")) => {
                lifecycle::parse(event_type, data)
            }
            other => content::parse(other, data),
        };
        Self {
            seq,
            time_ms,
            surface_op,
            surface_op_invalid,
            source_event_seqs,
            kind,
            raw,
        }
    }

    /// Whether this event must survive snapshot/history replay. Known
    /// display/accessory families are listed by kind; unknown surface
    /// operations stay replayable so newer DSH versions degrade visibly.
    pub fn is_replay_relevant(&self) -> bool {
        self.kind.is_surface() || self.surface_op.is_some() || self.surface_op_invalid
    }

    pub fn as_value(&self) -> &Value {
        &self.raw
    }
}

impl<'de> Deserialize<'de> for HostEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(Self::from_value)
    }
}

impl Serialize for HostEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.raw.serialize(serializer)
    }
}
