//! Outbound provider-neutral request routing for Pi RPC.

use e_tui::{
    action::AgentRequest,
    agent::{AgentEvent, TimelineEvent},
};

use crate::protocol::{ExtensionUiResponse, RpcCommand, StreamingBehavior};

use super::{
    extension, session, AdapterOutput, NewSubmission, PendingExtensionUi, PendingReload, PiAdapter,
    ReloadStep,
};

fn command_line(adapter: &mut PiAdapter, line: String) -> AdapterOutput {
    let line = normalize_skill_line(line);
    let skill = line.starts_with("/skill:");
    if line.trim() == "/reload" {
        if !adapter.reload_available {
            return adapter.unsupported("Pi resource reload companion unavailable; restart pie");
        }
        if adapter.is_streaming {
            return adapter.unsupported("Wait for Pi to become idle before /reload");
        }
        let id = adapter.request_id("reload");
        adapter.pending_reload = Some(PendingReload {
            id: id.clone(),
            step: ReloadStep::CompanionPrompt,
            error: None,
        });
        adapter.configuration_request = Some(id.clone());
        return AdapterOutput::command(RpcCommand::Prompt {
            id: Some(id),
            message: format!("/{}", super::RELOAD_COMMAND),
            streaming_behavior: None,
        });
    }
    if let Some(rest) = line
        .strip_prefix("/compact")
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return super::compaction::command(adapter, rest);
    }
    let id = adapter.request_id("command");
    // Its response triggers a same-session state refresh that reports
    // extension-side session renames.
    adapter.pending_command_prompt = Some(id.clone());
    prompt_command(
        adapter,
        id,
        line,
        (skill && adapter.is_streaming).then_some(StreamingBehavior::Steer),
    )
}

pub(super) fn prompt_command(
    adapter: &mut PiAdapter,
    id: String,
    line: String,
    streaming_behavior: Option<StreamingBehavior>,
) -> AdapterOutput {
    let mut message = normalize_skill_line(line);
    if let Some(skill) = message.strip_prefix("/skill:") {
        if let Some(split) = skill.find(char::is_whitespace).filter(|split| *split > 0) {
            let split = "/skill:".len() + split;
            let text = message[split..].trim_start();
            if !text.is_empty() {
                adapter.pending_skill_prompt = Some(super::PendingSkillPrompt {
                    id: id.clone(),
                    session_id: adapter.session_id.clone(),
                    trailing_text: Some(text.to_owned()),
                });
            }
            message.truncate(split);
        }
    }
    AdapterOutput::command(RpcCommand::Prompt {
        id: Some(id),
        message,
        streaming_behavior,
    })
}

fn normalize_skill_line(line: String) -> String {
    line.strip_prefix("/skill ")
        .map(|name| format!("/skill:{}", name.trim_start()))
        .unwrap_or(line)
}

pub(super) fn route(adapter: &mut PiAdapter, request: AgentRequest) -> AdapterOutput {
    match request {
        AgentRequest::Input { prompt } => {
            let Some(text) = prompt.plain_text().map(str::to_owned) else {
                return adapter.unsupported("Pi image prompts must be pasted as temporary file paths");
            };
            // The first prompt of an unnamed session is its title in the
            // session list; report it immediately so the status bar stops
            // showing `新会话` as soon as the conversation starts.
            session::note_first_user_title(adapter, &text);
            let mut output = AdapterOutput::command(RpcCommand::Prompt {
                id: Some(adapter.request_id("prompt")),
                message: text,
                streaming_behavior: adapter.is_streaming.then_some(StreamingBehavior::Steer),
            });
            output.events.extend(session::title_events(adapter));
            output
        }
        AgentRequest::Steer { prompt } => {
            let Some(text) = prompt.plain_text().map(str::to_owned) else {
                return super::queue::event(adapter, Some(e_tui::agent::AsapQueueOperation::Submit),
                    Some("Pi image prompts must be pasted as temporary file paths".into()));
            };
            let id = adapter.request_id("asap");
            adapter.pending_queue.operation = Some((id.clone(), e_tui::agent::AsapQueueOperation::Submit, super::queue::session_key(adapter)));
            AdapterOutput::command(RpcCommand::Prompt {
                id: Some(id),
                message: text,
                streaming_behavior: Some(StreamingBehavior::Steer),
            })
        }
        AgentRequest::ClearAsap => {
            let id = adapter.request_id("clear-asap");
            adapter.pending_queue.operation = Some((id.clone(), e_tui::agent::AsapQueueOperation::Clear, super::queue::session_key(adapter)));
            AdapterOutput::command(RpcCommand::ClearQueue { id: Some(id) })
        }
        AgentRequest::NewInput { mode: _, prompt } => {
            let Some(text) = prompt.plain_text().map(str::to_owned) else {
                return adapter.unsupported("Pi image prompts must be pasted as temporary file paths");
            };
            let id = adapter.request_id("new");
            adapter.configuration_request = Some(id.clone());
            adapter.pending_new.insert(id.clone(), NewSubmission {
                text: normalize_skill_line(text),
                model: adapter.current_model.clone(),
                thinking_level: adapter.thinking_level.clone(),
            });
            AdapterOutput::command(RpcCommand::NewSession { id: Some(id) })
        }
        AgentRequest::Command { line, images } => {
            if images.is_empty() {
                command_line(adapter, line)
            } else {
                adapter.unsupported("Pi command images must be pasted as temporary file paths")
            }
        }
        AgentRequest::Interrupt => {
            super::compaction::interrupt(adapter);
            adapter.pending_skill_prompt = None;
            AdapterOutput::command(RpcCommand::Abort {
                id: Some(adapter.request_id("abort")),
            })
        }
        AgentRequest::Attach { session_id } => {
            // Force the post-switch refresh to re-emit `Attached` even when the
            // resolved session key equals the last reported one (explicit re-attach).
            adapter.last_attached_session = None;
            AdapterOutput::command(RpcCommand::SwitchSession {
                id: Some(adapter.request_id("switch")),
                session_path: session_id,
            })
        }
        // The runner services viewport-driven native discovery on a blocking worker.
        AgentRequest::ListSessions => AdapterOutput::default(),
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
        } => extension::answer_question(adapter, request_id, answers),
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
        | AgentRequest::AuthGet { .. }
        | AgentRequest::AuthStart { .. }
        | AgentRequest::AuthReply { .. }
        | AgentRequest::AuthOpenUrl { .. }
        | AgentRequest::AuthCancel
        | AgentRequest::LoginSetApiKey { .. }
        | AgentRequest::LoginProxyCreate { .. }
        | AgentRequest::LoginProxyDelete { .. } => adapter.unsupported(
            "Manage Pi credentials with native `pi /login`; pie reuses Pi's auth.json and environment credentials",
        ),
        AgentRequest::ModelGet => {
            if super::compaction::controls_open(adapter) {
                super::model::available_model_catalog(adapter)
            } else {
                AdapterOutput {
                    commands: adapter.model_refresh_commands(),
                    events: Vec::new(),
                }
            }
        }
        AgentRequest::ModelSet {
            provider,
            model,
            reasoning_effort,
        } => {
            let id = adapter.request_id("model");
            adapter.configuration_request = Some(id.clone());
            adapter
                .pending_model_effort
                .insert(id.clone(), reasoning_effort.clone());
            super::compaction::begin_model_selection(
                adapter,
                id.clone(),
                provider.clone(),
                model.clone(),
                reasoning_effort,
            );
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
