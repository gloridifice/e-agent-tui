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

/// Pi thinking levels in the fixed order Pi itself presents them.
const PI_THINKING_LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Mirror Pi's `getSupportedThinkingLevels`: the fixed level order filtered by
/// the model's `thinkingLevelMap`, where `null` hides a level and `xhigh`/`max`
/// exist only through a non-null map entry. Map values translate a Pi level to a
/// provider effort and are never level ids.
fn supported_thinking_levels(model: &Value) -> Vec<String> {
    let map = model.get("thinkingLevelMap").and_then(Value::as_object);
    PI_THINKING_LEVELS
        .iter()
        .copied()
        .filter(|level| match map.and_then(|map| map.get(*level)) {
            Some(Value::Null) => false,
            entry => !matches!(*level, "xhigh" | "max") || entry.is_some(),
        })
        .map(str::to_owned)
        .collect()
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
            .then(|| ModelReasoning {
                efforts: supported_thinking_levels(model)
                    .into_iter()
                    .map(|level| ReasoningEffort {
                        name: thinking_label(&level),
                        id: level,
                        description: None,
                    })
                    .collect(),
                default_effort: adapter.thinking_level.clone(),
            });
        let descriptor = ModelDescriptor {
            id: model_id.to_owned(),
            name: model
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(model_id)
                .to_owned(),
            description: model
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    model
                        .get("contextWindow")
                        .and_then(Value::as_u64)
                        .map(|window| format!("context {window}"))
                }),
            context_window: model.get("contextWindow").and_then(Value::as_u64),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn providers(models: Value) -> Vec<ModelProvider> {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = models_response(&mut adapter, Some(&serde_json::json!({ "models": models })));
        match output.events.into_iter().next() {
            Some(AgentEvent::Catalog(CatalogEvent::Models { providers, .. })) => providers,
            _ => panic!("expected model catalog"),
        }
    }

    fn efforts<'a>(
        providers: &'a [ModelProvider],
        provider: &str,
        model: &str,
    ) -> Option<&'a ModelReasoning> {
        providers
            .iter()
            .find(|item| item.id == provider)?
            .models
            .iter()
            .find(|item| item.id == model)?
            .reasoning
            .as_ref()
    }

    fn effort_ids(reasoning: &ModelReasoning) -> Vec<&str> {
        reasoning
            .efforts
            .iter()
            .map(|effort| effort.id.as_str())
            .collect()
    }

    #[test]
    fn model_catalog_preserves_context_window_capacity() {
        let mut adapter = PiAdapter::new(".", "sessions");
        let output = models_response(
            &mut adapter,
            Some(&serde_json::json!({
                "models": [{
                    "provider": "openai",
                    "id": "gpt",
                    "name": "GPT",
                    "contextWindow": 276000
                }]
            })),
        );

        let AgentEvent::Catalog(CatalogEvent::Models { providers, .. }) = &output.events[0] else {
            panic!("expected model catalog");
        };
        assert_eq!(providers[0].models[0].context_window, Some(276_000));
    }

    #[test]
    fn model_catalog_derives_pi_thinking_levels_per_route() {
        let providers = providers(serde_json::json!([
            {
                "provider": "openai-codex",
                "id": "gpt-5.6-sol",
                "name": "GPT-5.6 Sol",
                "reasoning": true,
                "thinkingLevelMap": { "xhigh": "xhigh", "max": "max", "minimal": "low" }
            },
            {
                "provider": "openai-codex",
                "id": "gpt-6-astra",
                "name": "GPT-6 Astra",
                "reasoning": true,
                "thinkingLevelMap": {
                    "off": null, "minimal": "low", "low": "low", "medium": "medium",
                    "high": "high", "xhigh": "xhigh", "max": "max"
                }
            },
            {
                "provider": "codemaker",
                "id": "kimi-k2.7-code",
                "name": "Kimi K2.7 Code",
                "reasoning": true
            },
            {
                "provider": "openai",
                "id": "gpt-4o",
                "name": "GPT-4o",
                "reasoning": false
            }
        ]));

        let opt_in = efforts(&providers, "openai-codex", "gpt-5.6-sol").expect("sol reasoning");
        assert_eq!(
            effort_ids(opt_in),
            ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(opt_in.efforts[5].name, "Xhigh");

        let hidden = efforts(&providers, "openai-codex", "gpt-6-astra").expect("astra reasoning");
        assert_eq!(
            effort_ids(hidden),
            ["minimal", "low", "medium", "high", "xhigh", "max"]
        );

        let unmapped = efforts(&providers, "codemaker", "kimi-k2.7-code").expect("kimi reasoning");
        assert_eq!(
            effort_ids(unmapped),
            ["off", "minimal", "low", "medium", "high"]
        );

        assert!(efforts(&providers, "openai", "gpt-4o").is_none());
    }

    #[test]
    fn model_catalog_accepts_a_map_that_hides_every_level() {
        let providers = providers(serde_json::json!([{
            "provider": "openai",
            "id": "gpt",
            "name": "GPT",
            "reasoning": true,
            "thinkingLevelMap": {
                "off": null, "minimal": null, "low": null, "medium": null, "high": null
            }
        }]));

        let reasoning = efforts(&providers, "openai", "gpt").expect("reasoning");
        assert!(reasoning.efforts.is_empty());
    }
}
