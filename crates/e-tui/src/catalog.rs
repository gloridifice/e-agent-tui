//! Agent-provided and built-in presentation catalogs.

use crate::{
    agent::{
        CommandDescriptor, CredentialProvider, ModelProvider, ModelReasoning, ModelSelection,
        ProxyRoute, ReasoningEffort, SessionSummary, Skill,
    },
    command_catalog::NewMode,
};

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

impl CatalogModel {
    /// The exact current provider/model route's reasoning metadata, resolved by
    /// matching provider id then model id — never cross-provider by model id.
    pub fn current_model_reasoning(&self) -> Option<&ModelReasoning> {
        let current = self.current_model.as_ref()?;
        self.model_providers
            .iter()
            .find(|provider| provider.id == current.provider)?
            .models
            .iter()
            .find(|model| model.id == current.model)?
            .reasoning
            .as_ref()
    }

    /// Adapter-declared efforts for the exact current route, in declared order.
    pub fn current_efforts(&self) -> Option<&[ReasoningEffort]> {
        self.current_model_reasoning()
            .map(|reasoning| reasoning.efforts.as_slice())
    }

    /// The status-bar effort label in `Effort:<Label>` form. Returns `None`
    /// when the current route exposes no reasoning metadata (hidden entirely).
    /// Explicit selection wins over the adapter default; an unknown effort id
    /// falls back to the raw id; both absent render `Effort:Default`.
    pub fn effort_status_label(&self) -> Option<String> {
        let reasoning = self.current_model_reasoning()?;
        let id = self
            .current_model
            .as_ref()?
            .reasoning_effort
            .as_deref()
            .or(reasoning.default_effort.as_deref());
        let label = match id {
            Some(id) => reasoning
                .efforts
                .iter()
                .find(|effort| effort.id == id)
                .map(|effort| effort.name.clone())
                .unwrap_or_else(|| id.to_owned()),
            None => "Default".to_owned(),
        };
        Some(format!("Effort:{label}"))
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
            explicit.effort_status_label().as_deref(),
            Some("Effort:High")
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
            defaulted.effort_status_label().as_deref(),
            Some("Effort:Low")
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
            provider_default.effort_status_label().as_deref(),
            Some("Effort:Default")
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
        assert_eq!(none.effort_status_label(), None);

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
                    reasoning: Some(reasoning()),
                }],
            }],
            ..CatalogModel::default()
        };
        assert_eq!(unknown.effort_status_label(), None);
    }
}
