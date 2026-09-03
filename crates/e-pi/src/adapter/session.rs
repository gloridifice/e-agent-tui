//! Session attachment, title, snapshot, and session-list projection for Pi.

use e_tui::agent::{AgentEvent, AgentStatus, AttachedSession, SessionEvent, TimelineEvent};

use serde_json::Value;

use crate::session_index;

use super::{content::content_text, AdapterOutput, PiAdapter};

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
    let mut output = if switched {
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
        output.events.extend(adapter.title_events());
        output
    };
    output.merge(adapter.available_model_catalog());
    output
}

pub(super) fn current_title(adapter: &PiAdapter) -> Option<String> {
    adapter
        .session_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| adapter.derived_title.clone())
}

pub(super) fn title_events(adapter: &mut PiAdapter) -> Vec<AgentEvent> {
    let title = adapter.current_title();
    if title == adapter.emitted_title {
        return Vec::new();
    }
    adapter.emitted_title = title.clone();
    vec![AgentEvent::Session(SessionEvent::Title(
        title.unwrap_or_default(),
    ))]
}

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
        adapter.note_first_user_title(&content_text(
            first_user.get("content").unwrap_or(&Value::Null),
        ));
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
        records.extend(adapter.snapshot_message(&message, turn));
    }
    adapter.current_turn = snapshot_turn;
    let mut output = AdapterOutput::event(AgentEvent::Timeline(TimelineEvent::Snapshot {
        records,
        truncated: false,
    }));
    output.events.extend(adapter.title_events());
    output
}
