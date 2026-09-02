//! Stateful conversion between Pi RPC and provider-neutral `e-tui` contracts.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use e_tui::{
    action::{AgentRequest, QuestionAnswer},
    agent::{
        timeline::{ContentBlock, MessageSource, TimelineFact, TimelineRecord, TokenUsage},
        tool::{ActivityState, ToolActivity, ToolCapability, ToolReference},
        AgentEvent, AgentStatus, AttachedSession, CatalogEvent, CommandDescriptor,
        InteractionEvent, ModelDescriptor, ModelProvider, ModelReasoning, ModelSelection, Question,
        QuestionOption, ReasoningEffort, SessionEvent, Skill, TimelineEvent,
    },
    preview::{
        LineSelection, MutationDiff, MutationHunk, ToolMetrics, ToolPreview, ToolPreviewPrimary,
    },
};
use serde_json::Value;

use crate::{
    protocol::{
        extension_ui_request, response, ExtensionUiRequest, ExtensionUiResponse, RpcCommand,
        RpcRecord, StreamingBehavior,
    },
    session_index,
};

const GENERIC_JSON_CHARS: usize = 2_000;

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
    last_attached_session: Option<String>,
    current_model: Option<Value>,
    available_models: Vec<Value>,
    thinking_level: Option<String>,
    thinking_levels: Vec<String>,
    pending_new: HashMap<String, String>,
    pending_model_effort: HashMap<String, Option<String>>,
    /// Pi emits both `tool_execution_end` and the durable `message_end` for
    /// one result. Retain the id only until that duplicate message arrives.
    pending_tool_result_messages: HashSet<String>,
    extension_ui: HashMap<String, PendingExtensionUi>,
}

impl PiAdapter {
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
            last_attached_session: None,
            current_model: None,
            available_models: Vec::new(),
            thinking_level: None,
            thinking_levels: vec!["off".into()],
            pending_new: HashMap::new(),
            pending_model_effort: HashMap::new(),
            pending_tool_result_messages: HashSet::new(),
            extension_ui: HashMap::new(),
        }
    }

    pub fn startup_commands(&mut self) -> Vec<RpcCommand> {
        self.refresh_commands()
    }

    pub fn request(&mut self, request: AgentRequest) -> AdapterOutput {
        match request {
            AgentRequest::Input { prompt } => {
                let Some(text) = prompt.plain_text().map(str::to_owned) else {
                    return self.unsupported("Pi image prompts must be pasted as temporary file paths");
                };
                // The first prompt of an unnamed session is its title in the
                // session list; report it immediately so the status bar stops
                // showing `新会话` as soon as the conversation starts.
                self.note_first_user_title(&text);
                let mut output = AdapterOutput::command(RpcCommand::Prompt {
                    id: Some(self.request_id("prompt")),
                    message: text,
                    streaming_behavior: self.is_streaming.then_some(StreamingBehavior::Steer),
                });
                output.events.extend(self.title_events());
                output
            }
            AgentRequest::NewInput { mode: _, prompt } => {
                let Some(text) = prompt.plain_text().map(str::to_owned) else {
                    return self.unsupported("Pi image prompts must be pasted as temporary file paths");
                };
                let id = self.request_id("new");
                self.pending_new.insert(id.clone(), text);
                AdapterOutput::command(RpcCommand::NewSession { id: Some(id) })
            }
            AgentRequest::Command { line, images } => {
                if images.is_empty() {
                    self.command_line(line)
                } else {
                    self.unsupported("Pi command images must be pasted as temporary file paths")
                }
            }
            AgentRequest::Interrupt => AdapterOutput::command(RpcCommand::Abort {
                id: Some(self.request_id("abort")),
            }),
            AgentRequest::Attach { session_id } => {
                // Force the post-switch refresh to re-emit `Attached` even when the
                // resolved session key equals the last reported one (explicit re-attach).
                self.last_attached_session = None;
                AdapterOutput::command(RpcCommand::SwitchSession {
                    id: Some(self.request_id("switch")),
                    session_path: session_id,
                })
            }
            AgentRequest::ListSessions => {
                let index = session_index::list_current_project(&self.session_root, &self.cwd);
                let mut output = AdapterOutput::event(AgentEvent::Session(SessionEvent::List {
                    sessions: index.sessions,
                    titles_pending: false,
                }));
                if !index.diagnostics.is_empty() {
                    let total = index.diagnostics.len();
                    let mut message = index
                        .diagnostics
                        .into_iter()
                        .take(3)
                        .collect::<Vec<_>>()
                        .join("; ");
                    if total > 3 {
                        message.push_str(&format!("; and {} more", total - 3));
                    }
                    output.events.push(AgentEvent::Interaction(
                        InteractionEvent::Error {
                            code: "pi-session-index".into(),
                            message,
                        },
                    ));
                }
                output
            }
            AgentRequest::ApprovalAnswer { id, allow } => {
                if self.extension_ui.remove(&id) == Some(PendingExtensionUi::Confirm) {
                    AdapterOutput::command(RpcCommand::ExtensionUiResponse {
                        id,
                        response: ExtensionUiResponse::Confirmed { confirmed: allow },
                    })
                } else {
                    self.unsupported("Pi RPC does not expose a separate approval response")
                }
            }
            AgentRequest::AnswerQuestions {
                request_id,
                answers,
            } => self.answer_extension_question(request_id, answers),
            AgentRequest::CancelQuestions { request_id } => {
                if self.extension_ui.remove(&request_id).is_some() {
                    AdapterOutput::command(RpcCommand::ExtensionUiResponse {
                        id: request_id,
                        response: ExtensionUiResponse::Cancelled { cancelled: true },
                    })
                } else {
                    AdapterOutput::default()
                }
            }
            AgentRequest::History { .. } => AdapterOutput::event(AgentEvent::Timeline(
                TimelineEvent::History {
                    records: Vec::new(),
                    has_more: false,
                },
            )),
            AgentRequest::LoginGet
            | AgentRequest::LoginSetApiKey { .. }
            | AgentRequest::LoginProxyCreate { .. }
            | AgentRequest::LoginProxyDelete { .. } => self.unsupported(
                "Manage Pi credentials with native `pi /login`; pie reuses Pi's auth.json and environment credentials",
            ),
            AgentRequest::ModelGet => AdapterOutput {
                commands: self.model_refresh_commands(),
                events: Vec::new(),
            },
            AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort,
            } => {
                let id = self.request_id("model");
                self.pending_model_effort
                    .insert(id.clone(), reasoning_effort);
                AdapterOutput::command(RpcCommand::SetModel {
                    id: Some(id),
                    provider,
                    model_id: model,
                })
            }
            AgentRequest::Ping => AdapterOutput::command(RpcCommand::GetState {
                id: Some(self.request_id("state")),
            }),
        }
    }

    pub fn record(&mut self, record: RpcRecord) -> AdapterOutput {
        match record.kind.as_str() {
            "response" => self.rpc_response(record),
            "extension_ui_request" => self.extension_request(record),
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
                AdapterOutput::event(AgentEvent::Session(SessionEvent::Status(AgentStatus::Idle)))
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
            "message_update" => self.message_update(&record),
            "message_end" => record
                .field("message")
                .map(|message| self.live_message(message))
                .unwrap_or_default(),
            "tool_execution_start" => self.tool_start(&record),
            "tool_execution_end" => self.tool_end(&record),
            "compaction_start" => self.timeline(TimelineFact::CompactionStarted {
                id: "pi-compaction".into(),
            }),
            "compaction_end" => {
                let error = record.string("errorMessage").map(str::to_owned);
                self.timeline(TimelineFact::CompactionFinished {
                    id: "pi-compaction".into(),
                    error,
                })
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
                AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: "pi-extension".into(),
                    message: record
                        .string("error")
                        .unwrap_or("Pi extension error")
                        .to_owned(),
                }))
            }
            "queue_update" | "agent_end" | "message_start" | "tool_execution_update" => {
                AdapterOutput::default()
            }
            _ => AdapterOutput::default(),
        }
    }

    fn command_line(&mut self, line: String) -> AdapterOutput {
        if let Some(rest) = line.strip_prefix("/compact") {
            return AdapterOutput::command(RpcCommand::Compact {
                id: Some(self.request_id("compact")),
                custom_instructions: (!rest.trim().is_empty()).then(|| rest.trim().to_owned()),
            });
        }
        let id = self.request_id("command");
        // Remember the id: its response triggers a same-session state refresh
        // that reports extension-side session renames.
        self.pending_command_prompt = Some(id.clone());
        AdapterOutput::command(RpcCommand::Prompt {
            id: Some(id),
            message: line,
            streaming_behavior: None,
        })
    }

    fn answer_extension_question(
        &mut self,
        request_id: String,
        answers: Vec<QuestionAnswer>,
    ) -> AdapterOutput {
        let Some(method) = self.extension_ui.remove(&request_id) else {
            return AdapterOutput::default();
        };
        if method == PendingExtensionUi::Confirm {
            return self.unsupported("invalid Pi confirmation response route");
        }
        let value = answers.into_iter().next().and_then(|answer| {
            answer
                .custom
                .filter(|value| !value.is_empty())
                .or_else(|| answer.selected.into_iter().next())
        });
        AdapterOutput::command(RpcCommand::ExtensionUiResponse {
            id: request_id,
            response: value.map_or(
                ExtensionUiResponse::Cancelled { cancelled: true },
                |value| ExtensionUiResponse::Value { value },
            ),
        })
    }

    fn rpc_response(&mut self, record: RpcRecord) -> AdapterOutput {
        let response = match response(&record) {
            Ok(response) => response,
            Err(error) => return self.protocol_error(error.to_string()),
        };
        if !response.success {
            let message = response
                .error
                .unwrap_or_else(|| format!("Pi RPC command `{}` failed", response.command));
            if let Some(id) = response.id.as_ref() {
                if self.pending_new.remove(id).is_some() {
                    return AdapterOutput::event(AgentEvent::Interaction(
                        InteractionEvent::Error {
                            code: "new-failed".into(),
                            message,
                        },
                    ));
                }
                self.pending_model_effort.remove(id);
                if response.id.as_deref() == self.pending_command_prompt.as_deref() {
                    self.pending_command_prompt = None;
                }
            }
            return AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                code: format!("pi-rpc-{}", response.command),
                message,
            }));
        }

        match response.command.as_str() {
            "get_state" => self.state_response(response.data.as_ref()),
            "get_messages" => self.messages_response(response.data.as_ref()),
            "get_commands" => self.commands_response(response.data.as_ref()),
            "get_available_models" => self.models_response(response.data.as_ref()),
            "get_available_thinking_levels" => {
                if let Some(levels) = response
                    .data
                    .as_ref()
                    .and_then(|data| data.get("levels"))
                    .and_then(Value::as_array)
                {
                    self.thinking_levels = levels
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                }
                self.available_model_catalog()
            }
            "new_session" => {
                let Some(id) = response.id else {
                    return self.protocol_error("new_session response has no id".into());
                };
                let Some(text) = self.pending_new.remove(&id) else {
                    return AdapterOutput::default();
                };
                self.current_turn = 0;
                self.pending_tool_result_messages.clear();
                // The fresh session starts unnamed; the follow-up refresh
                // re-derives every session-scoped value.
                self.session_name = None;
                self.derived_title = None;
                self.emitted_title = None;
                let mut output = AdapterOutput {
                    commands: self.refresh_commands(),
                    events: Vec::new(),
                };
                // The prompt that opened the conversation seeds the fallback
                // title; it reaches the frontend once the switch settles (an
                // earlier `Title` event would be overwritten by `Attached`).
                self.note_first_user_title(&text);
                output.commands.push(RpcCommand::Prompt {
                    id: Some(self.request_id("prompt")),
                    message: text,
                    streaming_behavior: None,
                });
                output
            }
            "switch_session" => {
                self.current_turn = 0;
                self.pending_tool_result_messages.clear();
                self.session_name = None;
                self.derived_title = None;
                self.emitted_title = None;
                AdapterOutput {
                    commands: self.refresh_commands(),
                    events: Vec::new(),
                }
            }
            "set_model" => {
                if let Some(model) = response.data {
                    self.current_model = Some(model);
                }
                let effort = response
                    .id
                    .as_ref()
                    .and_then(|id| self.pending_model_effort.remove(id))
                    .flatten();
                let mut commands = Vec::new();
                if let Some(level) = effort {
                    commands.push(RpcCommand::SetThinkingLevel {
                        id: Some(self.request_id("thinking")),
                        level,
                    });
                }
                commands.extend(self.model_refresh_commands());
                AdapterOutput {
                    commands,
                    events: Vec::new(),
                }
            }
            "set_thinking_level" => AdapterOutput {
                commands: self.model_refresh_commands(),
                events: Vec::new(),
            },
            // A slash command ran through `prompt`; extension commands can
            // rename the session (`ctx.setSessionName`), so refresh state and
            // report the new title without re-attaching.
            "prompt" => {
                if response.id.as_deref() == self.pending_command_prompt.as_deref() {
                    self.pending_command_prompt = None;
                    AdapterOutput::command(RpcCommand::GetState {
                        id: Some(self.request_id("state")),
                    })
                } else {
                    AdapterOutput::default()
                }
            }
            _ => AdapterOutput::default(),
        }
    }

    fn state_response(&mut self, data: Option<&Value>) -> AdapterOutput {
        let Some(data) = data else {
            return self.protocol_error("get_state response has no data".into());
        };
        self.current_model = data.get("model").filter(|value| !value.is_null()).cloned();
        self.thinking_level = data
            .get("thinkingLevel")
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.is_streaming = data
            .get("isStreaming")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.session_id = data
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("pi-session")
            .to_owned();
        let session_key = data
            .get("sessionFile")
            .and_then(Value::as_str)
            .unwrap_or(&self.session_id)
            .to_owned();
        self.session_name = data
            .get("sessionName")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let provider = self
            .current_model
            .as_ref()
            .and_then(|model| model.get("provider"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let model = self
            .current_model
            .as_ref()
            .and_then(|model| model.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let status = if self.is_streaming {
            AgentStatus::Running
        } else {
            AgentStatus::Idle
        };
        // `Attached` mirrors the DSH welcome contract: it reports a session attach,
        // not a state refresh. Same-session refreshes (e.g. the `get_state` issued
        // after `/model` or a ping) must not re-emit it, because the frontend
        // discards a pending `/new` draft on every `Attached` and would visibly
        // switch back to the retained session.
        let switched = self.last_attached_session.as_deref() != Some(session_key.as_str());
        self.last_attached_session = Some(session_key.clone());
        let mut output = if switched {
            // `Attached` already carries the title; mirror it in the dedup key
            // so the follow-up refresh does not emit a duplicate event.
            self.emitted_title = self.session_name.clone();
            AdapterOutput::event(AgentEvent::Session(SessionEvent::Attached(
                AttachedSession {
                    protocol_version: None,
                    max_frame_bytes: None,
                    id: session_key,
                    status,
                    provider,
                    model,
                    mode: Some("pi".into()),
                    title: self.session_name.clone(),
                    workspace: Some(self.cwd.to_string_lossy().into_owned()),
                },
            )))
        } else {
            // Same-session refresh: the only path that observes in-session
            // renames (an extension command calling `set_session_name`).
            let mut output =
                AdapterOutput::event(AgentEvent::Session(SessionEvent::Status(status)));
            output.events.extend(self.title_events());
            output
        };
        output.merge(self.available_model_catalog());
        output
    }

    /// Status-bar title: Pi's explicit session name when set, else the first
    /// user message — the same precedence the session index uses, so the
    /// status bar and the session list agree.
    fn current_title(&self) -> Option<String> {
        self.session_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| self.derived_title.clone())
    }

    /// Report a `SessionEvent::Title` whenever the visible title changed since
    /// the last report. Same-session state refreshes, snapshots, and live
    /// first user messages flow through here; `Attached` already carries the
    /// title on session switches.
    fn title_events(&mut self) -> Vec<AgentEvent> {
        let title = self.current_title();
        if title == self.emitted_title {
            return Vec::new();
        }
        self.emitted_title = title.clone();
        vec![AgentEvent::Session(SessionEvent::Title(
            title.unwrap_or_default(),
        ))]
    }

    /// Seed the first-user-message fallback title. Pi only names sessions
    /// explicitly (`set_session_name`); every other session is identified by
    /// its first prompt, exactly like the session index.
    fn note_first_user_title(&mut self, text: &str) {
        let named = self
            .session_name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty());
        if named || self.derived_title.is_some() {
            return;
        }
        let title = session_index::clean_title(text);
        if !title.is_empty() {
            self.derived_title = Some(title);
        }
    }

    fn messages_response(&mut self, data: Option<&Value>) -> AdapterOutput {
        let messages = data
            .and_then(|data| data.get("messages"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        // The first user message is the fallback title for unnamed sessions,
        // matching what the session list already shows.
        if let Some(first_user) = messages
            .iter()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        {
            self.note_first_user_title(&content_text(
                first_user.get("content").unwrap_or(&Value::Null),
            ));
        }
        let mut records = Vec::new();
        let mut snapshot_turn = 0_u64;
        for message in messages {
            let turn = if message.get("role").and_then(Value::as_str) == Some("assistant") {
                snapshot_turn = snapshot_turn.saturating_add(1);
                Some(snapshot_turn)
            } else {
                None
            };
            records.extend(self.snapshot_message(&message, turn));
        }
        self.current_turn = snapshot_turn;
        let mut output = AdapterOutput::event(AgentEvent::Timeline(TimelineEvent::Snapshot {
            records,
            truncated: false,
        }));
        output.events.extend(self.title_events());
        output
    }

    fn commands_response(&mut self, data: Option<&Value>) -> AdapterOutput {
        let mut commands = Vec::new();
        let mut skills = Vec::new();
        for command in data
            .and_then(|data| data.get("commands"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = command.get("name").and_then(Value::as_str) else {
                continue;
            };
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
        AdapterOutput {
            commands: Vec::new(),
            events: vec![
                AgentEvent::Catalog(CatalogEvent::Commands(commands)),
                AgentEvent::Catalog(CatalogEvent::Skills(skills)),
            ],
        }
    }

    fn models_response(&mut self, data: Option<&Value>) -> AdapterOutput {
        self.available_models = data
            .and_then(|data| data.get("models"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.available_model_catalog()
    }

    fn available_model_catalog(&self) -> AdapterOutput {
        if self.available_models.is_empty() {
            let models = self.current_model.clone().into_iter().collect::<Vec<_>>();
            self.model_catalog(&models)
        } else {
            self.model_catalog(&self.available_models)
        }
    }

    fn model_catalog(&self, models: &[Value]) -> AdapterOutput {
        let mut providers: Vec<ModelProvider> = Vec::new();
        for model in models {
            let Some(provider_id) = model.get("provider").and_then(Value::as_str) else {
                continue;
            };
            let Some(model_id) = model.get("id").and_then(Value::as_str) else {
                continue;
            };
            let reasoning = model
                .get("reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .then(|| {
                    let levels = model
                        .get("thinkingLevelMap")
                        .and_then(Value::as_object)
                        .map(|map| map.keys().cloned().collect::<Vec<_>>())
                        .filter(|levels| !levels.is_empty())
                        .unwrap_or_else(|| self.thinking_levels.clone());
                    ModelReasoning {
                        efforts: levels
                            .iter()
                            .map(|level| ReasoningEffort {
                                id: level.clone(),
                                name: thinking_label(level),
                                description: None,
                            })
                            .collect(),
                        default_effort: self.thinking_level.clone(),
                    }
                });
            let descriptor = ModelDescriptor {
                id: model_id.to_owned(),
                name: model
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(model_id)
                    .to_owned(),
                description: model
                    .get("contextWindow")
                    .and_then(Value::as_u64)
                    .map(|window| format!("context {window}")),
                reasoning,
            };
            if let Some(provider) = providers.iter_mut().find(|item| item.id == provider_id) {
                provider.models.push(descriptor);
            } else {
                providers.push(ModelProvider {
                    id: provider_id.to_owned(),
                    name: provider_id.to_owned(),
                    models: vec![descriptor],
                });
            }
        }
        let current = self.current_model.as_ref().and_then(|model| {
            Some(ModelSelection {
                provider: model.get("provider")?.as_str()?.to_owned(),
                model: model.get("id")?.as_str()?.to_owned(),
                reasoning_effort: self.thinking_level.clone(),
            })
        });
        AdapterOutput::event(AgentEvent::Catalog(CatalogEvent::Models {
            providers,
            current,
        }))
    }

    fn message_update(&mut self, record: &RpcRecord) -> AdapterOutput {
        let Some(delta) = record.field("assistantMessageEvent") else {
            return AdapterOutput::default();
        };
        let kind = delta.get("type").and_then(Value::as_str);
        let (text, reasoning) = match kind {
            Some("text_delta") => (delta.get("delta").and_then(Value::as_str).unwrap_or(""), ""),
            Some("thinking_delta") => {
                ("", delta.get("delta").and_then(Value::as_str).unwrap_or(""))
            }
            _ => return AdapterOutput::default(),
        };
        self.timeline(TimelineFact::AssistantChunk {
            text: text.to_owned(),
            reasoning: reasoning.to_owned(),
            turn: Some(self.current_turn.max(1)),
            step: Some(0),
            usage: record.field("usage").map(token_usage),
        })
    }

    fn live_message(&mut self, message: &Value) -> AdapterOutput {
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                self.note_first_user_title(&content_text(
                    message.get("content").unwrap_or(&Value::Null),
                ));
                let mut output = self.timeline(user_fact(message));
                output.events.extend(self.title_events());
                output
            }
            Some("assistant") => self.timeline(assistant_fact(
                message,
                Some(self.current_turn.max(1)),
                Some(0),
            )),
            // `tool_execution_end` carries the same result immediately
            // before Pi appends its durable `toolResult` message. The former
            // owns live tool projection; suppress the latter duplicate.
            Some("toolResult") => {
                let id = message
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or("pi-tool");
                if self.pending_tool_result_messages.remove(id) {
                    AdapterOutput::default()
                } else {
                    self.timeline(tool_result_fact(message))
                }
            }
            _ => AdapterOutput::default(),
        }
    }

    fn snapshot_message(&mut self, message: &Value, turn: Option<u64>) -> Vec<TimelineRecord> {
        match message.get("role").and_then(Value::as_str) {
            Some("user") => vec![self.record_fact(user_fact(message))],
            Some("assistant") => {
                let mut records = vec![self.record_fact(assistant_fact(message, turn, Some(0)))];
                for call in content_parts(message)
                    .filter(|part| part.get("type").and_then(Value::as_str) == Some("toolCall"))
                {
                    records.push(self.record_fact(TimelineFact::ToolCall(tool_activity(
                        call.get("id").and_then(Value::as_str).unwrap_or("pi-tool"),
                        call.get("name").and_then(Value::as_str).unwrap_or("tool"),
                        call.get("arguments").cloned().unwrap_or(Value::Null),
                    ))));
                }
                records
            }
            Some("toolResult") => vec![self.record_fact(tool_result_fact(message))],
            Some("compactionSummary") => vec![self.record_fact(TimelineFact::CompactionSummary {
                id: "pi-compaction".into(),
                summary: message
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            })],
            Some("branchSummary") => vec![self.record_fact(TimelineFact::Custom {
                namespace: "pi".into(),
                kind: Some("branch-summary".into()),
                summary: message
                    .get("summary")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })],
            _ => Vec::new(),
        }
    }

    fn tool_start(&mut self, record: &RpcRecord) -> AdapterOutput {
        let id = record.string("toolCallId").unwrap_or("pi-tool");
        let name = record.string("toolName").unwrap_or("tool");
        let args = record.field("args").cloned().unwrap_or(Value::Null);
        self.timeline(TimelineFact::ToolCall(tool_activity(id, name, args)))
    }

    fn tool_end(&mut self, record: &RpcRecord) -> AdapterOutput {
        let result = record.field("result").cloned().unwrap_or(Value::Null);
        let is_error = record.bool("isError").unwrap_or(false);
        let activity_id = record.string("toolCallId").unwrap_or("pi-tool").to_owned();
        self.pending_tool_result_messages
            .insert(activity_id.clone());
        self.timeline(TimelineFact::ToolResult {
            activity_id,
            output: content_text(result.get("content").unwrap_or(&Value::Null)),
            state: if is_error {
                ActivityState::Failure
            } else {
                ActivityState::Success
            },
            output_truncated: result
                .get("details")
                .and_then(|details| details.get("truncation"))
                .is_some_and(|value| !value.is_null()),
            starts_thinking: false,
            mutation_diff: pi_edit_mutation_diff(record.string("toolName"), &result, is_error),
            mutation_hunks: Vec::new(),
        })
    }

    fn extension_request(&mut self, record: RpcRecord) -> AdapterOutput {
        let request = match extension_ui_request(&record) {
            Ok(request) => request,
            Err(error) => return self.protocol_error(error.to_string()),
        };
        match request.method.as_str() {
            "select" => self.extension_question(request, PendingExtensionUi::Select, true),
            "input" => self.extension_question(request, PendingExtensionUi::Input, false),
            "editor" => self.extension_question(request, PendingExtensionUi::Editor, false),
            "confirm" => {
                self.extension_ui
                    .insert(request.id.clone(), PendingExtensionUi::Confirm);
                AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Approval {
                    id: request.id,
                    capability: ToolCapability::Generic,
                    label: request.title.unwrap_or_else(|| "Confirm".into()),
                    reason: request.message.unwrap_or_default(),
                }))
            }
            "notify" => {
                let message = request.message.unwrap_or_default();
                if request.notify_type.as_deref() == Some("error") {
                    AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                        code: "pi-notify".into(),
                        message,
                    }))
                } else {
                    self.timeline(TimelineFact::Custom {
                        namespace: "pi".into(),
                        kind: Some("notification".into()),
                        summary: Some(message),
                    })
                }
            }
            "set_editor_text" => {
                AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::SetEditorText {
                    text: request.text.unwrap_or_default(),
                }))
            }
            "setTitle" => AdapterOutput::event(AgentEvent::Session(SessionEvent::Title(
                request.title.unwrap_or_default(),
            ))),
            "setStatus" | "setWidget" => AdapterOutput::default(),
            _ => AdapterOutput::default(),
        }
    }

    fn extension_question(
        &mut self,
        request: ExtensionUiRequest,
        method: PendingExtensionUi,
        has_options: bool,
    ) -> AdapterOutput {
        self.extension_ui.insert(request.id.clone(), method);
        let question = Question {
            id: "value".into(),
            question: request
                .message
                .or(request.placeholder)
                .or(request.prefill)
                .unwrap_or_else(|| request.title.clone().unwrap_or_default()),
            header: request.title,
            options: has_options.then(|| {
                request
                    .options
                    .into_iter()
                    .map(|label| QuestionOption {
                        label,
                        description: None,
                    })
                    .collect()
            }),
            multi_select: false,
        };
        AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Question {
            request_id: request.id,
            session_id: self.session_id.clone(),
            questions: vec![question],
        }))
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
            RpcCommand::GetAvailableThinkingLevels {
                id: Some(self.request_id("thinking-levels")),
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
            RpcCommand::GetAvailableThinkingLevels {
                id: Some(self.request_id("thinking-levels")),
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

fn pi_skill_name(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once("\" location=\"")?;
    let (location, rest) = rest.split_once("\">\n")?;
    let (_, suffix) = rest.rsplit_once("\n</skill>")?;
    if name.is_empty()
        || location.is_empty()
        || !(suffix.is_empty()
            || suffix
                .strip_prefix("\n\n")
                .is_some_and(|arguments| !arguments.is_empty()))
    {
        return None;
    }
    Some(name)
}

fn user_fact(message: &Value) -> TimelineFact {
    let value = message.get("content").unwrap_or(&Value::Null);
    let text = content_text(value);
    let skill_name = pi_skill_name(&text).map(str::to_owned);
    let content = if value.is_string() {
        vec![ContentBlock::Text(text.clone())]
    } else {
        content_parts(message)
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => Some(ContentBlock::Text(
                    part.get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                )),
                Some("image") => Some(ContentBlock::Image {
                    label: "image".into(),
                }),
                _ => None,
            })
            .collect()
    };
    TimelineFact::UserMessage {
        text,
        source_kind: Some(if skill_name.is_some() {
            "skill-invocation".into()
        } else {
            "user".into()
        }),
        content,
        source: MessageSource {
            kind: Some(if skill_name.is_some() {
                "skill-invocation".into()
            } else {
                "user".into()
            }),
            form: skill_name.as_ref().map(|_| "instructions".into()),
            summary: skill_name,
            producer: Some("pi".into()),
        },
    }
}

fn assistant_fact(message: &Value, turn: Option<u64>, step: Option<u64>) -> TimelineFact {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut content = Vec::new();
    for part in content_parts(message) {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let value = part.get("text").and_then(Value::as_str).unwrap_or("");
                text.push_str(value);
                content.push(ContentBlock::Text(value.to_owned()));
            }
            Some("thinking") => {
                let value = part.get("thinking").and_then(Value::as_str).unwrap_or("");
                reasoning.push_str(value);
                content.push(ContentBlock::Reasoning(value.to_owned()));
            }
            Some("image") => content.push(ContentBlock::Image {
                label: "image".into(),
            }),
            _ => {}
        }
    }
    TimelineFact::AssistantMessage {
        text,
        reasoning,
        content,
        turn,
        step,
        usage: message.get("usage").map(token_usage),
    }
}

fn tool_result_fact(message: &Value) -> TimelineFact {
    let is_error = message
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    TimelineFact::ToolResult {
        activity_id: message
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or("pi-tool")
            .to_owned(),
        output: content_text(message.get("content").unwrap_or(&Value::Null)),
        state: if is_error {
            ActivityState::Failure
        } else {
            ActivityState::Success
        },
        output_truncated: message
            .get("details")
            .and_then(|details| details.get("truncation"))
            .is_some_and(|value| !value.is_null()),
        starts_thinking: false,
        mutation_diff: pi_edit_mutation_diff(
            message.get("toolName").and_then(Value::as_str),
            message,
            is_error,
        ),
        mutation_hunks: Vec::new(),
    }
}

fn pi_edit_mutation_diff(
    tool_name: Option<&str>,
    result: &Value,
    is_error: bool,
) -> Option<MutationDiff> {
    if is_error || !tool_name.is_some_and(|name| name.eq_ignore_ascii_case("edit")) {
        return None;
    }
    result
        .get("details")
        .and_then(|details| details.get("patch"))
        .and_then(Value::as_str)
        .filter(|patch| !patch.is_empty())
        .map(|patch| MutationDiff {
            path: None,
            source: patch.to_owned(),
        })
}

fn content_parts(message: &Value) -> impl Iterator<Item = &Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            (part.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("")
}

fn token_usage(value: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: value.get("input").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: value.get("output").and_then(Value::as_u64).unwrap_or(0),
        cache_read_tokens: value.get("cacheRead").and_then(Value::as_u64).unwrap_or(0),
        cache_write_tokens: value.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0),
    }
}

fn tool_activity(id: &str, name: &str, arguments: Value) -> ToolActivity {
    let capability = match name.to_ascii_lowercase().as_str() {
        "read" => ToolCapability::Read,
        "edit" => ToolCapability::Edit,
        "write" => ToolCapability::Create,
        "grep" | "find" | "ls" => ToolCapability::Search,
        "bash" | "powershell" | "command" | "shell" | "sh" | "pwsh" => ToolCapability::Command,
        _ => ToolCapability::Custom {
            namespace: "pi".into(),
            name: name.into(),
        },
    };
    let string = |keys: &[&str]| {
        keys.iter().find_map(|key| {
            arguments
                .get(*key)
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    };
    let path = string(&["path", "file_path", "filePath"]);
    let command = string(&["command", "cmd"]);
    let query = string(&["pattern", "query"]);
    let summary = command
        .clone()
        .or_else(|| path.clone())
        .or_else(|| query.clone())
        .unwrap_or_else(|| bounded_json(&arguments).0);
    let reference = match capability {
        ToolCapability::Read | ToolCapability::Create => {
            path.clone().map(|path| ToolReference::Path { path })
        }
        ToolCapability::Edit => edit_mutation_hunks(&arguments, path.as_deref())
            .map(ToolReference::Hunks)
            .or_else(|| path.clone().map(|path| ToolReference::Path { path })),
        ToolCapability::Command => command
            .clone()
            .map(|command| ToolReference::Command { command }),
        ToolCapability::Search => Some(ToolReference::SearchResult {
            query: query.clone().unwrap_or_default(),
            matches: Vec::new(),
        }),
        _ => None,
    };
    let preview = match capability {
        ToolCapability::Read | ToolCapability::Create => path.map(|path| ToolPreview {
            name: name.into(),
            primary: ToolPreviewPrimary::Location {
                path,
                lines: arguments
                    .get("offset")
                    .and_then(Value::as_u64)
                    .map(|start| LineSelection {
                        start: start as usize,
                        end: arguments
                            .get("limit")
                            .and_then(Value::as_u64)
                            .map(|limit| start.saturating_add(limit).saturating_sub(1) as usize),
                    }),
            },
            secondary: None,
        }),
        ToolCapability::Command => command.map(|command| ToolPreview {
            name: name.into(),
            primary: ToolPreviewPrimary::Command {
                command,
                metrics: ToolMetrics::default(),
            },
            secondary: None,
        }),
        ToolCapability::Search => Some(ToolPreview {
            name: name.into(),
            primary: ToolPreviewPrimary::Search {
                query: query.unwrap_or_default(),
                path,
            },
            secondary: None,
        }),
        ToolCapability::Edit if reference.is_some() => None,
        ToolCapability::Edit
        | ToolCapability::Insert
        | ToolCapability::Replace
        | ToolCapability::View
        | ToolCapability::Generic
        | ToolCapability::Custom { .. } => {
            let (source, truncated) = bounded_json(&arguments);
            Some(ToolPreview {
                name: name.into(),
                primary: ToolPreviewPrimary::Json { source, truncated },
                secondary: None,
            })
        }
    };
    ToolActivity {
        id: id.into(),
        capability,
        label: name.into(),
        summary,
        state: ActivityState::Running,
        reference,
        items: Vec::new(),
        preview,
    }
}

fn edit_mutation_hunks(arguments: &Value, path: Option<&str>) -> Option<Vec<MutationHunk>> {
    let replacements = if let Some(edits) = arguments.get("edits").and_then(Value::as_array) {
        if edits.is_empty() {
            return None;
        }
        edits
            .iter()
            .map(|edit| {
                Some((
                    edit.get("oldText")?.as_str()?,
                    edit.get("newText")?.as_str()?,
                ))
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        vec![(
            arguments.get("oldText")?.as_str()?,
            arguments.get("newText")?.as_str()?,
        )]
    };
    Some(
        replacements
            .into_iter()
            .map(|(old, new)| MutationHunk {
                path: path.map(str::to_owned),
                old: Some(old.to_owned()),
                new: Some(new.to_owned()),
                anchor_line: None,
            })
            .collect(),
    )
}

fn bounded_json(value: &Value) -> (String, bool) {
    let source = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into());
    let mut chars = source.chars();
    let bounded = chars.by_ref().take(GENERIC_JSON_CHARS).collect::<String>();
    let truncated = chars.next().is_some();
    (bounded, truncated)
}

fn thinking_label(level: &str) -> String {
    let mut chars = level.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use e_tui::action::PromptInput;

    fn record(value: Value) -> RpcRecord {
        RpcRecord::from_value(value).unwrap()
    }

    #[test]
    fn startup_queries_authoritative_surfaces() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let commands = adapter.startup_commands();
        assert_eq!(commands.len(), 5);
        assert!(matches!(commands[0], RpcCommand::GetState { .. }));
        assert!(matches!(commands[1], RpcCommand::GetMessages { .. }));
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
        assert!(done.commands.iter().any(
            |command| matches!(command, RpcCommand::Prompt { message, .. } if message == "first")
        ));
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

        // new_session response: refresh + the opening prompt; no Title event
        // yet because the follow-up `Attached` would overwrite it.
        let output = adapter.record(record(serde_json::json!({
            "type":"response", "id":"pie-new-1", "command":"new_session", "success":true
        })));
        assert_eq!(title_event(&output), None);
        assert!(matches!(
            output.commands.first(),
            Some(RpcCommand::GetState { .. })
        ));
        assert!(matches!(
            output.commands.last(),
            Some(RpcCommand::Prompt { .. })
        ));

        // The switch refresh attaches the fresh unnamed session...
        let attached = adapter.record(unnamed_get_state("s2", "sessions/b.jsonl"));
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

        let refresh = adapter.record(record(serde_json::json!({
            "type":"response", "id":"pie-command-1", "command":"prompt", "success":true
        })));
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
