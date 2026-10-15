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
        matches!(&catalog.events[0], AgentEvent::Catalog(CatalogEvent::Commands(commands))
            if commands.iter().all(|command| command.name != RELOAD_COMMAND))
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

fn mcp_catalog() -> Value {
    json!({"commands":[
        {"name":RELOAD_COMMAND,"source":"extension"},
        {"name":mcp::GET,"source":"extension"},
        {"name":mcp::SAVE,"source":"extension"},
        {"name":"mcp","source":"extension","sourceInfo":{"path":"builtin:mcp"}}
    ]})
}

fn mcp_control(adapter: &mut PiAdapter, kind: &str) {
    adapter.record(
        serde_json::from_value(json!({
            "type":"extension_ui_request", "method":"setStatus", "statusKey":"pie-mcp-v1",
            "statusText":json!({"protocol":1, "sessionId":"s", "id":"edit", "kind":kind,
                "success":true, "saved":kind == "save", "snapshot":{
                    "revision":"updated", "trusted":true, "errors":[], "servers":[], "exposures":[]
                }}).to_string()
        }))
        .unwrap(),
    );
}

#[test]
fn mcp_save_prioritizes_reload_and_holds_work_until_native_status_settles() {
    use e_tui::mcp::{McpEvent, McpPatch, McpRequest};
    let mut adapter = PiAdapter::new(".", ".");
    response(
        &mut adapter,
        "bootstrap",
        "get_state",
        true,
        json!({"sessionId":"s"}),
    );
    adapter.commands_response(Some(&mcp_catalog()));
    adapter.request(AgentRequest::Mcp(McpRequest::Save {
        id: "edit".into(),
        server: "demo".into(),
        revision: "original".into(),
        patch: McpPatch::Enabled(false),
    }));
    let save_id = adapter.configuration_request.clone().unwrap();
    assert!(adapter
        .request(AgentRequest::Input {
            prompt: e_tui::PromptInput::text("waiting work")
        })
        .commands
        .is_empty());
    mcp_control(&mut adapter, "save");
    let output = response(
        &mut adapter,
        &save_id,
        "prompt",
        true,
        json!({"disposition":"handled"}),
    );
    assert!(
        matches!(&output.commands[..], [RpcCommand::Prompt { message, .. }] if message == &format!("/{RELOAD_COMMAND}"))
    );
    for (cmd, data) in [
        ("prompt", json!({"disposition":"handled"})),
        ("get_state", json!({"sessionId":"s"})),
        ("get_available_models", json!({"models":[]})),
        ("get_commands", mcp_catalog()),
    ] {
        let id = reload_id(&adapter);
        let output = response(&mut adapter, &id, cmd, true, data);
        assert!(!output.commands.iter().any(
            |cmd| matches!(cmd, RpcCommand::Prompt { message, .. } if message == "waiting work")
        ));
    }
    let get_id = adapter.configuration_request.clone().unwrap();
    mcp_control(&mut adapter, "get");
    let output = response(
        &mut adapter,
        &get_id,
        "prompt",
        true,
        json!({"disposition":"handled"}),
    );
    assert!(
        matches!(&output.commands[..], [RpcCommand::Prompt { message, .. }] if message == "/mcp")
    );
    let status_id = adapter.configuration_request.clone().unwrap();
    let output = response(
        &mut adapter,
        &status_id,
        "prompt",
        true,
        json!({"disposition":"handled"}),
    );
    assert!(output.events.iter().any(|event| matches!(event,
        AgentEvent::Mcp(McpEvent::Finished { id, error: None }) if id == "edit")));
    assert_eq!(
        output
            .commands
            .iter()
            .filter(|cmd| matches!(cmd,
        RpcCommand::Prompt { message, .. } if message == "waiting work"))
            .count(),
        1
    );
}

#[test]
fn mcp_inspection_timeout_releases_work_without_late_response_replay() {
    use e_tui::mcp::{McpEvent, McpRequest};
    let mut adapter = PiAdapter::new(".", ".");
    adapter.commands_response(Some(&mcp_catalog()));
    adapter.request(AgentRequest::Mcp(McpRequest::Get {
        id: "inspect".into(),
    }));
    let id = adapter.configuration_request.clone().unwrap();
    assert!(adapter
        .request(AgentRequest::Input {
            prompt: e_tui::PromptInput::text("waiting work")
        })
        .commands
        .is_empty());
    let output = adapter.tick_mcp(std::time::Instant::now() + std::time::Duration::from_secs(91));
    assert!(output.events.iter().any(|event| matches!(event,
        AgentEvent::Mcp(McpEvent::Finished { id, error: Some(_) }) if id == "inspect")));
    assert_eq!(
        output
            .commands
            .iter()
            .filter(|cmd| matches!(cmd,
        RpcCommand::Prompt { message, .. } if message == "waiting work"))
            .count(),
        1
    );
    let late = response(
        &mut adapter,
        &id,
        "prompt",
        true,
        json!({"disposition":"handled"}),
    );
    assert!(late.commands.is_empty());
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
