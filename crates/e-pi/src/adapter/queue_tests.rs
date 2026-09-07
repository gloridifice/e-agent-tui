use super::*;
use e_tui::{agent::AsapQueueOperation, AgentRequest};
use serde_json::json;

fn record(adapter: &mut PiAdapter, value: Value) -> AdapterOutput {
    adapter.record(serde_json::from_value(value).unwrap())
}

fn request_id(output: &AdapterOutput) -> String {
    serde_json::to_value(&output.commands[0]).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn queue_snapshots_acknowledge_admission_without_matching_expanded_or_duplicate_text() {
    let mut adapter = PiAdapter::new(".", ".");
    adapter.session_id = "id".into();
    adapter.last_attached_session = Some("session.jsonl".into());
    let output = adapter.request(AgentRequest::Steer {
        prompt: "original".into(),
    });
    let id = request_id(&output);
    assert!(record(&mut adapter, json!({"type":"queue_update", "steering":["expanded", "expanded"], "followUp":["extension"]})).events.is_empty());
    let output = record(
        &mut adapter,
        json!({"type":"response", "command":"prompt", "id":id, "success":true}),
    );
    assert!(
        matches!(output.events.as_slice(), [AgentEvent::Interaction(InteractionEvent::AsapQueue {
        session_id, prompts, operation: Some(AsapQueueOperation::Submit), error: None,
    })] if session_id == "session.jsonl" && prompts == &["expanded", "expanded", "extension"])
    );
    let output = record(
        &mut adapter,
        json!({"type":"queue_update", "steering":["expanded"], "followUp":[]}),
    );
    assert!(
        matches!(output.events.as_slice(), [AgentEvent::Interaction(InteractionEvent::AsapQueue {
        prompts, operation: None, ..
    })] if prompts == &["expanded"])
    );
}

#[test]
fn clear_waits_for_preflight_and_never_aborts_or_requeues() {
    let mut adapter = PiAdapter::new(".", ".");
    let submit = adapter.request(AgentRequest::Steer { prompt: "a".into() });
    assert!(adapter.request(AgentRequest::ClearAsap).commands.is_empty());
    record(
        &mut adapter,
        json!({"type":"queue_update", "steering":["a"], "followUp":["extension"]}),
    );
    let output = record(
        &mut adapter,
        json!({"type":"response", "command":"prompt", "id":request_id(&submit), "success":true}),
    );
    assert!(matches!(
        output.commands.as_slice(),
        [RpcCommand::ClearQueue { .. }]
    ));
    let clear_id = request_id(&output);
    assert!(record(
        &mut adapter,
        json!({"type":"queue_update", "steering":[], "followUp":[]})
    )
    .events
    .is_empty());
    let output = record(
        &mut adapter,
        json!({"type":"response", "command":"clear_queue", "id":clear_id, "success":true, "data":{"steering":["a"], "followUp":["extension"]}}),
    );
    assert!(output.commands.is_empty());
    assert!(
        matches!(output.events.as_slice(), [AgentEvent::Interaction(InteractionEvent::AsapQueue {
        prompts, operation: Some(AsapQueueOperation::Clear), error: None, ..
    })] if prompts.is_empty())
    );
}

#[test]
fn failed_clear_preserves_backend_snapshot_and_releases_barrier() {
    let mut adapter = PiAdapter::new(".", ".");
    record(
        &mut adapter,
        json!({"type":"queue_update", "steering":["a"], "followUp":[]}),
    );
    let clear = adapter.request(AgentRequest::ClearAsap);
    let output = record(
        &mut adapter,
        json!({"type":"response", "command":"clear_queue", "id":request_id(&clear), "success":false, "error":"unsupported"}),
    );
    assert!(
        matches!(output.events.as_slice(), [AgentEvent::Interaction(InteractionEvent::AsapQueue {
        prompts, operation: Some(AsapQueueOperation::Clear), error: Some(error), ..
    })] if prompts == &["a"] && error == "unsupported")
    );
    assert!(adapter.pending_queue.operation.is_none());
}

#[test]
fn consumed_before_prompt_ack_leaves_no_phantom_candidate() {
    let mut adapter = PiAdapter::new(".", ".");
    let submit = adapter.request(AgentRequest::Steer { prompt: "a".into() });
    record(
        &mut adapter,
        json!({"type":"queue_update", "steering":["a"], "followUp":[]}),
    );
    record(
        &mut adapter,
        json!({"type":"queue_update", "steering":[], "followUp":[]}),
    );
    let output = record(
        &mut adapter,
        json!({"type":"response", "command":"prompt", "id":request_id(&submit), "success":true}),
    );
    assert!(
        matches!(output.events.as_slice(), [AgentEvent::Interaction(InteractionEvent::AsapQueue {
        prompts, operation: Some(AsapQueueOperation::Submit), ..
    })] if prompts.is_empty())
    );
}

#[test]
fn failed_model_change_rejects_deferred_admission_without_stranding_the_frontend() {
    let mut adapter = PiAdapter::new(".", ".");
    adapter.configuration_request = Some("model-change".into());
    assert!(adapter
        .request(AgentRequest::Steer {
            prompt: "waiting".into()
        })
        .commands
        .is_empty());
    let output = record(
        &mut adapter,
        json!({
            "type":"response", "id":"model-change", "command":"set_model",
            "success":false, "error":"invalid model"
        }),
    );
    assert!(output.events.iter().any(|event| matches!(
        event,
        AgentEvent::Interaction(InteractionEvent::AsapQueue {
            operation: Some(AsapQueueOperation::Submit),
            error: Some(_),
            ..
        })
    )));
    assert!(adapter.deferred_requests.is_empty());
    assert!(adapter.pending_queue.operation.is_none());
}

#[test]
fn session_change_invalidates_queue_state_and_late_ack() {
    let mut adapter = PiAdapter::new(".", ".");
    let submit = adapter.request(AgentRequest::Steer {
        prompt: "old".into(),
    });
    record(
        &mut adapter,
        json!({"type":"queue_update", "steering":["old"], "followUp":[]}),
    );
    record(
        &mut adapter,
        json!({"type":"response", "command":"get_state", "success":true, "data":{"sessionId":"new", "sessionFile":"new.jsonl"}}),
    );
    assert!(adapter.pending_queue.prompts.is_empty());
    let output = record(
        &mut adapter,
        json!({"type":"response", "command":"prompt", "id":request_id(&submit), "success":false, "error":"old failure"}),
    );
    assert!(output.events.is_empty());
}
