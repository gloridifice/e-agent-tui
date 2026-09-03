//! Outbound provider-neutral request routing for Pi RPC.

use e_tui::{
    action::AgentRequest,
    agent::{AgentEvent, InteractionEvent, SessionEvent, TimelineEvent},
};

use crate::{
    protocol::{ExtensionUiResponse, RpcCommand, StreamingBehavior},
    session_index,
};

use super::{AdapterOutput, PendingExtensionUi, PiAdapter};

pub(super) fn route(adapter: &mut PiAdapter, request: AgentRequest) -> AdapterOutput {
    match request {
            AgentRequest::Input { prompt } => {
                let Some(text) = prompt.plain_text().map(str::to_owned) else {
                    return adapter.unsupported("Pi image prompts must be pasted as temporary file paths");
                };
                // The first prompt of an unnamed session is its title in the
                // session list; report it immediately so the status bar stops
                // showing `新会话` as soon as the conversation starts.
                adapter.note_first_user_title(&text);
                let mut output = AdapterOutput::command(RpcCommand::Prompt {
                    id: Some(adapter.request_id("prompt")),
                    message: text,
                    streaming_behavior: adapter.is_streaming.then_some(StreamingBehavior::Steer),
                });
                output.events.extend(adapter.title_events());
                output
            }
            AgentRequest::NewInput { mode: _, prompt } => {
                let Some(text) = prompt.plain_text().map(str::to_owned) else {
                    return adapter.unsupported("Pi image prompts must be pasted as temporary file paths");
                };
                let id = adapter.request_id("new");
                adapter.pending_new.insert(id.clone(), text);
                AdapterOutput::command(RpcCommand::NewSession { id: Some(id) })
            }
            AgentRequest::Command { line, images } => {
                if images.is_empty() {
                    adapter.command_line(line)
                } else {
                    adapter.unsupported("Pi command images must be pasted as temporary file paths")
                }
            }
            AgentRequest::Interrupt => AdapterOutput::command(RpcCommand::Abort {
                id: Some(adapter.request_id("abort")),
            }),
            AgentRequest::Attach { session_id } => {
                // Force the post-switch refresh to re-emit `Attached` even when the
                // resolved session key equals the last reported one (explicit re-attach).
                adapter.last_attached_session = None;
                AdapterOutput::command(RpcCommand::SwitchSession {
                    id: Some(adapter.request_id("switch")),
                    session_path: session_id,
                })
            }
            AgentRequest::ListSessions => {
                let index = session_index::list_current_project(&adapter.session_root, &adapter.cwd);
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
                if adapter.extension_ui.remove(&id) == Some(PendingExtensionUi::Confirm) {
                    AdapterOutput::command(RpcCommand::ExtensionUiResponse {
                        id,
                        response: ExtensionUiResponse::Confirmed { confirmed: allow },
                    })
                } else {
                    adapter.unsupported("Pi RPC does not expose a separate approval response")
                }
            }
            AgentRequest::AnswerQuestions {
                request_id,
                answers,
            } => adapter.answer_extension_question(request_id, answers),
            AgentRequest::CancelQuestions { request_id } => {
                if adapter.extension_ui.remove(&request_id).is_some() {
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
            | AgentRequest::LoginProxyDelete { .. } => adapter.unsupported(
                "Manage Pi credentials with native `pi /login`; pie reuses Pi's auth.json and environment credentials",
            ),
            AgentRequest::ModelGet => AdapterOutput {
                commands: adapter.model_refresh_commands(),
                events: Vec::new(),
            },
            AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort,
            } => {
                let id = adapter.request_id("model");
                adapter.pending_model_effort
                    .insert(id.clone(), reasoning_effort);
                AdapterOutput::command(RpcCommand::SetModel {
                    id: Some(id),
                    provider,
                    model_id: model,
                })
            }
            AgentRequest::Ping => AdapterOutput::command(RpcCommand::GetState {
                id: Some(adapter.request_id("state")),
            }),
        }
}
