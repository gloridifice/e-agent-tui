//! Catalog-backed Input Page state.

use super::*;

/// `/resume` session list. The page is visible immediately in a loading
/// state; the bridge may then send a fast header-only list followed by the
/// same rows enriched with titles.
pub struct ResumePage {
    pub sessions: Vec<SessionSummary>,
    pub parents: crate::resume::SessionParents,
    pub query: String,
    /// Selection index within [`Self::filtered_indices`].
    pub sel: usize,
    pub loading: bool,
    pub titles_pending: bool,
    pub paging: crate::resume::ResumePaging,
    pub(crate) age_refresh: Option<std::time::Instant>,
}

impl ResumePage {
    pub fn loading() -> Self {
        Self {
            sessions: Vec::new(),
            parents: Default::default(),
            query: String::new(),
            sel: 0,
            loading: true,
            titles_pending: false,
            paging: crate::resume::ResumePaging::default(),
            age_refresh: None,
        }
    }

    pub fn take_request(&mut self, workspace: &str) -> Option<crate::resume::ResumeRequest> {
        if self.paging.bind_workspace(workspace) {
            self.sessions.clear();
            self.parents.clear();
            self.sel = 0;
            self.loading = true;
            self.age_refresh = None;
        }
        self.paging
            .request(self.sel, self.sessions.len(), !self.query.is_empty())
    }

    pub fn apply_batch(&mut self, batch: crate::resume::ResumeBatch, workspace: &str) -> bool {
        self.apply_batch_with_parents(batch, workspace, &Default::default())
    }

    pub fn apply_batch_with_parents(
        &mut self,
        batch: crate::resume::ResumeBatch,
        workspace: &str,
        parents: &crate::resume::SessionParents,
    ) -> bool {
        if !self.paging.admit(&batch, workspace) {
            return false;
        }
        let selected = self.selected_id().map(str::to_owned);
        for session in &batch.sessions {
            if let Some(parent) = parents.get(&session.id) {
                self.parents.insert(session.id.clone(), parent.clone());
            }
        }
        self.sessions.extend(batch.sessions);
        self.loading = self.sessions.is_empty() && self.paging.has_more;
        if let Some(selected) = selected {
            self.sel = self
                .filtered_indices()
                .iter()
                .position(|index| self.sessions[*index].id == selected)
                .unwrap_or(0);
        }
        true
    }

    pub fn search_pending(&self) -> bool {
        self.loading || self.paging.loading() || (!self.query.is_empty() && self.paging.has_more)
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        self.tree_rows().into_iter().map(|row| row.index).collect()
    }

    pub fn tree_rows(&self) -> Vec<crate::resume::SessionTreeRow> {
        let ids: Vec<_> = self
            .sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect();
        let rows = crate::resume::session_tree(&ids, &self.parents);
        if self.query.is_empty() {
            return rows;
        }
        let query = self.query.to_lowercase();
        let mut included: Vec<_> = self
            .sessions
            .iter()
            .map(|session| {
                session.title.to_lowercase().contains(&query)
                    || session.id.to_lowercase().contains(&query)
            })
            .collect();
        for row in rows.iter().rev() {
            if included[row.index] {
                if let Some(parent) = row.parent {
                    included[parent] = true;
                }
            }
        }
        let indices: Vec<_> = rows
            .iter()
            .filter(|row| included[row.index])
            .map(|row| row.index)
            .collect();
        let ids: Vec<_> = indices.iter().map(|index| ids[*index]).collect();
        crate::resume::session_tree(&ids, &self.parents)
            .into_iter()
            .map(|mut row| {
                row.index = indices[row.index];
                row.parent = row.parent.map(|parent| indices[parent]);
                row
            })
            .collect()
    }

    fn selected_id(&self) -> Option<&str> {
        let filtered = self.filtered_indices();
        filtered
            .get(self.sel)
            .and_then(|index| self.sessions.get(*index))
            .map(|session| session.id.as_str())
    }

    pub fn apply_sessions(&mut self, sessions: Vec<SessionSummary>, titles_pending: bool) {
        let selected_id = self.selected_id().map(str::to_owned);
        self.sessions = sessions;
        self.parents.clear();
        self.loading = false;
        self.titles_pending = titles_pending;
        self.paging.has_more = false;
        let filtered = self.filtered_indices();
        self.sel = selected_id
            .as_ref()
            .and_then(|id| {
                filtered.iter().position(|index| {
                    self.sessions
                        .get(*index)
                        .is_some_and(|session| session.id == *id)
                })
            })
            .unwrap_or_else(|| self.sel.min(filtered.len().saturating_sub(1)));
    }

    pub(super) fn handle_input(&mut self, key: MappedKey) -> PageOutcome {
        match key {
            Command(Action::Cancel) => PageOutcome::close(),
            Command(Action::Previous) => {
                self.sel = self.sel.saturating_sub(1);
                PageOutcome::default()
            }
            Command(Action::Next) => {
                self.sel = (self.sel + 1).min(self.filtered_indices().len().saturating_sub(1));
                PageOutcome::default()
            }
            Command(Action::Confirm) => self
                .selected_id()
                .map(|session_id| {
                    PageOutcome::send(
                        AgentRequest::Attach {
                            session_id: session_id.to_owned(),
                        },
                        true,
                    )
                })
                .unwrap_or_default(),
            Command(Action::DeleteBackward) => {
                self.query.pop();
                self.sel = 0;
                PageOutcome::default()
            }
            Text(character) => {
                self.query.push(character);
                self.sel = 0;
                PageOutcome::default()
            }
            _ => PageOutcome::default(),
        }
    }
}

pub struct ModelPage {
    pub for_compaction: bool,
    pub providers: Vec<ModelProvider>,
    pub active_provider: Option<String>,
    pub current: Option<(String, String)>,
    pub loading: bool,
}

impl ModelPage {
    pub fn loading() -> Self {
        Self {
            for_compaction: false,
            providers: Vec::new(),
            active_provider: None,
            current: None,
            loading: true,
        }
    }

    pub fn apply_catalog(
        &mut self,
        providers: Vec<ModelProvider>,
        current: Option<(String, String)>,
        focus: &mut FocusState,
    ) {
        let had_focus = focus.current.is_some();
        let previous_provider = self.active_provider.clone();
        self.providers = providers;
        self.current = if self.for_compaction { None } else { current };
        self.loading = false;
        self.active_provider = previous_provider
            .filter(|id| self.providers.iter().any(|provider| provider.id == *id))
            .or_else(|| {
                self.current
                    .as_ref()
                    .map(|(provider, _)| provider.clone())
                    .filter(|id| self.providers.iter().any(|provider| provider.id == *id))
            })
            .or_else(|| self.providers.first().map(|provider| provider.id.clone()));
        self.rebuild_focus(focus);
        if !had_focus {
            if let Some((provider, model)) = self.current.as_ref() {
                focus.set(Self::model_focus(provider, model));
            }
        }
    }

    pub fn active_index(&self) -> Option<usize> {
        let active = self.active_provider.as_ref()?;
        self.providers
            .iter()
            .position(|provider| provider.id == *active)
    }

    pub fn active_models(&self) -> &[crate::agent::ModelDescriptor] {
        self.active_index()
            .and_then(|index| self.providers.get(index))
            .map(|provider| provider.models.as_slice())
            .unwrap_or(&[])
    }

    pub(super) fn provider_focus(id: &str) -> FocusId {
        FocusId::new(format!("provider:{id}"))
    }

    pub(super) fn model_focus(provider: &str, model: &str) -> FocusId {
        FocusId::new(format!("model:{provider}:{model}"))
    }

    pub fn rebuild_focus(&self, focus: &mut FocusState) {
        let mut nodes = Vec::new();
        for (index, provider) in self.providers.iter().enumerate() {
            let mut node = FocusNode::new(Self::provider_focus(&provider.id));
            node.up = index
                .checked_sub(1)
                .and_then(|i| self.providers.get(i))
                .map(|item| Self::provider_focus(&item.id));
            node.down = self
                .providers
                .get(index + 1)
                .map(|item| Self::provider_focus(&item.id));
            if self.active_provider.as_deref() == Some(provider.id.as_str()) {
                node.right = provider
                    .models
                    .first()
                    .map(|model| Self::model_focus(&provider.id, &model.id));
            }
            nodes.push(node);
        }
        if let Some(provider_index) = self.active_index() {
            let provider = &self.providers[provider_index];
            for (index, model) in provider.models.iter().enumerate() {
                let mut node = FocusNode::new(Self::model_focus(&provider.id, &model.id));
                node.left = Some(Self::provider_focus(&provider.id));
                node.up = index
                    .checked_sub(1)
                    .and_then(|i| provider.models.get(i))
                    .map(|item| Self::model_focus(&provider.id, &item.id));
                node.down = provider
                    .models
                    .get(index + 1)
                    .map(|item| Self::model_focus(&provider.id, &item.id));
                nodes.push(node);
            }
        }
        focus.replace(nodes);
    }

    fn focused_model(&self, focus: &FocusState) -> Option<(&str, &str)> {
        self.providers.iter().find_map(|provider| {
            provider.models.iter().find_map(|model| {
                focus
                    .is(&Self::model_focus(&provider.id, &model.id))
                    .then_some((provider.id.as_str(), model.id.as_str()))
            })
        })
    }

    pub(super) fn mark(
        &self,
        letter: char,
        focus: &FocusState,
        config: &mut Config,
    ) -> PageOutcome {
        if self.loading {
            return PageOutcome::default();
        }
        let Some((provider, model)) = self.focused_model(focus) else {
            return PageOutcome::default();
        };
        config.model_marks.toggle(letter, provider, model);
        PageOutcome {
            close: false,
            effects: vec![PageEffect::ConfigChanged],
        }
    }

    pub(super) fn select_mark(&self, letter: char, config: &Config) -> PageOutcome {
        let Some(mark) = config.model_marks.get(letter) else {
            return PageOutcome::default();
        };
        if self.loading
            || !self.providers.iter().any(|provider| {
                provider.id == mark.provider
                    && provider.models.iter().any(|model| model.id == mark.model)
            })
        {
            return PageOutcome::default();
        }
        self.select(&mark.provider, &mark.model, config)
    }

    fn select(&self, provider: &str, model: &str, config: &Config) -> PageOutcome {
        if self.for_compaction {
            return PageOutcome::send(
                AgentRequest::Command {
                    line: format!("/compact set-model {provider}/{model}"),
                    images: Vec::new(),
                },
                true,
            );
        }
        PageOutcome::send(
            AgentRequest::ModelSet {
                provider: provider.to_owned(),
                model: model.to_owned(),
                reasoning_effort: self
                    .providers
                    .iter()
                    .find(|item| item.id == provider)
                    .and_then(|provider| provider.models.iter().find(|item| item.id == model))
                    .and_then(|model| {
                        crate::catalog::configured_model_effort(
                            &config.model_default_efforts,
                            provider,
                            model,
                        )
                    }),
            },
            true,
        )
    }

    pub(super) fn activate(&mut self, focus: &mut FocusState, config: &Config) -> PageOutcome {
        let Some(id) = focus.current.as_ref().map(|id| id.0.clone()) else {
            return PageOutcome::default();
        };
        if let Some(provider_id) = id.strip_prefix("provider:") {
            self.active_provider = Some(provider_id.to_owned());
            self.rebuild_focus(focus);
            let target = self
                .providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .and_then(|provider| {
                    self.current
                        .as_ref()
                        .filter(|(current_provider, _)| current_provider == provider_id)
                        .and_then(|(_, current_model)| {
                            provider
                                .models
                                .iter()
                                .find(|model| model.id == *current_model)
                        })
                        .or_else(|| provider.models.first())
                        .map(|model| Self::model_focus(provider_id, &model.id))
                });
            if let Some(target) = target {
                focus.set(target);
            }
            return PageOutcome::default();
        }
        self.focused_model(focus)
            .map(|(provider, model)| self.select(provider, model, config))
            .unwrap_or_default()
    }
}

pub struct EffortPage {
    pub efforts: Vec<ReasoningEffort>,
    pub current: Option<ModelSelection>,
    pub default_effort: Option<String>,
    pub loading: bool,
    pub unavailable: bool,
}

impl EffortPage {
    pub fn shows_current_default(&self) -> bool {
        self.current
            .as_ref()
            .and_then(|current| current.reasoning_effort.as_ref())
            .is_none()
            && self.default_effort.is_none()
    }

    pub fn loading() -> Self {
        Self {
            efforts: Vec::new(),
            current: None,
            default_effort: None,
            loading: true,
            unavailable: false,
        }
    }

    /// Populate from the sole catalog owner: only the exact current route's
    /// adapter-declared efforts become selectable. An absent route or empty
    /// effort list becomes an unavailable state with no fake focus.
    pub fn apply_catalog(&mut self, catalog: &CatalogModel) {
        self.efforts.clear();
        self.current = catalog.current_model.clone();
        self.default_effort = None;
        self.loading = false;
        self.unavailable = true;
        if let Some(reasoning) = catalog.current_model_reasoning() {
            self.efforts = reasoning.efforts.clone();
            self.default_effort = reasoning.default_effort.clone();
            self.unavailable = self.efforts.is_empty();
        }
    }

    fn effort_focus(id: &str) -> FocusId {
        FocusId::new(format!("effort:{id}"))
    }

    pub fn rebuild_focus(&self, focus: &mut FocusState) {
        // Capture whether this is a fresh open so a catalog refresh preserves
        // the user's in-page navigation instead of snapping back to the
        // preferred effort.
        let was_empty = focus.current.is_none();
        let mut nodes = Vec::new();
        for (index, effort) in self.efforts.iter().enumerate() {
            let mut node = FocusNode::new(Self::effort_focus(&effort.id));
            node.up = index
                .checked_sub(1)
                .and_then(|i| self.efforts.get(i))
                .map(|item| Self::effort_focus(&item.id));
            node.down = self
                .efforts
                .get(index + 1)
                .map(|item| Self::effort_focus(&item.id));
            nodes.push(node);
        }
        focus.replace(nodes);
        if was_empty {
            let preferred = self
                .current
                .as_ref()
                .and_then(|current| current.reasoning_effort.clone())
                .or_else(|| self.default_effort.clone());
            let target = preferred
                .and_then(|id| self.efforts.iter().find(|effort| effort.id == id))
                .or_else(|| self.efforts.first())
                .map(|effort| Self::effort_focus(&effort.id));
            if let Some(target) = target {
                focus.set(target);
            }
        }
    }

    pub(super) fn activate(&mut self, focus: &mut FocusState) -> PageOutcome {
        let Some(id) = focus.current.as_ref().map(|id| id.0.clone()) else {
            return PageOutcome::default();
        };
        let Some(effort_id) = id.strip_prefix("effort:") else {
            return PageOutcome::default();
        };
        let Some(current) = self.current.as_ref() else {
            return PageOutcome::default();
        };
        PageOutcome::send(
            AgentRequest::ModelSet {
                provider: current.provider.clone(),
                model: current.model.clone(),
                reasoning_effort: Some(effort_id.to_owned()),
            },
            true,
        )
    }
}
