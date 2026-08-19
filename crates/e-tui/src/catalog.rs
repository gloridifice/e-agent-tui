//! Agent-provided and built-in presentation catalogs.

use crate::{
    agent::{
        CommandDescriptor, CredentialProvider, ModelProvider, ModelSelection, ProxyRoute,
        SessionSummary, Skill,
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
