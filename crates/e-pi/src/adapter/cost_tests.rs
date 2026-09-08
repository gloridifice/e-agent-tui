use super::*;
use serde_json::json;

fn record(value: Value) -> RpcRecord {
    RpcRecord::from_value(value).unwrap()
}

fn attach(adapter: &mut PiAdapter, session_id: &str) -> AdapterOutput {
    adapter.record(record(json!({
        "type": "response", "command": "get_state", "success": true,
        "data": {"sessionId": session_id, "sessionFile": format!("{session_id}.jsonl")}
    })))
}

fn stats_id(output: &AdapterOutput) -> String {
    output
        .commands
        .iter()
        .find_map(|command| match command {
            RpcCommand::GetSessionStats { id } => id.clone(),
            _ => None,
        })
        .expect("session stats requested")
}

fn stats(adapter: &mut PiAdapter, id: &str, session_id: &str, cost: Value) -> AdapterOutput {
    adapter.record(record(json!({
        "type": "response", "command": "get_session_stats", "id": id, "success": true,
        "data": {"sessionId": session_id, "cost": cost}
    })))
}

#[test]
fn session_cost_uses_full_session_stats_not_replayed_message_usage() {
    let mut adapter = PiAdapter::new(".", "sessions");
    let id = stats_id(&attach(&mut adapter, "resumed"));
    assert_eq!(
        serde_json::to_value(RpcCommand::GetSessionStats {
            id: Some(id.clone())
        })
        .unwrap(),
        json!({"type": "get_session_stats", "id": id})
    );
    adapter.record(record(json!({
        "type": "response", "command": "get_messages", "success": true,
        "data": {"messages": [{"role": "assistant", "content": [], "usage": {"cost": {"total": 0.1}}}]}
    })));
    let output = stats(&mut adapter, &id, "resumed", json!(2.75));
    assert!(matches!(output.events.as_slice(),
        [AgentEvent::Session(SessionEvent::Cost { session_id, usd })]
            if session_id == "resumed.jsonl" && *usd == Some(2.75)
    ));
    assert!(stats(&mut adapter, &id, "resumed", json!(2.75))
        .events
        .is_empty());
}

#[test]
fn session_cost_refreshes_on_billable_completion_and_coalesces_requests() {
    let mut adapter = PiAdapter::new(".", "sessions");
    let mut id = stats_id(&attach(&mut adapter, "current"));
    stats(&mut adapter, &id, "current", json!(0));
    for event in [
        json!({"type": "message_end", "message": {"role": "assistant", "content": []}}),
        json!({"type": "message_end", "message": {"role": "toolResult", "toolCallId": "tool", "content": []}}),
        json!({"type": "compaction_end"}),
        json!({"type": "agent_settled"}),
    ] {
        id = stats_id(&adapter.record(record(event.clone())));
        assert!(adapter.record(record(event)).commands.is_empty());
        let trailing = stats(&mut adapter, &id, "current", json!(1.0));
        id = stats_id(&trailing);
        assert!(stats(&mut adapter, &id, "current", json!(1.5))
            .commands
            .is_empty());
    }
    for event in [
        json!({"type": "message_end", "message": {"role": "user", "content": "hi"}}),
        json!({"type": "message_update", "assistantMessageEvent": {"type": "text_delta", "delta": "hi"}}),
    ] {
        assert!(adapter.record(record(event)).commands.is_empty());
    }
}

#[test]
fn session_cost_ignores_stale_sessions_and_invalid_or_failed_stats() {
    let mut adapter = PiAdapter::new(".", "sessions");
    let old_id = stats_id(&attach(&mut adapter, "old"));
    let new_id = stats_id(&attach(&mut adapter, "new"));
    assert!(stats(&mut adapter, &old_id, "old", json!(99))
        .events
        .is_empty());
    assert!(stats(&mut adapter, &new_id, "old", json!(99))
        .events
        .is_empty());
    for cost in [Value::Null, json!(-1), json!("unknown"), json!(0)] {
        let id = stats_id(&session::refresh_stats(&mut adapter));
        let output = stats(&mut adapter, &id, "new", cost.clone());
        let expected = if cost == json!(0) { Some(0.0) } else { None };
        assert!(matches!(output.events.as_slice(),
            [AgentEvent::Session(SessionEvent::Cost { session_id, usd })]
                if session_id == "new.jsonl" && *usd == expected
        ));
    }
    let id = stats_id(&session::refresh_stats(&mut adapter));
    let failed = adapter.record(record(json!({
        "type": "response", "command": "get_session_stats", "id": id,
        "success": false, "error": "unsupported"
    })));
    assert!(failed.events.is_empty());
    assert!(adapter.pending_stats_request.is_none());
    assert!(!session::refresh_stats(&mut adapter).commands.is_empty());
}
