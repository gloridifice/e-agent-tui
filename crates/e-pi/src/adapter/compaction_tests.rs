use super::*;
use serde_json::json;

fn configured() -> PiAdapter {
    let mut adapter = PiAdapter::new(".", ".");
    adapter.current_model = Some(json!({"provider":"p", "id":"large", "name":"Large"}));
    adapter.available_models = vec![
        adapter.current_model.clone().unwrap(),
        json!({"provider":"q", "id":"small", "name":"Small"}),
    ];
    adapter.thinking_level = Some("high".into());
    let output = command(&mut adapter, "/compact set-model q/small");
    assert!(output.commands.is_empty());
    adapter
}
fn command(adapter: &mut PiAdapter, line: &str) -> AdapterOutput {
    adapter.request(AgentRequest::Command {
        line: line.into(),
        images: Vec::new(),
    })
}
fn reply(
    adapter: &mut PiAdapter,
    output: &AdapterOutput,
    success: bool,
    data: Value,
) -> AdapterOutput {
    let sent = serde_json::to_value(&output.commands[0]).unwrap();
    adapter.record(serde_json::from_value(json!({"type":"response", "command":sent["type"], "id":sent["id"], "success":success, "data":data, "error":"test failure"})).unwrap())
}
fn snapshot() -> Value {
    json!({"model":{"provider":"p", "id":"large", "name":"Large"}, "thinkingLevel":"high"})
}
fn begin(adapter: &mut PiAdapter) -> AdapterOutput {
    let output = command(adapter, "/compact keep decisions");
    assert!(matches!(output.commands[0], RpcCommand::Abort { .. }));
    let output = reply(adapter, &output, true, Value::Null);
    assert!(matches!(output.commands[0], RpcCommand::GetState { .. }));
    let output = reply(adapter, &output, true, snapshot());
    assert!(
        matches!(&output.commands[0], RpcCommand::SetModel { provider, model_id, .. } if provider == "q" && model_id == "small")
    );
    output
}
fn restore(adapter: &mut PiAdapter, output: AdapterOutput) -> AdapterOutput {
    assert!(
        matches!(&output.commands[0], RpcCommand::SetModel { provider, model_id, .. } if provider == "p" && model_id == "large")
    );
    let output = reply(adapter, &output, true, snapshot()["model"].clone());
    assert!(
        matches!(&output.commands[0], RpcCommand::SetThinkingLevel { level, .. } if level == "high")
    );
    let output = reply(adapter, &output, true, Value::Null);
    assert!(matches!(output.commands[0], RpcCommand::GetState { .. }));
    reply(adapter, &output, true, snapshot())
}

#[test]
fn compaction_manual_holds_work_through_success_and_failure_restoration() {
    for success in [true, false] {
        let mut adapter = configured();
        let output = begin(&mut adapter);
        let output = reply(
            &mut adapter,
            &output,
            true,
            json!({"provider":"q","id":"small"}),
        );
        assert!(
            matches!(&output.commands[0], RpcCommand::Compact { custom_instructions: Some(text), .. } if text == "keep decisions")
        );
        assert!(adapter
            .request(AgentRequest::Input {
                prompt: "next".into()
            })
            .commands
            .is_empty());
        let started = adapter.record(
            serde_json::from_value(json!({"type":"compaction_start", "reason":"manual"})).unwrap(),
        );
        assert!(
            matches!(&started.events[0], AgentEvent::Timeline(TimelineEvent::Append(record)) if matches!(&record.fact, TimelineFact::CompactionStarted {model_name: Some(name), ..} if name == "Small"))
        );
        let output = reply(&mut adapter, &output, success, Value::Null);
        let output = restore(&mut adapter, output);
        assert!(adapter.pending_compaction.is_none());
        assert!(output.commands.iter().any(
            |command| matches!(command, RpcCommand::Prompt { message, .. } if message == "next")
        ));
    }
}

#[test]
fn compaction_selection_failure_and_cancellation_never_compact() {
    for cancelled in [false, true] {
        let mut adapter = configured();
        let output = begin(&mut adapter);
        if cancelled {
            adapter.request(AgentRequest::Interrupt);
        }
        let output = reply(&mut adapter, &output, cancelled, Value::Null);
        let output = restore(&mut adapter, output);
        assert!(output
            .commands
            .iter()
            .all(|command| matches!(command, RpcCommand::GetSessionStats { .. })));
        assert!(adapter.pending_compaction.is_none());
    }
}

#[test]
fn compaction_cancelled_end_is_not_success_and_restoration_waits_for_compact_response() {
    let mut adapter = configured();
    let output = begin(&mut adapter);
    let output = reply(&mut adapter, &output, true, Value::Null);
    let start = adapter.record(
        serde_json::from_value(json!({"type":"compaction_start", "reason":"manual"})).unwrap(),
    );
    adapter.request(AgentRequest::Interrupt);
    let end = adapter.record(
        serde_json::from_value(json!({"type":"compaction_end", "reason":"manual", "aborted":true}))
            .unwrap(),
    );
    let AgentEvent::Timeline(TimelineEvent::Append(start)) = &start.events[0] else {
        panic!("start")
    };
    let AgentEvent::Timeline(TimelineEvent::Append(end)) = &end.events[0] else {
        panic!("end")
    };
    assert!(
        matches!((&start.fact, &end.fact), (TimelineFact::CompactionStarted {id: a, ..}, TimelineFact::CompactionFinished {id: b, model_name: Some(name), error: Some(_), ..}) if a == b && name == "Small")
    );
    assert!(adapter.pending_compaction.is_some());
    let output = reply(&mut adapter, &output, false, Value::Null);
    restore(&mut adapter, output);
    assert!(adapter.pending_compaction.is_none());
}

#[test]
fn compaction_restoration_failure_does_not_release_dependent_work() {
    let mut adapter = configured();
    let output = begin(&mut adapter);
    let output = reply(&mut adapter, &output, true, Value::Null);
    let output = reply(&mut adapter, &output, false, Value::Null);
    assert!(adapter
        .request(AgentRequest::Input {
            prompt: "held".into()
        })
        .commands
        .is_empty());
    let output = reply(&mut adapter, &output, false, Value::Null);
    assert!(output.commands.is_empty());
    assert!(adapter.pending_compaction.is_some());
    assert_eq!(adapter.deferred_requests.len(), 1);
}

#[test]
fn compaction_automatic_uses_conversation_model_and_unset_restores_native_manual() {
    let mut adapter = configured();
    assert_eq!(
        compaction::active_model_name(&adapter, Some("threshold")).as_deref(),
        Some("Large")
    );
    assert!(adapter.pending_compaction.is_none());
    command(&mut adapter, "/compact unset-model");
    let output = command(&mut adapter, "/compact");
    assert!(matches!(output.commands[0], RpcCommand::Compact { .. }));
    assert!(adapter.compaction_model.is_none());
    let output = command(&mut adapter, "/compactor");
    assert!(
        matches!(&output.commands[0], RpcCommand::Prompt { message, .. } if message == "/compactor")
    );
}
