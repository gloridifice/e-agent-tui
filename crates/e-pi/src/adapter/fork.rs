//! Native session replacement with an optional post-replacement prompt.

use e_tui::{
    action::QuestionAnswer,
    agent::{AgentEvent, InteractionEvent, Question, QuestionOption},
};
use serde_json::Value;

use super::{model, session, AdapterOutput, PiAdapter};
use crate::protocol::{ExtensionUiResponse, RpcCommand, RpcRecord};

const MAX_CHOICES: usize = 512;

struct Choice {
    entry_id: String,
    label: String,
}

enum Stage {
    Messages,
    Choice(Vec<Choice>),
    Replace,
    State,
    History,
    Models,
    Commands,
    Prompt,
}

pub(super) struct Pending {
    id: String,
    kind: &'static str,
    source_session: String,
    target_session: Option<String>,
    stage: Stage,
    message: Option<String>,
    editor_text: Option<String>,
    interrupted: bool,
}

fn result(kind: &str, outcome: &str, text: Option<String>) -> AdapterOutput {
    AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::CommandResult {
        id: kind.into(),
        outcome: outcome.into(),
        text,
    }))
}

pub(super) fn command(adapter: &mut PiAdapter, line: &str) -> Option<AdapterOutput> {
    let (name, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let kind = match name {
        "/fork" => "fork",
        "/clone" => "clone",
        _ => return None,
    };
    if adapter.last_attached_session.is_none()
        || adapter.is_streaming
        || adapter.active_compaction_id.is_some()
        || !adapter.pending_queue.prompts.is_empty()
        || adapter.pending_command_prompt.is_some()
        || !adapter.extension_ui.is_empty()
        || !adapter.deferred_requests.is_empty()
    {
        return Some(result(
            kind,
            "error",
            Some(format!("Wait for Pi to become idle before /{kind}")),
        ));
    }
    let id = adapter.request_id("branch");
    let command = if kind == "fork" {
        RpcCommand::GetForkMessages {
            id: Some(id.clone()),
        }
    } else {
        RpcCommand::Clone {
            id: Some(id.clone()),
        }
    };
    let message = rest.trim_start();
    adapter.pending_fork = Some(Pending {
        id,
        kind,
        source_session: adapter.session_id.clone(),
        target_session: None,
        stage: if kind == "fork" {
            Stage::Messages
        } else {
            Stage::Replace
        },
        message: (!message.trim().is_empty()).then(|| message.to_owned()),
        editor_text: None,
        interrupted: false,
    });
    Some(AdapterOutput::command(command))
}

fn finish(
    adapter: &mut PiAdapter,
    pending: Pending,
    outcome: &str,
    text: Option<String>,
) -> AdapterOutput {
    adapter.pending_fork = None;
    let mut output = result(pending.kind, outcome, text);
    if outcome != "success" {
        if let Some(message) = pending.message {
            output
                .events
                .push(AgentEvent::Interaction(InteractionEvent::SetEditorText {
                    text: if pending.target_session.is_some() {
                        message
                    } else {
                        format!("/{} {message}", pending.kind)
                    },
                }));
        }
    }
    output
}

pub(super) fn answer(
    adapter: &mut PiAdapter,
    request_id: &str,
    answers: &[QuestionAnswer],
) -> Option<AdapterOutput> {
    let pending = adapter.pending_fork.as_ref()?;
    if pending.id != request_id || !matches!(pending.stage, Stage::Choice(_)) {
        return None;
    }
    let mut pending = adapter.pending_fork.take()?;
    let Stage::Choice(choices) = &pending.stage else {
        unreachable!()
    };
    let selected = answers.first().and_then(|answer| answer.selected.first());
    let entry_id = choices
        .iter()
        .find(|choice| Some(&choice.label) == selected)
        .map(|choice| choice.entry_id.clone());
    let mut output = resolved(request_id);
    if pending.source_session != adapter.session_id {
        output.merge(finish(
            adapter,
            pending,
            "error",
            Some("The source session changed; run /fork again".into()),
        ));
    } else if let Some(entry_id) = entry_id {
        pending.id = adapter.request_id("branch");
        pending.stage = Stage::Replace;
        output.commands.push(RpcCommand::Fork {
            id: Some(pending.id.clone()),
            entry_id,
        });
        adapter.pending_fork = Some(pending);
    } else {
        output.merge(finish(
            adapter,
            pending,
            "error",
            Some("Invalid fork selection".into()),
        ));
    }
    Some(output)
}

fn resolved(id: &str) -> AdapterOutput {
    AdapterOutput::event(AgentEvent::Interaction(
        InteractionEvent::QuestionResolved {
            request_id: id.into(),
            outcome: "completed".into(),
        },
    ))
}

pub(super) fn cancel(adapter: &mut PiAdapter, request_id: &str) -> Option<AdapterOutput> {
    let pending = adapter.pending_fork.as_ref()?;
    if pending.id != request_id || !matches!(pending.stage, Stage::Choice(_)) {
        return None;
    }
    interrupt(adapter)
}

pub(super) fn interrupt(adapter: &mut PiAdapter) -> Option<AdapterOutput> {
    let mut pending = adapter.pending_fork.take()?;
    if matches!(pending.stage, Stage::Messages | Stage::Choice(_)) {
        let mut output = resolved(&pending.id);
        output.merge(finish(adapter, pending, "cancelled", None));
        return Some(output);
    }
    pending.interrupted = true;
    let mut output = AdapterOutput::command(RpcCommand::Abort {
        id: Some(adapter.request_id("abort")),
    });
    for (id, _) in adapter.extension_ui.drain() {
        output.commands.push(RpcCommand::ExtensionUiResponse {
            id: id.clone(),
            response: ExtensionUiResponse::Cancelled { cancelled: true },
        });
        output.merge(resolved(&id));
    }
    adapter.pending_fork = Some(pending);
    Some(output)
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    let id = record.string("id")?;
    if !id.starts_with("pie-branch-") {
        return None;
    }
    if adapter
        .pending_fork
        .as_ref()
        .is_none_or(|pending| pending.id != id)
    {
        return Some(AdapterOutput::default());
    }
    let mut pending = adapter.pending_fork.take()?;
    if record.bool("success") != Some(true) {
        return Some(finish(
            adapter,
            pending,
            "error",
            Some(
                record
                    .string("error")
                    .unwrap_or("Pi session operation failed")
                    .into(),
            ),
        ));
    }
    let command = match pending.stage {
        Stage::Messages => "get_fork_messages",
        Stage::Choice(_) => {
            return Some(finish(
                adapter,
                pending,
                "error",
                Some("Unexpected fork response during selection".into()),
            ))
        }
        Stage::Replace => pending.kind,
        Stage::State => "get_state",
        Stage::History => "get_messages",
        Stage::Models => "get_available_models",
        Stage::Commands => "get_commands",
        Stage::Prompt => "prompt",
    };
    if record.string("command") != Some(command)
        || pending
            .target_session
            .as_ref()
            .is_some_and(|id| *id != adapter.session_id)
    {
        return Some(finish(
            adapter,
            pending,
            "error",
            Some("Stale or unexpected Pi session response; no further message was sent".into()),
        ));
    }
    let data = record.field("data").unwrap_or(&Value::Null);
    let mut output = AdapterOutput::default();
    let next_id = adapter.request_id("branch");
    let next = match pending.stage {
        Stage::Messages => {
            if adapter.session_id != pending.source_session {
                return Some(finish(
                    adapter,
                    pending,
                    "error",
                    Some("The source session changed; run /fork again".into()),
                ));
            }
            let Some(messages) = data.get("messages").and_then(Value::as_array) else {
                return Some(finish(
                    adapter,
                    pending,
                    "error",
                    Some("Pi returned no fork message list".into()),
                ));
            };
            let choices: Vec<_> = messages
                .iter()
                .rev()
                .take(MAX_CHOICES)
                .filter_map(|message| {
                    let entry_id = message.get("entryId")?.as_str()?.to_owned();
                    let text = message.get("text")?.as_str()?;
                    Some(Choice {
                        label: format!("{entry_id}  {}", crate::session_index::clean_title(text)),
                        entry_id,
                    })
                })
                .collect();
            if choices.is_empty() {
                return Some(finish(
                    adapter,
                    pending,
                    "error",
                    Some("No user messages are available to fork".into()),
                ));
            }
            let question = Question {
                id: "fork-entry".into(),
                header: Some("Fork session".into()),
                question: if messages.len() > MAX_CHOICES {
                    format!("Select a user message to edit or replace (latest {MAX_CHOICES} shown)")
                } else {
                    "Select a user message to edit or replace".into()
                },
                options: Some(
                    choices
                        .iter()
                        .map(|choice| QuestionOption {
                            label: choice.label.clone(),
                            description: None,
                        })
                        .collect(),
                ),
                multi_select: false,
            };
            pending.id = next_id.clone();
            pending.stage = Stage::Choice(choices);
            adapter.pending_fork = Some(pending);
            return Some(AdapterOutput::event(AgentEvent::Interaction(
                InteractionEvent::Question {
                    request_id: next_id,
                    session_id: super::queue::session_key(adapter),
                    questions: vec![question],
                },
            )));
        }
        Stage::Replace => {
            match data.get("cancelled").and_then(Value::as_bool) {
                Some(true) => return Some(finish(adapter, pending, "cancelled", None)),
                Some(false) => {}
                None => {
                    return Some(finish(
                        adapter,
                        pending,
                        "error",
                        Some(
                            "Pi did not confirm the fork/clone outcome; no message was sent".into(),
                        ),
                    ))
                }
            }
            pending.editor_text = data.get("text").and_then(Value::as_str).map(str::to_owned);
            adapter.current_turn = 0;
            adapter.pending_tool_result_messages.clear();
            adapter.extension_ui.clear();
            adapter.session_name = None;
            adapter.derived_title = None;
            adapter.emitted_title = None;
            adapter.pending_command_prompt = None;
            pending.stage = Stage::State;
            RpcCommand::GetState {
                id: Some(next_id.clone()),
            }
        }
        Stage::State => {
            let target = data.get("sessionId").and_then(Value::as_str);
            if target.is_none_or(|id| id == pending.source_session) {
                return Some(finish(
                    adapter,
                    pending,
                    "error",
                    Some("Pi did not confirm a replacement session; no message was sent".into()),
                ));
            }
            pending.target_session = target.map(str::to_owned);
            output.merge(session::state_response(adapter, Some(data)));
            output.merge(model::available_model_catalog(adapter));
            pending.stage = Stage::History;
            RpcCommand::GetMessages {
                id: Some(next_id.clone()),
            }
        }
        Stage::History => {
            if data.get("messages").and_then(Value::as_array).is_none() {
                return Some(finish(
                    adapter,
                    pending,
                    "error",
                    Some("Pi returned no replacement history; no message was sent".into()),
                ));
            }
            output.merge(session::messages_response(adapter, Some(data)));
            pending.stage = Stage::Models;
            RpcCommand::GetAvailableModels {
                id: Some(next_id.clone()),
            }
        }
        Stage::Models => {
            output.merge(model::models_response(adapter, Some(data)));
            pending.stage = Stage::Commands;
            RpcCommand::GetCommands {
                id: Some(next_id.clone()),
            }
        }
        Stage::Commands => {
            output.merge(adapter.commands_response(Some(data)));
            if pending.interrupted {
                output.merge(finish(adapter, pending, "cancelled", None));
                return Some(output);
            }
            if let Some(message) = pending.message.clone() {
                if adapter.is_streaming || adapter.active_compaction_id.is_some() {
                    output.merge(finish(adapter, pending, "error", Some("The new Pi session is busy; the message was restored to the editor instead of sent".into())));
                    return Some(output);
                }
                session::note_first_user_title(adapter, &message);
                output.events.extend(session::title_events(adapter));
                pending.stage = Stage::Prompt;
                RpcCommand::Prompt {
                    id: Some(next_id.clone()),
                    message,
                    streaming_behavior: None,
                }
            } else {
                output
                    .events
                    .push(AgentEvent::Interaction(InteractionEvent::SetEditorText {
                        text: pending.editor_text.take().unwrap_or_default(),
                    }));
                output.merge(finish(adapter, pending, "success", None));
                return Some(output);
            }
        }
        Stage::Prompt => return Some(finish(adapter, pending, "success", None)),
        Stage::Choice(_) => {
            adapter.pending_fork = Some(pending);
            return Some(output);
        }
    };
    pending.id = next_id;
    adapter.pending_fork = Some(pending);
    output.commands.push(next);
    Some(output)
}
