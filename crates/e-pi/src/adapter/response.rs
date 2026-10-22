//! RPC response and error dispatch for the Pi adapter.

use e_tui::agent::{AgentEvent, InteractionEvent};

use crate::protocol::{response, RpcCommand, RpcRecord};
use serde_json::Value;

use super::{
    model, session, AdapterOutput, NewSubmission, PendingCommandState, PendingReload, PiAdapter,
    ReloadStep,
};

fn request_order(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

fn settlement_event(
    adapter: &PiAdapter,
    pending: &PendingCommandState,
    confirmed: bool,
) -> AgentEvent {
    AgentEvent::Interaction(if pending.counted {
        InteractionEvent::CommandResult {
            id: pending.command_id.clone(),
            outcome: if confirmed { "success" } else { "unconfirmed" }.into(),
            text: None,
        }
    } else {
        InteractionEvent::PromptSettlement {
            request_id: pending.command_id.clone(),
            session_id: super::queue::session_key(adapter),
            confirmed,
        }
    })
}

pub(super) fn dispatch(adapter: &mut PiAdapter, mut record: RpcRecord) -> AdapterOutput {
    let interrupt_response = record.string("id").is_some_and(|id| {
        id.starts_with("pie-interrupt-abort-") || id.starts_with("pie-interrupt-state-")
    });
    if interrupt_response {
        if record.string("id") != adapter.pending_interrupt.as_deref() {
            return AdapterOutput::default();
        }
        adapter.pending_interrupt = None;
        let current = adapter
            .interrupt_context
            .take()
            .is_some_and(|(session, generation)| {
                session == adapter.session_id && generation == adapter.run_generation
            });
        if !current {
            let mut output = AdapterOutput::default();
            drain_deferred(adapter, &mut output);
            return output;
        }
        if record.bool("success") == Some(true) {
            if record.string("command") == Some("abort") {
                let id = adapter.request_id("interrupt-state");
                adapter.pending_interrupt = Some(id.clone());
                adapter.interrupt_context =
                    Some((adapter.session_id.clone(), adapter.run_generation));
                return AdapterOutput::command(RpcCommand::GetState { id: Some(id) });
            }
            let data = record.field("data");
            if data
                .and_then(|data| data.get("sessionId"))
                .and_then(Value::as_str)
                != Some(adapter.session_id.as_str())
                || data
                    .and_then(|data| data.get("isStreaming"))
                    .and_then(Value::as_bool)
                    .is_none()
            {
                let mut output = adapter.protocol_error(
                    "Interrupt state could not be confirmed for the current session".into(),
                );
                drain_deferred(adapter, &mut output);
                return output;
            }
        }
    }
    if let Some(output) = super::retry::response(adapter, &record) {
        return output;
    }
    if let Some(mut output) = super::prompt_state::response(adapter, &record) {
        drain_deferred(adapter, &mut output);
        return output;
    }
    if let Some(mut output) = super::mcp::response(adapter, &record) {
        drain_deferred(adapter, &mut output);
        return output;
    }
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
    let command_prompt = response_id.as_deref() == adapter.pending_command_prompt.as_deref()
        && adapter.pending_command_prompt.is_some();
    let command_state = response_id
        .as_ref()
        .and_then(|id| adapter.pending_command_states.remove(id));
    let response_session = record
        .field("data")
        .and_then(|data| data.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let response_command = record.string("command").map(str::to_owned);
    let admission = response_id
        .as_ref()
        .filter(|_| response_command.as_deref() == Some("prompt"))
        .and_then(|id| {
            adapter
                .pending_prompt_admissions
                .get(id)
                .cloned()
                .map(|pending| (id.clone(), pending))
        });
    let command_input = response_command.as_deref() == Some("prompt")
        && response_id
            .as_deref()
            .is_some_and(|id| id.starts_with("pie-command-"));
    if (response_command.as_deref() == Some("prompt")
        && response_id
            .as_deref()
            .is_some_and(|id| id.starts_with("pie-prompt-") || id.starts_with("pie-command-"))
        && admission
            .as_ref()
            .is_none_or(|(_, pending)| pending.session_id != adapter.session_id))
        || (response_command.as_deref() == Some("get_state")
            && response_id.as_deref().is_some_and(|id| {
                id.starts_with("pie-command-state-") || id.starts_with("pie-handled-state-")
            })
            && command_state.is_none())
    {
        return AdapterOutput::default();
    }
    let disposition = record
        .field("data")
        .and_then(|data| data.get("disposition"))
        .and_then(Value::as_str)
        .map(str::to_owned);
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
    let command_state_response = command_state.is_some();
    let mut output = dispatch_response(adapter, record);
    if command_state_response && failed {
        for event in &mut output.events {
            if let AgentEvent::Interaction(InteractionEvent::Error { code, .. }) = event {
                if code == "pi-rpc-get_state" {
                    *code = "command-state-failed".into();
                }
            }
        }
    }
    let counted = admission
        .as_ref()
        .is_some_and(|(_, pending)| pending.counted);
    if let Some((request_id, pending)) = admission {
        adapter.pending_prompt_admissions.remove(&request_id);
        if pending.session_id == adapter.session_id && successful {
            if let Some(expects_run) = disposition.as_deref().and_then(|value| match value {
                "started" | "queued" => Some(true),
                "handled" => Some(false),
                _ => None,
            }) {
                output
                    .events
                    .push(AgentEvent::Interaction(InteractionEvent::PromptAdmission {
                        request_id: request_id.clone(),
                        session_id: super::queue::session_key(adapter),
                        expects_run,
                    }));
            }
            if !command_prompt && (command_input || disposition.as_deref() == Some("handled")) {
                let id = adapter.request_id("handled-state");
                let required = pending.counted || disposition.as_deref() == Some("handled");
                adapter.pending_command_states.insert(
                    id.clone(),
                    PendingCommandState {
                        session_id: adapter.session_id.clone(),
                        command_id: request_id,
                        counted: pending.counted,
                        required,
                        failed: false,
                    },
                );
                if required {
                    adapter.configuration_request = Some(id.clone());
                }
                output.commands.push(RpcCommand::GetState { id: Some(id) });
            }
        }
    }
    if counted && failed {
        output
            .events
            .push(AgentEvent::Interaction(InteractionEvent::CommandResult {
                id: response_id.clone().unwrap_or_default(),
                outcome: "error".into(),
                text: None,
            }));
    }
    if let Some(mut pending) = command_state {
        if pending.session_id == adapter.session_id {
            let confirmed =
                successful && response_session.as_deref() == Some(pending.session_id.as_str());
            output
                .events
                .push(settlement_event(adapter, &pending, confirmed));
            if !confirmed && pending.required {
                pending.failed = true;
                adapter
                    .pending_command_states
                    .insert(response_id.clone().unwrap_or_default(), pending);
            }
        }
    }
    if successful
        && response_command.as_deref() == Some("get_state")
        && response_session.as_deref() == Some(adapter.session_id.as_str())
    {
        if let Some(order) = response_id.as_deref().and_then(request_order) {
            let settled: Vec<_> = adapter
                .pending_command_states
                .iter()
                .filter(|(id, pending)| {
                    pending.failed
                        && pending.session_id == adapter.session_id
                        && request_order(id).is_some_and(|previous| previous < order)
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in settled {
                if let Some(pending) = adapter.pending_command_states.remove(&id) {
                    output
                        .events
                        .push(settlement_event(adapter, &pending, true));
                }
            }
        }
    }
    if successful
        && response_command.as_deref() == Some("abort")
        && adapter
            .pending_command_states
            .values()
            .any(|pending| pending.failed && pending.session_id == adapter.session_id)
    {
        let id = adapter.request_id("command-recovery-state");
        adapter.configuration_request = Some(id.clone());
        output.commands.push(RpcCommand::GetState { id: Some(id) });
    }
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
        adapter.pending_skill_prompt = None;
    }
    if completed.is_some() && adapter.configuration_request == completed {
        adapter.configuration_request = None;
        if failed && !adapter.deferred_requests.is_empty() {
            let code =
                if adapter.deferred_requests.iter().any(|request| {
                    matches!(request.unwrapped(), e_tui::AgentRequest::NewInput { .. })
                }) {
                    "new-failed"
                } else {
                    "input-failed"
                };
            let deferred = std::mem::take(&mut adapter.deferred_requests);
            for request in deferred {
                if let e_tui::AgentRequest::CapturePromptState { record_id } = request.unwrapped() {
                    output.merge(super::prompt_state::failed(
                        adapter,
                        *record_id,
                        "Prior model/session selection failed before Record dispatch",
                    ));
                    continue;
                }
                let operation = match request.unwrapped() {
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
    if reload {
        output.merge(super::mcp::reload_finished(adapter, successful));
    }
    drain_deferred(adapter, &mut output);
    output
}

fn model_control_index(adapter: &PiAdapter) -> Option<usize> {
    if !super::compaction::controls_open(adapter) {
        return None;
    }
    for (index, request) in adapter.deferred_requests.iter().enumerate() {
        match request.unwrapped() {
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
        && adapter.pending_interrupt.is_none()
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
                    submission.record_id,
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
            adapter.preparing_tools.clear();
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
            adapter.preparing_tools.clear();
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
                let command = adapter.pending_command_prompt.take().unwrap();
                let id = adapter.request_id("command-state");
                let counted = adapter
                    .pending_prompt_admissions
                    .get(&command)
                    .is_none_or(|pending| pending.counted);
                let required = counted
                    || response
                        .data
                        .as_ref()
                        .and_then(|data| data.get("disposition"))
                        .and_then(Value::as_str)
                        == Some("handled");
                adapter.pending_command_states.insert(
                    id.clone(),
                    PendingCommandState {
                        session_id: adapter.session_id.clone(),
                        command_id: command,
                        counted,
                        required,
                        failed: false,
                    },
                );
                if required {
                    adapter.configuration_request = Some(id.clone());
                }
                AdapterOutput::command(RpcCommand::GetState { id: Some(id) })
            } else {
                AdapterOutput::default()
            }
        }
        _ => AdapterOutput::default(),
    }
}
