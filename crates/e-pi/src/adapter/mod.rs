//! Stateful conversion between Pi RPC and provider-neutral `e-tui` contracts.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use e_tui::{
    action::AgentRequest,
    agent::{
        timeline::{TimelineFact, TimelineRecord},
        AgentEvent, AgentStatus, CatalogEvent, CommandDescriptor, InteractionEvent, SessionEvent,
        Skill, TimelineEvent,
    },
};
use serde_json::Value;

use crate::protocol::{RpcCommand, RpcRecord};

#[derive(Debug, Default)]
pub struct AdapterOutput {
    pub commands: Vec<RpcCommand>,
    pub events: Vec<AgentEvent>,
}

impl AdapterOutput {
    fn command(command: RpcCommand) -> Self {
        Self {
            commands: vec![command],
            events: Vec::new(),
        }
    }

    fn event(event: AgentEvent) -> Self {
        Self {
            commands: Vec::new(),
            events: vec![event],
        }
    }

    fn merge(&mut self, mut other: Self) {
        self.commands.append(&mut other.commands);
        self.events.append(&mut other.events);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingExtensionUi {
    Select,
    Confirm,
    Input,
    Editor,
}

struct NewSubmission {
    text: String,
    model: Option<Value>,
    thinking_level: Option<String>,
}

struct PendingSkillPrompt {
    id: String,
    session_id: String,
    /// None keeps the ordering barrier until the trailing prompt is acknowledged.
    trailing_text: Option<String>,
}

/// Outstanding `/reload` step. Only the companion prompt can report that the
/// reload itself failed; catalog reads raise no extension errors.
struct PendingReload {
    id: String,
    step: ReloadStep,
    error: Option<String>,
}

#[derive(PartialEq)]
enum ReloadStep {
    CompanionPrompt,
    Catalog,
}

pub const AUTH_STATUS_KEY: &str = "pie-native-auth-v1";
pub const AUTH_CONTEXT_COMMAND: &str = "__pie_native_auth_context_v1";
pub const AUTH_REFRESH_COMMAND: &str = "__pie_native_auth_refresh_v1";
pub const RELOAD_COMMAND: &str = "__pie_reload_v1";

pub struct PiAdapter {
    cwd: PathBuf,
    session_root: PathBuf,
    next_id: u64,
    next_sequence: u64,
    current_turn: u64,
    is_streaming: bool,
    session_id: String,
    session_name: Option<String>,
    /// First-user-message fallback title, mirroring `session_index` so the
    /// status bar matches the session list for unnamed sessions.
    derived_title: Option<String>,
    /// Last title reported to the frontend; suppresses duplicate events.
    emitted_title: Option<String>,
    /// Id of a prompt sent for a slash command; its response triggers the
    /// same-session state refresh that observes extension-side renames.
    pending_command_prompt: Option<String>,
    pending_skill_prompt: Option<PendingSkillPrompt>,
    compaction_model: Option<Value>,
    compaction_model_path: Option<std::path::PathBuf>,
    reload_available: bool,
    pending_reload: Option<PendingReload>,
    pending_compaction: Option<compaction::Pending>,
    active_compaction_model: Option<String>,
    active_compaction_id: Option<String>,
    active_compaction_automatic: Option<bool>,
    last_attached_session: Option<String>,
    current_model: Option<Value>,
    available_models: Vec<Value>,
    thinking_level: Option<String>,
    pending_new: HashMap<String, NewSubmission>,
    pending_fork: Option<fork::Pending>,
    pending_model_effort: HashMap<String, Option<String>>,
    configuration_request: Option<String>,
    deferred_requests: std::collections::VecDeque<AgentRequest>,
    /// Pi emits both `tool_execution_end` and the durable `message_end` for
    /// one result. Retain the id only until that duplicate message arrives.
    pending_tool_result_messages: HashSet<String>,
    extension_ui: HashMap<String, PendingExtensionUi>,
    pending_queue: queue::PendingQueue,
    pending_stats_request: Option<String>,
    stats_refresh_queued: bool,
}

impl PiAdapter {
    pub fn session_index_root(&self, workspace: &str) -> PathBuf {
        if std::path::Path::new(workspace) == self.cwd {
            self.session_root.clone()
        } else {
            crate::session_index::project_session_root(std::path::Path::new(workspace))
        }
    }

    pub fn new(cwd: impl Into<PathBuf>, session_root: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            session_root: session_root.into(),
            next_id: 1,
            next_sequence: 1,
            current_turn: 0,
            is_streaming: false,
            session_id: "pi-starting".into(),
            session_name: None,
            derived_title: None,
            emitted_title: None,
            pending_command_prompt: None,
            pending_skill_prompt: None,
            compaction_model: None,
            compaction_model_path: None,
            reload_available: false,
            pending_reload: None,
            pending_compaction: None,
            active_compaction_model: None,
            active_compaction_id: None,
            active_compaction_automatic: None,
            last_attached_session: None,
            current_model: None,
            available_models: Vec::new(),
            thinking_level: None,
            pending_new: HashMap::new(),
            pending_fork: None,
            pending_model_effort: HashMap::new(),
            configuration_request: None,
            deferred_requests: std::collections::VecDeque::new(),
            pending_tool_result_messages: HashSet::new(),
            extension_ui: HashMap::new(),
            pending_queue: queue::PendingQueue::default(),
            pending_stats_request: None,
            stats_refresh_queued: false,
        }
    }

    pub fn with_compaction_model_path(mut self, path: std::path::PathBuf) -> Self {
        self.compaction_model_path = Some(path);
        self
    }

    pub fn startup_commands(&mut self) -> Vec<RpcCommand> {
        self.refresh_commands()
    }

    pub fn request(&mut self, request: AgentRequest) -> AdapterOutput {
        if self.pending_fork.is_some()
            && matches!(
                request,
                AgentRequest::Input { .. }
                    | AgentRequest::Steer { .. }
                    | AgentRequest::ClearAsap
                    | AgentRequest::Command { .. }
                    | AgentRequest::NewInput { .. }
                    | AgentRequest::ModelSet { .. }
                    | AgentRequest::ModelGet
                    | AgentRequest::Ping
                    | AgentRequest::Attach { .. }
            )
        {
            let message = "Wait for the fork/clone operation to finish";
            return match request {
                AgentRequest::Steer { .. } => queue::event(
                    self,
                    Some(e_tui::agent::AsapQueueOperation::Submit),
                    Some(message.into()),
                ),
                AgentRequest::ClearAsap => queue::event(
                    self,
                    Some(e_tui::agent::AsapQueueOperation::Clear),
                    Some(message.into()),
                ),
                _ => AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: if matches!(request, AgentRequest::Command { .. }) {
                        "command-failed"
                    } else {
                        "input-failed"
                    }
                    .into(),
                    message: message.into(),
                })),
            };
        }
        if (self.configuration_request.is_some()
            || self.pending_queue.operation.is_some()
            || self.pending_skill_prompt.is_some()
            || self.pending_compaction.is_some())
            && matches!(
                request,
                AgentRequest::Input { .. }
                    | AgentRequest::Steer { .. }
                    | AgentRequest::ClearAsap
                    | AgentRequest::Command { .. }
                    | AgentRequest::NewInput { .. }
                    | AgentRequest::ModelSet { .. }
                    | AgentRequest::ModelGet
                    | AgentRequest::Ping
                    | AgentRequest::Attach { .. }
            )
        {
            if self.deferred_requests.len() >= 64 {
                let operation = match request {
                    AgentRequest::Steer { .. } => Some(e_tui::agent::AsapQueueOperation::Submit),
                    AgentRequest::ClearAsap => Some(e_tui::agent::AsapQueueOperation::Clear),
                    _ => None,
                };
                if let Some(operation) = operation {
                    return queue::event(
                        self,
                        Some(operation),
                        Some("Too many requests waiting for the model/session change".into()),
                    );
                }
                return AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: if matches!(request, AgentRequest::NewInput { .. }) {
                        "new-failed"
                    } else {
                        "input-failed"
                    }
                    .into(),
                    message: "Too many requests waiting for the model/session change".into(),
                }));
            }
            self.deferred_requests.push_back(request);
            let mut output = AdapterOutput::default();
            response::drain_deferred(self, &mut output);
            return output;
        }
        request::route(self, request)
    }

    pub fn record(&mut self, record: RpcRecord) -> AdapterOutput {
        match record.kind.as_str() {
            "response" => response::dispatch(self, record),
            "extension_ui_request" => extension::request(self, record),
            "agent_start" => {
                self.pending_tool_result_messages.clear();
                self.is_streaming = true;
                AdapterOutput::event(AgentEvent::Session(SessionEvent::Status(
                    AgentStatus::Running,
                )))
            }
            "agent_settled" => {
                self.pending_tool_result_messages.clear();
                self.is_streaming = false;
                let mut output = session::refresh_stats(self);
                output
                    .events
                    .push(AgentEvent::Session(SessionEvent::Status(AgentStatus::Idle)));
                output
            }
            "turn_start" => {
                self.current_turn = self.current_turn.saturating_add(1);
                self.timeline(TimelineFact::TurnStart)
            }
            "turn_end" => self.timeline(TimelineFact::TurnEnd {
                reason: record
                    .field("message")
                    .and_then(|message| message.get("stopReason"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                error_message: record
                    .field("message")
                    .and_then(|message| message.get("errorMessage"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                error_code: None,
            }),
            "message_update" => tool::message_update(self, &record),
            "message_end" => {
                let mut output = record
                    .field("message")
                    .map(|message| session::live_message(self, message))
                    .unwrap_or_default();
                if record.field("message").is_some_and(|message| {
                    matches!(
                        message.get("role").and_then(Value::as_str),
                        Some("assistant" | "toolResult")
                    )
                }) {
                    output.merge(session::refresh_stats(self));
                }
                output
            }
            "tool_execution_start" => tool::tool_start(self, &record),
            "tool_execution_end" => tool::tool_end(self, &record),
            "compaction_start" => {
                let controls_open = compaction::manual_started(self, record.string("reason"));
                let id = self.request_id("compaction");
                let automatic = compaction::is_automatic(record.string("reason"));
                self.active_compaction_id = Some(id.clone());
                self.active_compaction_automatic = Some(automatic);
                self.active_compaction_model = if automatic {
                    None
                } else {
                    compaction::active_model_name(self, record.string("reason"))
                };
                let mut output = if automatic {
                    self.timeline(TimelineFact::AutoCompactionStarted { id })
                } else {
                    self.timeline(TimelineFact::CompactionStarted {
                        id,
                        model_name: self.active_compaction_model.clone(),
                    })
                };
                if controls_open {
                    response::drain_deferred(self, &mut output);
                }
                output
            }
            "compaction_end" => {
                let error = record
                    .string("errorMessage")
                    .map(str::to_owned)
                    .or_else(|| {
                        (record.bool("aborted") == Some(true))
                            .then(|| "Compaction cancelled".into())
                    });
                let model_name = self.active_compaction_model.take();
                let id = self
                    .active_compaction_id
                    .take()
                    .unwrap_or_else(|| self.request_id("compaction"));
                let automatic = self
                    .active_compaction_automatic
                    .take()
                    .unwrap_or_else(|| compaction::is_automatic(record.string("reason")));
                let mut output = if automatic {
                    self.timeline(TimelineFact::AutoCompactionFinished { id, error })
                } else {
                    self.timeline(TimelineFact::CompactionFinished {
                        id,
                        model_name,
                        error,
                    })
                };
                output.merge(session::refresh_stats(self));
                output
            }
            "auto_retry_start" => self.timeline(TimelineFact::RetryScheduled {
                id: "pi-auto-retry".into(),
                retry: record.field("attempt").and_then(Value::as_u64).unwrap_or(1),
                max_retries: record.field("maxAttempts").and_then(Value::as_u64),
                delay_ms: record.field("delayMs").and_then(Value::as_u64).unwrap_or(0),
                message: record
                    .string("errorMessage")
                    .unwrap_or("Pi retry")
                    .to_owned(),
            }),
            "auto_retry_end" => self.timeline(TimelineFact::RetryStarted {
                id: "pi-auto-retry".into(),
                retry: record.field("attempt").and_then(Value::as_u64).unwrap_or(1),
            }),
            "extension_error" => {
                // An unrelated extension failing while the catalog steps are
                // outstanding must not turn the reload into a failure; its own
                // error event below still reaches the frontend.
                if let Some(pending) = self.pending_reload.as_mut() {
                    if pending.step == ReloadStep::CompanionPrompt {
                        pending.error =
                            Some(record.string("error").unwrap_or("Pi reload failed").into());
                    }
                }
                AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: "pi-extension".into(),
                    message: record
                        .string("error")
                        .unwrap_or("Pi extension error")
                        .to_owned(),
                }))
            }
            "queue_update" => queue::update(self, &record),
            "agent_end" | "message_start" | "tool_execution_update" => AdapterOutput::default(),
            _ => AdapterOutput::default(),
        }
    }

    fn commands_response(&mut self, data: Option<&Value>) -> AdapterOutput {
        let mut commands = [
            ("fork", "Fork from a previous user message"),
            ("clone", "Duplicate the current branch"),
        ]
        .into_iter()
        .map(|(name, description)| CommandDescriptor {
            name: name.into(),
            description: description.into(),
            input_hint: Some("[message]".into()),
        })
        .collect::<Vec<_>>();
        let mut skills = Vec::new();
        let mut auth_context = false;
        let mut auth_refresh = false;
        self.reload_available = false;
        for command in data
            .and_then(|data| data.get("commands"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = command.get("name").and_then(Value::as_str) else {
                continue;
            };
            if matches!(name, "fork" | "clone") {
                continue;
            }
            if name == RELOAD_COMMAND {
                self.reload_available = true;
                continue;
            }
            if name == AUTH_CONTEXT_COMMAND {
                auth_context = true;
                continue;
            }
            if name == AUTH_REFRESH_COMMAND {
                auth_refresh = true;
                continue;
            }
            let description = command
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if command.get("source").and_then(Value::as_str) == Some("skill") {
                skills.push(Skill {
                    name: name.strip_prefix("skill:").unwrap_or(name).to_owned(),
                    description,
                });
                continue;
            }
            commands.push(CommandDescriptor {
                name: name.to_owned(),
                description,
                input_hint: None,
            });
        }
        let auth_companion = auth_context && auth_refresh;
        if auth_companion {
            commands.push(CommandDescriptor {
                name: "logout".into(),
                description: "Remove a stored provider credential".into(),
                input_hint: None,
            });
        }
        let mut output = AdapterOutput {
            commands: Vec::new(),
            events: vec![
                AgentEvent::Catalog(CatalogEvent::Commands(commands)),
                AgentEvent::Catalog(CatalogEvent::Skills(skills)),
            ],
        };
        if auth_companion {
            output.commands.push(RpcCommand::Prompt {
                id: Some(self.request_id("auth-context")),
                message: format!("/{AUTH_CONTEXT_COMMAND}"),
                streaming_behavior: None,
            });
        }
        output
    }

    fn timeline(&mut self, fact: TimelineFact) -> AdapterOutput {
        AdapterOutput::event(AgentEvent::Timeline(TimelineEvent::Append(
            self.record_fact(fact),
        )))
    }

    fn record_fact(&mut self, fact: TimelineFact) -> TimelineRecord {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        TimelineRecord {
            sequence: Some(sequence),
            time_ms: None,
            surface: None,
            source_sequences: Vec::new(),
            fact,
        }
    }

    fn refresh_commands(&mut self) -> Vec<RpcCommand> {
        vec![
            RpcCommand::GetState {
                id: Some(self.request_id("state")),
            },
            RpcCommand::GetMessages {
                id: Some(self.request_id("messages")),
            },
            RpcCommand::GetCommands {
                id: Some(self.request_id("commands")),
            },
            RpcCommand::GetAvailableModels {
                id: Some(self.request_id("models")),
            },
        ]
    }

    fn model_refresh_commands(&mut self) -> Vec<RpcCommand> {
        vec![
            RpcCommand::GetState {
                id: Some(self.request_id("state")),
            },
            RpcCommand::GetAvailableModels {
                id: Some(self.request_id("models")),
            },
        ]
    }

    fn request_id(&mut self, prefix: &str) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!("pie-{prefix}-{id}")
    }

    fn unsupported(&self, message: &str) -> AdapterOutput {
        AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
            code: "pi-unsupported".into(),
            message: message.into(),
        }))
    }

    fn protocol_error(&self, message: String) -> AdapterOutput {
        AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
            code: "pi-protocol".into(),
            message,
        }))
    }
}

mod compaction;
#[cfg(test)]
mod compaction_tests;
mod content;
#[cfg(test)]
mod cost_tests;
mod extension;
mod fork;
mod model;
mod queue;
#[cfg(test)]
mod queue_tests;
#[cfg(test)]
mod reload_tests;
mod request;
mod response;
mod session;
mod tool;

#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        content::user_fact,
        tool::{tool_activity, tool_result_fact},
    };
    use crate::protocol::{ExtensionUiResponse, StreamingBehavior};
    use e_tui::action::PromptInput;
    use e_tui::action::QuestionAnswer;
    use e_tui::{
        agent::{
            timeline::ContentBlock,
            tool::{ToolActivity, ToolCapability, ToolReference},
        },
        preview::{MutationDiff, ToolPreview, ToolPreviewPrimary},
    };

    fn record(value: Value) -> RpcRecord {
        RpcRecord::from_value(value).unwrap()
    }

    #[test]
    fn skill_reads_match_live_and_snapshot_without_changing_preview_or_history() {
        for (path, expected_name) in [
            (r"C:\Users\user\.agents\skills\review\SKILL.md", "review"),
            ("/home/user/.pi/skills/audit/SKILL.md", "audit"),
            (".pi/skills/local/SKILL.md", "local"),
            ("SKILL.md", ""),
        ] {
            let mut adapter = PiAdapter::new("/repo", "sessions");
            let live = adapter.record(record(serde_json::json!({
                "type": "tool_execution_start", "toolCallId": "skill-read",
                "toolName": "read", "args": {"path": path}
            })));
            let replay = super::session::snapshot_message(
                &mut adapter,
                &serde_json::json!({
                    "role": "assistant", "content": [{"type": "toolCall", "id": "skill-read",
                    "name": "read", "arguments": {"path": path}}]
                }),
                Some(1),
            );
            let AgentEvent::Timeline(TimelineEvent::Append(live_record)) = &live.events[0] else {
                panic!()
            };
            let TimelineFact::ToolCall(activity) = &live_record.fact else {
                panic!()
            };
            assert_eq!(replay.last().unwrap().fact, live_record.fact);
            assert_eq!(activity.capability, ToolCapability::SkillRead);
            assert_eq!(activity.summary, expected_name);
            assert!(matches!(&activity.preview, Some(ToolPreview {
                primary: ToolPreviewPrimary::Location { path: location, .. }, ..
            }) if location == path));
            let operation =
                e_tui::execution_history::OperationStart::from_tool(activity, None, None);
            assert_eq!(
                operation.kind,
                e_tui::execution_history::OperationKind::Read
            );
        }
        for (name, path) in [
            ("read", "README.md"),
            ("read", "skill.md"),
            ("read", "SKILL.md.bak"),
            ("edit", "review/SKILL.md"),
        ] {
            assert_ne!(
                tool_activity("ordinary", name, serde_json::json!({"path": path})).capability,
                ToolCapability::SkillRead
            );
        }
    }

    #[test]
    fn startup_queries_authoritative_surfaces() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let commands = adapter.startup_commands();
        assert_eq!(commands.len(), 4);
        assert!(matches!(commands[0], RpcCommand::GetState { .. }));
        assert!(matches!(commands[1], RpcCommand::GetMessages { .. }));
        assert!(matches!(commands[3], RpcCommand::GetAvailableModels { .. }));
    }

    #[test]
    fn skill_commands_are_kept_out_of_the_integrated_command_catalog() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = adapter.commands_response(Some(&serde_json::json!({
            "commands": [
                {"name":"review", "description":"Review", "source":"prompt"},
                {"name":"skill:code-review", "description":"Review code", "source":"skill"}
            ]
        })));
        assert!(matches!(
            output.events.as_slice(),
            [
                AgentEvent::Catalog(CatalogEvent::Commands(commands)),
                AgentEvent::Catalog(CatalogEvent::Skills(skills)),
            ] if commands.len() == 1
                && commands[0].name == "review"
                && skills.len() == 1
                && skills[0].name == "code-review"
        ));
    }

    #[test]
    fn auth_companion_controls_are_hidden_and_enable_logout() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = adapter.commands_response(Some(&serde_json::json!({
            "commands": [
                {"name": AUTH_CONTEXT_COMMAND, "description": "internal"},
                {"name": AUTH_REFRESH_COMMAND, "description": "internal"}
            ]
        })));
        assert!(matches!(
            output.events.as_slice(),
            [AgentEvent::Catalog(CatalogEvent::Commands(commands)), _]
                if commands.len() == 1 && commands[0].name == "logout"
        ));
        assert!(matches!(
            output.commands.as_slice(),
            [RpcCommand::Prompt { message, .. }]
                if message == &format!("/{AUTH_CONTEXT_COMMAND}")
        ));
    }

    #[test]
    fn expanded_skill_message_projects_as_injected_context() {
        let text = "<skill name=\"code-review\" location=\"C:/skills/code-review/SKILL.md\">\nReferences are relative to C:/skills/code-review.\n\nReview carefully.\n</skill>";
        let fact = user_fact(&serde_json::json!({
            "content": [{"type":"text", "text":text}]
        }));
        assert!(matches!(
            fact,
            TimelineFact::UserMessage { text: actual, source_kind, source, .. }
                if actual == text
                    && source_kind.as_deref() == Some("skill-invocation")
                    && source.kind.as_deref() == Some("skill-invocation")
                    && source.form.as_deref() == Some("instructions")
                    && source.summary.as_deref() == Some("code-review")
                    && source.producer.as_deref() == Some("pi")
        ));
    }

    #[test]
    fn user_image_content_remains_visible_in_the_timeline_projection() {
        let fact = user_fact(&serde_json::json!({
            "content": [
                {"type":"text", "text":"look"},
                {"type":"image", "data":"AA==", "mimeType":"image/png"}
            ]
        }));
        assert!(matches!(
            fact,
            TimelineFact::UserMessage { text, content, .. }
                if text == "look"
                    && content == vec![
                        ContentBlock::Text("look".into()),
                        ContentBlock::Image { label: "image".into() },
                    ]
        ));
    }

    #[test]
    fn streaming_prompt_uses_steering_behavior() {
        let mut adapter = PiAdapter::new(".", "sessions");
        adapter.record(record(serde_json::json!({"type":"agent_start"})));
        let output = adapter.request(AgentRequest::Input {
            prompt: e_tui::PromptInput::text("next"),
        });
        assert!(matches!(
            output.commands.as_slice(),
            [RpcCommand::Prompt {
                streaming_behavior: Some(StreamingBehavior::Steer),
                ..
            }]
        ));
    }

    #[test]
    fn explicit_steer_request_uses_steering_behavior() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = adapter.request(AgentRequest::Steer {
            prompt: e_tui::PromptInput::text("next"),
        });
        assert!(matches!(
            output.commands.as_slice(),
            [RpcCommand::Prompt {
                streaming_behavior: Some(StreamingBehavior::Steer),
                ..
            }]
        ));
    }

    fn reply_first(adapter: &mut PiAdapter, output: &AdapterOutput, data: Value) -> AdapterOutput {
        let command = serde_json::to_value(
            output
                .commands
                .iter()
                .find(|command| !matches!(command, RpcCommand::GetSessionStats { .. }))
                .expect("foreground command"),
        )
        .unwrap();
        adapter.record(record(serde_json::json!({
            "type": "response", "id": command["id"], "command": command["type"],
            "success": true, "data": data,
        })))
    }

    #[test]
    fn deferred_new_waits_for_success_before_prompt() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let start = adapter.request(AgentRequest::NewInput {
            mode: "pi".into(),
            prompt: e_tui::PromptInput::text("first"),
        });
        let [RpcCommand::NewSession { id: Some(id) }] = start.commands.as_slice() else {
            panic!("expected new_session")
        };
        let done = adapter.record(record(serde_json::json!({
            "type":"response", "id":id, "command":"new_session", "success":true,
            "data":{"cancelled":false}
        })));
        assert!(!done
            .commands
            .iter()
            .any(|command| matches!(command, RpcCommand::Prompt { .. })));
        let ready = reply_first(&mut adapter, &done, serde_json::json!({"sessionId":"new"}));
        assert!(ready.commands.iter().any(
            |command| matches!(command, RpcCommand::Prompt { message, .. } if message == "first")
        ));
    }

    #[test]
    fn model_and_effort_settle_before_first_skill_materializes() {
        let mut adapter = PiAdapter::new(".", "sessions");
        adapter.current_model = Some(serde_json::json!({"provider":"p", "id":"B"}));
        adapter.thinking_level = Some("low".into());
        let selected = adapter.request(AgentRequest::ModelSet {
            provider: "p".into(),
            model: "A".into(),
            reasoning_effort: Some("high".into()),
        });
        let queued = adapter.request(AgentRequest::NewInput {
            mode: "pi".into(),
            prompt: PromptInput::text("/skill review"),
        });
        assert!(queued.commands.is_empty());
        let effort = reply_first(
            &mut adapter,
            &selected,
            serde_json::json!({"provider":"p", "id":"A"}),
        );
        assert!(
            matches!(effort.commands.as_slice(), [RpcCommand::SetThinkingLevel { level, .. }] if level == "high")
        );
        let refresh = reply_first(&mut adapter, &effort, Value::Null);
        let create = reply_first(
            &mut adapter,
            &refresh,
            serde_json::json!({
                "sessionId":"old", "model":{"provider":"p", "id":"A"}, "thinkingLevel":"high"
            }),
        );
        assert!(matches!(
            create.commands.as_slice(),
            [
                RpcCommand::GetSessionStats { .. },
                RpcCommand::NewSession { .. }
            ]
        ));
        adapter.record(record(serde_json::json!({
            "type":"response", "id":"late-state", "command":"get_state", "success":true,
            "data":{"sessionId":"old", "model":{"provider":"p", "id":"B"}, "thinkingLevel":"low"}
        })));
        let restore_model = reply_first(
            &mut adapter,
            &create,
            serde_json::json!({"cancelled":false}),
        );
        assert!(
            matches!(restore_model.commands.as_slice(), [RpcCommand::SetModel { model_id, .. }] if model_id == "A")
        );
        let restore_effort = reply_first(
            &mut adapter,
            &restore_model,
            serde_json::json!({"provider":"p", "id":"A"}),
        );
        assert!(
            matches!(restore_effort.commands.as_slice(), [RpcCommand::SetThinkingLevel { level, .. }] if level == "high")
        );
        let refresh = reply_first(&mut adapter, &restore_effort, Value::Null);
        assert!(!refresh
            .commands
            .iter()
            .any(|command| matches!(command, RpcCommand::Prompt { .. })));
        let prompt = reply_first(
            &mut adapter,
            &refresh,
            serde_json::json!({
                "sessionId":"new", "model":{"provider":"p", "id":"A"}, "thinkingLevel":"high"
            }),
        );
        assert!(
            matches!(prompt.commands.as_slice(), [RpcCommand::GetSessionStats { .. }, RpcCommand::Prompt { message, .. }] if message == "/skill:review")
        );
        assert_eq!(adapter.current_model.as_ref().unwrap()["id"], "A");
        assert_eq!(adapter.thinking_level.as_deref(), Some("high"));
        assert!(adapter.configuration_request.is_none());
    }

    #[test]
    fn skill_trailing_prompt_waits_for_admission_in_existing_and_new_sessions() {
        for drafting in [false, true] {
            for prefix in ["/skill:review", "/skill review"] {
                let mut adapter = PiAdapter::new(".", "sessions");
                adapter.is_streaming = prefix == "/skill review";
                let text = "检查  code\n  next line  ";
                let line = format!("{prefix} \t\n {text}");
                let skill = if drafting {
                    let create = adapter.request(AgentRequest::NewInput {
                        mode: "pi".into(),
                        prompt: PromptInput::text(line),
                    });
                    let refresh = reply_first(
                        &mut adapter,
                        &create,
                        serde_json::json!({"cancelled":false}),
                    );
                    reply_first(
                        &mut adapter,
                        &refresh,
                        serde_json::json!({"sessionId":"new"}),
                    )
                } else {
                    adapter.request(AgentRequest::Command {
                        line,
                        images: vec![],
                    })
                };
                let prompts: Vec<_> = skill
                    .commands
                    .iter()
                    .filter_map(|command| match command {
                        RpcCommand::Prompt { message, .. } => Some(message.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(prompts, ["/skill:review"]);
                assert!(adapter
                    .request(AgentRequest::Attach {
                        session_id: "later".into()
                    })
                    .commands
                    .is_empty());
                let body = reply_first(&mut adapter, &skill, Value::Null);
                let prompts: Vec<_> = body
                    .commands
                    .iter()
                    .filter_map(|command| match command {
                        RpcCommand::Prompt {
                            message,
                            streaming_behavior,
                            ..
                        } => Some((message.as_str(), *streaming_behavior)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(prompts, [(text, Some(StreamingBehavior::Steer))]);
                assert!(!body
                    .commands
                    .iter()
                    .any(|command| { matches!(command, RpcCommand::SwitchSession { .. }) }));
                let tail = body
                    .commands
                    .iter()
                    .find(|command| matches!(command, RpcCommand::Prompt { .. }))
                    .unwrap()
                    .clone();
                let later = reply_first(&mut adapter, &AdapterOutput::command(tail), Value::Null);
                assert!(matches!(later.commands.as_slice(),
                    [RpcCommand::SwitchSession { session_path, .. }] if session_path == "later"
                ));
                assert!(adapter.pending_skill_prompt.is_none());
            }
        }
    }

    #[test]
    fn skill_trailing_prompt_is_discarded_on_failure_interrupt_or_session_change() {
        for reason in ["failure", "interrupt", "session-change"] {
            let mut adapter = PiAdapter::new(".", "sessions");
            let skill = adapter.request(AgentRequest::Command {
                line: "/skill:review must not send".into(),
                images: vec![],
            });
            let [RpcCommand::Prompt { id: Some(id), .. }] = skill.commands.as_slice() else {
                panic!("expected skill prompt");
            };
            if reason == "interrupt" {
                adapter.request(AgentRequest::Interrupt);
            } else if reason == "session-change" {
                adapter.record(record(serde_json::json!({
                    "type":"response", "command":"get_state", "success":true,
                    "data":{"sessionId":"another"}
                })));
            }
            let result = adapter.record(record(serde_json::json!({
                "type":"response", "id":id, "command":"prompt",
                "success": reason != "failure", "error":"failed"
            })));
            assert!(!result
                .commands
                .iter()
                .any(|command| matches!(command, RpcCommand::Prompt { .. })));
            assert!(adapter.pending_skill_prompt.is_none());
        }
    }

    #[test]
    fn skill_without_body_never_sends_an_empty_followup() {
        for line in ["/skill:review", "/skill review \t\n "] {
            let mut adapter = PiAdapter::new(".", "sessions");
            let skill = adapter.request(AgentRequest::Command {
                line: line.into(),
                images: vec![],
            });
            assert!(matches!(skill.commands.as_slice(),
                [RpcCommand::Prompt { message, .. }] if message == "/skill:review"
            ));
            let result = reply_first(&mut adapter, &skill, Value::Null);
            assert!(!result
                .commands
                .iter()
                .any(|command| matches!(command, RpcCommand::Prompt { .. })));
        }
    }

    #[test]
    fn cancelled_new_session_never_sends_the_skill() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let create = adapter.request(AgentRequest::NewInput {
            mode: "pi".into(),
            prompt: PromptInput::text("/skill:review"),
        });
        let cancelled = reply_first(&mut adapter, &create, serde_json::json!({"cancelled":true}));
        assert!(cancelled.commands.is_empty());
        assert!(
            matches!(cancelled.events.as_slice(), [AgentEvent::Interaction(InteractionEvent::Error { code, .. })] if code == "new-failed")
        );
        assert!(adapter.configuration_request.is_none());
    }

    #[test]
    fn snapshot_turns_seed_live_correlation_and_user_card_kind() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let snapshot = adapter.record(record(serde_json::json!({
            "type":"response", "id":"m", "command":"get_messages", "success":true,
            "data":{"messages":[
                {"role":"user","content":"old prompt"},
                {"role":"assistant","content":[{"type":"text","text":"old answer"}]}
            ]}
        })));
        assert!(matches!(
            &snapshot.events[0],
            AgentEvent::Timeline(TimelineEvent::Snapshot { records, .. })
                if matches!(
                    &records[1].fact,
                    TimelineFact::AssistantMessage { turn: Some(1), step: Some(0), .. }
                )
        ));

        adapter.record(record(serde_json::json!({"type":"turn_start"})));
        let user = adapter.record(record(serde_json::json!({
            "type":"message_end", "message":{"role":"user","content":"new prompt"}
        })));
        assert!(matches!(
            &user.events[0],
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                fact: TimelineFact::UserMessage { source_kind: Some(kind), .. }, ..
            })) if kind == "user"
        ));

        let delta = adapter.record(record(serde_json::json!({
            "type":"message_update", "usage":{},
            "assistantMessageEvent":{"type":"text_delta","delta":"new answer"}
        })));
        assert!(matches!(
            &delta.events[0],
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                fact: TimelineFact::AssistantChunk {
                    turn: Some(2),
                    step: Some(0),
                    ..
                },
                ..
            }))
        ));
    }

    #[test]
    fn text_delta_and_tool_events_are_normalized() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let delta = adapter.record(record(serde_json::json!({
            "type":"message_update", "usage":{"input":3,"output":1},
            "assistantMessageEvent":{"type":"text_delta","delta":"hi"}
        })));
        assert!(matches!(
            &delta.events[0],
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                fact: TimelineFact::AssistantChunk { text, .. }, ..
            })) if text == "hi"
        ));

        let tool = adapter.record(record(serde_json::json!({
            "type":"tool_execution_start", "toolCallId":"c1", "toolName":"read",
            "args":{"path":"src/lib.rs"}
        })));
        assert!(matches!(
            &tool.events[0],
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                fact: TimelineFact::ToolCall(ToolActivity {
                    capability: ToolCapability::Read,
                    ..
                }),
                ..
            }))
        ));
    }

    #[test]
    fn pi_tool_result_is_projected_once_and_waits_for_turn_start() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let result = adapter.record(record(serde_json::json!({
            "type":"tool_execution_end", "toolCallId":"c1", "toolName":"read",
            "result":{"content":[{"type":"text","text":"ok"}]}, "isError":false
        })));
        assert!(matches!(
            result.events.as_slice(),
            [AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                fact: TimelineFact::ToolResult { activity_id, starts_thinking: false, .. },
                ..
            }))] if activity_id == "c1"
        ));

        let durable = adapter.record(record(serde_json::json!({
            "type":"message_end", "message":{
                "role":"toolResult", "toolCallId":"c1", "toolName":"read",
                "content":[{"type":"text","text":"ok"}], "isError":false
            }
        })));
        assert!(
            durable.events.is_empty(),
            "durable tool result duplicates the execution end"
        );

        let turn = adapter.record(record(serde_json::json!({"type":"turn_start"})));
        assert!(matches!(
            turn.events.as_slice(),
            [AgentEvent::Timeline(TimelineEvent::Append(
                TimelineRecord {
                    fact: TimelineFact::TurnStart,
                    ..
                }
            ))]
        ));
    }

    #[test]
    fn pi_edit_calls_use_ordered_mutation_hunks_for_current_and_legacy_inputs() {
        let current = tool_activity(
            "e1",
            "edit",
            serde_json::json!({
                "path":"src/lib.rs",
                "edits":[
                    {"oldText":"old one","newText":"new one"},
                    {"oldText":"old two","newText":"new two"}
                ]
            }),
        );
        assert!(current.preview.is_none());
        assert!(matches!(
            current.reference,
            Some(ToolReference::Hunks(ref hunks))
                if hunks.len() == 2
                    && hunks[0].path.as_deref() == Some("src/lib.rs")
                    && hunks[0].old.as_deref() == Some("old one")
                    && hunks[1].new.as_deref() == Some("new two")
        ));

        let legacy = tool_activity(
            "e2",
            "edit",
            serde_json::json!({
                "path":"src/legacy.rs", "oldText":"before", "newText":"after"
            }),
        );
        assert!(matches!(
            legacy.reference,
            Some(ToolReference::Hunks(ref hunks))
                if hunks.len() == 1
                    && hunks[0].old.as_deref() == Some("before")
                    && hunks[0].new.as_deref() == Some("after")
        ));
    }

    #[test]
    fn incomplete_pi_edit_soft_falls_back_to_path_or_json() {
        let with_path = tool_activity(
            "e1",
            "edit",
            serde_json::json!({"path":"src/lib.rs", "edits":[{"oldText":"old"}]}),
        );
        assert!(matches!(
            with_path.reference,
            Some(ToolReference::Path { ref path }) if path == "src/lib.rs"
        ));
        assert!(with_path.preview.is_none());

        let without_path = tool_activity(
            "e2",
            "edit",
            serde_json::json!({"edits":[{"oldText":"old"}]}),
        );
        assert!(without_path.reference.is_none());
        assert!(matches!(
            without_path.preview,
            Some(ToolPreview {
                primary: ToolPreviewPrimary::Json { .. },
                ..
            })
        ));
    }

    #[test]
    fn live_and_replayed_pi_edit_results_keep_authoritative_patch() {
        let patch = "--- src/lib.rs\n+++ src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let mut adapter = PiAdapter::new(".", "sessions");
        let live = adapter.record(record(serde_json::json!({
            "type":"tool_execution_end", "toolCallId":"e1", "toolName":"edit",
            "result":{"content":[{"type":"text","text":"ok"}],"details":{"patch":patch}},
            "isError":false
        })));
        assert!(matches!(
            &live.events[0],
            AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
                fact: TimelineFact::ToolResult {
                    mutation_diff: Some(MutationDiff { source, .. }),
                    ..
                },
                ..
            })) if source == patch
        ));

        assert!(matches!(
            tool_result_fact(&serde_json::json!({
                "role":"toolResult", "toolCallId":"e2", "toolName":"edit",
                "content":[{"type":"text","text":"ok"}],
                "details":{"patch":patch}, "isError":false
            })),
            TimelineFact::ToolResult {
                mutation_diff: Some(MutationDiff { source, .. }),
                ..
            } if source == patch
        ));
        assert!(matches!(
            tool_result_fact(&serde_json::json!({
                "role":"toolResult", "toolCallId":"e3", "toolName":"edit",
                "content":[{"type":"text","text":"failed"}],
                "details":{"patch":patch}, "isError":true
            })),
            TimelineFact::ToolResult {
                mutation_diff: None,
                ..
            }
        ));
    }

    #[test]
    fn extension_editor_text_targets_the_frontend_composer() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = adapter.record(record(serde_json::json!({
            "type":"extension_ui_request", "id":"u0", "method":"set_editor_text",
            "text":"extension draft"
        })));
        assert!(matches!(
            output.events.as_slice(),
            [AgentEvent::Interaction(InteractionEvent::SetEditorText { text })]
                if text == "extension draft"
        ));
    }

    #[test]
    fn extension_select_round_trips_through_question() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = adapter.record(record(serde_json::json!({
            "type":"extension_ui_request", "id":"u1", "method":"select",
            "title":"Pick", "options":["A","B"]
        })));
        assert!(matches!(
            &output.events[0],
            AgentEvent::Interaction(InteractionEvent::Question { request_id, .. }) if request_id == "u1"
        ));
        let answer = adapter.request(AgentRequest::AnswerQuestions {
            request_id: "u1".into(),
            answers: vec![QuestionAnswer {
                id: "value".into(),
                selected: vec!["B".into()],
                custom: None,
            }],
        });
        assert!(matches!(
            answer.commands.as_slice(),
            [RpcCommand::ExtensionUiResponse { id, response: ExtensionUiResponse::Value { value } }]
                if id == "u1" && value == "B"
        ));
    }

    fn get_state_record(id: &str, session_file: &str) -> RpcRecord {
        record(serde_json::json!({
            "type":"response", "id":id, "command":"get_state", "success":true,
            "data":{
                "sessionId":"sess-1",
                "sessionFile":session_file,
                "sessionName":"Old",
                "isStreaming":false,
                "model":{"provider":"openai","id":"gpt-5"}
            }
        }))
    }

    #[test]
    fn same_session_state_refresh_does_not_re_attach() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let first = adapter.record(get_state_record("s1", "sessions/a.jsonl"));
        assert!(matches!(
            first.events.first(),
            Some(AgentEvent::Session(SessionEvent::Attached(attached)))
                if attached.id == "sessions/a.jsonl"
        ));

        // The model refresh after `/model` re-queries state for the same
        // session; re-emitting `Attached` would make the frontend drop a
        // pending `/new` draft and visibly jump back to the old conversation.
        let second = adapter.record(get_state_record("s2", "sessions/a.jsonl"));
        assert!(matches!(
            second.events.first(),
            Some(AgentEvent::Session(SessionEvent::Status(AgentStatus::Idle)))
        ));

        // An explicit attach resets the dedup key so re-attaching the current
        // session still reports a real attach.
        adapter.request(AgentRequest::Attach {
            session_id: "sessions/a.jsonl".into(),
        });
        let third = adapter.record(get_state_record("s3", "sessions/a.jsonl"));
        assert!(matches!(
            third.events.first(),
            Some(AgentEvent::Session(SessionEvent::Attached(_)))
        ));
    }

    fn unnamed_get_state(id: &str, session_file: &str) -> RpcRecord {
        record(serde_json::json!({
            "type":"response", "id":id, "command":"get_state", "success":true,
            "data":{
                "sessionId":"sess-1",
                "sessionFile":session_file,
                "isStreaming":false,
                "model":{"provider":"openai","id":"gpt-5"}
            }
        }))
    }

    fn title_event(output: &AdapterOutput) -> Option<&String> {
        output.events.iter().find_map(|event| match event {
            AgentEvent::Session(SessionEvent::Title(title)) => Some(title),
            _ => None,
        })
    }

    #[test]
    fn first_user_message_becomes_the_status_bar_title() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let attached = adapter.record(unnamed_get_state("s1", "sessions/a.jsonl"));
        assert!(matches!(
            attached.events.first(),
            Some(AgentEvent::Session(SessionEvent::Attached(attached)))
                if attached.title.is_none()
        ));

        // Resuming an unnamed session: the snapshot's first user message is
        // the title the session list already shows.
        let snapshot = adapter.record(record(serde_json::json!({
            "type":"response", "id":"m1", "command":"get_messages", "success":true,
            "data":{"messages":[
                {"role":"assistant","content":"hello"},
                {"role":"user","content":"  Fix the   login bug  "}
            ]}
        })));
        assert_eq!(
            title_event(&snapshot),
            Some(&"Fix the login bug".to_owned())
        );

        // An explicit rename through a same-session refresh wins over the
        // first-message fallback.
        let renamed = adapter.record(get_state_record("s2", "sessions/a.jsonl"));
        assert_eq!(title_event(&renamed), Some(&"Old".to_owned()));
        assert!(matches!(
            renamed.events.first(),
            Some(AgentEvent::Session(SessionEvent::Status(_)))
        ));
    }

    #[test]
    fn first_prompt_reports_its_title_immediately() {
        let mut adapter = PiAdapter::new(".", "sessions");
        adapter.record(unnamed_get_state("s1", "sessions/a.jsonl"));

        let output = adapter.request(AgentRequest::Input {
            prompt: PromptInput::text("Fix the login bug"),
        });
        assert!(matches!(
            output.commands.first(),
            Some(RpcCommand::Prompt { .. })
        ));
        assert_eq!(title_event(&output), Some(&"Fix the login bug".to_owned()));

        // Later prompts keep the first message as the title.
        let output = adapter.request(AgentRequest::Input {
            prompt: PromptInput::text("And another thing"),
        });
        assert_eq!(title_event(&output), None);
    }

    #[test]
    fn new_session_prompt_reports_its_title_once_the_switch_settles() {
        let mut adapter = PiAdapter::new(".", "sessions");
        adapter.record(unnamed_get_state("s1", "sessions/a.jsonl"));

        let commands = adapter
            .request(AgentRequest::NewInput {
                mode: "chat".into(),
                prompt: PromptInput::text("Fix the login bug"),
            })
            .commands;
        assert!(matches!(
            commands.as_slice(),
            [RpcCommand::NewSession { .. }]
        ));

        let command = serde_json::to_value(&commands[0]).unwrap();
        let output = adapter.record(record(serde_json::json!({
            "type":"response", "id":command["id"], "command":"new_session", "success":true
        })));
        assert_eq!(title_event(&output), None);
        assert!(matches!(
            output.commands.first(),
            Some(RpcCommand::SetModel { .. })
        ));
        let refresh = reply_first(
            &mut adapter,
            &output,
            serde_json::json!({"provider":"openai", "id":"gpt-5"}),
        );
        assert!(matches!(
            refresh.commands.first(),
            Some(RpcCommand::GetState { .. })
        ));
        let attached = reply_first(
            &mut adapter,
            &refresh,
            serde_json::json!({
                "sessionId":"s2", "sessionFile":"sessions/b.jsonl", "isStreaming":false,
                "model":{"provider":"openai", "id":"gpt-5"}
            }),
        );
        assert!(attached
            .commands
            .iter()
            .any(|command| matches!(command, RpcCommand::Prompt { .. })));
        assert!(matches!(
            attached.events.first(),
            Some(AgentEvent::Session(SessionEvent::Attached(attached)))
                if attached.title.is_none()
        ));

        // ...and the opening prompt's user message reports the title.
        let output = adapter.record(record(serde_json::json!({
            "type":"message_end", "message":{"role":"user","content":"Fix the login bug"}
        })));
        assert_eq!(title_event(&output), Some(&"Fix the login bug".to_owned()));
    }

    #[test]
    fn command_prompt_refreshes_state_to_report_renames() {
        let mut adapter = PiAdapter::new(".", "sessions");
        adapter.record(unnamed_get_state("s1", "sessions/a.jsonl"));

        let output = adapter.request(AgentRequest::Command {
            line: "/name New name".into(),
            images: Vec::new(),
        });
        assert!(matches!(
            output.commands.as_slice(),
            [RpcCommand::Prompt { .. }]
        ));

        let refresh = reply_first(&mut adapter, &output, Value::Null);
        assert!(matches!(
            refresh.commands.as_slice(),
            [RpcCommand::GetState { .. }]
        ));

        let state = adapter.record(record(serde_json::json!({
            "type":"response", "id":"s2", "command":"get_state", "success":true,
            "data":{
                "sessionId":"sess-1",
                "sessionFile":"sessions/a.jsonl",
                "sessionName":"New name",
                "isStreaming":false
            }
        })));
        assert_eq!(title_event(&state), Some(&"New name".to_owned()));
    }
}
