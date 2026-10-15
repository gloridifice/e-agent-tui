//! Dispatch-time state fence, serialized behind ordinary model/session controls.

use e_tui::agent::{AgentEvent, InteractionEvent, ModelSelection};
use serde_json::Value;

use super::{AdapterOutput, PiAdapter};
use crate::protocol::{RpcCommand, RpcRecord};

pub(super) fn failed(adapter: &PiAdapter, record_id: u64, message: &str) -> AdapterOutput {
    AdapterOutput::event(AgentEvent::Interaction(
        InteractionEvent::PromptStateCaptured {
            record_id,
            session_id: super::queue::session_key(adapter),
            result: Err(message.into()),
        },
    ))
}

pub(super) fn request(adapter: &mut PiAdapter, record_id: u64) -> AdapterOutput {
    let id = adapter.request_id("prompt-state");
    adapter.pending_prompt_state = Some((id.clone(), record_id, adapter.session_id.clone()));
    adapter.configuration_request = Some(id.clone());
    AdapterOutput::command(RpcCommand::GetState { id: Some(id) })
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    let id = record.string("id")?;
    if !id.starts_with("pie-prompt-state-") {
        return None;
    }
    if adapter
        .pending_prompt_state
        .as_ref()
        .is_none_or(|(pending, _, _)| pending != id)
    {
        return Some(AdapterOutput::default());
    }
    let (_, record_id, session_id) = adapter.pending_prompt_state.take().unwrap();
    if adapter.configuration_request.as_deref() == Some(id) {
        adapter.configuration_request = None;
    }
    let result = (|| {
        if record.bool("success") != Some(true) {
            return Err(record
                .string("error")
                .unwrap_or("Cannot capture Record baseline")
                .into());
        }
        let data = record.field("data").ok_or("Missing Record state")?;
        if session_id != adapter.session_id
            || data.get("sessionId").and_then(Value::as_str) != Some(session_id.as_str())
        {
            return Err("Record state belongs to a replaced session".into());
        }
        let model = data.get("model").ok_or("Missing Record model")?;
        Ok(ModelSelection {
            provider: model
                .get("provider")
                .and_then(Value::as_str)
                .ok_or("Missing Record provider")?
                .into(),
            model: model
                .get("id")
                .and_then(Value::as_str)
                .ok_or("Missing Record model ID")?
                .into(),
            reasoning_effort: data
                .get("thinkingLevel")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    })();
    let mut output = AdapterOutput::default();
    if result.is_ok() {
        output.merge(super::session::state_response(
            adapter,
            record.field("data"),
        ));
        output.merge(super::model::available_model_catalog(adapter));
    }
    output.events.push(AgentEvent::Interaction(
        InteractionEvent::PromptStateCaptured {
            record_id,
            session_id: super::queue::session_key(adapter),
            result,
        },
    ));
    Some(output)
}
