//! Correlated manual compaction transaction; automatic compaction stays native.
use e_tui::agent::{AgentEvent, InteractionEvent};
use serde_json::Value;

use super::{AdapterOutput, PiAdapter};
use crate::protocol::{RpcCommand, RpcRecord};

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Abort,
    Snapshot,
    Select,
    Compact,
    RestoreModel,
    RestoreEffort,
    Verify,
    Failed,
}

pub(super) struct Pending {
    id: String,
    stage: Stage,
    original: Option<Value>,
    effort: Option<String>,
    target: Value,
    instructions: Option<String>,
    cancelled: bool,
}

fn result(text: String) -> AdapterOutput {
    AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::CommandResult {
        id: "compaction-model".into(),
        outcome: "success".into(),
        text: Some(text),
    }))
}

pub(super) fn command(adapter: &mut PiAdapter, args: &str) -> AdapterOutput {
    let mut words = args.split_whitespace();
    match words.next() {
        Some("set-model") => {
            let Some(reference) = words.next().filter(|_| words.next().is_none()) else {
                return adapter.unsupported("Usage: /compact set-model <provider/model>");
            };
            let candidates: Vec<_> = adapter
                .available_models
                .iter()
                .filter(|model| {
                    let provider = model
                        .get("provider")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let id = model.get("id").and_then(Value::as_str).unwrap_or_default();
                    format!("{provider}/{id}").eq_ignore_ascii_case(reference)
                })
                .collect();
            let candidates: Vec<_> = if candidates.is_empty() {
                adapter
                    .available_models
                    .iter()
                    .filter(|model| {
                        model
                            .get("id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| id.eq_ignore_ascii_case(reference))
                    })
                    .collect()
            } else {
                candidates
            };
            if candidates.len() != 1 || set_model(String::new(), candidates[0]).is_none() {
                return adapter.unsupported("Unknown or ambiguous compaction model");
            }
            adapter.compaction_model = Some(candidates[0].clone());
            result(format!(
                "Compaction model set to {}",
                model_name(candidates[0]).unwrap_or_default()
            ))
        }
        Some("unset-model") => {
            if words.next().is_some() {
                return adapter.unsupported("Usage: /compact unset-model");
            }
            adapter.compaction_model = None;
            result("Compaction model unset".into())
        }
        _ => {
            let instructions = (!args.trim().is_empty()).then(|| args.trim().to_owned());
            let Some(target) = adapter.compaction_model.clone() else {
                return AdapterOutput::command(RpcCommand::Compact {
                    id: Some(adapter.request_id("compact")),
                    custom_instructions: instructions,
                });
            };
            let id = adapter.request_id("compact-abort");
            adapter.pending_compaction = Some(Pending {
                id: id.clone(),
                stage: Stage::Abort,
                original: None,
                effort: None,
                target,
                instructions,
                cancelled: false,
            });
            AdapterOutput::command(RpcCommand::Abort { id: Some(id) })
        }
    }
}

pub(super) fn model_name(model: &Value) -> Option<String> {
    model
        .get("name")
        .or_else(|| model.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(super) fn interrupt(adapter: &mut PiAdapter) {
    if let Some(pending) = &mut adapter.pending_compaction {
        pending.cancelled = true;
    }
}

fn set_model(id: String, model: &Value) -> Option<RpcCommand> {
    Some(RpcCommand::SetModel {
        id: Some(id),
        provider: model
            .get("provider")?
            .as_str()
            .filter(|id| !id.is_empty())?
            .into(),
        model_id: model
            .get("id")?
            .as_str()
            .filter(|id| !id.is_empty())?
            .into(),
    })
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    if adapter
        .pending_compaction
        .as_ref()
        .is_none_or(|p| record.string("id") != Some(p.id.as_str()))
    {
        return None;
    }
    let mut pending = adapter.pending_compaction.take()?;
    let success = record.bool("success") == Some(true);
    let mut output = AdapterOutput::default();
    if !success {
        output = adapter.unsupported(
            record
                .string("error")
                .unwrap_or("Compaction operation failed"),
        );
        match pending.stage {
            Stage::Abort | Stage::Snapshot => return Some(output),
            Stage::RestoreModel | Stage::RestoreEffort | Stage::Verify | Stage::Failed => {
                pending.stage = Stage::Failed;
                output.merge(adapter.unsupported("Could not restore the conversation model/effort. Dependent work is held; restart pie to recover."));
                adapter.pending_compaction = Some(pending);
                return Some(output);
            }
            Stage::Select | Stage::Compact => pending.stage = Stage::RestoreModel,
        }
    } else {
        pending.stage = match pending.stage {
            Stage::Abort => {
                if pending.cancelled {
                    return Some(adapter.unsupported("Compaction cancelled"));
                }
                Stage::Snapshot
            }
            Stage::Snapshot => {
                let data = record.field("data");
                pending.original = data
                    .and_then(|d| d.get("model"))
                    .filter(|m| !m.is_null())
                    .cloned();
                pending.effort = data
                    .and_then(|d| d.get("thinkingLevel"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if pending
                    .original
                    .as_ref()
                    .and_then(|m| set_model(String::new(), m))
                    .is_none()
                    || pending.effort.is_none()
                {
                    return Some(adapter.unsupported(
                        "Cannot compact with an override before the original model and thinking level are known",
                    ));
                }
                if pending.cancelled {
                    return Some(adapter.unsupported("Compaction cancelled"));
                }
                Stage::Select
            }
            Stage::Select => {
                if pending.cancelled {
                    Stage::RestoreModel
                } else {
                    Stage::Compact
                }
            }
            Stage::Compact => Stage::RestoreModel,
            Stage::RestoreModel => {
                if pending.effort.is_some() {
                    Stage::RestoreEffort
                } else {
                    Stage::Verify
                }
            }
            Stage::RestoreEffort => Stage::Verify,
            Stage::Verify => {
                let data = record.field("data");
                let restored = data.and_then(|d| d.get("model"));
                let route_matches =
                    pending
                        .original
                        .as_ref()
                        .zip(restored)
                        .is_some_and(|(a, b)| {
                            a.get("provider") == b.get("provider") && a.get("id") == b.get("id")
                        });
                let effort_matches = pending.effort.as_deref().is_none_or(|effort| {
                    data.and_then(|d| d.get("thinkingLevel"))
                        .and_then(Value::as_str)
                        == Some(effort)
                });
                if !route_matches || !effort_matches {
                    pending.stage = Stage::Failed;
                    adapter.pending_compaction = Some(pending);
                    return Some(adapter.unsupported("Conversation model/effort restoration was not confirmed. Dependent work is held; restart pie to recover."));
                }
                output.merge(super::session::state_response(adapter, data));
                output.merge(super::model::available_model_catalog(adapter));
                output.merge(AdapterOutput::event(AgentEvent::Interaction(
                    InteractionEvent::CommandResult {
                        id: "compaction-model".into(),
                        outcome: "success".into(),
                        text: None,
                    },
                )));
                return Some(output);
            }
            Stage::Failed => {
                adapter.pending_compaction = Some(pending);
                return Some(output);
            }
        };
    }
    let id = adapter.request_id("compaction-step");
    pending.id = id.clone();
    let command = match pending.stage {
        Stage::Snapshot | Stage::Verify => RpcCommand::GetState { id: Some(id) },
        Stage::Select => set_model(id, &pending.target).expect("catalog model route"),
        Stage::Compact => RpcCommand::Compact {
            id: Some(id),
            custom_instructions: pending.instructions.clone(),
        },
        Stage::RestoreModel => set_model(id, pending.original.as_ref().expect("original model"))
            .expect("original route"),
        Stage::RestoreEffort => RpcCommand::SetThinkingLevel {
            id: Some(id),
            level: pending.effort.clone().expect("original effort"),
        },
        Stage::Abort | Stage::Failed => unreachable!(),
    };
    adapter.pending_compaction = Some(pending);
    output.commands.push(command);
    Some(output)
}

pub(super) fn active_model_name(adapter: &PiAdapter, reason: Option<&str>) -> Option<String> {
    if reason == Some("manual") {
        if let Some(pending) = &adapter.pending_compaction {
            if pending.stage == Stage::Compact {
                return model_name(&pending.target);
            }
        }
    }
    adapter.current_model.as_ref().and_then(model_name)
}
