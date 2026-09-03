//! Session attachment, title, snapshot, and session-list projection for Pi.

use e_tui::agent::{
    timeline::{TimelineFact, TimelineRecord},
    AgentEvent, AgentStatus, AttachedSession, SessionEvent, TimelineEvent,
};

use serde_json::Value;

use crate::session_index;

use super::{
    content::{assistant_fact, content_parts, content_text, user_fact},
    tool, AdapterOutput, PiAdapter,
};

/// Classify one get_state payload into session attach/refresh events.
pub(super) fn state_response(adapter: &mut PiAdapter, data: Option<&Value>) -> AdapterOutput {
    let Some(data) = data else {
        return adapter.protocol_error("get_state response has no data".into());
    };
    adapter.current_model = data.get("model").filter(|value| !value.is_null()).cloned();
    adapter.thinking_level = data
        .get("thinkingLevel")
        .and_then(Value::as_str)
        .map(str::to_owned);
    adapter.is_streaming = data
        .get("isStreaming")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    adapter.session_id = data
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("pi-session")
        .to_owned();
    let session_key = data
        .get("sessionFile")
        .and_then(Value::as_str)
        .unwrap_or(&adapter.session_id)
        .to_owned();
    adapter.session_name = data
        .get("sessionName")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let provider = adapter
        .current_model
        .as_ref()
        .and_then(|model| model.get("provider"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let model = adapter
        .current_model
        .as_ref()
        .and_then(|model| model.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let status = if adapter.is_streaming {
        AgentStatus::Running
    } else {
        AgentStatus::Idle
    };
    // `Attached` mirrors the DSH welcome contract: it reports a session attach,
    // not a state refresh. Same-session refreshes (e.g. the `get_state` issued
    // after `/model` or a ping) must not re-emit it, because the frontend
    // discards a pending `/new` draft on every `Attached` and would visibly
    // switch back to the retained session.
    let switched = adapter.last_attached_session.as_deref() != Some(session_key.as_str());
    adapter.last_attached_session = Some(session_key.clone());
    let output = if switched {
        // `Attached` already carries the title; mirror it in the dedup key
        // so the follow-up refresh does not emit a duplicate event.
        adapter.emitted_title = adapter.session_name.clone();
        AdapterOutput::event(AgentEvent::Session(SessionEvent::Attached(
            AttachedSession {
                protocol_version: None,
                max_frame_bytes: None,
                id: session_key,
                status,
                provider,
                model,
                mode: Some("pi".into()),
                title: adapter.session_name.clone(),
                workspace: Some(adapter.cwd.to_string_lossy().into_owned()),
            },
        )))
    } else {
        // Same-session refresh: the only path that observes in-session
        // renames (an extension command calling `set_session_name`).
        let mut output = AdapterOutput::event(AgentEvent::Session(SessionEvent::Status(status)));
        output.events.extend(title_events(adapter));
        output
    };
    output
}

/// Status-bar title: Pi's explicit session name when set, else the first
/// user message — the same precedence the session index uses.
pub(super) fn current_title(adapter: &PiAdapter) -> Option<String> {
    adapter
        .session_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| adapter.derived_title.clone())
}

/// Report a `SessionEvent::Title` whenever the visible title changed since
/// the last report. Same-session state refreshes, snapshots, and live
/// first user messages flow through here; `Attached` already carries the
/// title on session switches.
pub(super) fn title_events(adapter: &mut PiAdapter) -> Vec<AgentEvent> {
    let title = current_title(adapter);
    if title == adapter.emitted_title {
        return Vec::new();
    }
    adapter.emitted_title = title.clone();
    vec![AgentEvent::Session(SessionEvent::Title(
        title.unwrap_or_default(),
    ))]
}

/// Seed the first-user-message fallback title. Pi only names sessions
/// explicitly (`set_session_name`); every other session is identified by
/// its first prompt, exactly like the session index.
pub(super) fn note_first_user_title(adapter: &mut PiAdapter, text: &str) {
    let named = adapter
        .session_name
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty());
    if named || adapter.derived_title.is_some() {
        return;
    }
    let title = session_index::clean_title(text);
    if !title.is_empty() {
        adapter.derived_title = Some(title);
    }
}

pub(super) fn live_message(adapter: &mut PiAdapter, message: &Value) -> AdapterOutput {
    match message.get("role").and_then(Value::as_str) {
        Some("user") => {
            note_first_user_title(
                adapter,
                &content_text(message.get("content").unwrap_or(&Value::Null)),
            );
            let mut output = adapter.timeline(user_fact(message));
            output.events.extend(title_events(adapter));
            output
        }
        Some("assistant") => adapter.timeline(assistant_fact(
            message,
            Some(adapter.current_turn.max(1)),
            Some(0),
        )),
        // `tool_execution_end` carries the same result immediately
        // before Pi appends its durable `toolResult` message. The former
        // owns live tool projection; suppress the latter duplicate.
        Some("toolResult") => {
            let id = message
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or("pi-tool");
            if adapter.pending_tool_result_messages.remove(id) {
                AdapterOutput::default()
            } else {
                adapter.timeline(tool::tool_result_fact(message))
            }
        }
        _ => AdapterOutput::default(),
    }
}

pub(super) fn snapshot_message(
    adapter: &mut PiAdapter,
    message: &Value,
    turn: Option<u64>,
) -> Vec<TimelineRecord> {
    match message.get("role").and_then(Value::as_str) {
        Some("user") => vec![adapter.record_fact(user_fact(message))],
        Some("assistant") => {
            let mut records = vec![adapter.record_fact(assistant_fact(message, turn, Some(0)))];
            for call in content_parts(message)
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("toolCall"))
            {
                records.push(
                    adapter.record_fact(TimelineFact::ToolCall(tool::tool_activity(
                        call.get("id").and_then(Value::as_str).unwrap_or("pi-tool"),
                        call.get("name").and_then(Value::as_str).unwrap_or("tool"),
                        call.get("arguments").cloned().unwrap_or(Value::Null),
                    ))),
                );
            }
            records
        }
        Some("toolResult") => vec![adapter.record_fact(tool::tool_result_fact(message))],
        Some("compactionSummary") => vec![adapter.record_fact(TimelineFact::CompactionSummary {
            id: "pi-compaction".into(),
            summary: message
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        })],
        Some("branchSummary") => vec![adapter.record_fact(TimelineFact::Custom {
            namespace: "pi".into(),
            kind: Some("branch-summary".into()),
            summary: message
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })],
        _ => Vec::new(),
    }
}

pub(super) fn messages_response(adapter: &mut PiAdapter, data: Option<&Value>) -> AdapterOutput {
    let messages = data
        .and_then(|data| data.get("messages"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // The first user message is the fallback title for unnamed sessions,
    // matching what the session list already shows.
    if let Some(first_user) = messages
        .iter()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    {
        note_first_user_title(
            adapter,
            &content_text(first_user.get("content").unwrap_or(&Value::Null)),
        );
    }
    let mut records = Vec::new();
    let mut snapshot_turn = 0_u64;
    for message in messages {
        let turn = if message.get("role").and_then(Value::as_str) == Some("assistant") {
            snapshot_turn = snapshot_turn.saturating_add(1);
            Some(snapshot_turn)
        } else {
            None
        };
        records.extend(snapshot_message(adapter, &message, turn));
    }
    adapter.current_turn = snapshot_turn;
    let mut output = AdapterOutput::event(AgentEvent::Timeline(TimelineEvent::Snapshot {
        records,
        truncated: false,
    }));
    output.events.extend(title_events(adapter));
    output
}
