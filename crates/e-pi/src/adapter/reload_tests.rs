use super::*;
use serde_json::json;

fn command(adapter: &mut PiAdapter, line: &str) -> AdapterOutput {
    adapter.request(AgentRequest::Command {
        line: line.into(),
        images: vec![],
    })
}
fn reload_id(adapter: &PiAdapter) -> String {
    adapter
        .pending_reload
        .as_ref()
        .expect("pending reload")
        .id
        .clone()
}
fn response(
    adapter: &mut PiAdapter,
    id: &str,
    cmd: &str,
    success: bool,
    data: Value,
) -> AdapterOutput {
    adapter.record(
        serde_json::from_value(
            json!({"type":"response", "id":id, "command":cmd, "success":success, "data":data}),
        )
        .unwrap(),
    )
}

#[test]
fn reload_uses_native_companion_then_replaces_catalog_before_releasing_work() {
    let mut adapter = PiAdapter::new(".", ".");
    let catalog = adapter.commands_response(Some(
        &json!({"commands":[{"name":RELOAD_COMMAND,"source":"extension"}]}),
    ));
    assert!(
        matches!(&catalog.events[0], AgentEvent::Catalog(CatalogEvent::Commands(commands)) if commands.is_empty())
    );
    let output = command(&mut adapter, "/reload");
    assert!(
        matches!(&output.commands[0], RpcCommand::Prompt { message, .. } if message == &format!("/{RELOAD_COMMAND}"))
    );
    let id = reload_id(&adapter);
    assert!(command(&mut adapter, "/skill:new").commands.is_empty());
    let output = response(&mut adapter, &id, "prompt", true, Value::Null);
    assert!(matches!(output.commands[0], RpcCommand::GetState { .. }));
    assert!(output.events.is_empty());
    let id = reload_id(&adapter);
    let output = response(
        &mut adapter,
        &id,
        "get_state",
        true,
        json!({"sessionId":"s", "cwd":"."}),
    );
    assert!(output
        .commands
        .iter()
        .any(|command| matches!(command, RpcCommand::GetAvailableModels { .. })));
    let id = reload_id(&adapter);
    let output = response(
        &mut adapter,
        &id,
        "get_available_models",
        true,
        json!({"models":[]}),
    );
    assert!(matches!(output.commands[0], RpcCommand::GetCommands { .. }));
    let id = reload_id(&adapter);
    let output = response(
        &mut adapter,
        &id,
        "get_commands",
        true,
        json!({"commands":[{"name":"skill:new","source":"skill"}]}),
    );
    assert!(output.events.iter().any(|event| matches!(event, AgentEvent::Catalog(CatalogEvent::Skills(skills)) if skills.len() == 1 && skills[0].name == "new")));
    assert!(output
        .commands
        .iter()
        .any(|cmd| matches!(cmd, RpcCommand::Prompt { message, .. } if message == "/skill:new")));
    assert!(adapter.pending_reload.is_none());
}

#[test]
fn reload_catalog_failure_releases_barrier_without_success() {
    let mut adapter = PiAdapter::new(".", ".");
    adapter.reload_available = true;
    command(&mut adapter, "/reload");
    for cmd in ["prompt", "get_state"] {
        let id = reload_id(&adapter);
        response(&mut adapter, &id, cmd, true, json!({"sessionId":"s"}));
    }
    let id = reload_id(&adapter);
    let output = response(
        &mut adapter,
        &id,
        "get_available_models",
        false,
        Value::Null,
    );
    assert!(adapter.pending_reload.is_none());
    assert!(adapter.configuration_request.is_none());
    assert!(output.events.iter().all(|event| !matches!(event, AgentEvent::Interaction(InteractionEvent::CommandResult { outcome, .. }) if outcome == "success")));
}

#[test]
fn reload_unavailable_busy_and_extension_failure_never_become_model_prompts_or_success() {
    let mut adapter = PiAdapter::new(".", ".");
    assert!(command(&mut adapter, "/reload").commands.is_empty());
    adapter.reload_available = true;
    adapter.is_streaming = true;
    assert!(command(&mut adapter, "/reload").commands.is_empty());
    adapter.is_streaming = false;
    command(&mut adapter, "/reload");
    let id = reload_id(&adapter);
    adapter.record(
        serde_json::from_value(json!({"type":"extension_error","error":"reload failed"})).unwrap(),
    );
    let output = response(&mut adapter, &id, "prompt", true, Value::Null);
    assert!(output.commands.is_empty());
    assert!(output.events.iter().any(|event| matches!(
        event,
        AgentEvent::Interaction(InteractionEvent::Error { .. })
    )));
    assert!(adapter.configuration_request.is_none());
}

#[test]
fn extension_errors_after_the_companion_step_do_not_fail_the_reload() {
    let mut adapter = PiAdapter::new(".", ".");
    adapter.reload_available = true;
    command(&mut adapter, "/reload");
    let id = reload_id(&adapter);
    let output = response(&mut adapter, &id, "prompt", true, Value::Null);
    assert!(matches!(output.commands[0], RpcCommand::GetState { .. }));
    // An unrelated extension failing during catalog reads is reported as an
    // error event, but it does not make the reload itself fail.
    adapter.record(
        serde_json::from_value(json!({"type":"extension_error","error":"other extension failed"}))
            .unwrap(),
    );
    for cmd in ["get_state", "get_available_models"] {
        let id = reload_id(&adapter);
        response(&mut adapter, &id, cmd, true, json!({"sessionId":"s"}));
    }
    let id = reload_id(&adapter);
    let output = response(
        &mut adapter,
        &id,
        "get_commands",
        true,
        json!({"commands":[]}),
    );
    assert!(adapter.pending_reload.is_none());
    assert!(adapter.configuration_request.is_none());
    assert!(output.events.iter().any(|event| matches!(
        event,
        AgentEvent::Interaction(InteractionEvent::CommandResult { id, outcome, .. })
            if id == "reload" && outcome == "success"
    )));
}

#[test]
fn abandoned_reload_error_never_reaches_a_later_reload() {
    let mut adapter = PiAdapter::new(".", ".");
    adapter.reload_available = true;
    command(&mut adapter, "/reload");
    let id = reload_id(&adapter);
    adapter.record(
        serde_json::from_value(json!({"type":"extension_error","error":"reload failed"})).unwrap(),
    );
    response(&mut adapter, &id, "prompt", true, Value::Null);
    assert!(adapter.pending_reload.is_none());
    assert_eq!(command(&mut adapter, "/skill:new").commands.len(), 1);
    command(&mut adapter, "/reload");
    let id = reload_id(&adapter);
    let output = response(&mut adapter, &id, "prompt", true, Value::Null);
    assert!(matches!(output.commands[0], RpcCommand::GetState { .. }));
    assert!(output.events.is_empty());
}
