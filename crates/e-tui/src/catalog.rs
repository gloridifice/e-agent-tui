//! Agent-provided and built-in presentation catalogs.

use crate::{
    agent::{
        CommandDescriptor, CredentialProvider, ModelDescriptor, ModelProvider, ModelReasoning,
        ModelSelection, ProxyRoute, ReasoningEffort, SessionSummary, Skill,
    },
    command_catalog::NewMode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffortStatus {
    pub id: Option<String>,
    /// Provider-declared label or raw effort id. `None` means provider default.
    pub label: Option<String>,
}

/// Catalog state for the currently attached scope. Composer and Input Pages
/// borrow these values; they do not retain synchronized roster copies.
#[derive(Debug, Clone, Default)]
pub struct CatalogModel {
    pub sessions: Vec<SessionSummary>,
    pub new_modes: Vec<NewMode>,
    pub integrated_commands: Vec<CommandDescriptor>,
    pub skills: Vec<Skill>,
    pub credential_providers: Vec<CredentialProvider>,
    pub proxies: Vec<ProxyRoute>,
    pub model_providers: Vec<ModelProvider>,
    pub current_model: Option<ModelSelection>,
}

/// Prefer a canonical route, otherwise accept only an unambiguous bare model id.
pub(crate) fn resolve_model_reference<'a>(
    providers: &'a [ModelProvider],
    reference: &str,
) -> Option<(&'a ModelProvider, &'a ModelDescriptor)> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    let unique = |canonical: bool| {
        let mut matches = providers.iter().flat_map(|provider| {
            provider.models.iter().filter_map(move |model| {
                let matches = if canonical {
                    format!("{}/{}", provider.id, model.id).eq_ignore_ascii_case(reference)
                } else {
                    model.id.eq_ignore_ascii_case(reference)
                };
                matches.then_some((provider, model))
            })
        });
        match (matches.next(), matches.next()) {
            (Some(route), None) => Ok(Some(route)),
            (None, _) => Ok(None),
            _ => Err(()),
        }
    };
    unique(true).ok()?.or_else(|| unique(false).ok().flatten())
}

pub(crate) fn configured_model_effort(
    defaults: &crate::model_defaults::ModelDefaultEfforts,
    provider: &str,
    model: &ModelDescriptor,
) -> Option<String> {
    let saved = defaults.get(provider, &model.id)?;
    model
        .reasoning
        .as_ref()?
        .efforts
        .iter()
        .find(|effort| effort.id == saved)
        .map(|effort| effort.id.clone())
}

impl CatalogModel {
    pub fn marked_model<'a>(
        &self,
        marks: &'a crate::model_marks::ModelMarks,
        letter: char,
    ) -> Option<(&'a crate::model_marks::ModelMark, &ModelDescriptor)> {
        let mark = marks.get(letter)?;
        let model = self
            .model_providers
            .iter()
            .find(|provider| provider.id == mark.provider)?
            .models
            .iter()
            .find(|model| model.id == mark.model)?;
        Some((mark, model))
    }

    /// Resolve the exact current provider/model route — never cross-provider
    /// by model id.
    pub fn current_model_descriptor(&self) -> Option<&ModelDescriptor> {
        let current = self.current_model.as_ref()?;
        self.model_providers
            .iter()
            .find(|provider| provider.id == current.provider)?
            .models
            .iter()
            .find(|model| model.id == current.model)
    }

    pub fn current_model_reasoning(&self) -> Option<&ModelReasoning> {
        self.current_model_descriptor()?.reasoning.as_ref()
    }

    pub fn current_model_context_window(&self) -> Option<u64> {
        self.current_model_descriptor()?
            .context_window
            .filter(|window| *window > 0)
    }

    /// Adapter-declared efforts for the exact current route, in declared order.
    pub fn current_efforts(&self) -> Option<&[ReasoningEffort]> {
        self.current_model_reasoning()
            .map(|reasoning| reasoning.efforts.as_slice())
    }

    /// Resolve the current route's provider-neutral effort status. Returns
    /// `None` when the route exposes no reasoning metadata; an inner `None`
    /// label means that the provider default is active.
    pub fn effort_status(&self) -> Option<EffortStatus> {
        let reasoning = self.current_model_reasoning()?;
        let id = self
            .current_model
            .as_ref()?
            .reasoning_effort
            .as_deref()
            .or(reasoning.default_effort.as_deref());
        let label = id.map(|id| {
            reasoning
                .efforts
                .iter()
                .find(|effort| effort.id == id)
                .map(|effort| effort.name.clone())
                .unwrap_or_else(|| id.to_owned())
        });
        Some(EffortStatus {
            id: id.map(str::to_owned),
            label,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(current: ModelSelection, reasoning: Option<ModelReasoning>) -> CatalogModel {
        CatalogModel {
            current_model: Some(current),
            model_providers: vec![ModelProvider {
                id: "openai".into(),
                name: "OpenAI".into(),
                models: vec![crate::agent::ModelDescriptor {
                    id: "gpt".into(),
                    name: "GPT".into(),
                    description: None,
                    context_window: None,
                    reasoning,
                }],
            }],
            ..CatalogModel::default()
        }
    }

    fn reasoning() -> ModelReasoning {
        ModelReasoning {
            efforts: vec![
                ReasoningEffort {
                    id: "low".into(),
                    name: "Low".into(),
                    description: None,
                },
                ReasoningEffort {
                    id: "high".into(),
                    name: "High".into(),
                    description: None,
                },
            ],
            default_effort: Some("low".into()),
        }
    }

    #[test]
    fn effort_label_prefers_explicit_then_default_then_default() {
        let explicit = catalog(
            ModelSelection {
                provider: "openai".into(),
                model: "gpt".into(),
                reasoning_effort: Some("high".into()),
            },
            Some(reasoning()),
        );
        assert_eq!(
            explicit.effort_status(),
            Some(EffortStatus {
                id: Some("high".into()),
                label: Some("High".into())
            })
        );

        let defaulted = catalog(
            ModelSelection {
                provider: "openai".into(),
                model: "gpt".into(),
                reasoning_effort: None,
            },
            Some(reasoning()),
        );
        assert_eq!(
            defaulted.effort_status(),
            Some(EffortStatus {
                id: Some("low".into()),
                label: Some("Low".into())
            })
        );

        let provider_default = catalog(
            ModelSelection {
                provider: "openai".into(),
                model: "gpt".into(),
                reasoning_effort: None,
            },
            Some(ModelReasoning {
                efforts: reasoning().efforts,
                default_effort: None,
            }),
        );
        assert_eq!(
            provider_default.effort_status(),
            Some(EffortStatus {
                id: None,
                label: None
            })
        );
    }

    #[test]
    fn effort_label_hides_without_reasoning_or_unknown_route() {
        let none = catalog(
            ModelSelection {
                provider: "openai".into(),
                model: "gpt".into(),
                reasoning_effort: None,
            },
            None,
        );
        assert_eq!(none.effort_status(), None);

        // The current route is not in the catalog: never fabricate efforts.
        let unknown = CatalogModel {
            current_model: Some(ModelSelection {
                provider: "other".into(),
                model: "x".into(),
                reasoning_effort: Some("high".into()),
            }),
            model_providers: vec![ModelProvider {
                id: "openai".into(),
                name: "OpenAI".into(),
                models: vec![crate::agent::ModelDescriptor {
                    id: "gpt".into(),
                    name: "GPT".into(),
                    description: None,
                    context_window: None,
                    reasoning: Some(reasoning()),
                }],
            }],
            ..CatalogModel::default()
        };
        assert_eq!(unknown.effort_status(), None);
    }
}
