//! RPC response and error dispatch for the Pi adapter.

use e_tui::agent::{AgentEvent, InteractionEvent};

use crate::protocol::{response, RpcCommand, RpcRecord};
use serde_json::Value;

use super::{model, session, AdapterOutput, PiAdapter};

pub(super) fn dispatch(adapter: &mut PiAdapter, record: RpcRecord) -> AdapterOutput {
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
            let Some(text) = adapter.pending_new.remove(&id) else {
                return AdapterOutput::default();
            };
            adapter.current_turn = 0;
            adapter.pending_tool_result_messages.clear();
            // The fresh session starts unnamed; the follow-up refresh
            // re-derives every session-scoped value.
            adapter.session_name = None;
            adapter.derived_title = None;
            adapter.emitted_title = None;
            let mut output = AdapterOutput {
                commands: adapter.refresh_commands(),
                events: Vec::new(),
            };
            // The prompt that opened the conversation seeds the fallback
            // title; it reaches the frontend once the switch settles (an
            // earlier `Title` event would be overwritten by `Attached`).
            session::note_first_user_title(adapter, &text);
            output.commands.push(RpcCommand::Prompt {
                id: Some(adapter.request_id("prompt")),
                message: text,
                streaming_behavior: None,
            });
            output
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
            let effort = response
                .id
                .as_ref()
                .and_then(|id| adapter.pending_model_effort.remove(id))
                .flatten();
            let mut commands = Vec::new();
            if let Some(level) = effort {
                commands.push(RpcCommand::SetThinkingLevel {
                    id: Some(adapter.request_id("thinking")),
                    level,
                });
            }
            commands.extend(adapter.model_refresh_commands());
            AdapterOutput {
                commands,
                events: Vec::new(),
            }
        }
        "set_thinking_level" => AdapterOutput {
            commands: adapter.model_refresh_commands(),
            events: Vec::new(),
        },
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
