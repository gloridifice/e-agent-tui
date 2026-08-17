//! Wire protocol between the dsh-tui client and the DSH bridge plugin.
//!
//! JSON messages share a `type` tag; field names are camelCase on the wire
//! (matching the bridge's JavaScript objects). Session events pass through
//! as opaque `serde_json::Value` for now; typed event views land with the
//! renderer (M2/M3).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/wire_contract.rs"));

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

fn parse_content(content: Option<&Value>) -> Vec<HostContentBlock> {
    content
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .map(|block| match block.get("type").and_then(Value::as_str) {
                    Some("text") => HostContentBlock::Text(
                        block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    ),
                    Some("reasoning") => HostContentBlock::Reasoning(
                        block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    ),
                    Some("image") => HostContentBlock::Image {
                        label: block
                            .get("attachment")
                            .and_then(|attachment| {
                                attachment.get("name").or_else(|| attachment.get("id"))
                            })
                            .and_then(Value::as_str)
                            .unwrap_or("image")
                            .to_owned(),
                    },
                    Some(block_type) => HostContentBlock::Other {
                        block_type: block_type.to_owned(),
                    },
                    None => HostContentBlock::Other {
                        block_type: "unknown".into(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let usage = value?;
    Some(TokenUsage {
        input_tokens: usage
            .get("inputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("outputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: usage
            .get("cacheReadTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write_tokens: usage
            .get("cacheWriteTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

fn content_text(content: &[HostContentBlock], reasoning: bool) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            HostContentBlock::Text(text) if !reasoning => Some(text.as_str()),
            HostContentBlock::Reasoning(text) if reasoning => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

impl HostEvent {
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
            Some("user/message") => {
                let content = parse_content(data.get("content"));
                let source_value = data.get("source").unwrap_or(&Value::Null);
                let source_kind = source_value
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                HostEventKind::UserMessage {
                    text: content_text(&content, false),
                    source_kind: source_kind.clone(),
                    content,
                    source: HostMessageSource {
                        kind: source_kind,
                        form: source_value
                            .get("form")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        summary: source_value
                            .get("summary")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        producer: source_value
                            .get("plugin")
                            .or_else(|| source_value.get("provider"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    },
                }
            }
            Some("assistant/chunk") => {
                let chunk = data.get("chunk").unwrap_or(&Value::Null);
                let chunk_type = chunk.get("type").and_then(Value::as_str);
                HostEventKind::AssistantChunk {
                    text: if chunk_type == Some("text-delta") {
                        chunk
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned()
                    } else {
                        String::new()
                    },
                    reasoning: if chunk_type == Some("reasoning-delta") {
                        chunk
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned()
                    } else {
                        String::new()
                    },
                    turn: data.get("turn").and_then(Value::as_u64),
                    step: data.get("step").and_then(Value::as_u64),
                    usage: (chunk_type == Some("usage"))
                        .then(|| parse_usage(chunk.get("usage")))
                        .flatten(),
                }
            }
            Some("assistant/message") => {
                let content = parse_content(
                    data.get("message")
                        .and_then(|message| message.get("content")),
                );
                HostEventKind::AssistantMessage {
                    text: content_text(&content, false),
                    reasoning: content_text(&content, true),
                    content,
                    turn: data.get("turn").and_then(Value::as_u64),
                    step: data.get("step").and_then(Value::as_u64),
                    usage: parse_usage(data.get("usage")),
                }
            }
            Some("tool/call") => HostEventKind::ToolCall {
                call_id: data
                    .get("callId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                name: data
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_owned(),
                arguments: data
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            },
            Some("tool/result") => {
                let result = data
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(Value::as_array)
                    .and_then(|blocks| blocks.first());
                HostEventKind::ToolResult {
                    call_id: result
                        .and_then(|block| block.get("toolCallId"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    output: content_text(
                        &parse_content(result.and_then(|block| block.get("content"))),
                        false,
                    ),
                    is_error: data.get("error").is_some()
                        || result
                            .and_then(|block| block.get("isError"))
                            .and_then(Value::as_bool)
                            == Some(true),
                    output_truncated: data.get("dshTuiOutputTrimmed").and_then(Value::as_bool)
                        == Some(true),
                }
            }
            Some("turn/start") => HostEventKind::TurnStart,
            Some("step/start") => HostEventKind::StepStart {
                turn: data.get("turn").and_then(Value::as_u64),
                step: data.get("step").and_then(Value::as_u64),
            },
            Some("step/end") => HostEventKind::StepEnd {
                turn: data.get("turn").and_then(Value::as_u64),
                step: data.get("step").and_then(Value::as_u64),
            },
            Some("turn/end") => {
                let reason_value = data.get("reason").unwrap_or(&Value::Null);
                let error = reason_value.get("error").unwrap_or(&Value::Null);
                HostEventKind::TurnEnd {
                    reason: reason_value
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    error_message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    error_code: error.get("code").and_then(Value::as_str).map(str::to_owned),
                }
            }
            Some("session/title") => HostEventKind::SessionTitle {
                title: data.get("title").and_then(Value::as_str).map(str::to_owned),
            },
            Some("todo/write") => HostEventKind::TodoWrite {
                todos: data
                    .get("todos")
                    .and_then(Value::as_array)
                    .map(|todos| {
                        todos
                            .iter()
                            .filter_map(|todo| {
                                Some((
                                    todo.get("content")?.as_str()?.to_owned(),
                                    todo.get("status")?.as_str()?.to_owned(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            Some("llm/retry") => HostEventKind::LlmRetry {
                retry_id: data
                    .get("retryId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                retry: data.get("retry").and_then(Value::as_u64).unwrap_or(0),
                max_retries: data.get("maxRetries").and_then(Value::as_u64),
                delay_ms: data.get("delayMs").and_then(Value::as_u64).unwrap_or(0),
                message: data
                    .get("failure")
                    .and_then(|failure| failure.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(160)
                    .collect(),
            },
            Some("llm/retry-started") => HostEventKind::LlmRetryStarted {
                retry_id: data
                    .get("retryId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                retry: data.get("retry").and_then(Value::as_u64).unwrap_or(0),
            },
            Some("command/run") => HostEventKind::CommandRun {
                command_id: data
                    .get("commandId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                name: data
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("command")
                    .to_owned(),
                args: data.get("args").and_then(Value::as_str).map(str::to_owned),
            },
            Some("command/done") => HostEventKind::CommandDone {
                command_id: data
                    .get("commandId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                success: data.get("kind").and_then(Value::as_str) == Some("success"),
                text: data
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| text.chars().take(200).collect()),
            },
            Some("tool/code-dispatch-start") => HostEventKind::CodeDispatchStart {
                root_call_id: data
                    .get("rootCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                parent_call_id: data
                    .get("parentCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                sub_call_id: data
                    .get("subCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                name: data
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_owned(),
                arguments: data
                    .get("arguments")
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            },
            Some("tool/code-dispatch") => HostEventKind::CodeDispatchEnd {
                sub_call_id: data
                    .get("subCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                is_error: data
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            },
            Some("tool-workflow/run-start") => HostEventKind::WorkflowRunStart {
                run_id: data
                    .get("runId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                name: data
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("workflow")
                    .to_owned(),
            },
            Some("tool-workflow/agent-start") => HostEventKind::WorkflowAgentStart {
                run_id: data
                    .get("runId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                member_seq: data.get("seq").and_then(Value::as_u64).unwrap_or(0),
                label: data
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("agent")
                    .to_owned(),
            },
            Some("tool-workflow/agent-end") => HostEventKind::WorkflowAgentEnd {
                run_id: data
                    .get("runId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                member_seq: data.get("seq").and_then(Value::as_u64).unwrap_or(0),
                outcome: match data.get("outcome").and_then(Value::as_str) {
                    Some("completed") => HostLifecycleOutcome::Success,
                    Some("cancelled") => HostLifecycleOutcome::Cancelled,
                    _ => HostLifecycleOutcome::Failure,
                },
            },
            Some("tool-workflow/run-end") => HostEventKind::WorkflowRunEnd {
                run_id: data
                    .get("runId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                outcome: match data.get("stopReason").and_then(Value::as_str) {
                    Some("completed") => HostLifecycleOutcome::Success,
                    Some("cancelled") => HostLifecycleOutcome::Cancelled,
                    _ => HostLifecycleOutcome::Failure,
                },
            },
            Some("compaction/start") => HostEventKind::CompactionStart {
                compaction_id: data
                    .get("compactionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            },
            Some("compaction/summary") => {
                let content = parse_content(data.get("summary"));
                HostEventKind::CompactionSummary {
                    compaction_id: data
                        .get("compactionId")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    summary: content_text(&content, false),
                }
            }
            Some("compaction/end") => HostEventKind::CompactionEnd {
                compaction_id: data
                    .get("compactionId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                error: data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(|error| error.chars().take(200).collect()),
            },
            Some("goal/change") => HostEventKind::GoalChange {
                summary: data
                    .get("goal")
                    .or_else(|| data.get("summary"))
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    })
                    .unwrap_or_default(),
            },
            Some("plan/mode") => HostEventKind::PlanMode {
                mode: data
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            },
            Some("agent-preset/selected") => HostEventKind::AgentPresetSelected {
                preset: data
                    .get("agentPreset")
                    .or_else(|| data.get("preset"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            },
            Some(
                event_type @ ("request/context" | "permission/preset" | "sandbox/mode"
                | "schedule/change"),
            ) => HostEventKind::SessionState {
                event_type: event_type.to_owned(),
            },
            Some(
                event_type @ ("request/header"
                | "session/end-seed"
                | "subagent/descriptor"
                | "session/title-llm-request"
                | "web/deepseek-search-llm-request"
                | "approval/asked"
                | "approval/decided"
                | "approval/policy"
                | "feedback/record"
                | "agent/inbox/spliced"),
            ) => HostEventKind::AuditOnly {
                event_type: event_type.to_owned(),
            },
            _ => HostEventKind::Unknown {
                event_type: event_type.map(|event_type| event_type.chars().take(160).collect()),
            },
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

/// Client → bridge messages.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ClientMessage {
    /// Authenticate and optionally attach to a specific live session.
    Hello {
        token: String,
        /// A fresh process with no resume target opens a NEW session on the
        /// bridge (each process shows one session).
        #[serde(skip_serializing_if = "Option::is_none")]
        resume_session_id: Option<String>,
        /// The directory the TUI was launched from. The bridge opens new
        /// sessions in this workspace (falling back to the attached
        /// session's header cwd when absent).
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// The configured default agent-preset mode for the session created
        /// at startup (the bridge falls back to `standard` when stale).
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        /// Wire contract understood by this client. Older bridges ignore it;
        /// newer bridges reject only clients requiring a newer contract.
        protocol_version: u64,
    },
    /// Ordinary user message (enters the agent inbox).
    Input { text: String },
    /// Slash command line.
    Command { line: String },
    /// Interrupt the current turn.
    Interrupt,
    /// Switch the connection to another live session.
    Attach { session_id: String },
    /// Request the session list (for the picker).
    ListSessions,
    /// Answer one pending approval.
    ApprovalAnswer { id: String, allow: bool },
    /// Submit answers for one pending user-question batch.
    AnswerQuestions {
        rpc_id: String,
        answers: Vec<QuestionAnswer>,
    },
    /// Cancel one pending user-question batch (the host resolves the tool
    /// call as cancelled).
    CancelQuestions { rpc_id: String },
    /// Request older history: surface events with seq < `before_seq`,
    /// newest first from the stored log (lazy scroll-back paging).
    History { before_seq: u64, limit: usize },
    /// Read the login page state (providers / proxies).
    LoginGet,
    /// Store one provider's API key (empty clears it; the value itself is
    /// never read back — only its configured/source/hint view).
    LoginSetApiKey { provider: String, value: String },
    /// Create a custom proxy provider route.
    LoginProxyCreate {
        base_url: String,
        api_key: String,
        protocol: String,
        model: String,
    },
    /// Remove one custom proxy provider route.
    LoginProxyDelete { id: String },
    /// Request the provider/model catalog (for the `/model` picker).
    ModelGet,
    /// Select the provider/model for the attached session.
    ModelSet { provider: String, model: String },
    /// Keepalive.
    Ping,
}

/// One question in an ask_user_question batch (as received from the host).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionItem {
    pub id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOption>>,
    #[serde(default)]
    pub multi_select: bool,
}

/// One selectable option of a question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One answered question: the selected option labels (and an optional free
/// text answer for questions that offered no options).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionAnswer {
    pub id: String,
    pub selected: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}

/// Bridge → client messages.
#[derive(Serialize, Deserialize, Debug)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    Welcome {
        #[serde(default)]
        protocol_version: Option<u64>,
        #[serde(default)]
        max_frame_bytes: Option<usize>,
        session_id: String,
        status: String,
        provider: Option<String>,
        model: Option<String>,
        /// Actual agent preset mounted for the attached session. Optional for
        /// compatibility with bridges that predate authoritative mode display.
        #[serde(default)]
        mode: Option<String>,
        /// Latest session/title of the attached session (absent until it
        /// has one; live updates ride ordinary `event` frames).
        #[serde(default)]
        title: Option<String>,
        /// Workspace path of the attached session (its header cwd), rendered
        /// right-aligned in the title row below the status bar.
        #[serde(default)]
        cwd: Option<String>,
    },
    Snapshot {
        events: Vec<HostEvent>,
        /// True when the bridge capped the replay window.
        #[serde(default)]
        truncated: bool,
    },
    Event {
        event: HostEvent,
    },
    Status {
        status: String,
    },
    /// One page of older history for the scroll-back request.
    History {
        events: Vec<HostEvent>,
        /// False when the returned page reaches the oldest stored event.
        #[serde(default)]
        has_more: bool,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    /// The agent-preset roster the host offers; feeds the `/new <mode>`
    /// suggestion popup. Sent after every `welcome` (attach/`/new`/picker).
    Presets {
        presets: Vec<PresetInfo>,
    },
    /// User-invocable skills visible in the attached session's cwd/scope;
    /// feeds `/skill:<name>` argument completion.
    Skills {
        #[serde(default)]
        skills: Vec<SkillInfo>,
    },
    /// Title of the attached session, fetched from the projection store
    /// when the log is cold (resumed sessions) and the welcome frame could
    /// not carry one.
    Title {
        title: String,
    },
    /// Login page state: the model providers (API-key entries) and the saved
    /// proxy routes. Secret values never cross the wire — only
    /// configured/source/hint views.
    Login {
        /// Providers that authenticate with an API key, in roster order.
        #[serde(default)]
        providers: Vec<ProviderInfo>,
        /// Custom proxy provider routes the user has added.
        #[serde(default)]
        proxies: Vec<ProxyInfo>,
        /// Message of the last rejected write (absent after a success).
        #[serde(default)]
        error: Option<String>,
    },
    Approval {
        id: String,
        tool_name: String,
        reason: String,
        call_id: Option<String>,
    },
    /// One pending user-question batch to answer in the TUI.
    Question {
        rpc_id: String,
        session_id: String,
        questions: Vec<QuestionItem>,
    },
    /// A question batch settled (answered anywhere, or cancelled) — the TUI
    /// must drop its pending selection UI for that batch.
    QuestionResolved {
        question_rpc_id: String,
        outcome: String,
    },
    /// Effective DSH/plugin command directory for the attached agent. The
    /// client merges this with its optimized built-ins; built-ins shadow a
    /// same-name descriptor.
    Commands {
        #[serde(default)]
        commands: Vec<CommandInfo>,
    },
    /// Direct UI outcome of a generically executed DSH/plugin command.
    CommandResult {
        command_id: String,
        kind: String,
        #[serde(default)]
        text: Option<String>,
    },
    /// The provider/model catalog (for the `/model` picker) plus the current
    /// selection.
    Model {
        #[serde(default)]
        providers: Vec<ModelProviderInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<ModelCurrent>,
    },
    Error {
        code: String,
        message: String,
    },
    Pong,
}

/// One session in the picker list.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub live: bool,
    pub created_at: u64,
}

/// One agent preset (a selectable `/new` mode) from the host roster.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u64>,
    /// Present when the preset's composition cannot mount — the client
    /// hides it from the mode popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken: Option<String>,
}

/// One user-invocable skill from the attached agent's effective registry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

/// One effective command discovered through DSH's `ctx.commands.list(agent)`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    /// Lowercase DSH command name without the leading slash.
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<CommandInputInfo>,
}

/// DSH currently exposes only an unstructured argument hint; it does not
/// expose a plugin argument-completion schema.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandInputInfo {
    pub hint: String,
}

/// One API-key model provider on the login page.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub api_key_configured: bool,
    #[serde(default)]
    pub api_key_writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_hint: Option<String>,
}

/// One custom proxy provider route.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInfo {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: String,
    pub model: String,
}

/// One model provider in the `/model` picker, with its model catalog.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
}

/// One selectable model in the `/model` picker.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The current provider/model selection.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelCurrent {
    pub provider: String,
    pub model: String,
}

impl ClientMessage {
    pub fn to_wire(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

impl ServerMessage {
    pub fn from_wire(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsh_event_fixture_covers_protocol_edges() {
        let events: Vec<Value> = serde_json::from_str(include_str!("../testdata/dsh-events.json"))
            .expect("bounded DSH fixture parses");
        assert!(events.len() <= 32, "fixture stays bounded");
        let types: std::collections::HashSet<&str> = events
            .iter()
            .filter_map(|event| event.get("type").and_then(Value::as_str))
            .collect();
        for required in [
            "user/message",
            "assistant/chunk",
            "assistant/message",
            "tool/result",
            "todo/write",
            "compaction/summary",
        ] {
            assert!(types.contains(required), "fixture contains {required}");
        }
        let replacement = events
            .iter()
            .find(|event| {
                event
                    .get("surfaceOp")
                    .and_then(|op| op.get("op"))
                    .and_then(Value::as_str)
                    == Some("replace")
            })
            .expect("fixture carries replace metadata");
        assert_eq!(replacement["time"], 1210);
        assert!(replacement["sourceEventSeqs"].is_array());

        let typed: Vec<HostEvent> = events.into_iter().map(HostEvent::from_value).collect();
        let reasoning = typed.iter().find(|event| matches!(event.kind, HostEventKind::AssistantChunk { ref reasoning, .. } if reasoning == "think")).expect("reasoning delta typed");
        assert_eq!(reasoning.time_ms, Some(1040));
        let context = typed
            .iter()
            .find_map(|event| match &event.kind {
                HostEventKind::UserMessage { source, .. }
                    if source.form.as_deref() == Some("instructions") =>
                {
                    Some(source)
                }
                _ => None,
            })
            .expect("context source typed");
        assert_eq!(context.producer.as_deref(), Some("instructions"));
        assert!(typed
            .iter()
            .any(|event| matches!(event.kind, HostEventKind::ToolResult { is_error: true, .. })));
        let replacement = typed
            .iter()
            .find(|event| matches!(event.surface_op, Some(HostSurfaceOp::Replace { .. })))
            .expect("replacement typed");
        assert_eq!(replacement.source_event_seqs, vec![2, 3, 7, 9]);
    }

    #[test]
    fn extended_history_fixture_enters_client_replay_roster() {
        let events: Vec<Value> = serde_json::from_str(include_str!(
            "../../bridge/test/fixtures/session-events.json"
        ))
        .unwrap();
        let typed: Vec<HostEvent> = events.into_iter().map(HostEvent::from_value).collect();
        for required in [
            "command/run",
            "compaction/summary",
            "llm/retry",
            "tool/code-dispatch",
            "tool-workflow/run-end",
            "todo/write",
        ] {
            let event = typed
                .iter()
                .find(|event| {
                    event.as_value().get("type").and_then(Value::as_str) == Some(required)
                })
                .unwrap();
            assert!(
                event.is_replay_relevant(),
                "{required} survives snapshot replay filtering"
            );
        }
        for audit in [
            "approval/asked",
            "request/header",
            "session/title-llm-request",
        ] {
            let event = typed
                .iter()
                .find(|event| event.as_value().get("type").and_then(Value::as_str) == Some(audit))
                .unwrap();
            assert!(!event.is_replay_relevant(), "{audit} remains audit-only");
        }
        assert!(typed.iter().any(|event| matches!(
            event.kind,
            HostEventKind::WorkflowAgentEnd {
                outcome: HostLifecycleOutcome::Success,
                ..
            }
        )));
        assert!(typed.iter().any(|event| matches!(
            event.kind,
            HostEventKind::WorkflowRunEnd {
                outcome: HostLifecycleOutcome::Success,
                ..
            }
        )));
    }

    #[test]
    fn host_events_are_typed_at_the_wire_boundary() {
        let message = ServerMessage::from_wire(
            r#"{"type":"event","event":{"seq":7,"type":"tool/call","data":{"callId":"c1","name":"read","arguments":"{\"file_path\":\"a.rs\"}","time":42}}}"#,
        )
        .expect("typed event parses");
        match message {
            ServerMessage::Event { event } => {
                assert_eq!(event.seq, Some(7));
                assert_eq!(event.time_ms, Some(42));
                assert_eq!(
                    event.kind,
                    HostEventKind::ToolCall {
                        call_id: "c1".into(),
                        name: "read".into(),
                        arguments: r#"{"file_path":"a.rs"}"#.into(),
                    }
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn unknown_host_event_is_preserved_without_entering_the_model_schema() {
        let event = HostEvent::from_value(serde_json::json!({
            "seq": 9,
            "type": "future/event",
            "data": { "new": true }
        }));
        assert!(matches!(
            event.kind,
            HostEventKind::Unknown { event_type: Some(ref kind) } if kind == "future/event"
        ));
        assert_eq!(event.as_value()["data"]["new"], true);
    }

    #[test]
    fn question_frame_parses_camel_case() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"question","rpcId":"r1","sessionId":"s1","questions":[{"id":"q1","question":"选哪个?","header":"Choose","options":[{"label":"A","description":"选项 A"}],"multiSelect":false}]}"#,
        )
        .expect("question parses");
        match msg {
            ServerMessage::Question {
                rpc_id,
                session_id,
                questions,
            } => {
                assert_eq!(rpc_id, "r1");
                assert_eq!(session_id, "s1");
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].header.as_deref(), Some("Choose"));
                let options = questions[0].options.as_ref().unwrap();
                assert_eq!(options[0].label, "A");
                assert_eq!(options[0].description.as_deref(), Some("选项 A"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn question_resolved_parses() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"question-resolved","questionRpcId":"r1","outcome":"answered"}"#,
        )
        .expect("resolved parses");
        assert!(matches!(
            msg,
            ServerMessage::QuestionResolved { question_rpc_id, outcome }
                if question_rpc_id == "r1" && outcome == "answered"
        ));
    }

    #[test]
    fn answer_questions_serializes_for_the_bridge() {
        let msg = ClientMessage::AnswerQuestions {
            rpc_id: "r1".into(),
            answers: vec![
                QuestionAnswer {
                    id: "q1".into(),
                    selected: vec!["A".into()],
                    custom: None,
                },
                QuestionAnswer {
                    id: "q2".into(),
                    selected: vec![],
                    custom: Some("自由".into()),
                },
            ],
        };
        let wire = msg.to_wire().unwrap();
        let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(value["type"], "answer-questions");
        assert_eq!(value["rpcId"], "r1");
        assert_eq!(value["answers"][0]["selected"][0], "A");
        assert!(
            value["answers"][0].get("custom").is_none(),
            "absent custom is omitted"
        );
        assert_eq!(value["answers"][1]["custom"], "自由");
        let cancel = ClientMessage::CancelQuestions {
            rpc_id: "r1".into(),
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&cancel.to_wire().unwrap()).unwrap()["type"],
            "cancel-questions"
        );
    }

    #[test]
    fn hello_carries_the_launch_cwd() {
        let msg = ClientMessage::Hello {
            token: "t".into(),
            resume_session_id: None,
            cwd: Some(r"D:\MyProjects\Chore\dsh".into()),
            mode: Some("standard".into()),
            protocol_version: WIRE_PROTOCOL_VERSION,
        };
        let value: serde_json::Value = serde_json::from_str(&msg.to_wire().unwrap()).unwrap();
        assert_eq!(value["type"], "hello");
        assert_eq!(value["cwd"], r"D:\MyProjects\Chore\dsh");
        assert_eq!(value["mode"], "standard", "the default mode rides hello");
        assert_eq!(value["protocolVersion"], WIRE_PROTOCOL_VERSION);
        let bare = ClientMessage::Hello {
            token: "t".into(),
            resume_session_id: Some("s1".into()),
            cwd: None,
            mode: None,
            protocol_version: WIRE_PROTOCOL_VERSION,
        };
        let bare_value: serde_json::Value = serde_json::from_str(&bare.to_wire().unwrap()).unwrap();
        assert!(
            bare_value.get("cwd").is_none(),
            "absent cwd is omitted so the old bridge keeps its fallback"
        );
        assert!(
            bare_value.get("mode").is_none(),
            "mode is omitted when attaching (nothing to create)"
        );
    }

    #[test]
    fn welcome_parses_mode_title_and_defaults_when_absent() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"welcome","sessionId":"s1","status":"idle","provider":"p","model":"m","mode":"cordis","title":"标题行"}"#,
        )
        .expect("welcome with mode and title parses");
        match msg {
            ServerMessage::Welcome {
                session_id,
                mode,
                title,
                ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(mode.as_deref(), Some("cordis"));
                assert_eq!(title.as_deref(), Some("标题行"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // An old bridge may send neither field — default to None, don't fail.
        let old =
            ServerMessage::from_wire(r#"{"type":"welcome","sessionId":"s2","status":"idle"}"#)
                .expect("old welcome parses");
        match old {
            ServerMessage::Welcome { mode, title, .. } => {
                assert_eq!(mode, None);
                assert_eq!(title, None);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn welcome_parses_cwd_and_defaults_when_absent() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"welcome","sessionId":"s1","status":"idle","cwd":"D:\\MyProjects\\Chore\\dsh"}"#,
        )
        .expect("welcome with cwd parses");
        match msg {
            ServerMessage::Welcome { cwd, .. } => {
                assert_eq!(cwd.as_deref(), Some(r"D:\MyProjects\Chore\dsh"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // The old bridge sends no cwd — default to None, don't fail.
        let old =
            ServerMessage::from_wire(r#"{"type":"welcome","sessionId":"s2","status":"idle"}"#)
                .expect("old welcome parses");
        match old {
            ServerMessage::Welcome { cwd, .. } => assert_eq!(cwd, None),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn presets_frame_parses_camel_case() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"presets","presets":[{"id":"standard","name":"标准模式","order":1},{"id":"minimal","name":"极简模式","description":"双工具编码","order":3},{"id":"mine","broken":"missing composition"}]}"#,
        )
        .expect("presets parses");
        match msg {
            ServerMessage::Presets { presets } => {
                assert_eq!(presets.len(), 3);
                assert_eq!(presets[0].id, "standard");
                assert_eq!(presets[0].name.as_deref(), Some("标准模式"));
                assert_eq!(presets[0].order, Some(1));
                assert_eq!(presets[1].description.as_deref(), Some("双工具编码"));
                assert!(presets[2].broken.is_some());
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn skills_frame_parses() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"skills","skills":[{"name":"code-review","description":"Review a change"}]}"#,
        )
        .expect("skills parses");
        match msg {
            ServerMessage::Skills { skills } => {
                assert_eq!(skills.len(), 1);
                assert_eq!(skills[0].name, "code-review");
                assert_eq!(skills[0].description, "Review a change");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn command_directory_and_result_frames_parse() {
        let directory = ServerMessage::from_wire(
            r#"{"type":"commands","commands":[{"name":"feedback","description":"record feedback","input":{"hint":"<text>"}}]}"#,
        )
        .expect("command directory parses");
        match directory {
            ServerMessage::Commands { commands } => {
                assert_eq!(commands[0].name, "feedback");
                assert_eq!(commands[0].input.as_ref().unwrap().hint, "<text>");
            }
            other => panic!("wrong variant: {other:?}"),
        }

        let result = ServerMessage::from_wire(
            r#"{"type":"command-result","commandId":"cmd-1","kind":"success","text":"done"}"#,
        )
        .expect("command result parses");
        match result {
            ServerMessage::CommandResult {
                command_id,
                kind,
                text,
            } => {
                assert_eq!(command_id, "cmd-1");
                assert_eq!(kind, "success");
                assert_eq!(text.as_deref(), Some("done"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn title_frame_parses() {
        let msg = ServerMessage::from_wire(r#"{"type":"title","title":"冷会话标题"}"#)
            .expect("title parses");
        match msg {
            ServerMessage::Title { title } => assert_eq!(title, "冷会话标题"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn login_frame_parses_providers_proxies() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"login","providers":[{"id":"deepseek","name":"DeepSeek","apiKeyConfigured":true,"apiKeyWritable":true,"apiKeyHint":"…1234"}],"proxies":[{"id":"proxy-1","name":"我的代理","baseUrl":"https://example.com/v1","protocol":"openai-completions","model":"gpt-4o"}]}"#,
        )
        .expect("login parses");
        match msg {
            ServerMessage::Login {
                providers,
                proxies,
                error,
            } => {
                assert_eq!(providers.len(), 1);
                assert!(providers[0].api_key_configured);
                assert_eq!(providers[0].api_key_hint.as_deref(), Some("…1234"));
                assert_eq!(proxies.len(), 1);
                assert_eq!(proxies[0].protocol, "openai-completions");
                assert_eq!(error, None);
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // A rejected write rides the same frame with `error`.
        let failed = ServerMessage::from_wire(
            r#"{"type":"login","providers":[],"proxies":[],"error":"credentials-local: bad value"}"#,
        )
        .expect("login error parses");
        match failed {
            ServerMessage::Login { error, .. } => assert!(error.is_some()),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn login_up_frames_serialize() {
        let set = ClientMessage::LoginSetApiKey {
            provider: "deepseek".into(),
            value: "sk-test".into(),
        };
        let v = serde_json::from_str::<serde_json::Value>(&set.to_wire().unwrap()).unwrap();
        assert_eq!(v["type"], "login-set-api-key");
        assert_eq!(v["provider"], "deepseek");
        assert_eq!(v["value"], "sk-test");
        let create = ClientMessage::LoginProxyCreate {
            base_url: "https://x/v1".into(),
            api_key: "k".into(),
            protocol: "openai-completions".into(),
            model: "m".into(),
        };
        let v = serde_json::from_str::<serde_json::Value>(&create.to_wire().unwrap()).unwrap();
        assert_eq!(v["type"], "login-proxy-create");
        assert_eq!(v["baseUrl"], "https://x/v1");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ClientMessage::LoginGet.to_wire().unwrap())
                .unwrap()["type"],
            "login-get"
        );
    }

    #[test]
    fn model_frame_parses_and_serializes() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"model","providers":[{"id":"deepseek","name":"DeepSeek","models":[{"id":"deepseek-v4-pro","name":"DeepSeek V4 Pro","description":"flagship"},{"id":"deepseek-v4","name":"DeepSeek V4"}]}],"current":{"provider":"deepseek","model":"deepseek-v4"}}"#,
        )
        .expect("model parses");
        match msg {
            ServerMessage::Model { providers, current } => {
                assert_eq!(providers.len(), 1);
                assert_eq!(providers[0].models.len(), 2);
                assert_eq!(
                    providers[0].models[0].description.as_deref(),
                    Some("flagship")
                );
                let cur = current.expect("current selection");
                assert_eq!(cur.provider, "deepseek");
                assert_eq!(cur.model, "deepseek-v4");
            }
            other => panic!("wrong variant: {other:?}"),
        }
        let set = ClientMessage::ModelSet {
            provider: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        };
        let v = serde_json::from_str::<serde_json::Value>(&set.to_wire().unwrap()).unwrap();
        assert_eq!(v["type"], "model-set");
        assert_eq!(v["provider"], "deepseek");
        assert_eq!(v["model"], "deepseek-v4-pro");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ClientMessage::ModelGet.to_wire().unwrap())
                .unwrap()["type"],
            "model-get"
        );
    }
}
