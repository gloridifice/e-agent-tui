//! RPC response and error dispatch for the Pi adapter.

use e_tui::agent::{AgentEvent, InteractionEvent};

use crate::protocol::{response, RpcCommand, RpcRecord};
use serde_json::Value;

use super::{model, session, AdapterOutput, NewSubmission, PiAdapter};

pub(super) fn dispatch(adapter: &mut PiAdapter, record: RpcRecord) -> AdapterOutput {
    let completed = adapter
        .configuration_request
        .clone()
        .filter(|id| record.string("id") == Some(id.as_str()));
    let failed = record.bool("success") == Some(false);
    let mut output = dispatch_response(adapter, record);
    if completed.is_some() && adapter.configuration_request == completed {
        adapter.configuration_request = None;
        if failed && !adapter.deferred_requests.is_empty() {
            let code = if adapter
                .deferred_requests
                .iter()
                .any(|request| matches!(request, e_tui::AgentRequest::NewInput { .. }))
            {
                "new-failed"
            } else {
                "input-failed"
            };
            adapter.deferred_requests.clear();
            output
                .events
                .push(AgentEvent::Interaction(InteractionEvent::Error {
                    code: code.into(),
                    message:
                        "Pending submissions cancelled because the model/session change failed"
                            .into(),
                }));
        }
        while adapter.configuration_request.is_none() {
            let Some(request) = adapter.deferred_requests.pop_front() else {
                break;
            };
            output.merge(adapter.request(request));
        }
    }
    output
}

fn configuration_refresh(adapter: &mut PiAdapter) -> AdapterOutput {
    let id = adapter.request_id("configuration-state");
    adapter.configuration_request = Some(id.clone());
    let mut commands = adapter.model_refresh_commands();
    if let Some(RpcCommand::GetState { id: refresh_id }) = commands.first_mut() {
        *refresh_id = Some(id);
    }
    AdapterOutput {
        commands,
        events: Vec::new(),
    }
}

fn finish_new(adapter: &mut PiAdapter, submission: NewSubmission) -> AdapterOutput {
    session::note_first_user_title(adapter, &submission.text);
    let mut output = configuration_refresh(adapter);
    let id = adapter
        .configuration_request
        .clone()
        .expect("configuration refresh id");
    adapter.pending_new.insert(id, submission);
    output
        .commands
        .extend(adapter.refresh_commands().into_iter().filter(|command| {
            matches!(
                command,
                RpcCommand::GetMessages { .. } | RpcCommand::GetCommands { .. }
            )
        }));
    output
}

fn restore_new_effort(adapter: &mut PiAdapter, submission: NewSubmission) -> AdapterOutput {
    if let Some(level) = submission.thinking_level.clone() {
        let id = adapter.request_id("new-thinking");
        adapter.configuration_request = Some(id.clone());
        adapter.pending_new.insert(id.clone(), submission);
        AdapterOutput::command(RpcCommand::SetThinkingLevel {
            id: Some(id),
            level,
        })
    } else {
        finish_new(adapter, submission)
    }
}

fn dispatch_response(adapter: &mut PiAdapter, record: RpcRecord) -> AdapterOutput {
    let response = match response(&record) {
        Ok(response) => response,
        Err(error) => return adapter.protocol_error(error.to_string()),
    };
    if !response.success {
        let message = response
            .error
            .unwrap_or_else(|| format!("Pi RPC command `{}` failed", response.command));
        if let Some(id) = response.id.as_ref() {
            if adapter.pending_new.remove(id).is_some() {
                return AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: "new-failed".into(),
                    message,
                }));
            }
            adapter.pending_model_effort.remove(id);
            if response.id.as_deref() == adapter.pending_command_prompt.as_deref() {
                adapter.pending_command_prompt = None;
            }
        }
        return AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
            code: format!("pi-rpc-{}", response.command),
            message,
        }));
    }

    match response.command.as_str() {
        "get_state" => {
            let mut output = session::state_response(adapter, response.data.as_ref());
            output.merge(model::available_model_catalog(adapter));
            if let Some(submission) = response
                .id
                .as_ref()
                .and_then(|id| adapter.pending_new.remove(id))
            {
                output.commands.push(RpcCommand::Prompt {
                    id: Some(adapter.request_id("prompt")),
                    message: submission.text,
                    streaming_behavior: None,
                });
            }
            output
        }
        "get_messages" => session::messages_response(adapter, response.data.as_ref()),
        "get_commands" => adapter.commands_response(response.data.as_ref()),
        "get_available_models" => model::models_response(adapter, response.data.as_ref()),
        "get_available_thinking_levels" => {
            if let Some(levels) = response
                .data
                .as_ref()
                .and_then(|data| data.get("levels"))
                .and_then(Value::as_array)
            {
                adapter.thinking_levels = levels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
            }
            model::available_model_catalog(adapter)
        }
        "new_session" => {
            let Some(id) = response.id else {
                return adapter.protocol_error("new_session response has no id".into());
            };
            let Some(submission) = adapter.pending_new.remove(&id) else {
                return AdapterOutput::default();
            };
            if response
                .data
                .as_ref()
                .and_then(|data| data.get("cancelled"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                return AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: "new-failed".into(),
                    message: "New session cancelled".into(),
                }));
            }
            adapter.current_turn = 0;
            adapter.pending_tool_result_messages.clear();
            // The fresh session starts unnamed; the follow-up refresh
            // re-derives every session-scoped value.
            adapter.session_name = None;
            adapter.derived_title = None;
            adapter.emitted_title = None;
            let route = submission.model.as_ref().and_then(|model| {
                Some((
                    model.get("provider")?.as_str()?.to_owned(),
                    model.get("id")?.as_str()?.to_owned(),
                ))
            });
            if let Some((provider, model_id)) = route {
                let id = adapter.request_id("new-model");
                adapter.configuration_request = Some(id.clone());
                adapter.pending_new.insert(id.clone(), submission);
                AdapterOutput::command(RpcCommand::SetModel {
                    id: Some(id),
                    provider,
                    model_id,
                })
            } else {
                restore_new_effort(adapter, submission)
            }
        }
        "switch_session" => {
            adapter.current_turn = 0;
            adapter.pending_tool_result_messages.clear();
            adapter.session_name = None;
            adapter.derived_title = None;
            adapter.emitted_title = None;
            AdapterOutput {
                commands: adapter.refresh_commands(),
                events: Vec::new(),
            }
        }
        "set_model" => {
            if let Some(model) = response.data {
                adapter.current_model = Some(model);
            }
            if let Some(submission) = response
                .id
                .as_ref()
                .and_then(|id| adapter.pending_new.remove(id))
            {
                return restore_new_effort(adapter, submission);
            }
            let effort = response
                .id
                .as_ref()
                .and_then(|id| adapter.pending_model_effort.remove(id))
                .flatten();
            if let Some(level) = effort {
                let id = adapter.request_id("thinking");
                adapter.configuration_request = Some(id.clone());
                AdapterOutput::command(RpcCommand::SetThinkingLevel {
                    id: Some(id),
                    level,
                })
            } else {
                configuration_refresh(adapter)
            }
        }
        "set_thinking_level" => {
            if let Some(submission) = response
                .id
                .as_ref()
                .and_then(|id| adapter.pending_new.remove(id))
            {
                finish_new(adapter, submission)
            } else {
                configuration_refresh(adapter)
            }
        }
        // A slash command ran through `prompt`; extension commands can
        // rename the session (`ctx.setSessionName`), so refresh state and
        // report the new title without re-attaching.
        "prompt" => {
            if response.id.as_deref() == adapter.pending_command_prompt.as_deref() {
                adapter.pending_command_prompt = None;
                AdapterOutput::command(RpcCommand::GetState {
                    id: Some(adapter.request_id("state")),
                })
            } else {
                AdapterOutput::default()
            }
        }
        _ => AdapterOutput::default(),
    }
}
