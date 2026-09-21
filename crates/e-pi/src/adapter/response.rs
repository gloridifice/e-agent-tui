//! RPC response and error dispatch for the Pi adapter.

use e_tui::agent::{AgentEvent, InteractionEvent};

use crate::protocol::{response, RpcCommand, RpcRecord};
use serde_json::Value;

use super::{model, session, AdapterOutput, NewSubmission, PendingReload, PiAdapter, ReloadStep};

pub(super) fn dispatch(adapter: &mut PiAdapter, mut record: RpcRecord) -> AdapterOutput {
    if let Some(mut output) = super::fork::response(adapter, &record) {
        drain_deferred(adapter, &mut output);
        return output;
    }
    let reload = adapter
        .pending_reload
        .as_ref()
        .is_some_and(|pending| record.string("id") == Some(pending.id.as_str()));
    if reload {
        if let Some(error) = adapter
            .pending_reload
            .as_mut()
            .and_then(|pending| pending.error.take())
        {
            record.fields.insert("success".into(), Value::Bool(false));
            record.fields.insert("error".into(), Value::String(error));
        }
        if record.bool("success") == Some(true) && record.string("command") != Some("get_commands")
        {
            let id = adapter.request_id("reload-catalog");
            let next = match record.string("command") {
                Some("prompt") => RpcCommand::GetState {
                    id: Some(id.clone()),
                },
                Some("get_state") => RpcCommand::GetAvailableModels {
                    id: Some(id.clone()),
                },
                Some("get_available_models") => RpcCommand::GetCommands {
                    id: Some(id.clone()),
                },
                _ => return adapter.protocol_error("Unexpected reload response".into()),
            };
            let mut output = dispatch_response(adapter, record);
            adapter.pending_reload = Some(PendingReload {
                id: id.clone(),
                step: ReloadStep::Catalog,
                error: None,
            });
            adapter.configuration_request = Some(id);
            output.commands.push(next);
            return output;
        }
        adapter.pending_reload = None;
    }
    if let Some(mut output) = super::compaction::response(adapter, &record) {
        drain_deferred(adapter, &mut output);
        return output;
    }
    if let Some(mut output) = super::queue::response(adapter, &record) {
        drain_deferred(adapter, &mut output);
        return output;
    }
    let response_id = record.string("id").map(str::to_owned);
    let response_command = record.string("command").map(str::to_owned);
    let selection_response =
        super::compaction::is_model_selection_response(adapter, response_id.as_deref());
    if selection_response
        && response_command.as_deref() == Some("get_state")
        && record.bool("success") == Some(true)
    {
        let validation = super::compaction::validate_model_selection(
            adapter,
            response_id.as_deref().expect("selection response id"),
            record.field("data"),
        );
        if let Err(error) = validation {
            record.fields.insert("success".into(), Value::Bool(false));
            record.fields.insert("error".into(), Value::String(error));
        }
    }
    let completed = adapter
        .configuration_request
        .clone()
        .filter(|id| response_id.as_deref() == Some(id.as_str()));
    let skill = adapter
        .pending_skill_prompt
        .as_ref()
        .is_some_and(|pending| {
            response_id.as_deref() == Some(pending.id.as_str())
                && response_command.as_deref() == Some("prompt")
        });
    let successful = record.bool("success") == Some(true);
    let failed = record.bool("success") == Some(false);
    let confirmed_selection =
        (selection_response && successful && response_command.as_deref() == Some("get_state"))
            .then(|| record.field("data").cloned())
            .flatten();
    let mut output = dispatch_response(adapter, record);
    if selection_response && successful && response_command.as_deref() != Some("get_state") {
        if let (Some(completed_id), Some(next_id)) = (
            response_id.as_deref(),
            adapter.configuration_request.clone(),
        ) {
            super::compaction::advance_model_selection(adapter, completed_id, next_id);
        }
    }
    if reload && successful {
        output
            .events
            .push(AgentEvent::Interaction(InteractionEvent::CommandResult {
                id: "reload".into(),
                outcome: "success".into(),
                text: Some("Pi resources and catalogs reloaded".into()),
            }));
    }
    if skill {
        output.merge(finish_skill_prompt(adapter, successful));
    }
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
            let deferred = std::mem::take(&mut adapter.deferred_requests);
            for request in deferred {
                let operation = match request {
                    e_tui::AgentRequest::Steer { .. } => {
                        Some(e_tui::agent::AsapQueueOperation::Submit)
                    }
                    e_tui::AgentRequest::ClearAsap => Some(e_tui::agent::AsapQueueOperation::Clear),
                    _ => None,
                };
                if let Some(operation) = operation {
                    output.merge(super::queue::event(adapter, Some(operation), Some("Pending queue operation cancelled because the model/session change failed".into())));
                }
            }
            output
                .events
                .push(AgentEvent::Interaction(InteractionEvent::Error {
                    code: code.into(),
                    message:
                        "Pending submissions cancelled because the model/session change failed"
                            .into(),
                }));
        }
    }
    if selection_response {
        if let (Some(id), Some(data)) = (response_id.as_deref(), confirmed_selection.as_ref()) {
            output.merge(super::compaction::confirm_model_selection(
                adapter, id, data,
            ));
        } else if failed {
            if let Some(id) = response_id.as_deref() {
                output.merge(super::compaction::fail_model_selection(adapter, id));
            }
        }
    }
    drain_deferred(adapter, &mut output);
    output
}

fn finish_skill_prompt(adapter: &mut PiAdapter, successful: bool) -> AdapterOutput {
    let Some(pending) = adapter.pending_skill_prompt.take() else {
        return AdapterOutput::default();
    };
    if !successful || pending.session_id != adapter.session_id {
        return AdapterOutput::default();
    }
    let Some(text) = pending.trailing_text else {
        return AdapterOutput::default();
    };
    let id = adapter.request_id("prompt");
    session::note_first_user_title(adapter, &text);
    adapter.pending_skill_prompt = Some(super::PendingSkillPrompt {
        id: id.clone(),
        session_id: pending.session_id,
        trailing_text: None,
    });
    let mut output = AdapterOutput::command(RpcCommand::Prompt {
        id: Some(id),
        message: text,
        streaming_behavior: Some(crate::protocol::StreamingBehavior::Steer),
    });
    output.events.extend(session::title_events(adapter));
    output
}

fn model_control_index(adapter: &PiAdapter) -> Option<usize> {
    if !super::compaction::controls_open(adapter) {
        return None;
    }
    for (index, request) in adapter.deferred_requests.iter().enumerate() {
        match request {
            e_tui::AgentRequest::ModelGet | e_tui::AgentRequest::ModelSet { .. } => {
                return Some(index)
            }
            e_tui::AgentRequest::Attach { .. }
            | e_tui::AgentRequest::NewInput { .. }
            | e_tui::AgentRequest::Command { .. } => return None,
            _ => {}
        }
    }
    None
}

pub(super) fn drain_deferred(adapter: &mut PiAdapter, output: &mut AdapterOutput) {
    while adapter.configuration_request.is_none()
        && adapter.pending_fork.is_none()
        && adapter.pending_queue.operation.is_none()
        && adapter.pending_skill_prompt.is_none()
    {
        let request = if adapter.pending_compaction.is_some() {
            let Some(index) = model_control_index(adapter) else {
                break;
            };
            adapter
                .deferred_requests
                .remove(index)
                .expect("deferred model control")
        } else {
            let Some(request) = adapter.deferred_requests.pop_front() else {
                break;
            };
            request
        };
        output.merge(super::request::route(adapter, request));
    }
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
    if response.command == "get_session_stats" {
        return session::stats_response(adapter, response);
    }
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
                let id = adapter.request_id("prompt");
                output.merge(super::request::prompt_command(
                    adapter,
                    id,
                    submission.text,
                    None,
                ));
            }
            output
        }
        "get_messages" => session::messages_response(adapter, response.data.as_ref()),
        "get_commands" => adapter.commands_response(response.data.as_ref()),
        "get_available_models" => model::models_response(adapter, response.data.as_ref()),
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
            let compaction_selection =
                super::compaction::is_model_selection_response(adapter, response.id.as_deref());
            if !compaction_selection {
                if let Some(model) = response.data {
                    adapter.current_model = Some(model);
                }
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
