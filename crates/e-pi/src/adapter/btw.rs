//! One-shot side questions use companion controls, not normal prompt admission.

use super::{AdapterOutput, PiAdapter};
use crate::protocol::{RpcCommand, RpcRecord};
use e_tui::agent::{AgentEvent, InteractionEvent};
use serde::Deserialize;

pub(super) const COMMAND: &str = "__pie_btw_v1";
pub(super) const STATUS: &str = "pie-btw-v1";

fn exchange(
    adapter: &PiAdapter,
    id: &str,
    question: String,
    reply: Option<Result<String, String>>,
) -> AgentEvent {
    AgentEvent::Interaction(InteractionEvent::BtwExchange {
        id: id.into(),
        session_id: super::queue::session_key(adapter),
        question,
        reply,
    })
}

pub(super) fn request(adapter: &mut PiAdapter, session: String, question: String) -> AdapterOutput {
    if !adapter.btw_available
        || adapter.last_attached_session.as_ref() != Some(&session)
        || adapter.pending_reload.is_some()
        || adapter.pending_resume.is_some()
        || adapter.pending_fork.is_some()
        || !adapter.pending_new.is_empty()
    {
        return adapter.unsupported(
            "[BTW] Side questions are unavailable until the Pi session and companion are ready",
        );
    }
    if question.trim().is_empty() || question.len() > 64 * 1024 {
        return adapter.unsupported("[BTW] Supply a nonempty question of at most 64 KiB");
    }
    if adapter.pending_btw.len() >= 3 {
        return adapter.unsupported("[BTW] Three side questions are already running");
    }
    let id = adapter.request_id("btw");
    let payload =
        serde_json::json!({ "id": id, "sessionId": adapter.session_id, "question": question });
    let mut output = AdapterOutput::command(RpcCommand::Prompt {
        id: Some(id.clone()),
        message: format!("/{COMMAND} {payload}"),
        streaming_behavior: None,
    });
    output
        .events
        .push(exchange(adapter, &id, question.clone(), None));
    adapter.pending_btw.insert(id, question);
    output
}

fn finish(adapter: &mut PiAdapter, id: &str, result: Result<String, String>) -> AdapterOutput {
    adapter
        .pending_btw
        .remove(id)
        .map(|q| AdapterOutput::event(exchange(adapter, id, q, Some(result))))
        .unwrap_or_default()
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    let id = record
        .string("id")
        .filter(|id| id.starts_with("pie-btw-"))?;
    // The companion publishes its terminal result before the command returns.
    // A response with no such result also settles rejected/withdrawn commands.
    Some(finish(
        adapter,
        id,
        Err("Pi did not return a side answer".into()),
    ))
}

pub(super) fn status(adapter: &mut PiAdapter, json: &str) -> AdapterOutput {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Answer {
        id: String,
        session_id: String,
        ok: bool,
        text: String,
    }
    if json.len() > 2 * 1024 * 1024 {
        return AdapterOutput::default();
    }
    let Ok(answer) = serde_json::from_str::<Answer>(json) else {
        return AdapterOutput::default();
    };
    if answer.session_id != adapter.session_id || answer.text.len() > 256 * 1024 {
        return AdapterOutput::default();
    }
    finish(
        adapter,
        &answer.id,
        if answer.ok {
            Ok(answer.text)
        } else {
            Err(answer.text)
        },
    )
}
