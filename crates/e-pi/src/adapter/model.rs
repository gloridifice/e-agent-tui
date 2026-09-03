//! Pi model catalog, thinking-level, and selected-route projection.

use e_tui::agent::{
    AgentEvent, CatalogEvent, ModelDescriptor, ModelProvider, ModelReasoning, ModelSelection,
    ReasoningEffort,
};

use serde_json::Value;

use super::{AdapterOutput, PiAdapter};

fn thinking_label(level: &str) -> String {
    let mut chars = level.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub(super) fn models_response(adapter: &mut PiAdapter, data: Option<&Value>) -> AdapterOutput {
    adapter.available_models = data
        .and_then(|data| data.get("models"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    available_model_catalog(adapter)
}

pub(super) fn available_model_catalog(adapter: &PiAdapter) -> AdapterOutput {
    if adapter.available_models.is_empty() {
        let models = adapter
            .current_model
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        model_catalog(adapter, &models)
    } else {
        model_catalog(adapter, &adapter.available_models)
    }
}

pub(super) fn model_catalog(adapter: &PiAdapter, models: &[Value]) -> AdapterOutput {
    let mut providers: Vec<ModelProvider> = Vec::new();
    for model in models {
        let Some(provider_id) = model.get("provider").and_then(Value::as_str) else {
            continue;
        };
        let Some(model_id) = model.get("id").and_then(Value::as_str) else {
            continue;
        };
        let reasoning = model
            .get("reasoning")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .then(|| {
                let levels = model
                    .get("thinkingLevelMap")
                    .and_then(Value::as_object)
                    .map(|map| map.keys().cloned().collect::<Vec<_>>())
                    .filter(|levels| !levels.is_empty())
                    .unwrap_or_else(|| adapter.thinking_levels.clone());
                ModelReasoning {
                    efforts: levels
                        .iter()
                        .map(|level| ReasoningEffort {
                            id: level.clone(),
                            name: thinking_label(level),
                            description: None,
                        })
                        .collect(),
                    default_effort: adapter.thinking_level.clone(),
                }
            });
        let descriptor = ModelDescriptor {
            id: model_id.to_owned(),
            name: model
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(model_id)
                .to_owned(),
            description: model
                .get("contextWindow")
                .and_then(Value::as_u64)
                .map(|window| format!("context {window}")),
            reasoning,
        };
        if let Some(provider) = providers.iter_mut().find(|item| item.id == provider_id) {
            provider.models.push(descriptor);
        } else {
            providers.push(ModelProvider {
                id: provider_id.to_owned(),
                name: provider_id.to_owned(),
                models: vec![descriptor],
            });
        }
    }
    let current = adapter.current_model.as_ref().and_then(|model| {
        Some(ModelSelection {
            provider: model.get("provider")?.as_str()?.to_owned(),
            model: model.get("id")?.as_str()?.to_owned(),
            reasoning_effort: adapter.thinking_level.clone(),
        })
    });
    AdapterOutput::event(AgentEvent::Catalog(CatalogEvent::Models {
        providers,
        current,
    }))
}
