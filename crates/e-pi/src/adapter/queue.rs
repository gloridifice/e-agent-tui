//! Authoritative Pi queue projection; admission and clear responses are barriers.

use e_tui::agent::{AgentEvent, AsapQueueOperation, InteractionEvent};
use serde_json::Value;

use super::{AdapterOutput, PiAdapter};
use crate::protocol::RpcRecord;

#[derive(Default)]
pub(super) struct PendingQueue {
    pub operation: Option<(String, AsapQueueOperation, String)>,
    pub prompts: Vec<String>,
}

pub(super) fn session_key(adapter: &PiAdapter) -> String {
    adapter
        .last_attached_session
        .clone()
        .unwrap_or_else(|| adapter.session_id.clone())
}

pub(super) fn event(
    adapter: &PiAdapter,
    operation: Option<AsapQueueOperation>,
    error: Option<String>,
) -> AdapterOutput {
    AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::AsapQueue {
        session_id: session_key(adapter),
        prompts: adapter.pending_queue.prompts.clone(),
        operation,
        error,
    }))
}

pub(super) fn update(adapter: &mut PiAdapter, record: &RpcRecord) -> AdapterOutput {
    let Some(steering) = record.field("steering").and_then(Value::as_array) else {
        return adapter.protocol_error("queue_update has no steering array".into());
    };
    let Some(follow_up) = record.field("followUp").and_then(Value::as_array) else {
        return adapter.protocol_error("queue_update has no followUp array".into());
    };
    let Some(prompts) = steering
        .iter()
        .chain(follow_up)
        .map(|text| text.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
    else {
        return adapter.protocol_error("queue_update contains a non-text prompt".into());
    };
    adapter.pending_queue.prompts = prompts;
    if adapter.pending_queue.operation.is_some() {
        AdapterOutput::default()
    } else {
        event(adapter, None, None)
    }
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    let matches = adapter
        .pending_queue
        .operation
        .as_ref()
        .is_some_and(|(id, _, _)| record.string("id") == Some(id.as_str()));
    if !matches {
        return record
            .string("id")
            .filter(|id| id.starts_with("pie-asap-") || id.starts_with("pie-clear-asap-"))
            .map(|_| AdapterOutput::default());
    }
    let (_, operation, session_id) = adapter.pending_queue.operation.take()?;
    if session_id != session_key(adapter) {
        return Some(AdapterOutput::default());
    }
    let error = (record.bool("success") != Some(true)).then(|| {
        record
            .string("error")
            .unwrap_or("Pi pending queue operation failed")
            .to_owned()
    });
    Some(event(adapter, Some(operation), error))
}
