//! Conversion between DSH wire values and kernel-neutral frontend contracts.

use e_tui::{
    action::{AgentRequest, QuestionAnswer as UiQuestionAnswer},
    agent::{
        timeline::{
            ContentBlock, LifecycleOutcome, MessageSource, SurfaceOperation, TimelineFact,
            TimelineRecord, TokenUsage,
        },
        tool::{ActivityState, ToolActivity, ToolCapability, ToolItem, ToolReference},
        AgentEvent, AgentStatus, AttachedSession, CatalogEvent, CommandDescriptor,
        CredentialProvider, InteractionEvent, ModelDescriptor, ModelProvider, ModelReasoning,
        ModelSelection, Preset, ProxyRoute, Question, QuestionOption, ReasoningEffort,
        SessionEvent, SessionSummary, Skill, TimelineEvent,
    },
    preview::{LineSelection, MutationHunk, ToolMetrics, ToolPreview, ToolPreviewPrimary},
};
use serde_json::Value;

use crate::protocol::{
    ClientMessage, CommandInfo, CommandInputInfo, HostContentBlock, HostEvent, HostEventKind,
    HostLifecycleOutcome, HostMessageSource, HostMutationHunk, HostSurfaceOp, ModelCurrent,
    ModelInfo, ModelProviderInfo, ModelReasoningEffortInfo, ModelReasoningInfo, PresetInfo,
    ProviderInfo, ProxyInfo, QuestionAnswer, QuestionItem, QuestionOption as WireQuestionOption,
    ServerMessage, SessionInfo, SkillInfo, TokenUsage as HostTokenUsage,
};

pub fn normalize_server_message(message: ServerMessage) -> AgentEvent {
    match message {
        ServerMessage::Welcome {
            protocol_version,
            max_frame_bytes,
            session_id,
            status,
            provider,
            model,
            mode,
            title,
            cwd,
        } => AgentEvent::Session(SessionEvent::Attached(AttachedSession {
            protocol_version,
            max_frame_bytes,
            id: session_id,
            status: normalize_status(status),
            provider,
            model,
            mode,
            title,
            workspace: cwd,
        })),
        ServerMessage::Snapshot { events, truncated } => {
            AgentEvent::Timeline(TimelineEvent::Snapshot {
                records: events.into_iter().map(normalize_host_event).collect(),
                truncated,
            })
        }
        ServerMessage::Event { event } => {
            AgentEvent::Timeline(TimelineEvent::Append(normalize_host_event(event)))
        }
        ServerMessage::Status { status } => {
            AgentEvent::Session(SessionEvent::Status(normalize_status(status)))
        }
        ServerMessage::History { events, has_more } => {
            AgentEvent::Timeline(TimelineEvent::History {
                records: events.into_iter().map(normalize_host_event).collect(),
                has_more,
            })
        }
        ServerMessage::Sessions {
            sessions,
            titles_pending,
        } => AgentEvent::Session(SessionEvent::List {
            sessions: sessions.into_iter().map(normalize_session).collect(),
            titles_pending,
        }),
        ServerMessage::Presets { presets } => AgentEvent::Catalog(CatalogEvent::Presets(
            presets
                .into_iter()
                .map(|preset| Preset {
                    id: preset.id,
                    name: preset.name,
                    description: preset.description,
                    order: preset.order,
                    unavailable_reason: preset.broken,
                })
                .collect(),
        )),
        ServerMessage::Skills { skills } => AgentEvent::Catalog(CatalogEvent::Skills(
            skills
                .into_iter()
                .map(|skill| Skill {
                    name: skill.name,
                    description: skill.description,
                })
                .collect(),
        )),
        ServerMessage::Title { title } => AgentEvent::Session(SessionEvent::Title(title)),
        ServerMessage::Commands { commands } => AgentEvent::Catalog(CatalogEvent::Commands(
            commands
                .into_iter()
                .map(|command| CommandDescriptor {
                    name: command.name,
                    description: command.description,
                    input_hint: command.input.map(|input| input.hint),
                })
                .collect(),
        )),
        ServerMessage::CommandResult {
            command_id,
            kind,
            text,
        } => AgentEvent::Interaction(InteractionEvent::CommandResult {
            id: command_id,
            outcome: kind,
            text,
        }),
        ServerMessage::Login {
            providers,
            proxies,
            error,
        } => AgentEvent::Catalog(CatalogEvent::Login {
            providers: providers.into_iter().map(normalize_provider).collect(),
            proxies: proxies.into_iter().map(normalize_proxy).collect(),
            error,
        }),
        ServerMessage::Model { providers, current } => AgentEvent::Catalog(CatalogEvent::Models {
            providers: providers
                .into_iter()
                .map(normalize_model_provider)
                .collect(),
            current: current.map(normalize_model_selection),
        }),
        ServerMessage::Approval {
            id,
            tool_name,
            reason,
            ..
        } => AgentEvent::Interaction(InteractionEvent::Approval {
            id,
            capability: normalize_capability(&tool_name, None),
            label: tool_name,
            reason,
        }),
        ServerMessage::Question {
            rpc_id,
            session_id,
            questions,
        } => AgentEvent::Interaction(InteractionEvent::Question {
            request_id: rpc_id,
            session_id,
            questions: questions.into_iter().map(normalize_question).collect(),
        }),
        ServerMessage::QuestionResolved {
            question_rpc_id,
            outcome,
        } => AgentEvent::Interaction(InteractionEvent::QuestionResolved {
            request_id: question_rpc_id,
            outcome,
        }),
        ServerMessage::Error { code, message } => {
            AgentEvent::Interaction(InteractionEvent::Error { code, message })
        }
        ServerMessage::Pong => AgentEvent::Interaction(InteractionEvent::Heartbeat),
    }
}

pub(crate) fn legacy_server_message(event: AgentEvent) -> Result<ServerMessage, AgentEvent> {
    Ok(match event {
        AgentEvent::Session(SessionEvent::Attached(session)) => ServerMessage::Welcome {
            protocol_version: session.protocol_version,
            max_frame_bytes: session.max_frame_bytes,
            session_id: session.id,
            status: legacy_status(session.status),
            provider: session.provider,
            model: session.model,
            mode: session.mode,
            title: session.title,
            cwd: session.workspace,
        },
        AgentEvent::Session(SessionEvent::Status(status)) => ServerMessage::Status {
            status: legacy_status(status),
        },
        AgentEvent::Session(SessionEvent::Title(title)) => ServerMessage::Title { title },
        AgentEvent::Session(SessionEvent::List {
            sessions,
            titles_pending,
        }) => ServerMessage::Sessions {
            sessions: sessions
                .into_iter()
                .map(|session| SessionInfo {
                    id: session.id,
                    title: session.title,
                    live: session.live,
                    created_at: session.created_at,
                })
                .collect(),
            titles_pending,
        },
        AgentEvent::Timeline(TimelineEvent::Snapshot { records, truncated }) => {
            ServerMessage::Snapshot {
                events: records.into_iter().map(legacy_host_event).collect(),
                truncated,
            }
        }
        AgentEvent::Timeline(TimelineEvent::Append(record)) => ServerMessage::Event {
            event: legacy_host_event(record),
        },
        AgentEvent::Timeline(TimelineEvent::History { records, has_more }) => {
            ServerMessage::History {
                events: records.into_iter().map(legacy_host_event).collect(),
                has_more,
            }
        }
        AgentEvent::Catalog(CatalogEvent::Presets(presets)) => ServerMessage::Presets {
            presets: presets
                .into_iter()
                .map(|preset| PresetInfo {
                    id: preset.id,
                    name: preset.name,
                    description: preset.description,
                    order: preset.order,
                    broken: preset.unavailable_reason,
                })
                .collect(),
        },
        AgentEvent::Catalog(CatalogEvent::Skills(skills)) => ServerMessage::Skills {
            skills: skills
                .into_iter()
                .map(|skill| SkillInfo {
                    name: skill.name,
                    description: skill.description,
                })
                .collect(),
        },
        AgentEvent::Catalog(CatalogEvent::Commands(commands)) => ServerMessage::Commands {
            commands: commands
                .into_iter()
                .map(|command| CommandInfo {
                    name: command.name,
                    description: command.description,
                    input: command.input_hint.map(|hint| CommandInputInfo { hint }),
                })
                .collect(),
        },
        AgentEvent::Catalog(CatalogEvent::Login {
            providers,
            proxies,
            error,
        }) => ServerMessage::Login {
            providers: providers
                .into_iter()
                .map(|provider| ProviderInfo {
                    id: provider.id,
                    name: provider.name,
                    api_key_configured: provider.api_key_configured,
                    api_key_writable: provider.api_key_writable,
                    api_key_source: provider.api_key_source,
                    api_key_hint: provider.api_key_hint,
                })
                .collect(),
            proxies: proxies
                .into_iter()
                .map(|proxy| ProxyInfo {
                    id: proxy.id,
                    name: proxy.name,
                    base_url: proxy.base_url,
                    protocol: proxy.protocol,
                    model: proxy.model,
                })
                .collect(),
            error,
        },
        AgentEvent::Catalog(CatalogEvent::Models { providers, current }) => ServerMessage::Model {
            providers: providers
                .into_iter()
                .map(|provider| ModelProviderInfo {
                    id: provider.id,
                    name: provider.name,
                    models: provider
                        .models
                        .into_iter()
                        .map(|model| ModelInfo {
                            id: model.id,
                            name: model.name,
                            description: model.description,
                            reasoning: model.reasoning.map(|reasoning| ModelReasoningInfo {
                                efforts: reasoning
                                    .efforts
                                    .into_iter()
                                    .map(|effort| ModelReasoningEffortInfo {
                                        id: effort.id,
                                        name: effort.name,
                                        description: effort.description,
                                    })
                                    .collect(),
                                default_effort: reasoning.default_effort,
                            }),
                        })
                        .collect(),
                })
                .collect(),
            current: current.map(|current| ModelCurrent {
                provider: current.provider,
                model: current.model,
                reasoning_effort: current.reasoning_effort,
            }),
        },
        AgentEvent::Interaction(InteractionEvent::CommandResult { id, outcome, text }) => {
            ServerMessage::CommandResult {
                command_id: id,
                kind: outcome,
                text,
            }
        }
        AgentEvent::Interaction(InteractionEvent::Approval {
            id, label, reason, ..
        }) => ServerMessage::Approval {
            id,
            tool_name: label,
            reason,
            call_id: None,
        },
        AgentEvent::Interaction(InteractionEvent::Question {
            request_id,
            session_id,
            questions,
        }) => ServerMessage::Question {
            rpc_id: request_id,
            session_id,
            questions: questions
                .into_iter()
                .map(|question| QuestionItem {
                    id: question.id,
                    question: question.question,
                    header: question.header,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(|option| WireQuestionOption {
                                label: option.label,
                                description: option.description,
                            })
                            .collect()
                    }),
                    multi_select: question.multi_select,
                })
                .collect(),
        },
        AgentEvent::Interaction(InteractionEvent::QuestionResolved {
            request_id,
            outcome,
        }) => ServerMessage::QuestionResolved {
            question_rpc_id: request_id,
            outcome,
        },
        AgentEvent::Interaction(InteractionEvent::Error { code, message }) => {
            ServerMessage::Error { code, message }
        }
        AgentEvent::Interaction(InteractionEvent::Heartbeat) => ServerMessage::Pong,
        other @ (AgentEvent::Interaction(InteractionEvent::SetEditorText { .. })
        | AgentEvent::Preview(_)
        | AgentEvent::EffectCompleted(_)
        | AgentEvent::Deadline(_)) => return Err(other),
    })
}

pub fn agent_request_to_client(request: AgentRequest) -> ClientMessage {
    match request {
        AgentRequest::Input { text } => ClientMessage::Input { text },
        AgentRequest::NewInput { mode, text } => ClientMessage::NewInput { mode, text },
        AgentRequest::Command { line } => ClientMessage::Command { line },
        AgentRequest::Interrupt => ClientMessage::Interrupt,
        AgentRequest::Attach { session_id } => ClientMessage::Attach { session_id },
        AgentRequest::ListSessions => ClientMessage::ListSessions,
        AgentRequest::ApprovalAnswer { id, allow } => ClientMessage::ApprovalAnswer { id, allow },
        AgentRequest::AnswerQuestions {
            request_id,
            answers,
        } => ClientMessage::AnswerQuestions {
            rpc_id: request_id,
            answers: answers.into_iter().map(question_answer_to_wire).collect(),
        },
        AgentRequest::CancelQuestions { request_id } => {
            ClientMessage::CancelQuestions { rpc_id: request_id }
        }
        AgentRequest::History {
            before_sequence,
            limit,
        } => ClientMessage::History {
            before_seq: before_sequence,
            limit,
        },
        AgentRequest::LoginGet => ClientMessage::LoginGet,
        AgentRequest::LoginSetApiKey { provider, value } => {
            ClientMessage::LoginSetApiKey { provider, value }
        }
        AgentRequest::LoginProxyCreate {
            base_url,
            api_key,
            protocol,
            model,
        } => ClientMessage::LoginProxyCreate {
            base_url,
            api_key,
            protocol,
            model,
        },
        AgentRequest::LoginProxyDelete { id } => ClientMessage::LoginProxyDelete { id },
        AgentRequest::ModelGet => ClientMessage::ModelGet,
        AgentRequest::ModelSet {
            provider,
            model,
            reasoning_effort,
        } => ClientMessage::ModelSet {
            provider,
            model,
            reasoning_effort,
        },
        AgentRequest::Ping => ClientMessage::Ping,
    }
}

pub fn client_message_to_agent_request(
    message: ClientMessage,
) -> Result<AgentRequest, ClientMessage> {
    Ok(match message {
        ClientMessage::Hello { .. } => return Err(message),
        ClientMessage::Input { text } => AgentRequest::Input { text },
        ClientMessage::NewInput { mode, text } => AgentRequest::NewInput { mode, text },
        ClientMessage::Command { line } => AgentRequest::Command { line },
        ClientMessage::Interrupt => AgentRequest::Interrupt,
        ClientMessage::Attach { session_id } => AgentRequest::Attach { session_id },
        ClientMessage::ListSessions => AgentRequest::ListSessions,
        ClientMessage::ApprovalAnswer { id, allow } => AgentRequest::ApprovalAnswer { id, allow },
        ClientMessage::AnswerQuestions { rpc_id, answers } => AgentRequest::AnswerQuestions {
            request_id: rpc_id,
            answers: answers.into_iter().map(question_answer_from_wire).collect(),
        },
        ClientMessage::CancelQuestions { rpc_id } => {
            AgentRequest::CancelQuestions { request_id: rpc_id }
        }
        ClientMessage::History { before_seq, limit } => AgentRequest::History {
            before_sequence: before_seq,
            limit,
        },
        ClientMessage::LoginGet => AgentRequest::LoginGet,
        ClientMessage::LoginSetApiKey { provider, value } => {
            AgentRequest::LoginSetApiKey { provider, value }
        }
        ClientMessage::LoginProxyCreate {
            base_url,
            api_key,
            protocol,
            model,
        } => AgentRequest::LoginProxyCreate {
            base_url,
            api_key,
            protocol,
            model,
        },
        ClientMessage::LoginProxyDelete { id } => AgentRequest::LoginProxyDelete { id },
        ClientMessage::ModelGet => AgentRequest::ModelGet,
        ClientMessage::ModelSet {
            provider,
            model,
            reasoning_effort,
        } => AgentRequest::ModelSet {
            provider,
            model,
            reasoning_effort,
        },
        ClientMessage::Ping => AgentRequest::Ping,
    })
}

pub(crate) fn normalize_host_event(event: HostEvent) -> TimelineRecord {
    let surface = if event.surface_op_invalid {
        Some(SurfaceOperation::Invalid)
    } else {
        event.surface_op.map(|operation| match operation {
            HostSurfaceOp::Append => SurfaceOperation::Append,
            HostSurfaceOp::Replace { start, end } => SurfaceOperation::Replace { start, end },
        })
    };
    TimelineRecord {
        sequence: event.seq,
        time_ms: event.time_ms,
        surface,
        source_sequences: event.source_event_seqs,
        fact: normalize_host_fact(event.kind),
    }
}

fn normalize_host_fact(kind: HostEventKind) -> TimelineFact {
    match kind {
        HostEventKind::UserMessage {
            text,
            source_kind,
            content,
            source,
        } => TimelineFact::UserMessage {
            text,
            source_kind,
            content: content.into_iter().map(normalize_content).collect(),
            source: normalize_message_source(source),
        },
        HostEventKind::AssistantChunk {
            text,
            reasoning,
            turn,
            step,
            usage,
        } => TimelineFact::AssistantChunk {
            text,
            reasoning,
            turn,
            step,
            usage: usage.map(normalize_usage),
        },
        HostEventKind::AssistantMessage {
            text,
            reasoning,
            content,
            turn,
            step,
            usage,
        } => TimelineFact::AssistantMessage {
            text,
            reasoning,
            content: content.into_iter().map(normalize_content).collect(),
            turn,
            step,
            usage: usage.map(normalize_usage),
        },
        HostEventKind::ToolCall {
            call_id,
            name,
            arguments,
        } => TimelineFact::ToolCall(normalize_tool_call(call_id, name, arguments)),
        HostEventKind::ToolResult {
            call_id,
            output,
            is_error,
            output_truncated,
            mutation_hunks,
        } => TimelineFact::ToolResult {
            activity_id: call_id,
            state: if is_error || exit_code(&output).is_some_and(|code| code != 0) {
                ActivityState::Failure
            } else {
                ActivityState::Success
            },
            output,
            output_truncated,
            mutation_hunks: mutation_hunks
                .into_iter()
                .map(normalize_mutation_hunk)
                .collect(),
        },
        HostEventKind::TurnStart => TimelineFact::TurnStart,
        HostEventKind::StepStart { turn, step } => TimelineFact::StepStart { turn, step },
        HostEventKind::StepEnd { turn, step } => TimelineFact::StepEnd { turn, step },
        HostEventKind::TurnEnd {
            reason,
            error_message,
            error_code,
        } => TimelineFact::TurnEnd {
            reason,
            error_message,
            error_code,
        },
        HostEventKind::SessionTitle { title } => TimelineFact::SessionTitle { title },
        HostEventKind::TodoWrite { todos } => TimelineFact::TodoWrite { todos },
        HostEventKind::LlmRetry {
            retry_id,
            retry,
            max_retries,
            delay_ms,
            message,
        } => TimelineFact::RetryScheduled {
            id: retry_id,
            retry,
            max_retries,
            delay_ms,
            message,
        },
        HostEventKind::LlmRetryStarted { retry_id, retry } => TimelineFact::RetryStarted {
            id: retry_id,
            retry,
        },
        HostEventKind::CommandRun {
            command_id,
            name,
            args,
        } => TimelineFact::CommandStarted {
            id: command_id,
            name,
            args,
        },
        HostEventKind::CommandDone {
            command_id,
            success,
            text,
        } => TimelineFact::CommandFinished {
            id: command_id,
            success,
            text,
        },
        HostEventKind::CodeDispatchStart {
            root_call_id,
            parent_call_id,
            sub_call_id,
            name,
            arguments,
        } => TimelineFact::SubagentStarted {
            root_id: root_call_id,
            parent_id: parent_call_id,
            id: sub_call_id,
            name,
            summary: arguments,
        },
        HostEventKind::CodeDispatchEnd {
            sub_call_id,
            is_error,
        } => TimelineFact::SubagentFinished {
            id: sub_call_id,
            failed: is_error,
        },
        HostEventKind::WorkflowRunStart { run_id, name } => {
            TimelineFact::WorkflowStarted { id: run_id, name }
        }
        HostEventKind::WorkflowAgentStart {
            run_id,
            member_seq,
            label,
        } => TimelineFact::WorkflowMemberStarted {
            workflow_id: run_id,
            sequence: member_seq,
            label,
        },
        HostEventKind::WorkflowAgentEnd {
            run_id,
            member_seq,
            outcome,
        } => TimelineFact::WorkflowMemberFinished {
            workflow_id: run_id,
            sequence: member_seq,
            outcome: normalize_outcome(outcome),
        },
        HostEventKind::WorkflowRunEnd { run_id, outcome } => TimelineFact::WorkflowFinished {
            id: run_id,
            outcome: normalize_outcome(outcome),
        },
        HostEventKind::CompactionStart { compaction_id } => {
            TimelineFact::CompactionStarted { id: compaction_id }
        }
        HostEventKind::CompactionSummary {
            compaction_id,
            summary,
        } => TimelineFact::CompactionSummary {
            id: compaction_id,
            summary,
        },
        HostEventKind::CompactionEnd {
            compaction_id,
            error,
        } => TimelineFact::CompactionFinished {
            id: compaction_id,
            error,
        },
        HostEventKind::GoalChange { summary } => TimelineFact::GoalChanged { summary },
        HostEventKind::PlanMode { mode } => TimelineFact::ModeChanged { mode },
        HostEventKind::AgentPresetSelected { preset } => TimelineFact::PresetSelected { preset },
        HostEventKind::SessionState { event_type } => {
            TimelineFact::SessionState { state: event_type }
        }
        HostEventKind::AuditOnly { event_type } => TimelineFact::Audit {
            namespace: "dsh".into(),
            kind: event_type,
        },
        HostEventKind::Unknown { event_type } => TimelineFact::Custom {
            namespace: "dsh".into(),
            kind: event_type,
            summary: None,
        },
    }
}

fn normalize_tool_call(call_id: String, name: String, arguments: String) -> ToolActivity {
    let parsed = serde_json::from_str::<Value>(&arguments).ok();
    let capability = normalize_capability(&name, parsed.as_ref());
    let path = parsed.as_ref().and_then(tool_path);
    let summary = path
        .clone()
        .unwrap_or_else(|| normalized_tool_summary(&name, parsed.as_ref(), &arguments));
    let label = capability_label(&capability, &name).to_owned();
    let reference = normalize_tool_reference(&capability, parsed.as_ref(), path.clone());
    let preview = normalize_tool_preview(&capability, &name, parsed.as_ref(), path, &arguments);
    let items = reference
        .clone()
        .map(|reference| {
            vec![ToolItem {
                id: format!("{call_id}:primary"),
                label: label.clone(),
                reference,
            }]
        })
        .unwrap_or_default();
    ToolActivity {
        id: call_id,
        capability,
        label,
        summary,
        state: ActivityState::Running,
        reference,
        items,
        preview,
    }
}

/// Bound on the pretty-printed JSON carried in a generic tool Preview primary.
const GENERIC_PREVIEW_MAX_CHARS: usize = 2000;

fn normalize_tool_reference(
    capability: &ToolCapability,
    arguments: Option<&Value>,
    path: Option<String>,
) -> Option<ToolReference> {
    let string = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            arguments
                .and_then(|value| value.get(*key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    };
    match capability {
        ToolCapability::Edit | ToolCapability::Replace => {
            let old = string(&["old_str", "old_string", "old", "before"]);
            let new = string(&["new_str", "new_string", "new", "after"]);
            if old.is_some() || new.is_some() {
                Some(ToolReference::Hunks(vec![MutationHunk {
                    path,
                    old,
                    new,
                    anchor_line: None,
                }]))
            } else {
                path.map(|path| ToolReference::Path { path })
            }
        }
        ToolCapability::Insert => {
            let new = string(&["new_str", "new_string", "new", "after"]);
            let anchor_line = arguments
                .and_then(|value| value.get("insert_line"))
                .and_then(Value::as_u64)
                .map(|value| value as usize);
            if new.is_some() || anchor_line.is_some() {
                Some(ToolReference::Hunks(vec![MutationHunk {
                    path,
                    old: None,
                    new,
                    anchor_line,
                }]))
            } else {
                path.map(|path| ToolReference::Path { path })
            }
        }
        ToolCapability::Read | ToolCapability::View => path.map(|path| ToolReference::Lines {
            path,
            start: arguments
                .and_then(|value| value.get("line_start").or_else(|| value.get("start")))
                .and_then(Value::as_u64)
                .unwrap_or(1) as usize,
            lines: Vec::new(),
        }),
        ToolCapability::Search => Some(ToolReference::SearchResult {
            query: string(&["pattern", "query"]).unwrap_or_default(),
            matches: Vec::new(),
        }),
        ToolCapability::Command => {
            string(&["command", "cmd"]).map(|command| ToolReference::Command { command })
        }
        _ => {
            if let Some(url) = string(&["url", "href"]) {
                Some(ToolReference::Link { label: None, url })
            } else if let Some(source) = string(&["markdown"]) {
                Some(ToolReference::Markdown { source })
            } else if let Some(text) = string(&["text", "content"]) {
                Some(ToolReference::PlainText { text })
            } else {
                path.map(|path| ToolReference::Path { path })
            }
        }
    }
}

/// Build the structured preview seed for common-format tools. Mutation tools
/// (Edit/Replace/Insert) return `None` because their preview rides in
/// `reference`; interaction tools (Custom) also return `None`.
fn normalize_tool_preview(
    capability: &ToolCapability,
    name: &str,
    arguments: Option<&Value>,
    path: Option<String>,
    raw_arguments: &str,
) -> Option<ToolPreview> {
    let string = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            arguments
                .and_then(|value| value.get(*key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    };
    match capability {
        ToolCapability::Read | ToolCapability::View => {
            let path = path?;
            let lines = if matches!(capability, ToolCapability::View) {
                // `str_replace_editor` view: `view_range: [start, end]`, where
                // `end` may be -1 for "to EOF".
                let range = arguments
                    .and_then(|value| value.get("view_range"))
                    .and_then(Value::as_array);
                range.and_then(|range| {
                    let start = range.first().and_then(Value::as_i64)?;
                    if start < 1 {
                        return None;
                    }
                    let end = range
                        .get(1)
                        .and_then(Value::as_i64)
                        .filter(|end| *end >= start);
                    Some(LineSelection {
                        start: start as usize,
                        end: end.map(|end| end as usize),
                    })
                })
            } else {
                // `read`: `offset` (1-based first line) + `limit` (count).
                let offset = arguments
                    .and_then(|value| value.get("offset"))
                    .and_then(Value::as_u64)
                    .map(|value| value as usize);
                offset.map(|start| {
                    let end = arguments
                        .and_then(|value| value.get("limit"))
                        .and_then(Value::as_u64)
                        .filter(|limit| *limit > 0)
                        .map(|limit| start.saturating_add(limit as usize).saturating_sub(1));
                    LineSelection { start, end }
                })
            };
            Some(ToolPreview {
                name: preview_tool_name(capability, name).to_owned(),
                primary: ToolPreviewPrimary::Location { path, lines },
                secondary: None,
            })
        }
        ToolCapability::Create => Some(ToolPreview {
            name: "create".to_owned(),
            primary: ToolPreviewPrimary::Location {
                path: path?,
                lines: None,
            },
            secondary: None,
        }),
        ToolCapability::Search => Some(ToolPreview {
            name: "search".to_owned(),
            primary: ToolPreviewPrimary::Search {
                query: string(&["pattern", "query"]).unwrap_or_default(),
                path,
            },
            secondary: None,
        }),
        ToolCapability::Command => {
            let command = string(&["command", "cmd"])?;
            Some(ToolPreview {
                name: preview_tool_name(capability, name).to_owned(),
                primary: ToolPreviewPrimary::Command {
                    command,
                    metrics: ToolMetrics::default(),
                },
                secondary: None,
            })
        }
        ToolCapability::Generic => {
            let (source, truncated) = bounded_json(arguments, raw_arguments);
            Some(ToolPreview {
                name: name.to_owned(),
                primary: ToolPreviewPrimary::Json { source, truncated },
                secondary: None,
            })
        }
        _ => None,
    }
}

/// Stable, bounded pretty JSON for unsupported tool arguments.
fn bounded_json(arguments: Option<&Value>, raw: &str) -> (String, bool) {
    let source = arguments
        .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| raw.to_owned()))
        .unwrap_or_else(|| raw.to_owned());
    let chars = source.chars().count();
    if chars <= GENERIC_PREVIEW_MAX_CHARS {
        (source, false)
    } else {
        (
            source.chars().take(GENERIC_PREVIEW_MAX_CHARS).collect(),
            true,
        )
    }
}

/// Display name for a Command-capability tool: the original tool name for
/// recognized shells (`bash`/`pwsh`/`cmd`/…), with `command` as the fallback.
/// Shared by the transcript label and the Preview header so both surfaces
/// preserve the real tool identity instead of collapsing into `command`.
fn shell_display_name(name: &str) -> &str {
    match name.to_ascii_lowercase().as_str() {
        "bash" | "pwsh" | "powershell" | "cmd" | "sh" | "shell" => name,
        _ => "command",
    }
}

/// Preview-only display name: keeps command identity distinct while transcript
/// labels stay unchanged.
fn preview_tool_name<'a>(capability: &ToolCapability, name: &'a str) -> &'a str {
    match capability {
        ToolCapability::Read => "read",
        ToolCapability::View => "view",
        ToolCapability::Create => "create",
        ToolCapability::Search => "search",
        ToolCapability::Command => shell_display_name(name),
        _ => name,
    }
}

fn normalize_mutation_hunk(hunk: HostMutationHunk) -> MutationHunk {
    MutationHunk {
        path: hunk.path,
        old: hunk.old_text,
        new: hunk.new_text,
        anchor_line: None,
    }
}

fn legacy_mutation_hunk(hunk: MutationHunk) -> HostMutationHunk {
    HostMutationHunk {
        path: hunk.path,
        old_text: hunk.old,
        new_text: hunk.new,
    }
}

pub fn normalize_capability(name: &str, arguments: Option<&Value>) -> ToolCapability {
    if let Some(command) = arguments.and_then(editor_command) {
        match command {
            "create" => return ToolCapability::Create,
            "str_replace" => return ToolCapability::Replace,
            "insert" => return ToolCapability::Insert,
            "view" => return ToolCapability::View,
            _ => {}
        }
    }

    let lower = name.to_ascii_lowercase();
    if lower.contains("grep") || lower.contains("search") || lower.contains("glob") {
        ToolCapability::Search
    } else if lower.contains("bash")
        || lower.contains("command")
        || lower.contains("shell")
        || lower.contains("pwsh")
        || lower.contains("cmd")
    {
        ToolCapability::Command
    } else if lower.contains("create") {
        ToolCapability::Create
    } else if lower.contains("replace") {
        ToolCapability::Replace
    } else if lower.contains("edit") {
        ToolCapability::Edit
    } else if lower.contains("view") {
        ToolCapability::View
    } else if lower.contains("read") {
        ToolCapability::Read
    } else if lower == "ask_user_question" {
        ToolCapability::Custom {
            namespace: "interaction".into(),
            name: "question".into(),
        }
    } else {
        ToolCapability::Generic
    }
}

fn capability_label<'a>(capability: &ToolCapability, fallback: &'a str) -> &'a str {
    match capability {
        ToolCapability::Read => "read",
        ToolCapability::View => "view",
        ToolCapability::Edit => "edit",
        ToolCapability::Insert => "insert",
        ToolCapability::Replace => "replace",
        ToolCapability::Search => "search",
        ToolCapability::Command => shell_display_name(fallback),
        ToolCapability::Create => "create",
        ToolCapability::Generic | ToolCapability::Custom { .. } => fallback,
    }
}

fn editor_command(arguments: &Value) -> Option<&str> {
    arguments.get("command").and_then(Value::as_str)
}

fn tool_path(arguments: &Value) -> Option<String> {
    ["path", "file_path", "filePath", "file"]
        .into_iter()
        .find_map(|key| arguments.get(key).and_then(Value::as_str))
        .or_else(|| {
            arguments
                .get("target")
                .and_then(Value::as_str)
                .filter(|target| target.contains('/') || target.contains('\\'))
        })
        .map(str::to_owned)
}

fn normalized_tool_summary(name: &str, arguments: Option<&Value>, raw: &str) -> String {
    if name == "grep" {
        if let Some((pattern, path)) = arguments.and_then(|arguments| {
            Some((
                arguments.get("pattern")?.as_str()?,
                arguments.get("path")?.as_str()?,
            ))
        }) {
            let pattern = serde_json::to_string(pattern).unwrap_or_else(|_| "\"\"".into());
            let path = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".into());
            return format!("{pattern} at {path}");
        }
    }
    if matches!(name, "bash" | "shell" | "powershell" | "pwsh") {
        if let Some(command) = arguments.and_then(|arguments| {
            ["command", "cmd", "script"]
                .into_iter()
                .find_map(|key| arguments.get(key).and_then(Value::as_str))
        }) {
            return command.lines().next().unwrap_or(command).to_owned();
        }
    }
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    bounded_summary(&compact, 120)
}

fn bounded_summary(value: &str, cap: usize) -> String {
    let mut out: String = value.chars().take(cap).collect();
    if value.chars().count() > cap {
        out.push('…');
    }
    out
}

fn exit_code(output: &str) -> Option<i32> {
    output.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix("[exit code: ")
            .and_then(|value| value.strip_suffix(']'))
            .and_then(|value| value.parse().ok())
    })
}

pub(crate) fn legacy_host_event(record: TimelineRecord) -> HostEvent {
    let (surface_op, surface_op_invalid) = match record.surface {
        Some(SurfaceOperation::Append) => (Some(HostSurfaceOp::Append), false),
        Some(SurfaceOperation::Replace { start, end }) => {
            (Some(HostSurfaceOp::Replace { start, end }), false)
        }
        Some(SurfaceOperation::Invalid) => (None, true),
        None => (None, false),
    };
    HostEvent::from_normalized_parts(
        record.sequence,
        record.time_ms,
        surface_op,
        surface_op_invalid,
        record.source_sequences,
        legacy_host_fact(record.fact),
    )
}

fn legacy_host_fact(fact: TimelineFact) -> HostEventKind {
    match fact {
        TimelineFact::UserMessage {
            text,
            source_kind,
            content,
            source,
        } => HostEventKind::UserMessage {
            text,
            source_kind,
            content: content.into_iter().map(legacy_content).collect(),
            source: HostMessageSource {
                kind: source.kind,
                form: source.form,
                summary: source.summary,
                producer: source.producer,
            },
        },
        TimelineFact::AssistantChunk {
            text,
            reasoning,
            turn,
            step,
            usage,
        } => HostEventKind::AssistantChunk {
            text,
            reasoning,
            turn,
            step,
            usage: usage.map(legacy_usage),
        },
        TimelineFact::AssistantMessage {
            text,
            reasoning,
            content,
            turn,
            step,
            usage,
        } => HostEventKind::AssistantMessage {
            text,
            reasoning,
            content: content.into_iter().map(legacy_content).collect(),
            turn,
            step,
            usage: usage.map(legacy_usage),
        },
        TimelineFact::ToolCall(activity) => HostEventKind::ToolCall {
            call_id: activity.id,
            name: activity.label,
            arguments: legacy_tool_arguments(activity.reference, activity.summary),
        },
        TimelineFact::ToolResult {
            activity_id,
            output,
            state,
            output_truncated,
            mutation_hunks,
        } => HostEventKind::ToolResult {
            call_id: activity_id,
            output,
            is_error: state == ActivityState::Failure,
            output_truncated,
            mutation_hunks: mutation_hunks
                .into_iter()
                .map(legacy_mutation_hunk)
                .collect(),
        },
        TimelineFact::TurnStart => HostEventKind::TurnStart,
        TimelineFact::StepStart { turn, step } => HostEventKind::StepStart { turn, step },
        TimelineFact::StepEnd { turn, step } => HostEventKind::StepEnd { turn, step },
        TimelineFact::TurnEnd {
            reason,
            error_message,
            error_code,
        } => HostEventKind::TurnEnd {
            reason,
            error_message,
            error_code,
        },
        TimelineFact::SessionTitle { title } => HostEventKind::SessionTitle { title },
        TimelineFact::TodoWrite { todos } => HostEventKind::TodoWrite { todos },
        TimelineFact::RetryScheduled {
            id,
            retry,
            max_retries,
            delay_ms,
            message,
        } => HostEventKind::LlmRetry {
            retry_id: id,
            retry,
            max_retries,
            delay_ms,
            message,
        },
        TimelineFact::RetryStarted { id, retry } => HostEventKind::LlmRetryStarted {
            retry_id: id,
            retry,
        },
        TimelineFact::CommandStarted { id, name, args } => HostEventKind::CommandRun {
            command_id: id,
            name,
            args,
        },
        TimelineFact::CommandFinished { id, success, text } => HostEventKind::CommandDone {
            command_id: id,
            success,
            text,
        },
        TimelineFact::SubagentStarted {
            root_id,
            parent_id,
            id,
            name,
            summary,
        } => HostEventKind::CodeDispatchStart {
            root_call_id: root_id,
            parent_call_id: parent_id,
            sub_call_id: id,
            name,
            arguments: summary,
        },
        TimelineFact::SubagentFinished { id, failed } => HostEventKind::CodeDispatchEnd {
            sub_call_id: id,
            is_error: failed,
        },
        TimelineFact::WorkflowStarted { id, name } => {
            HostEventKind::WorkflowRunStart { run_id: id, name }
        }
        TimelineFact::WorkflowMemberStarted {
            workflow_id,
            sequence,
            label,
        } => HostEventKind::WorkflowAgentStart {
            run_id: workflow_id,
            member_seq: sequence,
            label,
        },
        TimelineFact::WorkflowMemberFinished {
            workflow_id,
            sequence,
            outcome,
        } => HostEventKind::WorkflowAgentEnd {
            run_id: workflow_id,
            member_seq: sequence,
            outcome: legacy_outcome(outcome),
        },
        TimelineFact::WorkflowFinished { id, outcome } => HostEventKind::WorkflowRunEnd {
            run_id: id,
            outcome: legacy_outcome(outcome),
        },
        TimelineFact::CompactionStarted { id } => {
            HostEventKind::CompactionStart { compaction_id: id }
        }
        TimelineFact::CompactionSummary { id, summary } => HostEventKind::CompactionSummary {
            compaction_id: id,
            summary,
        },
        TimelineFact::CompactionFinished { id, error } => HostEventKind::CompactionEnd {
            compaction_id: id,
            error,
        },
        TimelineFact::GoalChanged { summary } => HostEventKind::GoalChange { summary },
        TimelineFact::ModeChanged { mode } => HostEventKind::PlanMode { mode },
        TimelineFact::PresetSelected { preset } => HostEventKind::AgentPresetSelected { preset },
        TimelineFact::SessionState { state } => HostEventKind::SessionState { event_type: state },
        TimelineFact::Audit { kind, .. } => HostEventKind::AuditOnly { event_type: kind },
        TimelineFact::Custom { kind, .. } => HostEventKind::Unknown { event_type: kind },
    }
}

fn legacy_content(block: ContentBlock) -> HostContentBlock {
    match block {
        ContentBlock::Text(text) => HostContentBlock::Text(text),
        ContentBlock::Reasoning(text) => HostContentBlock::Reasoning(text),
        ContentBlock::Image { label } => HostContentBlock::Image { label },
        ContentBlock::Custom { kind, .. } => HostContentBlock::Other { block_type: kind },
    }
}

fn legacy_usage(usage: TokenUsage) -> HostTokenUsage {
    HostTokenUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
    }
}

fn legacy_outcome(outcome: LifecycleOutcome) -> HostLifecycleOutcome {
    match outcome {
        LifecycleOutcome::Success => HostLifecycleOutcome::Success,
        LifecycleOutcome::Failure => HostLifecycleOutcome::Failure,
        LifecycleOutcome::Cancelled => HostLifecycleOutcome::Cancelled,
    }
}

fn legacy_tool_arguments(reference: Option<ToolReference>, summary: String) -> String {
    match reference {
        Some(ToolReference::Path { path }) => serde_json::json!({ "path": path }).to_string(),
        Some(ToolReference::Link { url, .. }) => serde_json::json!({ "url": url }).to_string(),
        Some(ToolReference::Text { text } | ToolReference::PlainText { text }) => {
            serde_json::json!({ "text": text }).to_string()
        }
        Some(ToolReference::Diff { path, diff }) => {
            serde_json::json!({ "path": path, "diff": diff }).to_string()
        }
        Some(ToolReference::Hunks(hunks)) => {
            let hunks = hunks
                .iter()
                .map(|hunk| {
                    serde_json::json!({
                        "path": hunk.path,
                        "old": hunk.old,
                        "new": hunk.new,
                        "anchor_line": hunk.anchor_line,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({ "hunks": hunks }).to_string()
        }
        Some(ToolReference::Lines { path, start, lines }) => {
            serde_json::json!({ "path": path, "start": start, "lines": lines }).to_string()
        }
        Some(ToolReference::SearchResult { query, matches }) => {
            serde_json::json!({ "query": query, "matches": matches }).to_string()
        }
        Some(ToolReference::Command { command }) => {
            serde_json::json!({ "command": command }).to_string()
        }
        Some(ToolReference::Markdown { source }) => {
            serde_json::json!({ "markdown": source }).to_string()
        }
        Some(ToolReference::Custom { value, .. }) => value,
        None => summary,
    }
}

fn legacy_status(status: AgentStatus) -> String {
    match status {
        AgentStatus::Idle => "idle".into(),
        AgentStatus::Running => "running".into(),
        AgentStatus::Waiting => "waiting".into(),
        AgentStatus::Error => "error".into(),
        AgentStatus::Custom(status) => status,
    }
}

fn normalize_status(status: String) -> AgentStatus {
    match status.as_str() {
        "idle" => AgentStatus::Idle,
        "running" => AgentStatus::Running,
        "waiting" => AgentStatus::Waiting,
        "error" => AgentStatus::Error,
        _ => AgentStatus::Custom(status),
    }
}

fn normalize_content(block: HostContentBlock) -> ContentBlock {
    match block {
        HostContentBlock::Text(text) => ContentBlock::Text(text),
        HostContentBlock::Reasoning(text) => ContentBlock::Reasoning(text),
        HostContentBlock::Image { label } => ContentBlock::Image { label },
        HostContentBlock::Other { block_type } => ContentBlock::Custom {
            namespace: "dsh".into(),
            kind: block_type,
        },
    }
}

fn normalize_message_source(source: HostMessageSource) -> MessageSource {
    MessageSource {
        kind: source.kind,
        form: source.form,
        summary: source.summary,
        producer: source.producer,
    }
}

fn normalize_usage(usage: HostTokenUsage) -> TokenUsage {
    TokenUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
    }
}

fn normalize_outcome(outcome: HostLifecycleOutcome) -> LifecycleOutcome {
    match outcome {
        HostLifecycleOutcome::Success => LifecycleOutcome::Success,
        HostLifecycleOutcome::Failure => LifecycleOutcome::Failure,
        HostLifecycleOutcome::Cancelled => LifecycleOutcome::Cancelled,
    }
}

fn normalize_session(session: SessionInfo) -> SessionSummary {
    SessionSummary {
        id: session.id,
        title: session.title,
        live: session.live,
        created_at: session.created_at,
    }
}

fn normalize_provider(provider: ProviderInfo) -> CredentialProvider {
    CredentialProvider {
        id: provider.id,
        name: provider.name,
        api_key_configured: provider.api_key_configured,
        api_key_writable: provider.api_key_writable,
        api_key_source: provider.api_key_source,
        api_key_hint: provider.api_key_hint,
    }
}

fn normalize_proxy(proxy: ProxyInfo) -> ProxyRoute {
    ProxyRoute {
        id: proxy.id,
        name: proxy.name,
        base_url: proxy.base_url,
        protocol: proxy.protocol,
        model: proxy.model,
    }
}

fn normalize_model_provider(provider: ModelProviderInfo) -> ModelProvider {
    ModelProvider {
        id: provider.id,
        name: provider.name,
        models: provider.models.into_iter().map(normalize_model).collect(),
    }
}

fn normalize_model(model: ModelInfo) -> ModelDescriptor {
    ModelDescriptor {
        id: model.id,
        name: model.name,
        description: model.description,
        reasoning: model.reasoning.map(|reasoning| ModelReasoning {
            efforts: reasoning
                .efforts
                .into_iter()
                .map(|effort| ReasoningEffort {
                    id: effort.id,
                    name: effort.name,
                    description: effort.description,
                })
                .collect(),
            default_effort: reasoning.default_effort,
        }),
    }
}

fn normalize_model_selection(current: ModelCurrent) -> ModelSelection {
    ModelSelection {
        provider: current.provider,
        model: current.model,
        reasoning_effort: current.reasoning_effort,
    }
}

fn normalize_question(question: QuestionItem) -> Question {
    Question {
        id: question.id,
        question: question.question,
        header: question.header,
        options: question.options.map(|options| {
            options
                .into_iter()
                .map(|option| QuestionOption {
                    label: option.label,
                    description: option.description,
                })
                .collect()
        }),
        multi_select: question.multi_select,
    }
}

fn question_answer_to_wire(answer: UiQuestionAnswer) -> QuestionAnswer {
    QuestionAnswer {
        id: answer.id,
        selected: answer.selected,
        custom: answer.custom,
    }
}

fn question_answer_from_wire(answer: QuestionAnswer) -> UiQuestionAnswer {
    UiQuestionAnswer {
        id: answer.id,
        selected: answer.selected,
        custom: answer.custom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_frame_is_normalized_before_frontend_projection() {
        let wire = ServerMessage::from_wire(
            r#"{"type":"event","event":{"seq":7,"type":"tool/call","data":{"callId":"c1","name":"str_replace_editor","arguments":"{\"command\":\"view\",\"path\":\"src/main.rs\"}"}}}"#,
        )
        .expect("tool event parses");
        let event = normalize_server_message(wire);
        let AgentEvent::Timeline(TimelineEvent::Append(record)) = event else {
            panic!("expected normalized timeline event");
        };
        let TimelineFact::ToolCall(activity) = record.fact else {
            panic!("expected normalized tool call");
        };
        assert_eq!(activity.capability, ToolCapability::View);
        assert_eq!(activity.label, "view");
        assert_eq!(activity.summary, "src/main.rs");
        assert!(matches!(
            activity.items.as_slice(),
            [ToolItem {
                reference: ToolReference::Lines { path, start: 1, lines },
                ..
            }] if path == "src/main.rs" && lines.is_empty()
        ));
    }

    #[test]
    fn str_replace_editor_view_produces_location_preview() {
        let activity = normalize_tool_call(
            "c1".into(),
            "str_replace_editor".into(),
            r#"{"command":"view","path":"/repo/src/main.rs","view_range":[12,20]}"#.into(),
        );
        let preview = activity.preview.expect("view call carries a preview seed");
        assert_eq!(preview.name, "view");
        assert!(matches!(
            &preview.primary,
            ToolPreviewPrimary::Location {
                path,
                lines: Some(LineSelection {
                    start: 12,
                    end: Some(20)
                })
            } if path == "/repo/src/main.rs"
        ));
        assert!(preview.secondary.is_none());
    }

    #[test]
    fn read_offset_limit_produces_closed_line_selection() {
        let activity = normalize_tool_call(
            "c1".into(),
            "read".into(),
            r#"{"file_path":"/repo/a.rs","offset":5,"limit":10}"#.into(),
        );
        let preview = activity.preview.expect("read call carries a preview seed");
        assert!(matches!(
            &preview.primary,
            ToolPreviewPrimary::Location {
                path,
                lines: Some(LineSelection {
                    start: 5,
                    end: Some(14)
                })
            } if path == "/repo/a.rs"
        ));
    }

    #[test]
    fn str_replace_produces_requested_mutation_hunks() {
        let activity = normalize_tool_call(
            "c1".into(),
            "str_replace_editor".into(),
            r#"{"command":"str_replace","path":"/repo/a.rs","old_str":"hello","new_str":"hi"}"#
                .into(),
        );
        assert!(
            activity.preview.is_none(),
            "mutations preview via reference"
        );
        assert!(matches!(
            activity.reference,
            Some(ToolReference::Hunks(ref hunks))
                if hunks.len() == 1
                    && hunks[0].old.as_deref() == Some("hello")
                    && hunks[0].new.as_deref() == Some("hi")
                    && hunks[0].anchor_line.is_none()
        ));
    }

    #[test]
    fn insert_produces_addition_only_hunk_with_anchor() {
        let activity = normalize_tool_call(
            "c1".into(),
            "str_replace_editor".into(),
            r#"{"command":"insert","path":"/repo/a.rs","insert_line":3,"new_str":"use x;"}"#.into(),
        );
        assert!(matches!(
            activity.reference,
            Some(ToolReference::Hunks(ref hunks))
                if hunks.len() == 1
                    && hunks[0].old.is_none()
                    && hunks[0].new.as_deref() == Some("use x;")
                    && hunks[0].anchor_line == Some(3)
        ));
    }

    #[test]
    fn shell_tools_keep_distinct_preview_names() {
        let bash = normalize_tool_call("c1".into(), "bash".into(), r#"{"command":"ls"}"#.into());
        let pwsh = normalize_tool_call("c2".into(), "pwsh".into(), r#"{"command":"ls"}"#.into());
        let bash_preview = bash.preview.expect("bash preview");
        let pwsh_preview = pwsh.preview.expect("pwsh preview");
        assert_eq!(bash_preview.name, "bash");
        assert_eq!(pwsh_preview.name, "pwsh");
        assert!(matches!(
            &pwsh_preview.primary,
            ToolPreviewPrimary::Command { command, metrics }
                if command == "ls" && *metrics == ToolMetrics::default()
        ));
    }

    #[test]
    fn shell_tools_preserve_transcript_label_and_command_is_fallback() {
        let bash = normalize_tool_call("c1".into(), "bash".into(), r#"{"command":"ls"}"#.into());
        let pwsh = normalize_tool_call("c2".into(), "pwsh".into(), r#"{"command":"ls"}"#.into());
        let cmd = normalize_tool_call("c3".into(), "cmd".into(), r#"{"command":"dir"}"#.into());
        assert_eq!(bash.label, "bash");
        assert_eq!(pwsh.label, "pwsh");
        assert_eq!(cmd.label, "cmd");
        // A Command-capability name that is not a recognized shell falls back
        // to the generic `command` label.
        let weird = normalize_tool_call(
            "c4".into(),
            "run_command".into(),
            r#"{"command":"x"}"#.into(),
        );
        assert_eq!(weird.label, "command");
    }

    #[test]
    fn unknown_tool_produces_bounded_json_preview() {
        let activity = normalize_tool_call(
            "c1".into(),
            "write".into(),
            r#"{"file_path":"/repo/a.rs","content":"fn main() {}"}"#.into(),
        );
        let preview = activity.preview.expect("generic tool preview");
        assert_eq!(preview.name, "write");
        assert!(matches!(
            &preview.primary,
            ToolPreviewPrimary::Json { source, truncated: false } if source.contains("file_path")
        ));
    }

    #[test]
    fn tool_result_meta_diffs_narrow_to_mutation_hunks() {
        let wire = ServerMessage::from_wire(
            r#"{"type":"event","event":{"seq":8,"type":"tool/result","data":{"message":{"content":[{"toolCallId":"c1","content":[{"type":"text","text":"ok"}]}]},"meta":{"diffs":[{"path":"/repo/a.rs","oldText":"hello","newText":"hi"}]}}}}"#,
        )
        .expect("tool result parses");
        let event = normalize_server_message(wire);
        let AgentEvent::Timeline(TimelineEvent::Append(record)) = event else {
            panic!("expected normalized timeline event");
        };
        let TimelineFact::ToolResult { mutation_hunks, .. } = record.fact else {
            panic!("expected tool result");
        };
        assert_eq!(mutation_hunks.len(), 1);
        assert_eq!(mutation_hunks[0].old.as_deref(), Some("hello"));
        assert_eq!(mutation_hunks[0].new.as_deref(), Some("hi"));
    }

    #[test]
    fn question_action_maps_to_existing_wire_shape() {
        let message = agent_request_to_client(AgentRequest::AnswerQuestions {
            request_id: "rpc-1".into(),
            answers: vec![UiQuestionAnswer {
                id: "q1".into(),
                selected: vec!["A".into()],
                custom: None,
            }],
        });
        assert!(matches!(
            message,
            ClientMessage::AnswerQuestions { rpc_id, answers }
                if rpc_id == "rpc-1"
                    && answers.len() == 1
                    && answers[0].selected == ["A"]
        ));
    }

    #[test]
    fn attached_session_survives_the_transitional_controller_adapter() {
        let event = normalize_server_message(ServerMessage::Welcome {
            protocol_version: Some(4),
            max_frame_bytes: Some(1024),
            session_id: "s1".into(),
            status: "idle".into(),
            provider: Some("p".into()),
            model: Some("m".into()),
            mode: Some("standard".into()),
            title: Some("title".into()),
            cwd: Some("C:\\work".into()),
        });
        let restored = legacy_server_message(event).expect("supported agent event");
        assert!(matches!(
            restored,
            ServerMessage::Welcome {
                session_id,
                protocol_version: Some(4),
                max_frame_bytes: Some(1024),
                cwd: Some(cwd),
                ..
            } if session_id == "s1" && cwd == "C:\\work"
        ));
    }
}
