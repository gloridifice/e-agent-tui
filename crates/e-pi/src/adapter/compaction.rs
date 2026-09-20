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

#[derive(Clone)]
struct Route {
    model: Value,
    effort: String,
}

struct ModelSelection {
    id: String,
    provider: String,
    model: String,
    requested_effort: Option<String>,
    session_id: String,
}

pub(super) struct Pending {
    id: String,
    stage: Stage,
    original: Option<Route>,
    return_route: Option<Route>,
    target: Value,
    instructions: Option<String>,
    session_id: String,
    controls_open: bool,
    compact_finished: bool,
    selection: Option<ModelSelection>,
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
            if let Some(path) = &adapter.compaction_model_path {
                let route = e_tui::config::CompactionModel {
                    version: 1,
                    provider: candidates[0]["provider"].as_str().unwrap().into(),
                    model: candidates[0]["id"].as_str().unwrap().into(),
                };
                if let Err(error) = crate::compaction_store::save(path, Some(&route)) {
                    return adapter
                        .unsupported(&format!("Cannot save global compaction model: {error}"));
                }
            }
            adapter.compaction_model = Some(candidates[0].clone());
            result(format!(
                "Global compaction model set to {}",
                model_name(candidates[0]).unwrap_or_default()
            ))
        }
        Some("unset-model") => {
            if words.next().is_some() {
                return adapter.unsupported("Usage: /compact unset-model");
            }
            if let Some(path) = &adapter.compaction_model_path {
                if let Err(error) = crate::compaction_store::save(path, None) {
                    return adapter
                        .unsupported(&format!("Cannot clear global compaction model: {error}"));
                }
            }
            adapter.compaction_model = None;
            result("Global compaction model unset".into())
        }
        _ => {
            if let Some(path) = &adapter.compaction_model_path {
                let route = match crate::compaction_store::load(path) {
                    Ok(route) => route,
                    Err(error) => {
                        return adapter
                            .unsupported(&format!("Cannot load global compaction model: {error}"))
                    }
                };
                adapter.compaction_model = if let Some(route) = route {
                    let Some(model) = adapter.available_models.iter().find(|model| {
                        model["provider"].as_str() == Some(route.provider.as_str())
                            && model["id"].as_str() == Some(route.model.as_str())
                    }) else {
                        return adapter.unsupported(&format!("Global compaction model {}/{} is unavailable; use /compact set-model or /compact unset-model", route.provider, route.model));
                    };
                    Some(model.clone())
                } else {
                    None
                };
            }
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
                return_route: None,
                target,
                instructions,
                session_id: adapter.session_id.clone(),
                controls_open: false,
                compact_finished: false,
                selection: None,
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
        pending.controls_open = false;
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

pub(super) fn controls_open(adapter: &PiAdapter) -> bool {
    adapter.pending_compaction.as_ref().is_some_and(|pending| {
        pending.stage == Stage::Compact
            && pending.controls_open
            && !pending.compact_finished
            && !pending.cancelled
            && pending.session_id == adapter.session_id
    })
}

pub(super) fn manual_started(adapter: &mut PiAdapter, reason: Option<&str>) -> bool {
    if reason != Some("manual") {
        return false;
    }
    let Some(pending) = adapter.pending_compaction.as_mut() else {
        return false;
    };
    if pending.stage != Stage::Compact
        || pending.compact_finished
        || pending.cancelled
        || pending.controls_open
        || pending.session_id != adapter.session_id
    {
        return false;
    }
    pending.controls_open = true;
    true
}

pub(super) fn begin_model_selection(
    adapter: &mut PiAdapter,
    id: String,
    provider: String,
    model: String,
    requested_effort: Option<String>,
) {
    let Some(pending) = adapter.pending_compaction.as_mut() else {
        return;
    };
    if pending.stage != Stage::Compact
        || !pending.controls_open
        || pending.compact_finished
        || pending.selection.is_some()
        || pending.session_id != adapter.session_id
    {
        return;
    }
    pending.selection = Some(ModelSelection {
        id,
        provider,
        model,
        requested_effort,
        session_id: adapter.session_id.clone(),
    });
}

pub(super) fn is_model_selection_response(adapter: &PiAdapter, id: Option<&str>) -> bool {
    id.is_some_and(|id| {
        adapter
            .pending_compaction
            .as_ref()
            .and_then(|pending| pending.selection.as_ref())
            .is_some_and(|selection| selection.id == id)
    })
}

pub(super) fn advance_model_selection(
    adapter: &mut PiAdapter,
    completed_id: &str,
    next_id: String,
) {
    let Some(selection) = adapter
        .pending_compaction
        .as_mut()
        .and_then(|pending| pending.selection.as_mut())
    else {
        return;
    };
    if selection.id == completed_id {
        selection.id = next_id;
    }
}

pub(super) fn validate_model_selection(
    adapter: &PiAdapter,
    id: &str,
    data: Option<&Value>,
) -> Result<(), String> {
    let pending = adapter
        .pending_compaction
        .as_ref()
        .ok_or_else(|| "Compaction transaction is no longer active".to_owned())?;
    let selection = pending
        .selection
        .as_ref()
        .filter(|selection| selection.id == id)
        .ok_or_else(|| "Model selection response is stale".to_owned())?;
    if selection.session_id != pending.session_id || pending.session_id != adapter.session_id {
        return Err("Model selection belongs to a replaced session".into());
    }
    let data = data.ok_or_else(|| "Model selection state is missing".to_owned())?;
    if data.get("sessionId").and_then(Value::as_str) != Some(pending.session_id.as_str()) {
        return Err("Model selection state belongs to a different session".into());
    }
    let model = data
        .get("model")
        .filter(|model| !model.is_null())
        .ok_or_else(|| "Selected model state is missing".to_owned())?;
    if model.get("provider").and_then(Value::as_str) != Some(selection.provider.as_str())
        || model.get("id").and_then(Value::as_str) != Some(selection.model.as_str())
    {
        return Err("Selected model was not confirmed".into());
    }
    let effort = data
        .get("thinkingLevel")
        .and_then(Value::as_str)
        .ok_or_else(|| "Selected thinking level is missing".to_owned())?;
    if selection
        .requested_effort
        .as_deref()
        .is_some_and(|requested| requested != effort)
    {
        return Err("Selected thinking level was not confirmed".into());
    }
    Ok(())
}

fn continue_transaction(
    adapter: &mut PiAdapter,
    mut pending: Pending,
    mut output: AdapterOutput,
) -> AdapterOutput {
    let id = adapter.request_id("compaction-step");
    pending.id = id.clone();
    let command = match pending.stage {
        Stage::Snapshot | Stage::Verify => RpcCommand::GetState { id: Some(id) },
        Stage::Select => set_model(id, &pending.target).expect("catalog model route"),
        Stage::Compact => RpcCommand::Compact {
            id: Some(id),
            custom_instructions: pending.instructions.clone(),
        },
        Stage::RestoreModel => set_model(
            id,
            &pending.return_route.as_ref().expect("return route").model,
        )
        .expect("return route"),
        Stage::RestoreEffort => RpcCommand::SetThinkingLevel {
            id: Some(id),
            level: pending
                .return_route
                .as_ref()
                .expect("return route")
                .effort
                .clone(),
        },
        Stage::Abort | Stage::Failed => unreachable!(),
    };
    adapter.pending_compaction = Some(pending);
    output.commands.push(command);
    output
}

fn restore_or_wait(
    adapter: &mut PiAdapter,
    mut pending: Pending,
    mut output: AdapterOutput,
) -> AdapterOutput {
    pending.controls_open = false;
    if pending.session_id != adapter.session_id {
        pending.stage = Stage::Failed;
        output.merge(adapter.unsupported(
            "Compaction belongs to a replaced session. Dependent work is held; restart pie to recover.",
        ));
        adapter.pending_compaction = Some(pending);
        return output;
    }
    if pending.selection.is_some() {
        adapter.pending_compaction = Some(pending);
        output
    } else {
        pending.stage = Stage::RestoreModel;
        continue_transaction(adapter, pending, output)
    }
}

pub(super) fn confirm_model_selection(
    adapter: &mut PiAdapter,
    id: &str,
    data: &Value,
) -> AdapterOutput {
    let Some(mut pending) = adapter.pending_compaction.take() else {
        return AdapterOutput::default();
    };
    let matches = pending
        .selection
        .as_ref()
        .is_some_and(|selection| selection.id == id);
    if !matches {
        adapter.pending_compaction = Some(pending);
        return AdapterOutput::default();
    }
    let Some(model) = data.get("model").filter(|model| !model.is_null()).cloned() else {
        adapter.pending_compaction = Some(pending);
        return AdapterOutput::default();
    };
    let Some(effort) = data
        .get("thinkingLevel")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        adapter.pending_compaction = Some(pending);
        return AdapterOutput::default();
    };
    pending.return_route = Some(Route { model, effort });
    pending.selection = None;
    if pending.compact_finished {
        restore_or_wait(adapter, pending, AdapterOutput::default())
    } else {
        adapter.pending_compaction = Some(pending);
        AdapterOutput::default()
    }
}

pub(super) fn fail_model_selection(adapter: &mut PiAdapter, id: &str) -> AdapterOutput {
    let Some(mut pending) = adapter.pending_compaction.take() else {
        return AdapterOutput::default();
    };
    if pending
        .selection
        .as_ref()
        .is_none_or(|selection| selection.id != id)
    {
        adapter.pending_compaction = Some(pending);
        return AdapterOutput::default();
    }
    pending.selection = None;
    if pending.compact_finished {
        restore_or_wait(adapter, pending, AdapterOutput::default())
    } else {
        adapter.pending_compaction = Some(pending);
        AdapterOutput::default()
    }
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    if adapter
        .pending_compaction
        .as_ref()
        .is_none_or(|pending| record.string("id") != Some(pending.id.as_str()))
    {
        return None;
    }
    let mut pending = adapter.pending_compaction.take()?;
    if pending.stage == Stage::Compact && pending.compact_finished {
        adapter.pending_compaction = Some(pending);
        return Some(AdapterOutput::default());
    }
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
            Stage::Select => pending.stage = Stage::RestoreModel,
            Stage::Compact => {
                pending.compact_finished = true;
                return Some(restore_or_wait(adapter, pending, output));
            }
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
                if data
                    .and_then(|data| data.get("sessionId"))
                    .and_then(Value::as_str)
                    .is_some_and(|session_id| session_id != pending.session_id)
                {
                    return Some(
                        adapter.unsupported("Cannot compact a replaced conversation session"),
                    );
                }
                let model = data
                    .and_then(|data| data.get("model"))
                    .filter(|model| !model.is_null())
                    .cloned();
                let effort = data
                    .and_then(|data| data.get("thinkingLevel"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let Some(route) = model
                    .zip(effort)
                    .filter(|(model, _)| set_model(String::new(), model).is_some())
                    .map(|(model, effort)| Route { model, effort })
                else {
                    return Some(adapter.unsupported(
                        "Cannot compact with an override before the original model and thinking level are known",
                    ));
                };
                adapter.current_model = Some(route.model.clone());
                adapter.thinking_level = Some(route.effort.clone());
                pending.original = Some(route.clone());
                pending.return_route = Some(route);
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
            Stage::Compact => {
                pending.compact_finished = true;
                return Some(restore_or_wait(adapter, pending, output));
            }
            Stage::RestoreModel => Stage::RestoreEffort,
            Stage::RestoreEffort => Stage::Verify,
            Stage::Verify => {
                let data = record.field("data");
                let restored = data.and_then(|data| data.get("model"));
                let route = pending.return_route.as_ref().expect("return route");
                let route_matches = restored.is_some_and(|restored| {
                    route.model.get("provider") == restored.get("provider")
                        && route.model.get("id") == restored.get("id")
                });
                let effort_matches = data
                    .and_then(|data| data.get("thinkingLevel"))
                    .and_then(Value::as_str)
                    == Some(route.effort.as_str());
                let session_matches = data
                    .and_then(|data| data.get("sessionId"))
                    .and_then(Value::as_str)
                    .is_none_or(|session_id| session_id == pending.session_id);
                if !route_matches || !effort_matches || !session_matches {
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
    Some(continue_transaction(adapter, pending, output))
}

pub(super) fn is_automatic(reason: Option<&str>) -> bool {
    matches!(reason, Some("threshold" | "overflow"))
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
